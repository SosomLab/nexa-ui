//! ★ **폴더 변경 감시**(nexa-sql T-293 · D-263 · 사용자 10-07 "폴더 변경 감시 전체 개발") — 프로젝트 폴더 아래에서 파일/폴더가
//! 생기거나·지워지거나·바뀌면 **어느 폴더가 바뀌었는지**를 묶어서(디바운스) 알린다. 호스트(nexa-sql)는 그 폴더만 다시 읽어 Ctrl+P/필터
//! 색인과 프로젝트 트리를 즉시 맞춘다(T-299와 결합).
//!
//! - **Windows** = `ReadDirectoryChangesW`(루트당 핸들 1 · 재귀 · 비동기 OVERLAPPED + 중지 이벤트 · 버퍼 64 KB) — 하위 폴더 제외가 API에
//!   없어 **제외 폴더 아래 이벤트는 즉시 버린다** · 버퍼 넘침(`bytes == 0`) = [`DirEvent::Overflow`](호스트 = 제외 아닌 폴더 재열거).
//! - **macOS(FSEvents) · Linux(inotify)** = 후속 단계([`DirWatch::supported`] = false → 호스트는 유지 시간(TTL) 방식으로 폴백 · 상태줄 안내).
//! - **묶음**: 코디네이터 스레드가 `debounce_ms`(기본 400) 동안 모아 [`DirEvent::Changed`] 한 번(폴더 집합 · 중복 없음).
//! - **자원**(nexa-sql 39 §3): 스레드 = 루트 수 + 1 · 유휴 CPU 0(커널 대기) · 네트워크 0 · 외부 crate 0 · [`Drop`] = 중지 이벤트 → 스레드 종료.
//! - 네트워크·이동식 드라이브 · 루트 삭제/이름 변경 = 읽기 실패 → [`DirEvent::RootGone`](호스트 = 폴백).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 감시 옵션.
#[derive(Debug, Clone)]
pub struct DirWatchOpts {
    /// 건너뛸 폴더 **이름**(어느 깊이든 · nexa-sql `project.exclude`).
    pub exclude_names: Vec<String>,
    /// 묶음 시간(ms · 이 안의 이벤트는 한 번에).
    pub debounce_ms: u64,
}

impl Default for DirWatchOpts {
    fn default() -> Self {
        DirWatchOpts {
            exclude_names: Vec::new(),
            debounce_ms: 400,
        }
    }
}

/// 호스트가 받는 사건.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirEvent {
    /// 바뀐 폴더들(절대 경로 · 중복 없음 · 제외 폴더 아래는 빠짐) — 그 폴더의 **직접 자식**이 바뀌었다.
    Changed(Vec<PathBuf>),
    /// 이 루트의 커널 버퍼가 넘쳐 일부를 놓쳤다 — 루트 아래(제외 아닌) 전부를 다시 읽을 것.
    Overflow(PathBuf),
    /// 루트를 더 못 읽는다(삭제 · 이름 변경 · 드라이브 분리) — 호스트는 폴백.
    RootGone(PathBuf),
}

/// 폴더 감시 핸들 — 떨어뜨리면 멈춘다.
pub struct DirWatch {
    rx: Receiver<DirEvent>,
    stop: Arc<AtomicBool>,
    #[cfg(windows)]
    stop_events: Vec<win::StopEvent>,
}

impl std::fmt::Debug for DirWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DirWatch")
            .field("stopped", &self.stop.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl DirWatch {
    /// 이 OS에 백엔드가 있는가(없으면 [`Self::spawn`]은 `None` · 호스트는 TTL 폴백).
    #[must_use]
    pub fn supported() -> bool {
        cfg!(windows)
    }

    /// 감시 시작(바로 돌아옴 · 사건은 [`Self::try_recv`]). 백엔드가 없거나 루트가 비면 `None`.
    #[must_use]
    pub fn spawn(roots: &[PathBuf], opts: DirWatchOpts) -> Option<DirWatch> {
        if roots.is_empty() || !Self::supported() {
            return None;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let (raw_tx, raw_rx) = mpsc::channel::<Raw>();
        let (tx, rx) = mpsc::channel::<DirEvent>();
        #[cfg(windows)]
        let mut stop_events = Vec::new();
        #[cfg(windows)]
        for root in roots {
            match win::start_root(root.clone(), opts.exclude_names.clone(), raw_tx.clone()) {
                Some(ev) => stop_events.push(ev),
                None => {
                    let _ = raw_tx.send(Raw::RootGone(root.clone()));
                }
            }
        }
        drop(raw_tx);
        // 코디네이터: 묶음(디바운스) → 호스트 채널.
        let debounce = Duration::from_millis(opts.debounce_ms.max(50));
        let stop2 = Arc::clone(&stop);
        let spawned = std::thread::Builder::new()
            .name("nexa-dirwatch".into())
            .spawn(move || coordinate(raw_rx, tx, debounce, stop2));
        if spawned.is_err() {
            return None;
        }
        Some(DirWatch {
            rx,
            stop,
            #[cfg(windows)]
            stop_events,
        })
    }

    /// 도착한 사건 하나(없으면 `None` · 막지 않음).
    pub fn try_recv(&self) -> Option<DirEvent> {
        self.rx.try_recv().ok()
    }
}

impl Drop for DirWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        #[cfg(windows)]
        for ev in &self.stop_events {
            ev.signal();
        }
    }
}

/// 백엔드 → 코디네이터 날것.
// Windows 백엔드만 있어 다른 OS에서는 변형이 만들어지지 않는다(`supported()` = false · macOS FSEvents · Linux inotify 후속) — OS별 허용(93 §P2 규칙).
#[cfg_attr(not(windows), allow(dead_code))]
enum Raw {
    Dir(PathBuf),
    Overflow(PathBuf),
    RootGone(PathBuf),
}

/// 묶음: 첫 사건 뒤 `debounce` 동안 모아 한 번에 · 넘침/루트 소실은 바로.
fn coordinate(rx: Receiver<Raw>, tx: Sender<DirEvent>, debounce: Duration, stop: Arc<AtomicBool>) {
    let mut pending: Vec<PathBuf> = Vec::new();
    let mut first: Option<Instant> = None;
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let wait = match first {
            Some(t0) => debounce.saturating_sub(t0.elapsed()),
            None => Duration::from_millis(500),
        };
        match rx.recv_timeout(wait) {
            Ok(Raw::Dir(d)) => {
                if !pending.contains(&d) {
                    pending.push(d);
                }
                first.get_or_insert_with(Instant::now);
            }
            Ok(Raw::Overflow(r)) => {
                pending.clear();
                first = None;
                if tx.send(DirEvent::Overflow(r)).is_err() {
                    return;
                }
            }
            Ok(Raw::RootGone(r)) => {
                if tx.send(DirEvent::RootGone(r)).is_err() {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                if !pending.is_empty() {
                    let _ = tx.send(DirEvent::Changed(std::mem::take(&mut pending)));
                }
                return;
            }
        }
        if first.is_some_and(|t0| t0.elapsed() >= debounce) && !pending.is_empty() {
            first = None;
            if tx
                .send(DirEvent::Changed(std::mem::take(&mut pending)))
                .is_err()
            {
                return;
            }
        }
    }
}

/// 상대 경로의 어느 조각이 제외 이름인가(순수 · 시험).
#[must_use]
pub fn under_excluded(rel: &Path, exclude_names: &[String]) -> bool {
    rel.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        exclude_names.iter().any(|n| *n == s)
    })
}

#[cfg(windows)]
mod win {
    use super::Raw;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::sync::mpsc::Sender;

    type Handle = *mut c_void;
    const INVALID_HANDLE: Handle = usize::MAX as Handle;
    const FILE_LIST_DIRECTORY: u32 = 0x0001;
    const FILE_SHARE_READ: u32 = 0x1;
    const FILE_SHARE_WRITE: u32 = 0x2;
    const FILE_SHARE_DELETE: u32 = 0x4;
    const OPEN_EXISTING: u32 = 3;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OVERLAPPED: u32 = 0x4000_0000;
    const FILE_NOTIFY_CHANGE_FILE_NAME: u32 = 0x1;
    const FILE_NOTIFY_CHANGE_DIR_NAME: u32 = 0x2;
    const FILE_NOTIFY_CHANGE_SIZE: u32 = 0x8;
    const FILE_NOTIFY_CHANGE_LAST_WRITE: u32 = 0x10;
    const WAIT_OBJECT_0: u32 = 0;
    const INFINITE: u32 = 0xFFFF_FFFF;
    const BUF_LEN: usize = 64 * 1024;

    #[repr(C)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        h_event: Handle,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            sa: *const c_void,
            disposition: u32,
            flags: u32,
            template: Handle,
        ) -> Handle;
        fn CloseHandle(h: Handle) -> i32;
        fn CreateEventW(sa: *const c_void, manual: i32, initial: i32, name: *const u16) -> Handle;
        fn SetEvent(h: Handle) -> i32;
        fn ResetEvent(h: Handle) -> i32;
        fn ReadDirectoryChangesW(
            dir: Handle,
            buf: *mut c_void,
            len: u32,
            subtree: i32,
            filter: u32,
            returned: *mut u32,
            ov: *mut Overlapped,
            completion: *const c_void,
        ) -> i32;
        fn WaitForMultipleObjects(n: u32, handles: *const Handle, all: i32, ms: u32) -> u32;
        fn GetOverlappedResult(h: Handle, ov: *mut Overlapped, bytes: *mut u32, wait: i32) -> i32;
        fn CancelIoEx(h: Handle, ov: *mut Overlapped) -> i32;
    }

    /// 중지 이벤트(스레드마다 하나 · `Drop`에서 신호).
    pub(super) struct StopEvent(usize);
    // SAFETY: 이벤트 핸들은 어느 스레드에서든 SetEvent 가능.
    unsafe impl Send for StopEvent {}
    unsafe impl Sync for StopEvent {}
    impl StopEvent {
        pub(super) fn signal(&self) {
            // SAFETY: 살아 있는 이벤트 핸들.
            unsafe {
                SetEvent(self.0 as Handle);
            }
        }
    }

    fn wide(p: &std::path::Path) -> Vec<u16> {
        p.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    /// 루트 하나의 감시 스레드 시작 — 열기 실패면 `None`.
    pub(super) fn start_root(
        root: PathBuf,
        excludes: Vec<String>,
        tx: Sender<Raw>,
    ) -> Option<StopEvent> {
        let name = wide(&root);
        // SAFETY: NUL 종료 wide 문자열 · 폴더 핸들(BACKUP_SEMANTICS) · OVERLAPPED.
        let h = unsafe {
            CreateFileW(
                name.as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        };
        if h == INVALID_HANDLE || h.is_null() {
            return None;
        }
        // SAFETY: 수동 리셋 이벤트 둘(완료 · 중지).
        let (done, stop) = unsafe {
            (
                CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()),
                CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()),
            )
        };
        if done.is_null() || stop.is_null() {
            // SAFETY: 방금 연 핸들 정리.
            unsafe {
                CloseHandle(h);
            }
            return None;
        }
        let stop_u = stop as usize;
        let (h_u, done_u) = (h as usize, done as usize);
        let spawned = std::thread::Builder::new()
            .name("nexa-dirwatch-root".into())
            .spawn(move || run_root(root, excludes, tx, h_u, done_u, stop_u));
        if spawned.is_err() {
            // SAFETY: 핸들 정리.
            unsafe {
                CloseHandle(h);
                CloseHandle(done);
                CloseHandle(stop);
            }
            return None;
        }
        Some(StopEvent(stop_u))
    }

    #[allow(clippy::cast_possible_truncation)]
    fn run_root(
        root: PathBuf,
        excludes: Vec<String>,
        tx: Sender<Raw>,
        h_u: usize,
        done_u: usize,
        stop_u: usize,
    ) {
        let (h, done, stop) = (h_u as Handle, done_u as Handle, stop_u as Handle);
        let mut buf = vec![0u8; BUF_LEN];
        let filter = FILE_NOTIFY_CHANGE_FILE_NAME
            | FILE_NOTIFY_CHANGE_DIR_NAME
            | FILE_NOTIFY_CHANGE_SIZE
            | FILE_NOTIFY_CHANGE_LAST_WRITE;
        loop {
            let mut ov = Overlapped {
                internal: 0,
                internal_high: 0,
                offset: 0,
                offset_high: 0,
                h_event: done,
            };
            // SAFETY: 살아 있는 핸들 · 버퍼는 이 루프가 끝날 때까지 유효 · 완료는 이벤트로.
            let ok = unsafe {
                ResetEvent(done);
                ReadDirectoryChangesW(
                    h,
                    buf.as_mut_ptr().cast(),
                    BUF_LEN as u32,
                    1,
                    filter,
                    std::ptr::null_mut(),
                    &mut ov,
                    std::ptr::null(),
                )
            };
            if ok == 0 {
                let _ = tx.send(Raw::RootGone(root));
                break;
            }
            let handles = [done, stop];
            // SAFETY: 핸들 두 개 배열.
            let which = unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) };
            if which != WAIT_OBJECT_0 {
                // 중지(또는 실패) — 대기 중인 읽기 취소.
                // SAFETY: 살아 있는 핸들.
                unsafe {
                    CancelIoEx(h, &mut ov);
                }
                break;
            }
            let mut bytes: u32 = 0;
            // SAFETY: 완료된 OVERLAPPED.
            let got = unsafe { GetOverlappedResult(h, &mut ov, &mut bytes, 0) };
            if got == 0 {
                let _ = tx.send(Raw::RootGone(root));
                break;
            }
            if bytes == 0 {
                // 커널 버퍼 넘침 = 일부를 놓쳤다.
                if tx.send(Raw::Overflow(root.clone())).is_err() {
                    break;
                }
                continue;
            }
            if !parse_and_send(&root, &excludes, &buf[..bytes as usize], &tx) {
                break;
            }
        }
        // SAFETY: 이 스레드만 쓰는 핸들 셋 정리.
        unsafe {
            CloseHandle(h);
            CloseHandle(done);
            CloseHandle(stop);
        }
    }

    /// FILE_NOTIFY_INFORMATION 목록 → 바뀐 항목의 **부모 폴더**(제외 아래는 버림). 돌려주는 값 = 채널 살아 있음.
    fn parse_and_send(
        root: &std::path::Path,
        excludes: &[String],
        buf: &[u8],
        tx: &Sender<Raw>,
    ) -> bool {
        let mut off = 0usize;
        loop {
            if off + 12 > buf.len() {
                break;
            }
            let next =
                u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]) as usize;
            let name_len =
                u32::from_le_bytes([buf[off + 8], buf[off + 9], buf[off + 10], buf[off + 11]])
                    as usize;
            let name_off = off + 12;
            if name_off + name_len > buf.len() {
                break;
            }
            let units: Vec<u16> = buf[name_off..name_off + name_len]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            let rel = PathBuf::from(String::from_utf16_lossy(&units));
            if !super::under_excluded(&rel, excludes) {
                let full = root.join(&rel);
                let dir = full
                    .parent()
                    .map_or_else(|| root.to_path_buf(), |p| p.to_path_buf());
                if tx.send(Raw::Dir(dir)).is_err() {
                    return false;
                }
            }
            if next == 0 {
                break;
            }
            off += next;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excluded_components() {
        let ex = vec!["target".to_string(), ".git".to_string()];
        assert!(under_excluded(Path::new("target/debug/x.o"), &ex));
        assert!(under_excluded(Path::new("a/.git/HEAD"), &ex));
        assert!(!under_excluded(Path::new("src/main.rs"), &ex));
        assert!(
            !under_excluded(Path::new("targets/x"), &ex),
            "이름 전체 일치만"
        );
    }

    /// Windows: 임시 폴더를 감시하다 파일을 만들면 그 부모 폴더가 묶음으로 온다 · 제외 폴더 아래 변경은 안 온다 · 떨어뜨리면 멈춘다.
    #[cfg(windows)]
    #[test]
    fn windows_watch_reports_parent_dir_and_skips_excluded() {
        let dir = std::env::temp_dir().join(format!("nexa-dirwatch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).expect("mkdir");
        std::fs::create_dir_all(dir.join("target")).expect("mkdir");
        let w = DirWatch::spawn(
            std::slice::from_ref(&dir),
            DirWatchOpts {
                exclude_names: vec!["target".into()],
                debounce_ms: 100,
            },
        )
        .expect("spawn");
        std::thread::sleep(Duration::from_millis(150));
        std::fs::write(dir.join("sub/a.txt"), "x").expect("write");
        std::fs::write(dir.join("target/b.txt"), "y").expect("write");
        let t0 = Instant::now();
        let mut got: Vec<PathBuf> = Vec::new();
        while t0.elapsed() < Duration::from_secs(3) {
            if let Some(DirEvent::Changed(dirs)) = w.try_recv() {
                got.extend(dirs);
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(got.iter().any(|d| d.ends_with("sub")), "{got:?}");
        assert!(
            !got.iter().any(|d| d.ends_with("target")),
            "제외 폴더 아래는 안 온다 {got:?}"
        );
        drop(w);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

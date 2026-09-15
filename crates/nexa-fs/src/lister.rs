//! 백그라운드 폴더 열거(docs/20 §2 `listing` 배치·취소 — 사용자 09-15 "파일 탐색이 성능·메모리에 영향 없게 · 스레드 분리 · 사용 후 회수").
//!
//! - 요청마다 **짧게 사는 스레드** 하나: `read_dir`를 스트리밍하며 `batch`개마다 [`ListMsg::Batch`]를 보내고, 다 읽으면 폴더마다
//!   [`ListMsg::Probe`](현재 보기로 보여줄 자식이 있는가 · 하위 폴더가 있는가 — 첫 일치에서 멈춤)를 보낸 뒤 [`ListMsg::Done`].
//! - **취소·회수**: [`ListHandle`]이 떨어지면(`Drop`) 취소 플래그가 서고 수신 채널이 닫힌다 → 스레드는 다음 배치/프로브 경계에서
//!   즉시 빠져나가고, 보내지 못한 배치는 채널과 함께 사라진다. 대화상자가 닫히면 핸들이 전부 떨어지므로 남는 스레드·메모리가 없다.
//! - UI 스레드는 `try_recv`로만 만난다(절대 블록 없음) · 프레임마다 소비 상한은 호출자가 정한다.

use crate::{has_subfolder, has_visible_child, is_virtual_root, list_opts, Entry};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

/// 워커 → UI 메시지.
#[derive(Debug)]
pub enum ListMsg {
    /// 항목 묶음(도착 순 · 정렬은 호출자).
    Batch(Vec<Entry>),
    /// 폴더 프로브 결과 — (경로, 현재 보기로 보여줄 자식이 있는가, 하위 폴더가 있는가).
    Probe(PathBuf, bool, bool),
    /// 끝 — `Ok(총 항목 수)` 또는 오류 문구.
    Done(Result<usize, String>),
}

/// 열거 옵션.
#[derive(Clone, Debug, Default)]
pub struct ListOpts {
    /// 숨김 항목 표시.
    pub show_hidden: bool,
    /// 점 파일 표시.
    pub show_dot: bool,
    /// 확장자 필터(비면 전체 · 프로브의 "보여줄 자식" 판정에도 쓴다).
    pub exts: Vec<String>,
    /// 폴더만(사이드바 트리).
    pub dirs_only: bool,
    /// 배치 크기(0 = 256).
    pub batch: usize,
    /// 프로브 생략(빠른 첫 화면이 더 중요할 때).
    pub skip_probe: bool,
}

/// 진행 중인 열거의 손잡이 — 떨어뜨리면 취소.
#[derive(Debug)]
pub struct ListHandle {
    rx: Receiver<ListMsg>,
    cancel: Arc<AtomicBool>,
    done: bool,
}

impl ListHandle {
    /// 폴더 열거 시작(가상 최상위 = 드라이브 목록 한 배치).
    #[must_use]
    pub fn start(dir: PathBuf, opts: ListOpts) -> Self {
        let (tx, rx) = mpsc::channel::<ListMsg>();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let batch = if opts.batch == 0 { 256 } else { opts.batch };
        let (fb_dir, fb_opts) = (dir.clone(), opts.clone());
        let spawn = std::thread::Builder::new()
            .name("nexa-fs-list".into())
            .spawn(move || {
                let cancelled = || flag.load(Ordering::Relaxed);
                if is_virtual_root(&dir) {
                    let drives = crate::drive_entries();
                    let paths: Vec<PathBuf> = drives.iter().map(|e| e.path.clone()).collect();
                    let n = drives.len();
                    if tx.send(ListMsg::Batch(drives)).is_err() {
                        return;
                    }
                    for p in paths {
                        if cancelled() {
                            return;
                        }
                        let a = has_visible_child(&p, opts.show_hidden, opts.show_dot, &opts.exts);
                        let b = has_subfolder(&p, opts.show_hidden, opts.show_dot);
                        if tx.send(ListMsg::Probe(p, a, b)).is_err() {
                            return;
                        }
                    }
                    let _ = tx.send(ListMsg::Done(Ok(n)));
                    return;
                }
                // 스트리밍 열거 — 실패는 Done(Err)로 한 번만.
                let rd = match std::fs::read_dir(&dir) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(ListMsg::Done(Err(e.to_string())));
                        return;
                    }
                };
                let mut buf: Vec<Entry> = Vec::with_capacity(batch);
                let mut dirs: Vec<PathBuf> = Vec::new();
                let mut total = 0usize;
                for de in rd.flatten() {
                    if cancelled() {
                        return;
                    }
                    let Some(e) = crate::entry_of(&de, opts.show_hidden, opts.show_dot) else {
                        continue;
                    };
                    if opts.dirs_only && !e.is_dir {
                        continue;
                    }
                    if !e.is_dir && !opts.exts.is_empty() && !opts.exts.contains(&e.ext()) {
                        continue;
                    }
                    if e.is_dir {
                        dirs.push(e.path.clone());
                    }
                    buf.push(e);
                    total += 1;
                    if buf.len() >= batch {
                        if tx.send(ListMsg::Batch(std::mem::take(&mut buf))).is_err() {
                            return;
                        }
                        buf.reserve(batch);
                    }
                }
                if !buf.is_empty() && tx.send(ListMsg::Batch(buf)).is_err() {
                    return;
                }
                if !opts.skip_probe {
                    for p in dirs {
                        if cancelled() {
                            return;
                        }
                        let a = has_visible_child(&p, opts.show_hidden, opts.show_dot, &opts.exts);
                        let b = has_subfolder(&p, opts.show_hidden, opts.show_dot);
                        if tx.send(ListMsg::Probe(p, a, b)).is_err() {
                            return;
                        }
                    }
                }
                let _ = tx.send(ListMsg::Done(Ok(total)));
            });
        if spawn.is_err() {
            // 스레드를 못 만들면 동기 폴백(작은 폴더면 체감 없음).
            let (tx2, rx2) = mpsc::channel::<ListMsg>();
            match list_opts(&fb_dir, fb_opts.show_hidden, fb_opts.show_dot) {
                Ok(v) => {
                    let n = v.len();
                    let _ = tx2.send(ListMsg::Batch(v));
                    let _ = tx2.send(ListMsg::Done(Ok(n)));
                }
                Err(e) => {
                    let _ = tx2.send(ListMsg::Done(Err(e.to_string())));
                }
            }
            return Self {
                rx: rx2,
                cancel,
                done: false,
            };
        }
        Self {
            rx,
            cancel,
            done: false,
        }
    }

    /// 경로 목록만 프로브(사이드바 장소·드라이브 — 열거 없이 셰브론 유무만).
    #[must_use]
    pub fn probe(paths: Vec<PathBuf>, opts: ListOpts) -> Self {
        let (tx, rx) = mpsc::channel::<ListMsg>();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let _ = std::thread::Builder::new()
            .name("nexa-fs-probe".into())
            .spawn(move || {
                for p in paths {
                    if flag.load(Ordering::Relaxed) {
                        return;
                    }
                    let a = has_visible_child(&p, opts.show_hidden, opts.show_dot, &opts.exts);
                    let b = has_subfolder(&p, opts.show_hidden, opts.show_dot);
                    if tx.send(ListMsg::Probe(p, a, b)).is_err() {
                        return;
                    }
                }
                let _ = tx.send(ListMsg::Done(Ok(0)));
            });
        Self {
            rx,
            cancel,
            done: false,
        }
    }

    /// 도착한 메시지 하나(없으면 `None` · 블록 없음). `Done` 뒤에는 늘 `None`.
    pub fn try_recv(&mut self) -> Option<ListMsg> {
        if self.done {
            return None;
        }
        match self.rx.try_recv() {
            Ok(m) => {
                if matches!(m, ListMsg::Done(_)) {
                    self.done = true;
                }
                Some(m)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.done = true;
                None
            }
        }
    }

    /// 끝났는가(Done 수신 또는 워커 종료).
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// 취소(스레드는 다음 경계에서 빠져나간다).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for ListHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn streams_batches_then_probes_then_done_and_cancels_on_drop() {
        let d = std::env::temp_dir().join(format!("nexa-fs-lister-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap_or(());
        std::fs::create_dir_all(d.join("empty")).unwrap_or(());
        std::fs::write(d.join("sub/x.sql"), b"1").unwrap_or(());
        for i in 0..600 {
            std::fs::write(d.join(format!("f{i}.txt")), b"x").unwrap_or(());
        }
        let mut h = ListHandle::start(
            d.clone(),
            ListOpts {
                show_hidden: false,
                show_dot: true,
                exts: vec!["sql".into()],
                dirs_only: false,
                batch: 100,
                skip_probe: false,
            },
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        let (mut got, mut probes, mut done) = (0usize, 0usize, None);
        while done.is_none() && Instant::now() < deadline {
            match h.try_recv() {
                Some(ListMsg::Batch(v)) => got += v.len(),
                Some(ListMsg::Probe(p, a, b)) => {
                    probes += 1;
                    if p.ends_with("sub") {
                        assert!(a, "sub에는 sql이 있다");
                        assert!(!b, "sub에는 하위 폴더가 없다");
                    }
                    if p.ends_with("empty") {
                        assert!(!a && !b);
                    }
                }
                Some(ListMsg::Done(r)) => done = Some(r),
                None => std::thread::sleep(Duration::from_millis(2)),
            }
        }
        assert_eq!(got, 2, "필터(sql)로 txt 600개는 걸러지고 폴더 2개만");
        assert_eq!(probes, 2);
        assert!(matches!(done, Some(Ok(2))));
        assert!(h.is_done());
        // Drop = 취소 플래그(스레드는 이미 끝남 · 플래그만 확인).
        let flag = Arc::clone(&h.cancel);
        drop(h);
        assert!(flag.load(Ordering::Relaxed));
        let _ = std::fs::remove_dir_all(&d);
    }
}

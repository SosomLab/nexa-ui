//! OS 셸 아이콘·종류 이름(docs/20 §2 `kind` — **OS 차이는 여기에만**).
//!
//! 사용자 09-15: 파일 대화상자에 "OS에서 지정된 폴더 이미지"를 탐색기처럼 보이게. 세 OS 동일 화면이 원칙(D-6)이지만
//! 사용자가 OS 아이콘을 명시해 **있으면 OS 것 · 없으면 자체 그림**으로 한다 — 호출자(nexa-dlg)가 `None`이면 자체 마스크로 폴백.
//!
//! - Windows: `SHGetFileInfoW`(shell32 인박스) — 확장자 기반은 `SHGFI_USEFILEATTRIBUTES`로 **디스크를 건드리지 않는다**(빠름 · 확장자마다 1회 캐시는 호출자) ·
//!   실제 경로 기반은 특수 폴더(다운로드·문서 …)의 고유 아이콘. HICON → 32bpp DIB(알파 있으면 그대로 · 없으면 마스크).
//! - macOS(`NSWorkspace iconForFile`)·Linux(테마 아이콘)는 후속 — 지금은 `None`(자체 그림).

/// RGBA 픽셀 아이콘(외부 타입 의존 0 — 호출자가 자기 이미지 타입으로 바꾼다).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaIcon {
    /// 폭.
    pub w: u32,
    /// 높이.
    pub h: u32,
    /// `w*h*4` RGBA.
    pub rgba: Vec<u8>,
}

/// 확장자(점 없음 · 소문자) 또는 폴더의 **종류별** 아이콘(디스크 접근 없음).
#[must_use]
pub fn icon_for_kind(ext: &str, is_dir: bool, large: bool) -> Option<RgbaIcon> {
    imp::icon_for_kind(ext, is_dir, large)
}

/// 실제 경로의 아이콘(특수 폴더·드라이브 — 셸이 경로를 본다 · 느릴 수 있어 사이드바 같은 소수 항목에만).
#[must_use]
pub fn icon_for_path(path: &std::path::Path, large: bool) -> Option<RgbaIcon> {
    imp::icon_for_path(path, large)
}

/// OS가 붙이는 종류 이름(예: "파일 폴더" · "Microsoft Word 문서") — 확장자 기반 · 없으면 `None`.
#[must_use]
pub fn kind_name(ext: &str, is_dir: bool) -> Option<String> {
    imp::kind_name(ext, is_dir)
}

// ───────────────────────── 아이콘 서비스(비동기 · 캐시 · 상한) ─────────────────────────
//
// ★ 실측(09-15 Windows 11): `icon_for_kind` ≈ 12ms/확장자 · `kind_name` ≈ 2ms · `icon_for_path`(홈) ≈ 17ms — UI 스레드에서 동기로
// 부르면 폴더 하나 여는 데 수백 ms가 샌다. 그래서 **조회는 워커 스레드**(COM STA · 요청 채널) · 호출자는 즉시 캐시 값 또는
// `Pending`을 받고 자체 그림으로 그리다가 `version()`이 바뀌면 다시 그린다. 캐시는 프로세스 수명(대화상자를 다시 열어도 0ms) ·
// **상한 512개**(16×16×4 ≈ 1KB → ≤ 0.5MB · 오래된 것부터 버림) · 실패도 기억(재조회 0).

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock};

/// 조회 키.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum IconKey {
    /// 종류(폴더 또는 확장자 · 디스크 접근 없음).
    Kind {
        /// 점 없는 소문자 확장자(폴더면 빈 문자열).
        ext: String,
        /// 폴더인가.
        is_dir: bool,
    },
    /// 실제 경로(특수 폴더·드라이브 고유 아이콘 · 셸이 경로를 본다).
    Path(PathBuf),
}

/// 조회 결과 — 캐시에 있으면 즉시, 없으면 워커에 맡기고 `Pending`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lookup<T> {
    /// 결과(`None` = 이 OS/종류에 아이콘 없음 → 호출자 자체 그림).
    Ready(Option<T>),
    /// 조회 중 — 지금은 자체 그림으로 · `version()`이 바뀌면 다시 묻는다.
    Pending,
}

enum Req {
    Icon(IconKey, bool),
    Name(String, bool),
}

#[derive(Default)]
struct Inner {
    icons: HashMap<(IconKey, bool), Option<Arc<RgbaIcon>>>,
    order: VecDeque<(IconKey, bool)>,
    names: HashMap<(String, bool), Option<String>>,
    pending_icons: HashSet<(IconKey, bool)>,
    pending_names: HashSet<(String, bool)>,
}

/// 프로세스 전역 아이콘 서비스([`IconService::global`]).
pub struct IconService {
    inner: Mutex<Inner>,
    version: AtomicU64,
    tx: Mutex<Option<Sender<Req>>>,
}

impl std::fmt::Debug for IconService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IconService")
            .field("version", &self.version.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

/// 캐시 상한(아이콘 수).
pub const ICON_CACHE_MAX: usize = 512;

static GLOBAL: OnceLock<IconService> = OnceLock::new();

impl IconService {
    /// 전역 인스턴스(워커는 첫 요청 때 만든다).
    pub fn global() -> &'static IconService {
        GLOBAL.get_or_init(|| IconService {
            inner: Mutex::new(Inner::default()),
            version: AtomicU64::new(0),
            tx: Mutex::new(None),
        })
    }

    /// 결과가 하나라도 도착할 때마다 1씩 오른다 — 호출자는 마지막에 본 값과 비교해 다시 묻는다.
    #[must_use]
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn send(&self, req: Req) {
        let mut tx = self
            .tx
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if tx.is_none() {
            let (s, r) = mpsc::channel::<Req>();
            let spawned = std::thread::Builder::new()
                .name("nexa-fs-icons".into())
                .spawn(move || {
                    for req in r {
                        let svc = IconService::global();
                        match req {
                            Req::Icon(key, large) => {
                                let got = match &key {
                                    IconKey::Kind { ext, is_dir } => {
                                        imp::icon_for_kind(ext, *is_dir, large)
                                    }
                                    IconKey::Path(p) => imp::icon_for_path(p, large),
                                };
                                svc.store_icon(key, large, got.map(Arc::new));
                            }
                            Req::Name(ext, is_dir) => {
                                let got = imp::kind_name(&ext, is_dir);
                                svc.store_name(ext, is_dir, got);
                            }
                        }
                    }
                });
            if spawned.is_ok() {
                *tx = Some(s);
            }
        }
        if let Some(s) = tx.as_ref() {
            let _ = s.send(req);
        }
    }

    fn store_icon(&self, key: IconKey, large: bool, v: Option<Arc<RgbaIcon>>) {
        let mut g = self.lock();
        g.pending_icons.remove(&(key.clone(), large));
        g.order.push_back((key.clone(), large));
        g.icons.insert((key, large), v);
        while g.order.len() > ICON_CACHE_MAX {
            if let Some(old) = g.order.pop_front() {
                g.icons.remove(&old);
            }
        }
        drop(g);
        self.version.fetch_add(1, Ordering::AcqRel);
    }

    fn store_name(&self, ext: String, is_dir: bool, v: Option<String>) {
        let mut g = self.lock();
        g.pending_names.remove(&(ext.clone(), is_dir));
        g.names.insert((ext, is_dir), v);
        drop(g);
        self.version.fetch_add(1, Ordering::AcqRel);
    }

    /// 아이콘 조회 — 캐시 적중이면 즉시 · 아니면 워커에 맡기고 `Pending`(이 OS에 셸 아이콘이 없으면 즉시 `Ready(None)`).
    pub fn icon(&self, key: &IconKey, large: bool) -> Lookup<Arc<RgbaIcon>> {
        if !imp::SUPPORTED {
            return Lookup::Ready(None);
        }
        let mut g = self.lock();
        if let Some(v) = g.icons.get(&(key.clone(), large)) {
            return Lookup::Ready(v.clone());
        }
        if g.pending_icons.insert((key.clone(), large)) {
            drop(g);
            self.send(Req::Icon(key.clone(), large));
        }
        Lookup::Pending
    }

    /// OS 종류 이름 조회(같은 규칙).
    pub fn kind_name(&self, ext: &str, is_dir: bool) -> Lookup<String> {
        if !imp::SUPPORTED {
            return Lookup::Ready(None);
        }
        let mut g = self.lock();
        let k = (ext.to_string(), is_dir);
        if let Some(v) = g.names.get(&k) {
            return Lookup::Ready(v.clone());
        }
        if g.pending_names.insert(k.clone()) {
            drop(g);
            self.send(Req::Name(k.0, k.1));
        }
        Lookup::Pending
    }

    /// 캐시된 아이콘 수(진단).
    #[must_use]
    pub fn cached(&self) -> usize {
        self.lock().icons.len()
    }

    /// 아직 도착하지 않은 요청 수(호스트가 폴링 타이머를 유지할 근거).
    #[must_use]
    pub fn pending(&self) -> usize {
        let g = self.lock();
        g.pending_icons.len() + g.pending_names.len()
    }
}

/// `shell:` 별칭(탐색기 규약 · `shell:startup` `shell:common startup` `shell:downloads` `shell:::{GUID}` …) → 실제 폴더.
/// 접두가 아니면 `None`(호출자는 일반 경로로) · 해석 실패도 `None`.
/// Windows = `SHParseDisplayName`(KnownFolders 레지스트리 전부 · 향후 추가분 자동) · macOS/Linux = 공통 이름 표(홈·바탕 화면·문서·다운로드·시작프로그램·앱데이터).
#[must_use]
pub fn resolve_alias(input: &str) -> Option<std::path::PathBuf> {
    let t = input.trim();
    if t.len() < 6 || !t[..6].eq_ignore_ascii_case("shell:") {
        return None;
    }
    let name = t[6..].trim();
    if let Some(p) = imp::shell_alias(name) {
        return Some(p);
    }
    portable_alias(name)
}

/// OS 공통 별칭 표(Windows에서도 셸이 모르는 이름의 폴백).
fn portable_alias(name: &str) -> Option<std::path::PathBuf> {
    let home = crate::home_dir()?;
    let n = name.to_ascii_lowercase();
    let sub = |s: &str| Some(home.join(s));
    match n.as_str() {
        "home" | "profile" | "userprofile" => Some(home),
        "desktop" => sub("Desktop"),
        "personal" | "documents" | "my documents" => sub("Documents"),
        "downloads" => sub("Downloads"),
        "startup" => {
            if cfg!(target_os = "macos") {
                sub("Library/LaunchAgents")
            } else if cfg!(unix) {
                sub(".config/autostart")
            } else {
                None
            }
        }
        "common startup" => {
            if cfg!(target_os = "macos") {
                Some(std::path::PathBuf::from("/Library/LaunchAgents"))
            } else if cfg!(unix) {
                Some(std::path::PathBuf::from("/etc/xdg/autostart"))
            } else {
                None
            }
        }
        "appdata" | "local appdata" => {
            if cfg!(target_os = "macos") {
                sub("Library/Application Support")
            } else if cfg!(unix) {
                sub(".config")
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(windows)]
mod imp {
    use super::RgbaIcon;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    type Handle = *mut c_void;

    #[repr(C)]
    struct ShFileInfoW {
        h_icon: Handle,
        i_icon: i32,
        dw_attributes: u32,
        sz_display_name: [u16; 260],
        sz_type_name: [u16; 80],
    }

    #[repr(C)]
    struct IconInfo {
        f_icon: i32,
        x_hotspot: u32,
        y_hotspot: u32,
        hbm_mask: Handle,
        hbm_color: Handle,
    }

    #[repr(C)]
    struct Bitmap {
        bm_type: i32,
        bm_width: i32,
        bm_height: i32,
        bm_width_bytes: i32,
        bm_planes: u16,
        bm_bits_pixel: u16,
        bm_bits: *mut c_void,
    }

    #[repr(C)]
    struct BitmapInfoHeader {
        bi_size: u32,
        bi_width: i32,
        bi_height: i32,
        bi_planes: u16,
        bi_bit_count: u16,
        bi_compression: u32,
        bi_size_image: u32,
        bi_x_pels: i32,
        bi_y_pels: i32,
        bi_clr_used: u32,
        bi_clr_important: u32,
    }

    #[repr(C)]
    struct BitmapInfo {
        header: BitmapInfoHeader,
        colors: [u32; 3],
    }

    pub(super) const SUPPORTED: bool = true;

    const SHGFI_ICON: u32 = 0x100;
    const SHGFI_TYPENAME: u32 = 0x400;
    const SHGFI_SMALLICON: u32 = 0x1;
    const SHGFI_USEFILEATTRIBUTES: u32 = 0x10;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;

    #[link(name = "shell32")]
    extern "system" {
        fn SHGetFileInfoW(
            path: *const u16,
            attrs: u32,
            info: *mut ShFileInfoW,
            cb: u32,
            flags: u32,
        ) -> usize;
    }
    #[link(name = "user32")]
    extern "system" {
        fn GetIconInfo(icon: Handle, out: *mut IconInfo) -> i32;
        fn DestroyIcon(icon: Handle) -> i32;
        fn GetDC(hwnd: Handle) -> Handle;
        fn ReleaseDC(hwnd: Handle, hdc: Handle) -> i32;
    }
    #[link(name = "gdi32")]
    extern "system" {
        fn GetObjectW(h: Handle, cb: i32, out: *mut c_void) -> i32;
        fn GetDIBits(
            hdc: Handle,
            hbm: Handle,
            start: u32,
            lines: u32,
            bits: *mut c_void,
            info: *mut BitmapInfo,
            usage: u32,
        ) -> i32;
        fn DeleteObject(h: Handle) -> i32;
    }
    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
        fn CoTaskMemFree(p: *mut c_void);
    }
    #[link(name = "shell32")]
    extern "system" {
        fn SHParseDisplayName(
            name: *const u16,
            bind_ctx: *mut c_void,
            pidl: *mut *mut c_void,
            attr_in: u32,
            attr_out: *mut u32,
        ) -> i32;
        fn SHGetPathFromIDListEx(pidl: *mut c_void, out: *mut u16, cch: u32, opts: u32) -> i32;
    }

    /// `shell:` 이름 → 경로(셸 정식 해석기 · 가상 폴더는 FS 경로가 없어 `None`).
    pub(super) fn shell_alias(name: &str) -> Option<std::path::PathBuf> {
        ensure_com();
        let full = format!("shell:{name}");
        let w = wide(std::ffi::OsStr::new(&full));
        // SAFETY: 널 종단 문자열 · pidl은 성공 시 CoTaskMemFree로 해제 · 버퍼 길이 전달.
        unsafe {
            let mut pidl: *mut c_void = std::ptr::null_mut();
            let mut attr: u32 = 0;
            if SHParseDisplayName(w.as_ptr(), std::ptr::null_mut(), &mut pidl, 0, &mut attr) < 0
                || pidl.is_null()
            {
                return None;
            }
            let mut buf = [0u16; 1024];
            let ok = SHGetPathFromIDListEx(pidl, buf.as_mut_ptr(), buf.len() as u32, 0);
            CoTaskMemFree(pidl);
            if ok == 0 {
                return None;
            }
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(std::path::PathBuf::from(String::from_utf16_lossy(
                &buf[..end],
            )))
        }
    }

    thread_local! {
        /// COM 초기화는 **스레드마다**(STA) — 전역 Once면 첫 스레드만 초기화되어 다른 스레드의 `SHParseDisplayName`이 실패한다.
        static COM_READY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn ensure_com() {
        COM_READY.with(|c| {
            if !c.get() {
                // SAFETY: 인자 없음 · 실패해도(이미 다른 모드로 초기화) 조회는 동작한다.
                unsafe {
                    let _ = CoInitializeEx(std::ptr::null_mut(), 2);
                }
                c.set(true);
            }
        });
    }

    /// HBITMAP(32bpp로 변환) → BGRA 상하 정방향 바이트.
    unsafe fn dib(hdc: Handle, hbm: Handle) -> Option<(i32, i32, Vec<u8>)> {
        let mut bm: Bitmap = std::mem::zeroed();
        if GetObjectW(
            hbm,
            std::mem::size_of::<Bitmap>() as i32,
            (&mut bm as *mut Bitmap).cast(),
        ) == 0
        {
            return None;
        }
        let (w, h) = (bm.bm_width, bm.bm_height);
        if w <= 0 || h <= 0 || w > 512 || h > 512 {
            return None;
        }
        let mut info = BitmapInfo {
            header: BitmapInfoHeader {
                bi_size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                bi_width: w,
                bi_height: -h, // 음수 = top-down
                bi_planes: 1,
                bi_bit_count: 32,
                bi_compression: 0,
                bi_size_image: 0,
                bi_x_pels: 0,
                bi_y_pels: 0,
                bi_clr_used: 0,
                bi_clr_important: 0,
            },
            colors: [0; 3],
        };
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let n = GetDIBits(hdc, hbm, 0, h as u32, buf.as_mut_ptr().cast(), &mut info, 0);
        (n > 0).then_some((w, h, buf))
    }

    /// HICON → RGBA(알파 채널이 전부 0이면 마스크 비트맵으로 알파를 만든다).
    fn icon_to_rgba(hicon: Handle) -> Option<RgbaIcon> {
        // SAFETY: 셸이 준 유효한 HICON · 모든 GDI 객체는 여기서 해제한다.
        unsafe {
            let mut ii: IconInfo = std::mem::zeroed();
            if GetIconInfo(hicon, &mut ii) == 0 {
                return None;
            }
            let hdc = GetDC(std::ptr::null_mut());
            let color = dib(hdc, ii.hbm_color);
            let mask = dib(hdc, ii.hbm_mask);
            ReleaseDC(std::ptr::null_mut(), hdc);
            if !ii.hbm_color.is_null() {
                DeleteObject(ii.hbm_color);
            }
            if !ii.hbm_mask.is_null() {
                DeleteObject(ii.hbm_mask);
            }
            let (w, h, bgra) = color?;
            let mut rgba = Vec::with_capacity(bgra.len());
            let any_alpha = bgra.chunks(4).any(|p| p[3] != 0);
            for (i, p) in bgra.chunks(4).enumerate() {
                let a = if any_alpha {
                    p[3]
                } else {
                    // 마스크: 흰(≠0) = 투명.
                    match &mask {
                        Some((mw, mh, mb)) if *mw == w && *mh == h => {
                            if mb[i * 4] == 0 {
                                255
                            } else {
                                0
                            }
                        }
                        _ => 255,
                    }
                };
                rgba.extend_from_slice(&[p[2], p[1], p[0], a]);
            }
            Some(RgbaIcon {
                w: w as u32,
                h: h as u32,
                rgba,
            })
        }
    }

    fn query(path: &[u16], attrs: u32, flags: u32) -> Option<ShFileInfoW> {
        ensure_com();
        // SAFETY: 널 종단 경로 · 구조체 크기 전달 · 반환 0 = 실패.
        unsafe {
            let mut info: ShFileInfoW = std::mem::zeroed();
            let ok = SHGetFileInfoW(
                path.as_ptr(),
                attrs,
                &mut info,
                std::mem::size_of::<ShFileInfoW>() as u32,
                flags,
            );
            (ok != 0).then_some(info)
        }
    }

    fn take_icon(info: ShFileInfoW) -> Option<RgbaIcon> {
        if info.h_icon.is_null() {
            return None;
        }
        let out = icon_to_rgba(info.h_icon);
        // SAFETY: SHGFI_ICON으로 받은 아이콘은 호출자가 파괴한다.
        unsafe {
            DestroyIcon(info.h_icon);
        }
        out
    }

    pub(super) fn icon_for_kind(ext: &str, is_dir: bool, large: bool) -> Option<RgbaIcon> {
        let name = if is_dir {
            "folder".to_string()
        } else if ext.is_empty() {
            "file".to_string()
        } else {
            format!("file.{ext}")
        };
        let attrs = if is_dir {
            FILE_ATTRIBUTE_DIRECTORY
        } else {
            FILE_ATTRIBUTE_NORMAL
        };
        let flags = SHGFI_ICON | SHGFI_USEFILEATTRIBUTES | if large { 0 } else { SHGFI_SMALLICON };
        let w = wide(std::ffi::OsStr::new(&name));
        query(&w, attrs, flags).and_then(take_icon)
    }

    pub(super) fn icon_for_path(path: &Path, large: bool) -> Option<RgbaIcon> {
        let flags = SHGFI_ICON | if large { 0 } else { SHGFI_SMALLICON };
        let w = wide(path.as_os_str());
        query(&w, 0, flags).and_then(take_icon)
    }

    pub(super) fn kind_name(ext: &str, is_dir: bool) -> Option<String> {
        let name = if is_dir {
            "folder".to_string()
        } else if ext.is_empty() {
            "file".to_string()
        } else {
            format!("file.{ext}")
        };
        let attrs = if is_dir {
            FILE_ATTRIBUTE_DIRECTORY
        } else {
            FILE_ATTRIBUTE_NORMAL
        };
        let w = wide(std::ffi::OsStr::new(&name));
        let info = query(&w, attrs, SHGFI_TYPENAME | SHGFI_USEFILEATTRIBUTES)?;
        let end = info
            .sz_type_name
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(info.sz_type_name.len());
        let s = String::from_utf16_lossy(&info.sz_type_name[..end]);
        (!s.trim().is_empty()).then_some(s)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::RgbaIcon;
    use std::path::Path;
    pub(super) const SUPPORTED: bool = false;
    pub(super) fn shell_alias(_name: &str) -> Option<std::path::PathBuf> {
        None
    }
    pub(super) fn icon_for_kind(_ext: &str, _is_dir: bool, _large: bool) -> Option<RgbaIcon> {
        None
    }
    pub(super) fn icon_for_path(_path: &Path, _large: bool) -> Option<RgbaIcon> {
        None
    }
    pub(super) fn kind_name(_ext: &str, _is_dir: bool) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod service_tests {
    use super::*;

    #[test]
    fn service_never_blocks_and_settles() {
        let svc = IconService::global();
        let key = IconKey::Kind {
            ext: "txt".into(),
            is_dir: false,
        };
        let t = std::time::Instant::now();
        let first = svc.icon(&key, false);
        assert!(
            t.elapsed().as_millis() < 5,
            "첫 조회는 즉시 돌아온다(워커에 맡김)"
        );
        match first {
            Lookup::Ready(None) => {} // 이 OS는 셸 아이콘 없음
            Lookup::Ready(Some(_)) | Lookup::Pending => {
                let v0 = svc.version();
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                while matches!(svc.icon(&key, false), Lookup::Pending) {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "5초 안에 도착해야 한다"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                assert!(svc.version() >= v0);
                assert!(matches!(svc.icon(&key, false), Lookup::Ready(_)));
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    #[ignore = "수동 계측 — cargo test -p nexa-fs -- --ignored --nocapture"]
    fn measure_lookup_cost() {
        let exts = [
            "sql", "txt", "docx", "pptx", "zip", "jpg", "png", "pdf", "xlsx", "md", "json", "csv",
        ];
        let t = std::time::Instant::now();
        for e in exts {
            let _ = icon_for_kind(e, false, false);
        }
        let per = t.elapsed() / exts.len() as u32;
        let t2 = std::time::Instant::now();
        for e in exts {
            let _ = kind_name(e, false);
        }
        let per2 = t2.elapsed() / exts.len() as u32;
        let t3 = std::time::Instant::now();
        let _ = icon_for_path(
            Path::new(&std::env::var("USERPROFILE").unwrap_or_default()),
            false,
        );
        println!(
            "icon_for_kind ≈ {per:?}/ext · kind_name ≈ {per2:?}/ext · icon_for_path(home) = {:?}",
            t3.elapsed()
        );
    }

    #[test]
    fn shell_aliases_resolve_like_explorer() {
        let st = resolve_alias("shell:startup").expect("startup");
        assert!(st.to_string_lossy().to_lowercase().ends_with("startup"));
        let common = resolve_alias("shell:common startup").expect("common startup");
        assert!(common
            .to_string_lossy()
            .to_lowercase()
            .contains("programdata"));
        assert!(resolve_alias("Shell:Downloads").is_some());
        assert!(resolve_alias("shell:no-such-folder-xyz").is_none());
        assert!(resolve_alias("C:\\Windows").is_none(), "접두가 아니면 None");
    }

    #[test]
    fn folder_icon_and_kind_come_from_shell() {
        let ic = icon_for_kind("", true, false).expect("폴더 아이콘");
        assert!(ic.w >= 16 && ic.h >= 16);
        assert_eq!(ic.rgba.len(), (ic.w * ic.h * 4) as usize);
        assert!(
            ic.rgba.chunks(4).any(|p| p[3] > 0),
            "불투명 픽셀이 있어야 한다"
        );
        assert!(kind_name("", true).is_some());
        assert!(kind_name("txt", false).is_some());
    }
}

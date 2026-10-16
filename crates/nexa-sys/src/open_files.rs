//! ★ OS가 "이 파일을 열어 달라"고 넘기는 경로(nexa-sql T-352 · 130차 mac · 10-16).
//!
//! | OS | 어디서 오나 | 구현 |
//! |---|---|---|
//! | macOS | Finder 더블클릭 · Dock 아이콘에 떨어뜨리기 · `open -a` = **Apple Event `odoc`**(argv가 아니다) → AppKit이 앱 델리게이트의 `application:openFiles:`를 부른다 | winit 0.30의 델리게이트(`WinitApplicationDelegate`)엔 그 메서드가 없어 사건이 버려진다 → [`install`]이 **그 클래스에 메서드를 런타임으로 덧붙인다**(`class_addMethod` · objc 런타임 수동 extern · 외부 crate 0). AppKit은 사건이 올 때마다 `respondsToSelector:`를 다시 보므로 델리게이트가 설치된 뒤(이벤트 루프 생성 뒤 · `run` 전)에 붙이면 **기동 때 넘어온 파일**(`applicationWillFinishLaunching` 뒤 배달)도 받는다 |
//! | Windows · Linux | 파일 연결은 argv로 온다(기존 경로) | [`install`] = `false` · [`take`] = 빈 벡터 |
//!
//! 쓰는 법: 이벤트 루프를 만든 뒤 `install()` 한 번 → 호출자는 `about_to_wait`(또는 그에 준하는 틱)마다 `take()`로 큐를 비워 연다. 깨우기는
//! 필요 없다 — 콜백은 메인 스레드의 AppKit 사건 처리 안에서 오고, winit은 그 사건을 다 처리한 뒤 `about_to_wait`를 돈다.
//! 규칙(크레이트 머리): 실패 = `false`/빈 값 · 프로세스 생성 0.

use std::path::PathBuf;
use std::sync::Mutex;

/// 받은 경로 큐(메인 스레드가 넣고 메인 스레드가 비운다 · 잠금은 형식상).
static QUEUE: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// OS 파일 열기 사건 받기를 설치한다. `true` = 이번 프로세스부터 [`take`]로 경로가 온다 · `false` = 미지원 OS · 델리게이트 없음 ·
/// 이미 같은 메서드가 있어 덮지 않음(그 경우 그쪽 구현이 받는다).
#[must_use]
pub fn install() -> bool {
    imp::install()
}

/// 마지막 호출 뒤 OS가 넘긴 경로를 **순서대로** 꺼낸다(비우며).
#[must_use]
pub fn take() -> Vec<PathBuf> {
    std::mem::take(&mut *QUEUE.lock().unwrap_or_else(|e| e.into_inner()))
}

/// 시험·진단용: OS 사건을 흉내 내 큐에 넣는다(모든 OS).
pub fn simulate<I: IntoIterator<Item = PathBuf>>(paths: I) {
    enqueue(paths.into_iter().collect());
}

fn enqueue(mut paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    QUEUE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .append(&mut paths);
}

#[cfg(target_os = "macos")]
mod imp {
    use core::ffi::{c_char, c_void, CStr};
    use std::path::PathBuf;

    type Id = *mut c_void;
    type Sel = *const c_void;
    type Imp = unsafe extern "C" fn();

    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> Id;
        fn sel_registerName(name: *const c_char) -> Sel;
        fn objc_msgSend();
        fn object_getClass(obj: Id) -> Id;
        fn class_getInstanceMethod(cls: Id, sel: Sel) -> *mut c_void;
        fn class_addMethod(cls: Id, sel: Sel, imp: Imp, types: *const c_char) -> i8;
    }

    fn sel(name: &'static CStr) -> Sel {
        // SAFETY: NUL로 끝나는 정적 C 문자열.
        unsafe { sel_registerName(name.as_ptr()) }
    }
    // `objc_msgSend`는 호출하는 쪽 서명으로 캐스팅해 부른다(layer_present.rs와 같은 규칙).
    unsafe fn send_id(obj: Id, s: Sel) -> Id {
        let f: unsafe extern "C" fn(Id, Sel) -> Id = std::mem::transmute(objc_msgSend as *const ());
        f(obj, s)
    }
    unsafe fn send_usize(obj: Id, s: Sel) -> usize {
        let f: unsafe extern "C" fn(Id, Sel) -> usize =
            std::mem::transmute(objc_msgSend as *const ());
        f(obj, s)
    }
    unsafe fn send_id_usize(obj: Id, s: Sel, a: usize) -> Id {
        let f: unsafe extern "C" fn(Id, Sel, usize) -> Id =
            std::mem::transmute(objc_msgSend as *const ());
        f(obj, s, a)
    }
    unsafe fn send_cstr(obj: Id, s: Sel) -> *const c_char {
        let f: unsafe extern "C" fn(Id, Sel) -> *const c_char =
            std::mem::transmute(objc_msgSend as *const ());
        f(obj, s)
    }
    unsafe fn send_void_usize(obj: Id, s: Sel, a: usize) {
        let f: unsafe extern "C" fn(Id, Sel, usize) =
            std::mem::transmute(objc_msgSend as *const ());
        f(obj, s, a);
    }

    /// `- (void)application:(NSApplication *)app openFiles:(NSArray<NSString *> *)files` — 경로를 큐에 넣고
    /// `replyToOpenOrPrint:NSApplicationDelegateReplySuccess`(0)로 Finder에 답한다(답하지 않으면 Finder가 "열 수 없음"을 띄운다).
    unsafe extern "C" fn open_files_imp(_this: Id, _sel: Sel, app: Id, files: Id) {
        let mut out = Vec::new();
        if !files.is_null() {
            let n = send_usize(files, sel(c"count"));
            for i in 0..n {
                let s = send_id_usize(files, sel(c"objectAtIndex:"), i);
                if s.is_null() {
                    continue;
                }
                let p = send_cstr(s, sel(c"UTF8String"));
                if !p.is_null() {
                    out.push(PathBuf::from(
                        CStr::from_ptr(p).to_string_lossy().into_owned(),
                    ));
                }
            }
        }
        if !app.is_null() {
            send_void_usize(app, sel(c"replyToOpenOrPrint:"), 0);
        }
        super::enqueue(out);
    }

    pub(super) fn install() -> bool {
        // SAFETY: 메인 스레드 · 클래스·셀렉터 이름은 정적 C 문자열 · `class_addMethod`는 이미 있는 메서드를 덮지 않는다(미리 확인).
        unsafe {
            let cls = objc_getClass(c"NSApplication".as_ptr());
            if cls.is_null() {
                return false;
            }
            let app = send_id(cls, sel(c"sharedApplication"));
            if app.is_null() {
                return false;
            }
            let delegate = send_id(app, sel(c"delegate"));
            if delegate.is_null() {
                return false;
            }
            let dcls = object_getClass(delegate);
            if dcls.is_null() {
                return false;
            }
            let s = sel(c"application:openFiles:");
            if !class_getInstanceMethod(dcls, s).is_null() {
                return false;
            }
            let imp: Imp =
                std::mem::transmute(open_files_imp as unsafe extern "C" fn(Id, Sel, Id, Id));
            class_addMethod(dcls, s, imp, c"v@:@@".as_ptr()) != 0
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub(super) fn install() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulate_then_take_in_order_and_empties() {
        let a = PathBuf::from("/tmp/a.sql");
        let b = PathBuf::from("/tmp/b.nsql-project");
        simulate([a.clone(), b.clone()]);
        assert_eq!(take(), vec![a, b]);
        assert!(take().is_empty());
    }
}

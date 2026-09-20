//! 키보드 **입력 소스** 신호(nexa-sql T-139 · 09-20) — "지금 한글 입력기인가"와 "입력 소스가 바뀌었다".
//!
//! 왜 필요한가: macOS에서 winit 창이 시스템 한글 IME를 거치면 ① 입력란이 IME를 막 붙인 직후의 첫 키가 조합 없이 자모로
//! 흘러 들어오고 ② 조합을 확정한 직후의 첫 1바이트 글자(숫자·기호·영문)가 앱에 도달하지 않는다(nexa-beep docs/34
//! H-14·H-26 — winit NSView `interpretKeyEvents` 경계). 앱이 IME를 끄고 raw 자모를 직접 조합하면(nexa-ctl `hangul`) 둘 다
//! 사라진다. 다만 IME를 끄면 일본어·중국어 입력이 막히므로 **한글 입력 소스일 때만** 그렇게 해야 한다 → 이 신호.
//!
//! | 함수 | macOS | 그 밖 |
//! |---|---|---|
//! | [`is_korean`] | Carbon TIS `TISCopyCurrentKeyboardInputSource` → `kTISPropertyInputSourceID`가 `com.apple.inputmethod.Korean`으로 시작 | `None` |
//! | [`watch`] / [`take_changed`] | 분산 알림 `kTISNotifySelectedKeyboardInputSourceChanged` 구독(콜백 = 깃발만 세움) | no-op / `false` |
//!
//! 규칙: **주 스레드에서만** 부른다(TIS·런 루프 알림) · 프로세스 생성 0 · 외부 crate 0 · 실패 = `None`.

/// 지금 키보드 입력 소스가 한글 입력기인가. `None` = 알 수 없음(미지원 OS · 호출 실패).
#[must_use]
pub fn is_korean() -> Option<bool> {
    imp::is_korean()
}

/// 입력 소스 바뀜 알림을 구독한다(프로세스당 1회 · 여러 번 불러도 한 번만). 구독됐으면 `true`.
pub fn watch() -> bool {
    imp::watch()
}

/// 마지막으로 확인한 뒤 입력 소스가 바뀌었는가(1회성 · [`watch`] 뒤에만 뜻이 있다).
#[must_use]
pub fn take_changed() -> bool {
    imp::take_changed()
}

#[cfg(target_os = "macos")]
mod imp {
    use core::ffi::{c_char, c_void};
    use std::sync::atomic::{AtomicBool, Ordering};

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFIndex = isize;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    /// `CFNotificationSuspensionBehaviorDeliverImmediately`.
    const DELIVER_IMMEDIATELY: CFIndex = 4;

    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn TISCopyCurrentKeyboardInputSource() -> CFTypeRef;
        fn TISGetInputSourceProperty(source: CFTypeRef, key: CFStringRef) -> *const c_void;
        static kTISPropertyInputSourceID: CFStringRef;
        static kTISNotifySelectedKeyboardInputSourceChanged: CFStringRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: CFTypeRef);
        fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, size: CFIndex, encoding: u32)
            -> u8;
        fn CFNotificationCenterGetDistributedCenter() -> CFTypeRef;
        fn CFNotificationCenterAddObserver(
            center: CFTypeRef,
            observer: *const c_void,
            callback: extern "C" fn(CFTypeRef, *mut c_void, CFStringRef, *const c_void, CFTypeRef),
            name: CFStringRef,
            object: *const c_void,
            suspension_behavior: CFIndex,
        );
    }

    static WATCHING: AtomicBool = AtomicBool::new(false);
    static CHANGED: AtomicBool = AtomicBool::new(false);

    extern "C" fn on_changed(
        _center: CFTypeRef,
        _observer: *mut c_void,
        _name: CFStringRef,
        _object: *const c_void,
        _info: CFTypeRef,
    ) {
        CHANGED.store(true, Ordering::Relaxed);
    }

    pub(super) fn is_korean() -> Option<bool> {
        // SAFETY: TIS는 주 스레드에서 부른다(호출 규약) · Copy로 얻은 소스는 끝에서 놓는다 · 속성 값은 빌린 참조(놓지 않는다).
        unsafe {
            let src = TISCopyCurrentKeyboardInputSource();
            if src.is_null() {
                return None;
            }
            let id = TISGetInputSourceProperty(src, kTISPropertyInputSourceID);
            let mut buf = [0 as c_char; 256];
            let ok = !id.is_null()
                && CFStringGetCString(
                    id,
                    buf.as_mut_ptr(),
                    buf.len() as CFIndex,
                    K_CF_STRING_ENCODING_UTF8,
                ) != 0;
            CFRelease(src);
            if !ok {
                return None;
            }
            let s = std::ffi::CStr::from_ptr(buf.as_ptr()).to_string_lossy();
            Some(s.starts_with("com.apple.inputmethod.Korean"))
        }
    }

    pub(super) fn watch() -> bool {
        if WATCHING.swap(true, Ordering::Relaxed) {
            return true;
        }
        // SAFETY: 분산 알림 센터는 프로세스 수명 동안 산다 · 콜백은 깃발만 세운다(관찰자 포인터 없음 = NULL).
        unsafe {
            let center = CFNotificationCenterGetDistributedCenter();
            if center.is_null() {
                WATCHING.store(false, Ordering::Relaxed);
                return false;
            }
            CFNotificationCenterAddObserver(
                center,
                core::ptr::null(),
                on_changed,
                kTISNotifySelectedKeyboardInputSourceChanged,
                core::ptr::null(),
                DELIVER_IMMEDIATELY,
            );
        }
        true
    }

    pub(super) fn take_changed() -> bool {
        CHANGED.swap(false, Ordering::Relaxed)
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub(super) fn is_korean() -> Option<bool> {
        None
    }
    pub(super) fn watch() -> bool {
        false
    }
    pub(super) fn take_changed() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    /// 어느 OS에서든 패닉 없이 값(또는 None)을 준다 — 맥에서는 `Some(_)`.
    #[test]
    fn reads_without_panicking() {
        let v = super::is_korean();
        if cfg!(target_os = "macos") {
            assert!(v.is_some());
        } else {
            assert!(v.is_none());
        }
        assert!(!super::take_changed());
    }
}

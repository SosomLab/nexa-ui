//! `nexa-sys` — OS 신호 읽기(nexa-sql [docs/39 §4-4] · **D-60**: `nexa-fs::sys`가 아니라 이름이 맞는 별도 크레이트).
//!
//! 앱의 성능 모드(`perf.mode = auto`)와 "배터리 전원 — balanced 권장" 안내가 읽는 네 신호:
//!
//! | 신호 | Windows | macOS | Linux |
//! |---|---|---|---|
//! | [`on_battery`] | `GetSystemPowerStatus` | IOKit `IOPSCopyPowerSourcesInfo` + `IOPSGetProvidingPowerSourceType` | `/sys/class/power_supply/*` |
//! | [`remote_session`] | `GetSystemMetrics(SM_REMOTESESSION)` | 감지 불가 → `None` | `SSH_CONNECTION` · `DISPLAY`/`WAYLAND_DISPLAY` |
//! | [`reduce_motion`] | `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)` | CoreFoundation `CFPreferencesCopyAppValue(reduceMotion, com.apple.universalaccess)` | `~/.config/gtk-3.0/settings.ini` `gtk-enable-animations` |
//! | [`cpu_count`] | `available_parallelism` | 동일 | 동일 |
//!
//! 원칙
//! - **실패는 전부 `None`** — 신호가 없으면 호출자는 아무것도 바꾸지 않는다(안내도 하지 않는다).
//! - **프로세스 생성 0 · 외부 crate 0** — FFI는 수동 `extern` 선언(Windows kernel32/user32 · macOS IOKit/CoreFoundation 프레임워크 링크).
//! - 값은 싸지만 공짜는 아니다(macOS는 CF 객체를 만들었다 놓는다) — 호출자가 **기동 1회 + 60초마다** 정도로 읽고 캐시한다(전원 이벤트 구독 없음).
//! - 지원하지 않는 OS는 no-op 폴백(`None`).

pub mod input_source;

/// 네 신호를 한 번에 읽은 스냅샷(호출자가 캐시한다).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Signals {
    /// 배터리 전원으로 도는 중(`Some(false)` = AC/전원 연결 · `None` = 모름/데스크톱).
    pub on_battery: Option<bool>,
    /// 원격 세션(RDP · SSH X 포워딩 등).
    pub remote_session: Option<bool>,
    /// OS "동작 줄이기"(접근성 · Windows "애니메이션 표시" 끔).
    pub reduce_motion: Option<bool>,
    /// 논리 CPU 코어 수(최소 1).
    pub cpu_count: usize,
}

impl Signals {
    /// 지금 값으로 스냅샷.
    #[must_use]
    pub fn read() -> Signals {
        Signals {
            on_battery: on_battery(),
            remote_session: remote_session(),
            reduce_motion: reduce_motion(),
            cpu_count: cpu_count(),
        }
    }
}

/// 배터리 전원으로 도는 중인가. `None` = 알 수 없음(데스크톱 · 미지원 OS · 호출 실패).
#[must_use]
pub fn on_battery() -> Option<bool> {
    imp::on_battery()
}

/// 원격 세션인가(RDP · SSH). `None` = 알 수 없음(macOS 화면 공유는 감지하지 않는다).
#[must_use]
pub fn remote_session() -> Option<bool> {
    imp::remote_session()
}

/// OS가 "동작 줄이기"를 켰는가. `None` = 알 수 없음.
#[must_use]
pub fn reduce_motion() -> Option<bool> {
    imp::reduce_motion()
}

/// 논리 CPU 코어 수(실패 시 1).
#[must_use]
pub fn cpu_count() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get())
}

#[cfg(windows)]
mod imp {
    //! Windows — kernel32 `GetSystemPowerStatus` · user32 `GetSystemMetrics` · `SystemParametersInfoW`(수동 extern · windows-sys 없음).

    #[repr(C)]
    #[derive(Default)]
    struct SystemPowerStatus {
        ac_line_status: u8,
        battery_flag: u8,
        battery_life_percent: u8,
        system_status_flag: u8,
        battery_life_time: u32,
        battery_full_life_time: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetSystemPowerStatus(status: *mut SystemPowerStatus) -> i32;
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetSystemMetrics(index: i32) -> i32;
        fn SystemParametersInfoW(
            action: u32,
            param: u32,
            pv: *mut core::ffi::c_void,
            win_ini: u32,
        ) -> i32;
    }

    const SM_REMOTESESSION: i32 = 0x1000;
    const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;
    /// `BatteryFlag` — 시스템 배터리 없음.
    const BATTERY_FLAG_NO_BATTERY: u8 = 128;

    pub(super) fn on_battery() -> Option<bool> {
        let mut st = SystemPowerStatus::default();
        // SAFETY: 구조체는 문서의 `SYSTEM_POWER_STATUS`와 배치가 같고 유효한 쓰기 포인터를 넘긴다.
        if unsafe { GetSystemPowerStatus(&mut st) } == 0 {
            return None;
        }
        if st.battery_flag == BATTERY_FLAG_NO_BATTERY {
            return Some(false);
        }
        match st.ac_line_status {
            0 => Some(true),
            1 => Some(false),
            _ => None,
        }
    }

    pub(super) fn remote_session() -> Option<bool> {
        // SAFETY: 인자 없는 순수 조회.
        Some(unsafe { GetSystemMetrics(SM_REMOTESESSION) } != 0)
    }

    pub(super) fn reduce_motion() -> Option<bool> {
        let mut on: i32 = 1;
        // SAFETY: SPI_GETCLIENTAREAANIMATION은 BOOL 하나를 쓴다 — 유효한 포인터.
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                (&mut on as *mut i32).cast(),
                0,
            )
        };
        (ok != 0).then_some(on == 0)
    }
}

#[cfg(target_os = "macos")]
mod imp {
    //! macOS — IOKit(`IOPSCopyPowerSourcesInfo`·`IOPSGetProvidingPowerSourceType`) + CoreFoundation(문자열 · 환경 설정)을
    //! C ABI로 직접 링크한다(objc2 없음 · `pmset`/`defaults` 프로세스 생성 없음).
    //! 원격 세션(화면 공유)은 공개 API가 없어 `None`.

    use core::ffi::{c_char, c_void};

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFIndex = isize;
    type CFTypeID = usize;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    /// `kCFNumberSInt32Type`.
    const K_CF_NUMBER_SINT32: CFIndex = 3;

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
        fn IOPSGetProvidingPowerSourceType(snapshot: CFTypeRef) -> CFStringRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: CFTypeRef);
        fn CFGetTypeID(cf: CFTypeRef) -> CFTypeID;
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            s: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, size: CFIndex, encoding: u32)
            -> u8;
        fn CFPreferencesCopyAppValue(key: CFStringRef, app: CFStringRef) -> CFTypeRef;
        fn CFBooleanGetTypeID() -> CFTypeID;
        fn CFBooleanGetValue(b: CFTypeRef) -> u8;
        fn CFNumberGetTypeID() -> CFTypeID;
        fn CFNumberGetValue(n: CFTypeRef, ty: CFIndex, out: *mut c_void) -> u8;
    }

    /// 소유한 CF 객체 — Drop에서 `CFRelease`.
    struct Owned(CFTypeRef);
    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: Copy/Create로 얻은 참조를 정확히 한 번 놓는다.
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn cf_str(s: &str) -> Option<Owned> {
        let c = std::ffi::CString::new(s).ok()?;
        // SAFETY: NUL 종료 UTF-8 · alloc NULL = 기본 할당자.
        let r = unsafe {
            CFStringCreateWithCString(core::ptr::null(), c.as_ptr(), K_CF_STRING_ENCODING_UTF8)
        };
        (!r.is_null()).then_some(Owned(r))
    }

    fn cf_string_to_rust(s: CFStringRef) -> Option<String> {
        if s.is_null() {
            return None;
        }
        let mut buf = [0 as c_char; 64];
        // SAFETY: 버퍼 크기를 함께 넘긴다 — CF가 NUL 종료를 보장.
        let ok = unsafe {
            CFStringGetCString(
                s,
                buf.as_mut_ptr(),
                buf.len() as CFIndex,
                K_CF_STRING_ENCODING_UTF8,
            )
        };
        if ok == 0 {
            return None;
        }
        let bytes: Vec<u8> = buf
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as u8)
            .collect();
        String::from_utf8(bytes).ok()
    }

    pub(super) fn on_battery() -> Option<bool> {
        // SAFETY: 인자 없음 · 반환은 소유 참조(Copy 규칙) → Owned가 놓는다.
        let info = Owned(unsafe { IOPSCopyPowerSourcesInfo() });
        if info.0.is_null() {
            return None;
        }
        // SAFETY: 유효한 스냅샷 · 반환 문자열은 스냅샷이 소유(놓지 않는다).
        let ty = unsafe { IOPSGetProvidingPowerSourceType(info.0) };
        match cf_string_to_rust(ty)?.as_str() {
            "Battery Power" | "UPS Power" => Some(true),
            "AC Power" => Some(false),
            _ => None,
        }
    }

    pub(super) fn remote_session() -> Option<bool> {
        None
    }

    pub(super) fn reduce_motion() -> Option<bool> {
        let key = cf_str("reduceMotion")?;
        let app = cf_str("com.apple.universalaccess")?;
        // SAFETY: 두 CFString이 유효 · 반환은 소유 참조(없으면 NULL).
        let v = Owned(unsafe { CFPreferencesCopyAppValue(key.0, app.0) });
        if v.0.is_null() {
            // 키가 없다 = 사용자가 한 번도 켜지 않았다 = 꺼짐.
            return Some(false);
        }
        // SAFETY: 유효한 CF 객체의 타입 조회·값 읽기.
        unsafe {
            let id = CFGetTypeID(v.0);
            if id == CFBooleanGetTypeID() {
                return Some(CFBooleanGetValue(v.0) != 0);
            }
            if id == CFNumberGetTypeID() {
                let mut n: i32 = 0;
                if CFNumberGetValue(v.0, K_CF_NUMBER_SINT32, (&mut n as *mut i32).cast()) != 0 {
                    return Some(n != 0);
                }
            }
        }
        None
    }
}

#[cfg(target_os = "linux")]
mod imp {
    //! Linux — sysfs(`/sys/class/power_supply`) · 환경변수 · GTK settings.ini(파일 읽기만 · `gsettings` 프로세스 없음).

    use std::path::Path;

    fn read_trim(p: &Path) -> Option<String> {
        std::fs::read_to_string(p)
            .ok()
            .map(|s| s.trim().to_string())
    }

    pub(super) fn on_battery() -> Option<bool> {
        let dir = std::fs::read_dir("/sys/class/power_supply").ok()?;
        let mut mains_online: Option<bool> = None;
        let mut battery_discharging: Option<bool> = None;
        for e in dir.flatten() {
            let p = e.path();
            match read_trim(&p.join("type")).as_deref() {
                Some("Mains") | Some("USB") => {
                    if let Some(on) = read_trim(&p.join("online")).map(|s| s == "1") {
                        mains_online = Some(mains_online.unwrap_or(false) || on);
                    }
                }
                Some("Battery") => {
                    let st = read_trim(&p.join("status"));
                    let d = matches!(st.as_deref(), Some("Discharging"));
                    battery_discharging = Some(battery_discharging.unwrap_or(false) || d);
                }
                _ => {}
            }
        }
        match (mains_online, battery_discharging) {
            (Some(true), _) => Some(false),
            (_, Some(true)) => Some(true),
            (Some(false), Some(false)) => Some(true),
            (Some(false), None) => Some(true),
            (None, Some(false)) => Some(false),
            (None, None) => None,
        }
    }

    pub(super) fn remote_session() -> Option<bool> {
        if std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_CLIENT").is_some()
        {
            return Some(true);
        }
        if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some() {
            return Some(false);
        }
        None
    }

    pub(super) fn reduce_motion() -> Option<bool> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))?;
        for sub in ["gtk-4.0", "gtk-3.0"] {
            let Some(text) = read_trim(&base.join(sub).join("settings.ini")) else {
                continue;
            };
            for line in text.lines() {
                let l = line.trim();
                if let Some(v) = l.strip_prefix("gtk-enable-animations") {
                    let v = v
                        .trim_start()
                        .strip_prefix('=')
                        .map(str::trim)
                        .unwrap_or("");
                    return Some(matches!(v, "0" | "false" | "FALSE" | "False"));
                }
            }
        }
        None
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod imp {
    //! 미지원 OS — 전부 `None`.
    pub(super) fn on_battery() -> Option<bool> {
        None
    }
    pub(super) fn remote_session() -> Option<bool> {
        None
    }
    pub(super) fn reduce_motion() -> Option<bool> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_count_is_at_least_one() {
        assert!(cpu_count() >= 1);
        assert!(Signals::read().cpu_count >= 1);
    }

    /// 신호 읽기는 어떤 OS에서도 패닉하지 않고, 값은 있거나 없거나다(호출 실패 = None).
    #[test]
    fn signals_never_panic() {
        let s = Signals::read();
        eprintln!("signals: {s:?}");
        assert_eq!(s, Signals::read(), "60초 안에서는 안정된 값");
    }
}

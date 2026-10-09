//! OS 화면 언어 — BCP 47 **주 언어 태그**(소문자 · `"ko"` · `"en"` · `"zh"` · `"ja"` …).
//!
//! 원천 = nexa-sql `crates/nsql-i18n/src/syslang.rs`(09-28 · 사용자 "언어가 있는 경우 해당 언어로 기본 설정, 없는 경우는 영어")
//! — 그쪽은 앱의 지원 언어(`Lang`)로 바로 접었지만, 여기는 **라이브러리라 앱 언어 집합을 모른다**(DR-2 앱 도메인 의존 0):
//! 주 언어 태그 문자열만 돌려주고, 지원 여부·폴백(보통 영어)은 앱이 정한다.
//!
//! | OS | 원천 |
//! |---|---|
//! | Windows | `GetUserDefaultUILanguage`(kernel32 · 표시 언어 LANGID) → `LCIDToLocaleName`(`ko-KR`) — 시작 메뉴에서 띄운 GUI에는 `LANG`이 없다 |
//! | macOS | `CFLocaleCopyPreferredLanguages`(CoreFoundation · 시스템 설정 ▸ 언어 목록의 첫째) — Finder에서 띄운 앱도 같다 |
//! | 그 밖(Linux 등) | `LANGUAGE`(콜론 목록의 첫째) → `LC_ALL` → `LC_MESSAGES` → `LANG`(gettext 우선순위) · **첫 "값 있는" 변수가 답**(`C`/`POSIX` = 모름 · 다음 변수로 넘어가지 않는다) |
//!
//! ★ **캐시하지 않는다** — 매 호출 OS에 묻는다(nexa-beep: 부팅마다 · 설정 화면을 열 때마다 OS 언어를 다시 따른다).
//! 호출 비용 = Windows 함수 2개 · macOS CF 배열 1개를 만들었다 놓음 · Linux 환경 변수 4개 — 자주 도는 길(그리기 · 틱)에서는 부르지 않는다.
//! 실패·모름 = `None`(호출자는 자기 기본 언어를 쓴다). 프로세스 생성 0 · 외부 crate 0.

/// 지금 OS 화면 언어의 주 언어 태그(소문자 · 2~3글자 알파벳). 모르면 `None`. **매 호출 판정**(캐시 없음).
#[must_use]
pub fn ui_language() -> Option<String> {
    detect()
}

/// 로캘 문자열(`ko_KR.UTF-8@euro` · `ko-KR` · `zh-Hans-CN` · `EN` · `ko`) → 주 언어 태그(소문자).
/// `C` · `POSIX` · 빈 값 · 알파벳 2~3글자가 아닌 머리 = `None`.
#[must_use]
pub fn primary_from_locale(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("C") || s.eq_ignore_ascii_case("POSIX") {
        return None;
    }
    // `ko_KR.UTF-8@euro` → `ko` · `zh-Hans-CN` → `zh`(인코딩 · 수식어 · 지역 · 문자 체계 접미를 뗀다).
    let head = s.split(['_', '-', '.', '@']).next().unwrap_or("");
    let ok = (2..=3).contains(&head.len()) && head.bytes().all(|b| b.is_ascii_alphabetic());
    ok.then(|| head.to_ascii_lowercase())
}

/// 유닉스 계열 환경 변수 순서(gettext): `LANGUAGE`(콜론 목록의 첫째) → `LC_ALL` → `LC_MESSAGES` → `LANG`.
/// **첫 "값 있는" 변수가 답**이다 — 그 값이 `C`/`POSIX`·판독 불가여도 다음 변수로 넘어가지 않고 `None`.
/// 환경을 주입받는 순수 함수(시험이 프로세스 환경을 건드리지 않는다).
#[must_use]
pub fn primary_from_env(get: impl Fn(&str) -> Option<String>) -> Option<String> {
    for key in ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"] {
        let Some(v) = get(key).filter(|v| !v.trim().is_empty()) else {
            continue;
        };
        let first = if key == "LANGUAGE" {
            v.split(':')
                .find(|p| !p.trim().is_empty())
                .unwrap_or("")
                .to_string()
        } else {
            v
        };
        return primary_from_locale(&first);
    }
    None
}

#[cfg(windows)]
fn detect() -> Option<String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
        fn LCIDToLocaleName(locale: u32, name: *mut u16, cch: i32, flags: u32) -> i32;
    }
    const LOCALE_NAME_MAX_LENGTH: usize = 85;
    // SAFETY: 인자 없는 조회 함수 · 전역 상태를 바꾸지 않는다.
    let langid = unsafe { GetUserDefaultUILanguage() };
    if langid == 0 {
        return None;
    }
    let mut buf = [0u16; LOCALE_NAME_MAX_LENGTH];
    // SAFETY: LCID = LANGID(정렬 ID 0) · 버퍼 길이를 그대로 넘긴다 · 반환값 = NUL 포함 글자 수(0 = 실패).
    let n = unsafe { LCIDToLocaleName(u32::from(langid), buf.as_mut_ptr(), buf.len() as i32, 0) };
    if n <= 1 {
        return None;
    }
    let name = String::from_utf16_lossy(&buf[..(n as usize - 1)]);
    primary_from_locale(&name)
}

#[cfg(target_os = "macos")]
fn detect() -> Option<String> {
    use std::ffi::{c_char, c_void, CStr};
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFLocaleCopyPreferredLanguages() -> *const c_void;
        fn CFArrayGetCount(a: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(a: *const c_void, i: isize) -> *const c_void;
        fn CFStringGetCString(s: *const c_void, buf: *mut c_char, len: isize, enc: u32) -> u8;
        fn CFRelease(p: *const c_void);
    }
    const UTF8: u32 = 0x0800_0100;
    // SAFETY: Copy 규칙 = 받은 배열은 우리가 CFRelease · 원소는 빌린 참조(배열이 살아 있는 동안만 읽는다) · 버퍼 길이를 넘겨 준다.
    unsafe {
        let arr = CFLocaleCopyPreferredLanguages();
        if arr.is_null() {
            return None;
        }
        let mut out = None;
        if CFArrayGetCount(arr) > 0 {
            let s = CFArrayGetValueAtIndex(arr, 0);
            let mut buf = [0 as c_char; 64];
            if !s.is_null()
                && CFStringGetCString(s, buf.as_mut_ptr(), buf.len() as isize, UTF8) != 0
            {
                let code = CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned();
                out = primary_from_locale(&code);
            }
        }
        CFRelease(arr);
        out
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn detect() -> Option<String> {
    primary_from_env(|k| std::env::var(k).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    #[test]
    fn locale_strings_give_lowercase_primary_tag() {
        assert_eq!(primary_from_locale("ko_KR.UTF-8"), s("ko"));
        assert_eq!(primary_from_locale("ko-KR"), s("ko"));
        assert_eq!(primary_from_locale("ko"), s("ko"));
        assert_eq!(primary_from_locale("en_US.UTF-8"), s("en"));
        assert_eq!(primary_from_locale("EN-gb"), s("en"), "대문자 → 소문자");
        assert_eq!(primary_from_locale("zh-Hans-CN"), s("zh"), "문자 체계 접미");
        assert_eq!(primary_from_locale("ja_JP.eucJP@x"), s("ja"));
        assert_eq!(primary_from_locale("fil-PH"), s("fil"), "3글자 언어");
        assert_eq!(primary_from_locale("de@euro"), s("de"));
        assert_eq!(primary_from_locale("C"), None);
        assert_eq!(primary_from_locale("C.UTF-8"), None, "C 로캘 + 인코딩");
        assert_eq!(primary_from_locale("POSIX"), None);
        assert_eq!(primary_from_locale(""), None);
        assert_eq!(primary_from_locale("  "), None);
        assert_eq!(primary_from_locale("x"), None, "1글자");
        assert_eq!(primary_from_locale("k1-KR"), None, "알파벳 아님");
    }

    #[test]
    fn env_priority_follows_gettext() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| (*v).to_string())
            }
        };
        assert_eq!(primary_from_env(env(&[("LANG", "ko_KR.UTF-8")])), s("ko"));
        assert_eq!(
            primary_from_env(env(&[("LANG", "en_US.UTF-8"), ("LC_ALL", "ko_KR.UTF-8")])),
            s("ko"),
            "LC_ALL이 LANG보다 앞"
        );
        assert_eq!(
            primary_from_env(env(&[
                ("LC_MESSAGES", "ja_JP.UTF-8"),
                ("LANG", "en_US.UTF-8")
            ])),
            s("ja"),
            "LC_MESSAGES가 LANG보다 앞"
        );
        assert_eq!(
            primary_from_env(env(&[("LANGUAGE", "ko:en"), ("LANG", "en_US.UTF-8")])),
            s("ko"),
            "LANGUAGE 목록의 첫째"
        );
        assert_eq!(
            primary_from_env(env(&[("LANGUAGE", ":zh_CN:en"), ("LANG", "en_US.UTF-8")])),
            s("zh"),
            "LANGUAGE 앞 빈 칸 건너뜀"
        );
        assert_eq!(
            primary_from_env(env(&[("LANGUAGE", "  "), ("LANG", "fr_FR.UTF-8")])),
            s("fr"),
            "공백뿐인 변수 = 값 없음 → 다음"
        );
        assert_eq!(
            primary_from_env(env(&[("LC_ALL", "C"), ("LANG", "ko_KR.UTF-8")])),
            None,
            "첫 값 있는 변수가 C = 모름(다음으로 넘어가지 않음)"
        );
        assert_eq!(primary_from_env(env(&[])), None);
    }

    #[test]
    fn ui_language_is_well_formed_when_known() {
        // 실제 OS 값은 환경마다 다르다 — 형식만 본다(소문자 알파벳 2~3글자 · 두 번 불러도 같은 값).
        let a = ui_language();
        if let Some(tag) = &a {
            assert!((2..=3).contains(&tag.len()), "{tag}");
            assert!(tag.bytes().all(|b| b.is_ascii_lowercase()), "{tag}");
        }
        assert_eq!(a, ui_language());
    }
}

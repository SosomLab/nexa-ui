//! ★ **자연 정렬 부품**(nexa-sql 사용자 10-09 "컬럼 정렬시에 자연정렬" · nexa-dir3 `ndir-tree::cmp_natural` 이식 · 30 §2 부품):
//! 숫자가 이어진 구간은 **수의 크기**로(`file2 < file10` · 앞의 0 무시 `007 = 7` · 아주 긴 숫자도 자릿수 비교라 안전), 그 밖의 글자는
//! 글자 단위로(대소문자 무시 옵션) 비교한다. 순수 · 힙 할당 0.
//!
//! 전역 스위치([`set_enabled`] · 호스트 설정 `ui.sort_natural`이 정한다 · 기본 켬)와 그것을 따르는 [`cmp_names`](켜짐 = 자연 정렬 ·
//! 꺼짐 = 대소문자 무시 글자 순)를 두어, 호스트의 모든 목록·트리·그리드 열 정렬이 **한 함수**를 부른다(영역별 설정 없음 —
//! 정렬 방식은 사용자의 눈높이 취향이라 영역마다 다를 이유가 없다 · nexa-sql 10-09 검토).

use std::cmp::Ordering;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

static ENABLED: AtomicBool = AtomicBool::new(true);

/// 전역 스위치(호스트 설정이 정한다 · 프로세스 전역).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, AtomicOrdering::Relaxed);
}

#[must_use]
pub fn enabled() -> bool {
    ENABLED.load(AtomicOrdering::Relaxed)
}

/// 자연 정렬 비교(순수 · 힙 할당 없음): ASCII 숫자가 이어진 구간은 수의 크기로(앞의 0은 무시 — `007` = `7`), 그 밖의 글자는
/// 글자 단위로(`ignore_case`면 소문자로 맞춰) 비교한다. 수가 같으면 다음 글자로 넘어간다.
#[must_use]
pub fn cmp_natural(a: &str, b: &str, ignore_case: bool) -> Ordering {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0usize, 0usize);
    while i < ab.len() && j < bb.len() {
        if ab[i].is_ascii_digit() && bb[j].is_ascii_digit() {
            let run = |s: &[u8], mut k: usize| {
                let start = k;
                while k < s.len() && s[k].is_ascii_digit() {
                    k += 1;
                }
                (start, k)
            };
            let ((si, ei), (sj, ej)) = (run(ab, i), run(bb, j));
            let sig = |s: &'_ [u8]| -> usize { s.iter().take_while(|&&c| c == b'0').count() };
            let (da, db) = (&ab[si..ei], &bb[sj..ej]);
            let (da, db) = (&da[sig(da)..], &db[sig(db)..]);
            let ord = da.len().cmp(&db.len()).then_with(|| da.cmp(db));
            if ord != Ordering::Equal {
                return ord;
            }
            (i, j) = (ei, ej);
        } else {
            let (Some(ca), Some(cb)) = (a[i..].chars().next(), b[j..].chars().next()) else {
                break;
            };
            let ord = if ignore_case {
                ca.to_lowercase().cmp(cb.to_lowercase())
            } else {
                ca.cmp(&cb)
            };
            if ord != Ordering::Equal {
                return ord;
            }
            i += ca.len_utf8();
            j += cb.len_utf8();
        }
    }
    (ab.len() - i).cmp(&(bb.len() - j))
}

/// 대소문자 무시 글자 순 비교(힙 할당 없음) — 자연 정렬을 끈 상태의 기본.
#[must_use]
pub fn cmp_ci(a: &str, b: &str) -> Ordering {
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
}

/// ★ 이름 비교 — 전역 스위치를 따른다(켜짐 = [`cmp_natural`] 대소문자 무시 · 꺼짐 = [`cmp_ci`]). 호스트의 목록·트리·그리드 글자 열이
/// 전부 이것을 부른다.
#[must_use]
pub fn cmp_names(a: &str, b: &str) -> Ordering {
    if enabled() {
        cmp_natural(a, b, true)
    } else {
        cmp_ci(a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering::{Equal, Greater, Less};

    /// nexa-dir3 이식 시험 그대로 + 한글·버전 문자열.
    #[test]
    fn natural_compare_orders_numbers_by_value() {
        assert_eq!(cmp_natural("file2.txt", "file10.txt", true), Less);
        assert_eq!(cmp_natural("file10.txt", "file2.txt", true), Greater);
        assert_eq!(cmp_natural("a007", "a7", true), Equal, "앞의 0 무시");
        assert_eq!(cmp_natural("a7b", "a007c", true), Less);
        assert_eq!(cmp_natural("File1", "file1", true), Equal);
        assert_eq!(
            cmp_natural("File1", "file1", false),
            Less,
            "구분 = 대문자 먼저"
        );
        assert_eq!(cmp_natural("보고서 9차", "보고서 12차", true), Less);
        assert_eq!(cmp_natural("abc", "abcd", true), Less);
        assert_eq!(cmp_natural("abc1", "abc", true), Greater);
        assert_eq!(cmp_natural("1", "a", true), Less);
        assert_eq!(
            cmp_natural(
                "n99999999999999999999999",
                "n100000000000000000000000",
                true
            ),
            Less
        );
        assert_eq!(cmp_natural("", "", true), Equal);
        assert_eq!(cmp_natural("v1.2.10", "v1.2.9", true), Greater);
        let mut v = vec![
            "img12.png",
            "img1.png",
            "IMG3.png",
            "img02.png",
            "img10.png",
        ];
        v.sort_by(|a, b| cmp_natural(a, b, true));
        assert_eq!(
            v,
            [
                "img1.png",
                "img02.png",
                "IMG3.png",
                "img10.png",
                "img12.png"
            ]
        );
        // DB 객체 이름(nexa-sql 사용자 10-09): `M4E_I30102 < M4E_I301010` · `ITEM2 < ITEM10`.
        assert_eq!(cmp_natural("M4E_I30102", "M4E_I301010", true), Less);
        assert_eq!(cmp_natural("ITEM10", "ITEM2", true), Greater);
    }

    /// 전역 스위치 = `cmp_names`가 따른다 · 끄면 글자 순(대소문자 무시).
    #[test]
    fn cmp_names_follows_switch() {
        set_enabled(true);
        assert_eq!(cmp_names("a2", "a10"), Less);
        set_enabled(false);
        assert_eq!(cmp_names("a2", "a10"), Greater, "글자 순 = '1' < '2'");
        assert_eq!(cmp_names("ABC", "abd"), Less, "대소문자 무시");
        assert_eq!(cmp_ci("Abc", "abc"), Equal);
        set_enabled(true);
    }
}

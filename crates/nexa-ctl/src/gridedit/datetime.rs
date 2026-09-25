//! 날짜·시각 입력 해석 — 사용자가 치는 형식 여럿을 **ISO 정규형**으로(87 §4).
//!
//! 정규형: Date `YYYY-MM-DD` · Time `HH:MM:SS[.fff]` · DateTime `YYYY-MM-DD HH:MM:SS[.fff]`.
//! 토큰 `now`·`today`(`sysdate`·`current_timestamp`·`current_date`·`getdate` 포함 · 대소문자 무관)는 **그대로** 돌려준다 — 방언 함수로 바꾸는 것은 호스트 몫.

use super::spec::CellKind;

/// 정규형 예시(오류 안내용).
#[must_use]
pub fn expected_form(kind: CellKind) -> &'static str {
    match kind {
        CellKind::Date => "2026-09-26",
        CellKind::Time => "14:05:07",
        _ => "2026-09-26 14:05:07",
    }
}

/// 종류에 맞게 해석 → 정규형. 실패 = `None`.
#[must_use]
pub fn parse(kind: CellKind, s: &str) -> Option<String> {
    let t = s.trim();
    if let Some(tok) = token(t) {
        return Some(tok.to_string());
    }
    match kind {
        CellKind::Date => parse_date(t).map(|(y, m, d)| fmt_date(y, m, d)),
        CellKind::Time => parse_time(t).map(fmt_time),
        _ => {
            if let Some((date, time)) = split_date_time(t) {
                let (y, m, d) = parse_date(date)?;
                let tm = if time.is_empty() {
                    (0, 0, 0, String::new())
                } else {
                    parse_time(time)?
                };
                Some(format!("{} {}", fmt_date(y, m, d), fmt_time(tm)))
            } else {
                None
            }
        }
    }
}

fn token(t: &str) -> Option<&'static str> {
    match t.to_ascii_lowercase().as_str() {
        "now" | "sysdate" | "systimestamp" | "current_timestamp" | "getdate" | "getdate()"
        | "now()" => Some("now"),
        "today" | "current_date" | "trunc(sysdate)" => Some("today"),
        _ => None,
    }
}

/// `2026-09-26 14:05` / `2026-09-26T14:05` / `20260926 1405` / 날짜만 → (날짜, 시각 또는 빈 글).
fn split_date_time(t: &str) -> Option<(&str, &str)> {
    if let Some(i) = t.find(['T', 't']) {
        // 'T' 구분자는 날짜 뒤 바로.
        let (d, rest) = t.split_at(i);
        if !d.is_empty() && looks_date(d) {
            return Some((d, &rest[1..]));
        }
    }
    if let Some(i) = t.find(char::is_whitespace) {
        let (d, rest) = t.split_at(i);
        if looks_date(d) {
            return Some((d, rest.trim_start()));
        }
        return None;
    }
    if looks_date(t) {
        return Some((t, ""));
    }
    None
}

fn looks_date(d: &str) -> bool {
    let digits = d.chars().filter(char::is_ascii_digit).count();
    let others = d
        .chars()
        .filter(|c| !c.is_ascii_digit())
        .all(|c| matches!(c, '-' | '/' | '.'));
    others && (6..=8).contains(&digits) && d.len() <= 10
}

/// `YYYY-MM-DD` · `YYYY/M/D` · `YYYY.MM.DD` · `YYYYMMDD` · (`YYMMDD`는 받지 않는다 — 세기 모호).
#[must_use]
pub fn parse_date(s: &str) -> Option<(u32, u32, u32)> {
    let t = s.trim();
    let parts: Vec<&str> = t.split(['-', '/', '.']).collect();
    let (y, m, d) = if parts.len() == 3 {
        (
            parts[0].parse::<u32>().ok()?,
            parts[1].parse::<u32>().ok()?,
            parts[2].parse::<u32>().ok()?,
        )
    } else if parts.len() == 1 && t.len() == 8 && t.bytes().all(|b| b.is_ascii_digit()) {
        (
            t[0..4].parse().ok()?,
            t[4..6].parse().ok()?,
            t[6..8].parse().ok()?,
        )
    } else {
        return None;
    };
    if parts.len() == 3 && parts[0].len() != 4 {
        return None;
    }
    valid_date(y, m, d).then_some((y, m, d))
}

fn valid_date(y: u32, m: u32, d: u32) -> bool {
    if !(1..=9999).contains(&y) || !(1..=12).contains(&m) || d == 0 {
        return false;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if leap {
                29
            } else {
                28
            }
        }
    };
    d <= days
}

/// `HH:MM` · `HH:MM:SS` · `HH:MM:SS.fff…` · `HHMM` · `HHMMSS` → (h, m, s, 소수 자릿수 문자열).
#[must_use]
pub fn parse_time(s: &str) -> Option<(u32, u32, u32, String)> {
    let t = s.trim();
    let (main, frac) = match t.split_once('.') {
        Some((a, f)) => {
            if f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()) || f.len() > 9 {
                return None;
            }
            (a, f.to_string())
        }
        None => (t, String::new()),
    };
    let (h, m, sec) = if main.contains(':') {
        let p: Vec<&str> = main.split(':').collect();
        if p.len() < 2 || p.len() > 3 {
            return None;
        }
        (
            p[0].parse::<u32>().ok()?,
            p[1].parse::<u32>().ok()?,
            if p.len() == 3 {
                p[2].parse::<u32>().ok()?
            } else {
                0
            },
        )
    } else if main.bytes().all(|b| b.is_ascii_digit()) && (main.len() == 4 || main.len() == 6) {
        (
            main[0..2].parse().ok()?,
            main[2..4].parse().ok()?,
            if main.len() == 6 {
                main[4..6].parse().ok()?
            } else {
                0
            },
        )
    } else {
        return None;
    };
    (h < 24 && m < 60 && sec < 60).then_some((h, m, sec, frac))
}

fn fmt_date(y: u32, m: u32, d: u32) -> String {
    format!("{y:04}-{m:02}-{d:02}")
}

fn fmt_time((h, m, s, frac): (u32, u32, u32, String)) -> String {
    if frac.is_empty() {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{h:02}:{m:02}:{s:02}.{frac}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CellKind::{Date, DateTime, Time};

    #[test]
    fn dates() {
        for (input, want) in [
            ("2026-09-26", "2026-09-26"),
            ("2026/9/26", "2026-09-26"),
            ("2026.09.26", "2026-09-26"),
            ("20260926", "2026-09-26"),
            (" 2024-02-29 ", "2024-02-29"),
        ] {
            assert_eq!(parse(Date, input).as_deref(), Some(want), "{input}");
        }
        for bad in [
            "2023-02-29",
            "2026-13-01",
            "26-09-26",
            "260926",
            "2026-9",
            "abc",
            "2026-00-10",
        ] {
            assert_eq!(parse(Date, bad), None, "{bad}");
        }
    }

    #[test]
    fn times() {
        for (input, want) in [
            ("14:05", "14:05:00"),
            ("14:05:07", "14:05:07"),
            ("14:05:07.123", "14:05:07.123"),
            ("1405", "14:05:00"),
            ("140507", "14:05:07"),
            ("00:00:00.000000", "00:00:00.000000"),
        ] {
            assert_eq!(parse(Time, input).as_deref(), Some(want), "{input}");
        }
        for bad in ["24:00", "14:60", "14", "14:05:07.", "1:2:3:4"] {
            assert_eq!(parse(Time, bad), None, "{bad}");
        }
    }

    #[test]
    fn datetimes_and_tokens() {
        for (input, want) in [
            ("2026-09-26", "2026-09-26 00:00:00"),
            ("2026-09-26 14:05", "2026-09-26 14:05:00"),
            ("2026-09-26T14:05:07", "2026-09-26 14:05:07"),
            ("2026/9/26  14:05:07.5", "2026-09-26 14:05:07.5"),
            ("20260926 1405", "2026-09-26 14:05:00"),
            ("NOW", "now"),
            ("SYSDATE", "now"),
            ("Today", "today"),
            ("current_date", "today"),
        ] {
            assert_eq!(parse(DateTime, input).as_deref(), Some(want), "{input}");
        }
        assert_eq!(parse(DateTime, "14:05"), None);
        assert_eq!(parse(DateTime, "2026-09-26 25:00"), None);
        assert_eq!(parse(Date, "now").as_deref(), Some("now"));
    }
}

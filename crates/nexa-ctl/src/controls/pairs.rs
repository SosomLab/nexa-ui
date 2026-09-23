//! **괄호·인용부호 쌍 표**(nexa-sql docs/51 §2·§8 — 사용자 09-17 "레인보우 괄호 · 짝/형제/상위/하위 이동 · 자동 닫기").
//!
//! 한 번 스캔(O(n))해 표를 만들고, 색칠·현재 쌍 강조·이동 명령·자동 닫기가 **같은 표**를 읽는다.
//! 강조기 토큰을 따라 걷는다: 주석은 건너뛰고, 문자열은 **양 끝 인용부호만** 쌍으로 등록(안쪽은 스캔 안 함).
//! 편집기 밖(UI 없음)이라 테스트가 쉽다.

use crate::highlight::{Highlighter, TokenKind};

/// 쌍 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum PairKind {
    Round,
    Square,
    Curly,
    Angle,
    DQuote,
    SQuote,
    BQuote,
}

impl PairKind {
    #[must_use]
    pub fn open_char(self) -> char {
        match self {
            PairKind::Round => '(',
            PairKind::Square => '[',
            PairKind::Curly => '{',
            PairKind::Angle => '<',
            PairKind::DQuote => '"',
            PairKind::SQuote => '\'',
            PairKind::BQuote => '`',
        }
    }
    #[must_use]
    pub fn close_char(self) -> char {
        match self {
            PairKind::Round => ')',
            PairKind::Square => ']',
            PairKind::Curly => '}',
            PairKind::Angle => '>',
            PairKind::DQuote => '"',
            PairKind::SQuote => '\'',
            PairKind::BQuote => '`',
        }
    }
    #[must_use]
    pub fn from_open(c: char) -> Option<PairKind> {
        Some(match c {
            '(' => PairKind::Round,
            '[' => PairKind::Square,
            '{' => PairKind::Curly,
            '<' => PairKind::Angle,
            '"' => PairKind::DQuote,
            '\'' => PairKind::SQuote,
            '`' => PairKind::BQuote,
            _ => return None,
        })
    }
    #[must_use]
    pub fn from_close(c: char) -> Option<PairKind> {
        Some(match c {
            ')' => PairKind::Round,
            ']' => PairKind::Square,
            '}' => PairKind::Curly,
            '>' => PairKind::Angle,
            _ => return None,
        })
    }
    #[must_use]
    pub fn is_quote(self) -> bool {
        matches!(self, PairKind::DQuote | PairKind::SQuote | PairKind::BQuote)
    }
    /// [`PairOpts::kinds`]의 비트.
    #[must_use]
    pub fn bit(self) -> u8 {
        1 << (self as u8)
    }
    /// 일곱 종류 전부(열림 글자 순).
    pub const ALL: [PairKind; 7] = [
        PairKind::Round,
        PairKind::Square,
        PairKind::Curly,
        PairKind::Angle,
        PairKind::DQuote,
        PairKind::SQuote,
        PairKind::BQuote,
    ];
}

/// 쌍 하나(문자 인덱스).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    pub open: usize,
    pub close: usize,
    pub depth: u16,
    pub kind: PairKind,
    /// 감싸는 쌍(표 인덱스).
    pub parent: Option<u32>,
}

/// 스캔 옵션 — **편집 코어 설정**(nexa-sql `editor.pair_kinds` · `editor.pair_in_strings` · 사용자 09-23 "Rainbow 확장이 아니라
/// 기본 기능 설정으로 · 강조 대상을 지정해서").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairOpts {
    /// 쌍으로 다룰 종류의 비트 집합([`PairKind::bit`]) — 기본 = `< >` 빼고 전부.
    pub kinds: u8,
    /// 문자열 **안**의 `{ ( [` 와 다른 인용부호도 쌍으로(그 문자열 안에서만 · 기본 켬). 끄면 문자열 안에서는
    /// 그 문자열의 인용부호 쌍만 보이고 안쪽은 보지 않는다.
    pub in_strings: bool,
}

impl Default for PairOpts {
    fn default() -> Self {
        PairOpts {
            kinds: PairOpts::ALL & !PairKind::Angle.bit(),
            in_strings: true,
        }
    }
}

impl PairOpts {
    /// 일곱 종류 전부.
    pub const ALL: u8 = 0x7F;

    /// 이 종류를 쌍으로 다루는가.
    #[must_use]
    pub fn has(self, k: PairKind) -> bool {
        self.kinds & k.bit() != 0
    }

    /// 인용부호 종류가 하나라도 켜져 있는가.
    #[must_use]
    pub fn any_quote(self) -> bool {
        PairKind::ALL.iter().any(|k| k.is_quote() && self.has(*k))
    }

    /// 설정 문자열(`() [] {} <> "" '' ``` · 공백/콤마 구분 · 토큰의 첫 글자 = 열림 글자) → 비트 집합. 모르는 토큰은 무시.
    #[must_use]
    pub fn kinds_from_spec(spec: &str) -> u8 {
        spec.split(|c: char| c.is_whitespace() || c == ',')
            .filter_map(|t| t.chars().next())
            .filter_map(PairKind::from_open)
            .fold(0, |acc, k| acc | k.bit())
    }
}

/// 쌍 표 — `pairs`는 열림 위치 오름차순.
#[derive(Clone, Debug, Default)]
pub struct PairTable {
    pub pairs: Vec<Pair>,
    /// 닫힘 위치 순 인덱스.
    close_order: Vec<u32>,
    /// 짝 없는 괄호(위치 · 종류).
    pub unmatched: Vec<(usize, PairKind)>,
    /// 모든 괄호 글자 (위치, 깊이, 짝 없음) 정렬 — 페인트의 색 조회.
    marks: Vec<(usize, u16, bool)>,
}

impl PairTable {
    /// 텍스트 전체 스캔. `hl`이 없으면 전부 평문으로 본다(인용부호는 문자열 시작/끝으로 직접 판정).
    #[must_use]
    pub fn build(text: &str, hl: Option<&dyn Highlighter>, opts: PairOpts) -> PairTable {
        // (열림, 종류)
        let mut stack: Vec<(usize, PairKind)> = Vec::new();
        // ★ 문자열 안도 스캔한다(사용자 09-23 "' 안이라도 { " 등의 쌍을 보여 달라"): 열린 인용부호를 스택에 올려 **바닥**으로
        //   삼는다 — 문자열 안의 괄호는 그 문자열 안에서만 짝이 되고(바닥 아래로는 내려가지 않음), 문자열이 닫힐 때
        //   안에 남은 열림은 **조용히 버린다**(짝 없음 표시는 코드 층에서만 · `'('` 같은 문자열이 빨갛게 되지 않게).
        //   같은 인용부호가 겹치면 가장 안쪽부터 닫는다 · `\"` 이스케이프는 닫지 않는다.
        let mut floors: Vec<usize> = Vec::new(); // 열린 인용부호의 스택 인덱스(안쪽이 뒤)
        let mut done: Vec<(usize, usize, PairKind, Option<usize>)> = Vec::new(); // open, close, kind, parent_open
        let mut unmatched: Vec<(usize, PairKind)> = Vec::new();
        let mut state = 0u32;
        let mut spans: Vec<(usize, TokenKind)> = Vec::new();
        let mut base = 0usize;
        // ★ 구문이 "같은 인용부호 두 번 = 이스케이프"(SQL `'O''Neil'`)이면 열린 문자열 안의 `''`는 시작/끝이 아니다(사용자 09-23).
        let doubled = hl.is_some_and(Highlighter::doubled_quote_escapes);
        for line in text.split('\n') {
            let chars: Vec<char> = line.chars().collect();
            spans.clear();
            if let Some(h) = hl {
                h.line_spans(line, &mut state, &mut spans);
            } else {
                spans.push((chars.len(), TokenKind::Plain));
            }
            let mut ci = 0usize;
            for &(n, kind) in &spans {
                let end = (ci + n).min(chars.len());
                if matches!(kind, TokenKind::Comment) {
                    ci = end;
                    continue;
                }
                // 인용부호가 구분자로 작동하는 곳 = 강조기의 문자열 토큰 · 평문 모드 전부.
                let quotes_here = hl.is_none() || matches!(kind, TokenKind::Str);
                let mut i = ci;
                while i < end {
                    let c = chars[i];
                    let pos = base + i;
                    let in_string = !floors.is_empty();
                    if quotes_here
                        && matches!(c, '"' | '\'' | '`')
                        && !(i > 0 && chars[i - 1] == '\\')
                    {
                        let k = PairKind::from_open(c).expect("quote kind");
                        if !opts.has(k) {
                            i += 1;
                            continue;
                        }
                        // 안쪽을 보지 않는 모드(`in_strings` 끔)에서는 지금 열린 인용부호와 같은 것만 닫는다.
                        if !opts.in_strings
                            && in_string
                            && floors.last().is_some_and(|&f| stack[f].1 != k)
                        {
                            i += 1;
                            continue;
                        }
                        match floors.iter().rposition(|&f| stack[f].1 == k) {
                            // 같은 인용부호로 열린 문자열 안에서 그 인용부호가 두 번 = 이스케이프 → 둘 다 건너뛴다.
                            Some(_) if doubled && chars.get(i + 1) == Some(&c) => {
                                i += 2;
                                continue;
                            }
                            Some(fi) => {
                                let f = floors[fi];
                                stack.truncate(f + 1);
                                floors.truncate(fi);
                                let (o, _) = stack.pop().expect("quote open");
                                done.push((o, pos, k, stack.last().map(|s| s.0)));
                            }
                            None => {
                                floors.push(stack.len());
                                stack.push((pos, k));
                            }
                        }
                        i += 1;
                        continue;
                    }
                    if in_string && !opts.in_strings {
                        // 종전 동작: 문자열 안쪽의 괄호는 보지 않는다.
                        i += 1;
                        continue;
                    }
                    if let Some(k) = PairKind::from_open(c).filter(|k| !k.is_quote()) {
                        if opts.has(k) {
                            stack.push((pos, k));
                        }
                    } else if let Some(k) = PairKind::from_close(c) {
                        if !opts.has(k) {
                            i += 1;
                            continue;
                        }
                        let floor = floors.last().map_or(0, |&f| f + 1);
                        match stack[floor..]
                            .iter()
                            .rposition(|(_, sk)| *sk == k)
                            .map(|si| si + floor)
                        {
                            Some(si) if si + 1 == stack.len() => {
                                let (o, _) = stack.pop().expect("top");
                                done.push((o, pos, k, stack.last().map(|s| s.0)));
                            }
                            Some(si) => {
                                // 사이에 닫히지 않은 열림들 = 짝 없음(문자열 안이면 조용히).
                                for (o, ok) in stack.drain(si + 1..) {
                                    if floor == 0 {
                                        unmatched.push((o, ok));
                                    }
                                }
                                let (o, _) = stack.pop().expect("top");
                                done.push((o, pos, k, stack.last().map(|s| s.0)));
                            }
                            None if floor == 0 => unmatched.push((pos, k)),
                            None => {}
                        }
                    }
                    i += 1;
                }
                ci = end;
            }
            // ★ 줄 끝 재동기화(fail-over · 사용자 09-23): 평문, 그리고 문자열이 줄을 넘지 않는 구문(`strings_span_lines` = false ·
            //   SQL 규격)에서는 줄 끝에 남은 열린 인용부호와 그 안을 버리고 다음 줄은 코드 층에서 다시 시작한다 — 짝 없는 `'` 하나가
            //   파일 끝까지 짝을 뒤집어 놓지 않는다(JetBrains 렉서·Sublime 구문의 "문자열은 줄 끝에서 끝난다" 규칙).
            if hl.is_none_or(|h| !h.strings_span_lines()) {
                if let Some(&f) = floors.first() {
                    stack.truncate(f);
                }
                floors.clear();
            }
            base += chars.len() + 1;
        }
        // 끝까지 안 닫힌 문자열과 그 안은 버리고, 코드 층에 남은 열림만 짝 없음.
        if let Some(&f) = floors.first() {
            stack.truncate(f);
        }
        for (o, k) in stack {
            unmatched.push((o, k));
        }
        done.sort_by_key(|d| d.0);
        let index_of = |open: usize| done.binary_search_by_key(&open, |d| d.0).ok();
        let mut pairs: Vec<Pair> = Vec::with_capacity(done.len());
        for d in &done {
            let parent = d.3.and_then(index_of).map(|i| i as u32);
            pairs.push(Pair {
                open: d.0,
                close: d.1,
                depth: 0,
                kind: d.2,
                parent,
            });
        }
        // 깊이 = 부모 사슬 길이(부모는 항상 앞 인덱스라 한 번에).
        for i in 0..pairs.len() {
            pairs[i].depth = match pairs[i].parent {
                Some(p) => pairs[p as usize].depth + 1,
                None => 0,
            };
        }
        let mut close_order: Vec<u32> = (0..pairs.len() as u32).collect();
        close_order.sort_by_key(|&i| pairs[i as usize].close);
        let mut marks: Vec<(usize, u16, bool)> =
            Vec::with_capacity(pairs.len() * 2 + unmatched.len());
        for p in &pairs {
            marks.push((p.open, p.depth, false));
            marks.push((p.close, p.depth, false));
        }
        for (pos, _) in &unmatched {
            marks.push((*pos, 0, true));
        }
        marks.sort_unstable();
        unmatched.sort_unstable();
        PairTable {
            pairs,
            close_order,
            unmatched,
            marks,
        }
    }

    /// `[from, to)` 안의 괄호 글자 표시(위치 · 깊이 · 짝 없음).
    #[must_use]
    pub fn marks_in(&self, from: usize, to: usize) -> &[(usize, u16, bool)] {
        let lo = self.marks.partition_point(|m| m.0 < from);
        let hi = self.marks.partition_point(|m| m.0 < to);
        &self.marks[lo..hi]
    }

    /// 캐럿 옆(바로 뒤 또는 바로 앞) 괄호의 쌍 인덱스.
    #[must_use]
    pub fn pair_at(&self, caret: usize) -> Option<usize> {
        for pos in [caret.checked_sub(1), Some(caret)].into_iter().flatten() {
            if let Ok(i) = self.pairs.binary_search_by_key(&pos, |p| p.open) {
                return Some(i);
            }
            if let Ok(k) = self
                .close_order
                .binary_search_by_key(&pos, |&i| self.pairs[i as usize].close)
            {
                return Some(self.close_order[k] as usize);
            }
        }
        None
    }

    /// 캐럿을 감싸는 가장 안쪽 쌍(`open < caret <= close`).
    #[must_use]
    pub fn enclosing(&self, caret: usize) -> Option<usize> {
        let hi = self.pairs.partition_point(|p| p.open < caret);
        (0..hi).rev().find(|&i| self.pairs[i].close >= caret)
    }

    #[must_use]
    pub fn parent(&self, i: usize) -> Option<usize> {
        self.pairs.get(i)?.parent.map(|p| p as usize)
    }

    /// 첫 자식(열림 순).
    #[must_use]
    pub fn first_child(&self, i: usize) -> Option<usize> {
        let close = self.pairs.get(i)?.close;
        (i + 1..self.pairs.len())
            .take_while(|&j| self.pairs[j].open < close)
            .find(|&j| self.pairs[j].parent == Some(i as u32))
    }

    /// 같은 부모 아래 다음/이전 형제.
    #[must_use]
    pub fn sibling(&self, i: usize, next: bool) -> Option<usize> {
        let p = self.pairs.get(i)?.parent;
        if next {
            (i + 1..self.pairs.len()).find(|&j| self.pairs[j].parent == p)
        } else {
            (0..i).rev().find(|&j| self.pairs[j].parent == p)
        }
    }

    #[must_use]
    pub fn get(&self, i: usize) -> Option<&Pair> {
        self.pairs.get(i)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 평문 스캔: 깊이 · 인용부호 쌍 · 짝 없음 · 조회(옆/감싸는/부모/자식/형제).
    #[test]
    fn scan_depth_quotes_unmatched_and_navigation() {
        //           0123456789012345678901234
        let text = "f(a, [b, 'x)'], {c}) )";
        let t = PairTable::build(text, None, PairOpts::default());
        let opens: Vec<(usize, u16)> = t.pairs.iter().map(|p| (p.open, p.depth)).collect();
        assert_eq!(
            opens,
            vec![(1, 0), (5, 1), (9, 2), (16, 1)],
            "열림 순 · 깊이"
        );
        assert_eq!(
            t.unmatched,
            vec![(21, PairKind::Round)],
            "마지막 `)`는 짝 없음"
        );
        assert_eq!(t.pair_at(2), Some(0), "( 뒤");
        assert_eq!(t.pair_at(20), Some(0), ") 앞");
        assert_eq!(t.enclosing(7), Some(1), "[ 안");
        assert_eq!(t.parent(1), Some(0));
        assert_eq!(t.first_child(0), Some(1));
        assert_eq!(t.sibling(1, true), Some(3), "대괄호 → 중괄호");
        assert_eq!(t.sibling(3, false), Some(1));
        assert_eq!(t.sibling(0, true), None);
        let m = t.marks_in(0, 6);
        assert_eq!(m.iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 5]);
        // 인용부호 안 `)`는 세지 않는다.
        assert!(t.pairs.iter().all(|p| p.open != 11));
        // 꺾쇠는 기본 제외
        let a = PairTable::build("a < b > c", None, PairOpts::default());
        assert!(a.is_empty() && a.unmatched.is_empty());
        let b = PairTable::build(
            "a < b > c",
            None,
            PairOpts {
                kinds: PairOpts::ALL,
                in_strings: true,
            },
        );
        assert_eq!(b.len(), 1);
    }

    /// 종류 지정(설정 문자열) — 고른 종류만 쌍 · 나머지 글자는 없는 것처럼.
    #[test]
    fn kinds_spec_selects_pairs() {
        assert_eq!(
            PairOpts::kinds_from_spec("() [] {} \"\" '' ``"),
            PairOpts::default().kinds
        );
        assert_eq!(
            PairOpts::kinds_from_spec("(),{}, <>"),
            PairKind::Round.bit() | PairKind::Curly.bit() | PairKind::Angle.bit()
        );
        assert_eq!(PairOpts::kinds_from_spec("zz"), 0);
        let only_round = PairOpts {
            kinds: PairKind::Round.bit(),
            in_strings: true,
        };
        let t = PairTable::build("f([a], 'b', {c})", None, only_round);
        assert_eq!(t.pairs.len(), 1);
        assert_eq!((t.pairs[0].open, t.pairs[0].close), (1, 15));
        assert!(
            t.unmatched.is_empty(),
            "고르지 않은 종류는 짝 없음으로도 세지 않는다"
        );
        assert!(!only_round.any_quote());
    }

    /// `in_strings` 끔 = 종전 동작(문자열은 양 끝 인용부호만 · 안쪽 괄호·인용부호 무시).
    #[test]
    fn in_strings_off_keeps_legacy_behaviour() {
        let text = "awk '{printf \"%s\", $1}' (x)";
        let t = PairTable::build(
            text,
            None,
            PairOpts {
                in_strings: false,
                ..PairOpts::default()
            },
        );
        let opens: Vec<usize> = t.pairs.iter().map(|p| p.open).collect();
        assert_eq!(opens, vec![4, 24], "'…' 와 바깥 ( ) 만");
        assert!(t.unmatched.is_empty());
    }

    /// 문자열 안의 괄호·다른 인용부호도 쌍(그 문자열 안에서만) · 안에 남은 짝 없음은 조용히(사용자 09-23).
    #[test]
    fn pairs_inside_strings_are_scoped_and_quiet() {
        //           0         1         2         3
        //           0123456789012345678901234567890123
        let text = "awk '{printf \"%s_%s\", $1}' x ( '(' )";
        let t = PairTable::build(text, None, PairOpts::default());
        let find = |open: usize| t.pairs.iter().find(|p| p.open == open).copied();
        let sq = find(4).expect("'…' 쌍");
        assert_eq!((sq.close, sq.kind), (25, PairKind::SQuote));
        let curly = find(5).expect("문자열 안 { }");
        assert_eq!(
            (curly.close, curly.parent),
            (24, Some(0)),
            "부모 = 인용부호 쌍"
        );
        let dq = find(13).expect("문자열 안 \" \"");
        assert_eq!((dq.close, dq.kind, dq.depth), (19, PairKind::DQuote, 2));
        // 바깥 ( … ) 는 문자열 안의 `(`와 짝짓지 않는다 · 안의 `(`는 짝 없음으로 표시하지 않는다.
        let round = find(29).expect("바깥 괄호");
        assert_eq!(round.close, 35);
        assert!(t.unmatched.is_empty(), "문자열 안의 짝 없음은 조용히");
        // 이스케이프 `\"`는 닫지 않는다.
        let e = PairTable::build("\"a\\\"b\" (", None, PairOpts::default());
        assert_eq!(e.pairs.len(), 1);
        assert_eq!((e.pairs[0].open, e.pairs[0].close), (0, 5));
        assert_eq!(e.unmatched, vec![(7, PairKind::Round)]);
    }

    /// SQL처럼 "같은 인용부호 두 번 = 이스케이프"인 구문에서는 문자열 안의 `''`가 시작/끝이 아니다(사용자 09-23
    /// "`'O''Neil'`의 `''`는 `'` 하나를 전달하는 이스케이프 · 쌍 대상에서 제외"). 빈 문자열 `''`과 `'a'''`(= a')는 그대로 쌍.
    #[test]
    fn doubled_quote_is_escape_in_sql() {
        let sql = crate::highlight::SyntaxSpec::sql();
        assert!(sql.doubled_quote_escapes());
        //          0         1         2
        //          012345678901234567890123456
        let text = "DEFINE who = 'O''Neil' ('')";
        let t = PairTable::build(text, Some(&sql), PairOpts::default());
        let quotes: Vec<(usize, usize)> = t
            .pairs
            .iter()
            .filter(|p| p.kind == PairKind::SQuote)
            .map(|p| (p.open, p.close))
            .collect();
        assert_eq!(
            quotes,
            vec![(13, 21), (24, 25)],
            "'O''Neil' 하나 · 빈 문자열 하나"
        );
        let round = t
            .pairs
            .iter()
            .find(|p| p.kind == PairKind::Round)
            .expect("( )");
        assert_eq!((round.open, round.close), (23, 26));
        // `'a'''` = a' — 끝의 `''`가 이스케이프고 마지막 `'`가 닫는다.
        let t = PairTable::build("x 'a''' y", Some(&sql), PairOpts::default());
        assert_eq!(t.pairs.len(), 1);
        assert_eq!((t.pairs[0].open, t.pairs[0].close), (2, 6));
        // 평문(구문 없음)은 종전대로 — `''`는 열고 닫는 빈 쌍.
        let p = PairTable::build("'O''Neil'", None, PairOpts::default());
        let opens: Vec<usize> = p.pairs.iter().map(|p| p.open).collect();
        assert_eq!(opens, vec![0, 3]);
    }

    /// `"O'Neil"` — SQL 규격에서 `"`도 문자열이라 안의 `'`는 그 문자열이 닫히며 조용히 버려진다 · 짝 없는 `'`가 남은 줄은 **줄 끝에서
    /// 재동기화**되어 다음 줄의 짝이 뒤집히지 않는다(사용자 09-23 "`'` 하나가 전체 짝 찾기를 오염" · fail-over).
    #[test]
    fn dq_string_apostrophe_and_eol_resync() {
        let sql = crate::highlight::SyntaxSpec::sql();
        assert!(!sql.strings_span_lines());
        //          0         1         2         3
        //          0123456789012345678901234567890123456
        let text = "DEFINE who = \"O'Neil\"\nSELECT 'a' || 'b';";
        let t = PairTable::build(text, Some(&sql), PairOpts::default());
        let q: Vec<(usize, usize, PairKind)> = t
            .pairs
            .iter()
            .filter(|p| p.kind.is_quote())
            .map(|p| (p.open, p.close, p.kind))
            .collect();
        assert_eq!(
            q,
            vec![
                (13, 20, PairKind::DQuote),
                (29, 31, PairKind::SQuote),
                (36, 38, PairKind::SQuote)
            ]
        );
        // 짝 없는 `'`(첫 줄) → 그 줄에서 버려지고 둘째 줄의 `'y'`·`( )`는 정상.
        let text2 = "x = 'broken\n('y')";
        let t = PairTable::build(text2, Some(&sql), PairOpts::default());
        let opens: Vec<(usize, PairKind)> = t.pairs.iter().map(|p| (p.open, p.kind)).collect();
        assert_eq!(
            opens,
            vec![(12, PairKind::Round), (13, PairKind::SQuote)],
            "둘째 줄부터 다시 코드 층"
        );
        assert!(
            t.unmatched.is_empty(),
            "버려진 `'`는 짝 없음으로도 세지 않는다"
        );
    }
}

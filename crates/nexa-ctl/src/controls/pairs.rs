//! **괄호·인용부호 쌍 표**(nexa-sql docs/51 §2·§8 — 사용자 09-17 "레인보우 괄호 · 짝/형제/상위/하위 이동 · 자동 닫기").
//!
//! 한 번 스캔(O(n))해 표를 만들고, 색칠·현재 쌍 강조·이동 명령·자동 닫기가 **같은 표**를 읽는다.
//! 강조기 토큰을 따라 걷는다: 주석은 건너뛰고, 문자열은 **양 끝 인용부호만** 쌍으로 등록(안쪽은 스캔 안 함).
//! 편집기 밖(UI 없음)이라 테스트가 쉽다.

use crate::highlight::{Highlighter, TokenKind};

/// 쌍 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
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

/// 스캔 옵션(설정 `rainbow.quotes` · `rainbow.angle`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PairOpts {
    pub quotes: bool,
    pub angle: bool,
}

impl Default for PairOpts {
    fn default() -> Self {
        PairOpts {
            quotes: true,
            angle: false,
        }
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
        // (열림, 종류, 깊이)
        let mut stack: Vec<(usize, PairKind)> = Vec::new();
        let mut done: Vec<(usize, usize, PairKind, Option<usize>)> = Vec::new(); // open, close, kind, parent_open
        let mut unmatched: Vec<(usize, PairKind)> = Vec::new();
        let mut state = 0u32;
        let mut spans: Vec<(usize, TokenKind)> = Vec::new();
        let mut base = 0usize;
        let mut in_plain_string: Option<(usize, char)> = None; // hl 없을 때 인용부호 추적
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
                match kind {
                    TokenKind::Comment => {}
                    TokenKind::Str => {
                        // 양 끝이 같은 인용부호면 쌍(안쪽은 보지 않는다).
                        if opts.quotes && end > ci + 1 {
                            let a = chars[ci];
                            let b = chars[end - 1];
                            if a == b {
                                if let Some(k) = PairKind::from_open(a).filter(|k| k.is_quote()) {
                                    done.push((
                                        base + ci,
                                        base + end - 1,
                                        k,
                                        stack.last().map(|s| s.0),
                                    ));
                                }
                            }
                        }
                    }
                    _ => {
                        let mut i = ci;
                        while i < end {
                            let c = chars[i];
                            let pos = base + i;
                            if hl.is_none() && opts.quotes {
                                // 평문 모드: 인용부호를 직접 짝짓는다(같은 줄 안에서만).
                                if let Some((qpos, q)) = in_plain_string {
                                    if c == q {
                                        if let Some(k) = PairKind::from_open(q) {
                                            done.push((qpos, pos, k, stack.last().map(|s| s.0)));
                                        }
                                        in_plain_string = None;
                                    }
                                    i += 1;
                                    continue;
                                }
                                if matches!(c, '"' | '\'' | '`') {
                                    in_plain_string = Some((pos, c));
                                    i += 1;
                                    continue;
                                }
                            }
                            if let Some(k) = PairKind::from_open(c).filter(|k| !k.is_quote()) {
                                if k != PairKind::Angle || opts.angle {
                                    stack.push((pos, k));
                                }
                            } else if let Some(k) = PairKind::from_close(c) {
                                if k == PairKind::Angle && !opts.angle {
                                    i += 1;
                                    continue;
                                }
                                match stack.iter().rposition(|(_, sk)| *sk == k) {
                                    Some(si) if si + 1 == stack.len() => {
                                        let (o, _) = stack.pop().expect("top");
                                        done.push((o, pos, k, stack.last().map(|s| s.0)));
                                    }
                                    Some(si) => {
                                        // 사이에 닫히지 않은 열림들 = 짝 없음.
                                        for (o, ok) in stack.drain(si + 1..) {
                                            unmatched.push((o, ok));
                                        }
                                        let (o, _) = stack.pop().expect("top");
                                        done.push((o, pos, k, stack.last().map(|s| s.0)));
                                    }
                                    None => unmatched.push((pos, k)),
                                }
                            }
                            i += 1;
                        }
                    }
                }
                ci = end;
            }
            in_plain_string = None; // 평문 문자열은 줄을 넘지 않는다
            base += chars.len() + 1;
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
                quotes: true,
                angle: true,
            },
        );
        assert_eq!(b.len(), 1);
    }
}

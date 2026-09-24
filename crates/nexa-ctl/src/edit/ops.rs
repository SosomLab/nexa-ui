//! Sublime Text식 **줄·선택 편집 명령**(nexa-sql T-98 · 사용자 09-16) — [`EditState`] 위의 순수 로직.
//!
//! 전부 **모든 구간(다중 커서)에 한 번에** 적용되고 되돌리기 1번이다. 줄 단위 명령은 구간이 걸친 줄 블록을 합쳐
//! (겹치거나 같은 줄이면 하나) 뒤에서 앞으로 처리해 앞 인덱스가 밀리지 않게 한다. 캐럿·앵커·추가 구간은 조각
//! 편집([`Piece`])의 길이 변화로 다시 매핑한다.

use super::{CharSeq, EditOp, EditState, TextBuf};

/// 편집 명령(키맵·메뉴·팔레트가 같은 어휘를 쓴다).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditCommand {
    /// 줄 복제(선택이 있으면 선택 텍스트를 바로 뒤에 복제 · `Ctrl+Shift+D`).
    DuplicateLines,
    /// 줄 삭제(`Ctrl+Shift+K`).
    DeleteLines,
    /// 다음 줄과 합치기(선택이 여러 줄이면 그 줄들 전부 · `Ctrl+J`).
    JoinLines,
    /// 줄을 위로(`Ctrl+Shift+↑`).
    SwapLinesUp,
    /// 줄을 아래로(`Ctrl+Shift+↓`).
    SwapLinesDown,
    /// 줄 주석 토글(`Ctrl+/` · 접두는 문법이 준다).
    ToggleComment,
    /// 블록 주석 토글(`Ctrl+Shift+/` · `/* … */` — 선택 영역, 없으면 현재 줄 · 이미 감싸져 있으면 벗김 · nexa-sql 09-24).
    ToggleBlockComment,
    /// 들여쓰기 한 단계(`Ctrl+]` · 여러 줄 선택 + Tab).
    Indent,
    /// 내어쓰기 한 단계(`Ctrl+[` · `Shift+Tab`).
    Unindent,
    /// 선택을 줄 전체로 확장(`Ctrl+L`).
    SelectLines,
    /// 선택을 줄마다 나눠 여러 선택으로(`Ctrl+Shift+L`).
    SplitIntoLines,
    /// 위 줄 같은 열에 캐럿 추가(`Ctrl+Alt+↑`).
    AddCaretUp,
    /// 아래 줄 같은 열에 캐럿 추가(`Ctrl+Alt+↓`).
    AddCaretDown,
    /// 대문자로(`Ctrl+K, Ctrl+U`).
    UpperCase,
    /// 소문자로(`Ctrl+K, Ctrl+L`).
    LowerCase,
}

/// 조각 편집 — `[pos, pos + del)`을 지우고 `ins`를 넣는다(서로 겹치지 않는다).
struct Piece {
    pos: usize,
    del: usize,
    ins: Vec<char>,
}

/// 줄 블록 `(첫 글자, 끝)` — 끝은 그 줄의 `'\n'` 위치(없으면 버퍼 길이) · 포함하지 않는다.
type Block = (usize, usize);

impl EditState {
    // ───────────────────────── 줄 도우미 ─────────────────────────

    /// `i`가 속한 줄의 시작.
    fn line_start_of(&self, i: usize) -> usize {
        self.buf.line_start_of(i)
    }

    /// `i`가 속한 줄의 끝(`'\n'` 위치 또는 버퍼 길이).
    fn line_end_of(&self, i: usize) -> usize {
        self.buf.line_end_of(i)
    }

    /// 구간이 걸친 줄 블록. 선택이 다음 줄 첫 칸에서 끝나면 그 줄은 넣지 않는다(Sublime).
    fn block_of(&self, a: usize, b: usize) -> Block {
        let (a, b) = (a.min(b), a.max(b));
        let end_ref = if b > a && b > 0 && self.buf.get(b - 1) == Some('\n') {
            b - 1
        } else {
            b
        };
        (self.line_start_of(a), self.line_end_of(end_ref))
    }

    /// 모든 구간의 줄 블록(겹치거나 맞닿으면 합침 · 오름차순).
    fn blocks(&self) -> Vec<Block> {
        let mut v: Vec<Block> = self
            .regions()
            .into_iter()
            .map(|(a, b)| self.block_of(a, b))
            .collect();
        v.sort_unstable();
        let mut out: Vec<Block> = Vec::with_capacity(v.len());
        for (s, e) in v {
            match out.last_mut() {
                Some(last) if s <= last.1 => last.1 = last.1.max(e),
                _ => out.push((s, e)),
            }
        }
        out
    }

    /// 블록 안의 줄들 `(시작, 끝)` 목록.
    fn lines_in(&self, (s, e): Block) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut ls = s;
        loop {
            let le = self.line_end_of(ls);
            out.push((ls, le));
            if le >= e || le >= self.buf.len() {
                break;
            }
            ls = le + 1;
        }
        out
    }

    /// 조각 편집 일괄 적용 — 되돌리기 1번 · 캐럿/앵커/추가 구간을 새 위치로 매핑한다.
    /// 매핑 규칙: 조각 앞의 점은 길이 변화만큼 밀리고, 조각 **안**의 점은 삽입 길이 안에서 같은 오프셋을 지키며
    /// (삭제뿐이면 조각 시작으로), 순수 삽입의 시작점에 있는 점은 밀리지 않는다(삽입은 그 뒤에 들어간다).
    fn apply_pieces(&mut self, mut pieces: Vec<Piece>) -> bool {
        pieces.retain(|p| p.del > 0 || !p.ins.is_empty());
        if pieces.is_empty() {
            return false;
        }
        pieces.sort_by_key(|p| p.pos);
        let map = |pos: usize| -> usize {
            let mut delta: isize = 0;
            for p in &pieces {
                let end = p.pos + p.del;
                if end < pos || (end == pos && p.del > 0) {
                    delta += p.ins.len() as isize - p.del as isize;
                } else if p.pos < pos {
                    let off = (pos - p.pos).min(p.ins.len());
                    return ((p.pos + off) as isize + delta).max(0) as usize;
                } else {
                    break;
                }
            }
            (pos as isize + delta).max(0) as usize
        };
        let caret = map(self.caret);
        let anchor = self.anchor.map(map);
        let extra: Vec<(usize, usize)> =
            self.extra.iter().map(|&(a, c)| (map(a), map(c))).collect();
        self.record(EditOp::Other, true);
        self.splice_pieces(&pieces);
        let n = self.buf.len();
        self.caret = caret.min(n);
        self.anchor = anchor.map(|a| a.min(n)).filter(|&a| a != self.caret);
        self.extra = extra
            .into_iter()
            .map(|(a, c)| (a.min(n), c.min(n)))
            .collect();
        self.last_op = None;
        true
    }

    /// 명령 실행 — 버퍼나 선택이 바뀌었으면 `true`.
    /// `indent_unit` = 한 단계 들여쓰기 문자열(`"\t"` 또는 공백들) · `tab_size` = 내어쓰기 때 지울 공백 상한 ·
    /// `comment` = 줄 주석 접두(없으면 주석 토글은 아무것도 안 한다).
    pub fn command(
        &mut self,
        cmd: EditCommand,
        indent_unit: &str,
        tab_size: usize,
        comment: Option<&str>,
    ) -> bool {
        // 본문을 바꾸는 명령이 거대 선택 위에서 돌면 그만큼을 기록해야 한다 — 두 번 눌러야 한다(D-130).
        let mutates = !matches!(
            cmd,
            EditCommand::SelectLines
                | EditCommand::SplitIntoLines
                | EditCommand::AddCaretUp
                | EditCommand::AddCaretDown
        );
        if mutates {
            let n = self.selected_bytes();
            if self.giant_refused(n, true) {
                return false;
            }
        }
        let changed = self.command_checked(cmd, indent_unit, tab_size, comment);
        self.giant_done();
        changed
    }

    fn command_checked(
        &mut self,
        cmd: EditCommand,
        indent_unit: &str,
        tab_size: usize,
        comment: Option<&str>,
    ) -> bool {
        match cmd {
            EditCommand::DuplicateLines => self.duplicate_lines(),
            EditCommand::DeleteLines => self.delete_lines(),
            EditCommand::JoinLines => self.join_lines(),
            EditCommand::SwapLinesUp => self.swap_lines(true),
            EditCommand::SwapLinesDown => self.swap_lines(false),
            EditCommand::ToggleComment => comment.is_some_and(|c| self.toggle_comment(c)),
            EditCommand::ToggleBlockComment => self.toggle_block_comment("/*", "*/"),
            EditCommand::Indent => self.indent_lines(indent_unit),
            EditCommand::Unindent => self.unindent_lines(tab_size),
            EditCommand::SelectLines => self.select_lines(),
            EditCommand::SplitIntoLines => self.split_into_lines(),
            EditCommand::AddCaretUp => self.add_caret_vert(true),
            EditCommand::AddCaretDown => self.add_caret_vert(false),
            EditCommand::UpperCase => self.transform_case(true),
            EditCommand::LowerCase => self.transform_case(false),
        }
    }

    /// 주 선택이 여러 줄에 걸치는가(편집기의 Tab = 들여쓰기 판정).
    #[must_use]
    pub fn selection_spans_lines(&self) -> bool {
        self.selection()
            .is_some_and(|(a, b)| self.buf.contains_in(a, b, '\n'))
    }

    /// `n`번째 줄(1 기준)의 시작 인덱스(넘치면 마지막 줄).
    #[must_use]
    pub fn line_start_index(&self, n: usize) -> usize {
        let last = self.buf.line_count().saturating_sub(1);
        self.buf.line_start((n.max(1) - 1).min(last))
    }

    // ───────────────────────── 명령 ─────────────────────────

    /// 조각 목록(원래 좌표 · 오름차순 · 비겹침)을 **한 번 훑어** 적용한다 — 본문 변경의 단일 통로(`replace_many_inner`)를
    /// 타므로 되돌리기 기록·세대(`rev`) 갱신이 함께 된다(종전 = 조각마다 `splice` · 세대를 올리지 않았다).
    fn splice_pieces(&mut self, pieces: &[Piece]) {
        let texts: Vec<String> = pieces.iter().map(|p| p.ins.iter().collect()).collect();
        let edits: Vec<(usize, usize, &str)> = pieces
            .iter()
            .zip(&texts)
            .map(|(p, t)| (p.pos, p.pos + p.del, t.as_str()))
            .collect();
        self.replace_many_inner(&edits);
    }

    fn duplicate_lines(&mut self) -> bool {
        let regions = self.regions();
        if regions.iter().all(|(a, b)| a == b) {
            // 캐럿만: 블록을 그 위에 복사(캐럿은 길이만큼 밀려 아래쪽 = 새 사본에 남는다 · Sublime).
            let blocks = self.blocks();
            let mut pieces = Vec::new();
            let mut shifts: Vec<(usize, usize, usize)> = Vec::new();
            for (s, e) in blocks {
                let mut ins: Vec<char> = self.buf.slice_vec(s, e);
                ins.push('\n');
                shifts.push((s, e, ins.len()));
                pieces.push(Piece {
                    pos: s,
                    del: 0,
                    ins,
                });
            }
            let shift = |pos: usize| -> usize {
                let mut d = 0usize;
                for &(s, e, len) in &shifts {
                    if pos >= s && pos <= e {
                        d += len;
                        break;
                    }
                    if pos > e {
                        d += len;
                    }
                }
                pos + d
            };
            let caret = shift(self.caret);
            let extra: Vec<(usize, usize)> = self
                .extra
                .iter()
                .map(|&(a, c)| (shift(a), shift(c)))
                .collect();
            self.record(EditOp::Other, true);
            self.splice_pieces(&pieces);
            self.caret = caret;
            self.anchor = None;
            self.extra = extra;
            self.last_op = None;
            return true;
        }
        // 선택: 각 선택 텍스트를 바로 뒤에 복제하고 새 사본을 선택한다.
        let mut pieces = Vec::new();
        for &(a, b) in &regions {
            if b > a {
                pieces.push(Piece {
                    pos: b,
                    del: 0,
                    ins: self.buf.slice_vec(a, b),
                });
            }
        }
        self.record(EditOp::Other, true);
        let mut delta = 0usize;
        let mut new_regions: Vec<(usize, usize)> = Vec::with_capacity(regions.len());
        for &(a, b) in &regions {
            let len = b - a;
            if len == 0 {
                new_regions.push((a + delta, a + delta));
                continue;
            }
            let ins_at = b + delta;
            new_regions.push((ins_at, ins_at + len));
            delta += len;
        }
        self.splice_pieces(&pieces);
        self.set_regions(&new_regions);
        true
    }

    fn delete_lines(&mut self) -> bool {
        let blocks = self.blocks();
        let mut pieces = Vec::new();
        for (s, e) in blocks {
            // 뒤의 '\n'까지 · 마지막 줄이면 앞의 '\n'을 함께.
            let (pos, end) = if e < self.buf.len() {
                (s, e + 1)
            } else if s > 0 {
                (s - 1, e)
            } else {
                (s, e)
            };
            pieces.push(Piece {
                pos,
                del: end - pos,
                ins: Vec::new(),
            });
        }
        if !self.apply_pieces(pieces) {
            return false;
        }
        // 선택은 접힌다(줄이 사라졌으니) — 캐럿만 남긴다.
        self.anchor = None;
        self.extra = self.extra.iter().map(|&(_, c)| (c, c)).collect();
        self.extra.dedup();
        true
    }

    fn join_lines(&mut self) -> bool {
        let mut pieces = Vec::new();
        let mut seen: Vec<usize> = Vec::new();
        for (a, b) in self.regions() {
            let (s, e) = self.block_of(a, b);
            // 한 줄(또는 캐럿)이면 다음 줄과 · 여러 줄이면 그 안의 줄바꿈 전부.
            let mut nls: Vec<usize> = self
                .buf
                .iter_from(s)
                .take(e - s)
                .enumerate()
                .filter(|(_, c)| *c == '\n')
                .map(|(i, _)| s + i)
                .collect();
            if nls.is_empty() && e < self.buf.len() {
                nls.push(e);
            }
            for nl in nls {
                if seen.contains(&nl) {
                    continue;
                }
                seen.push(nl);
                let ws = self
                    .buf
                    .iter_from(nl + 1)
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .count();
                // 앞 줄 끝이 이미 공백이면 공백을 더하지 않는다.
                let prev_ws = nl > 0 && matches!(self.buf.at(nl - 1), ' ' | '\t');
                let next_empty = nl + 1 + ws >= self.buf.len() || self.buf.at(nl + 1 + ws) == '\n';
                let ins = if prev_ws || next_empty {
                    Vec::new()
                } else {
                    vec![' ']
                };
                pieces.push(Piece {
                    pos: nl,
                    del: 1 + ws,
                    ins,
                });
            }
        }
        self.apply_pieces(pieces)
    }

    fn swap_lines(&mut self, up: bool) -> bool {
        let mut blocks = self.blocks();
        if blocks.is_empty() {
            return false;
        }
        if up {
            if blocks[0].0 == 0 {
                return false;
            }
        } else if blocks.last().is_some_and(|b| b.1 >= self.buf.len()) {
            return false;
        }
        if !up {
            blocks.reverse();
        }
        self.record(EditOp::Other, true);
        for (s, e) in blocks {
            let block: Vec<char> = self.buf.slice_vec(s, e);
            if up {
                let ps = self.line_start_of(s - 1);
                let prev: Vec<char> = self.buf.slice_vec(ps, s - 1);
                let mut ins = block.clone();
                ins.push('\n');
                ins.extend_from_slice(&prev);
                self.splice_rec(ps, e - ps, &ins);
                let d = prev.len() + 1;
                self.shift_points(s, e, -(d as isize));
            } else {
                let ne = self.line_end_of(e + 1);
                let next: Vec<char> = self.buf.slice_vec(e + 1, ne);
                let mut ins = next.clone();
                ins.push('\n');
                ins.extend_from_slice(&block);
                self.splice_rec(s, ne - s, &ins);
                let d = next.len() + 1;
                self.shift_points(s, e, d as isize);
            }
        }
        self.last_op = None;
        true
    }

    /// `[s, e]` 안의 점들을 `d`만큼 옮긴다(줄 이동).
    fn shift_points(&mut self, s: usize, e: usize, d: isize) {
        let mv = |p: usize| -> usize {
            if p >= s && p <= e {
                (p as isize + d).max(0) as usize
            } else {
                p
            }
        };
        self.caret = mv(self.caret);
        self.anchor = self.anchor.map(mv);
        self.extra = self.extra.iter().map(|&(a, c)| (mv(a), mv(c))).collect();
    }

    fn toggle_comment(&mut self, prefix: &str) -> bool {
        let pre: Vec<char> = prefix.chars().collect();
        if pre.is_empty() {
            return false;
        }
        let lines: Vec<(usize, usize)> = self
            .blocks()
            .into_iter()
            .flat_map(|b| self.lines_in(b))
            .collect();
        let indent_of = |buf: &TextBuf, s: usize, e: usize| -> usize {
            buf.iter_from(s)
                .take(e - s)
                .take_while(|c| *c == ' ' || *c == '\t')
                .count()
        };
        let non_blank: Vec<(usize, usize)> = lines
            .iter()
            .copied()
            .filter(|&(s, e)| indent_of(&self.buf, s, e) < e - s)
            .collect();
        if non_blank.is_empty() {
            return false;
        }
        let all_commented = non_blank.iter().all(|&(s, e)| {
            let i = s + indent_of(&self.buf, s, e);
            i + pre.len() <= e && self.buf.starts_with_at(i, &pre)
        });
        let mut pieces = Vec::new();
        if all_commented {
            for &(s, e) in &non_blank {
                let i = s + indent_of(&self.buf, s, e);
                let mut del = pre.len();
                if self.buf.get(i + del) == Some(' ') {
                    del += 1;
                }
                pieces.push(Piece {
                    pos: i,
                    del,
                    ins: Vec::new(),
                });
            }
        } else {
            let col = non_blank
                .iter()
                .map(|&(s, e)| indent_of(&self.buf, s, e))
                .min()
                .unwrap_or(0);
            let mut ins = pre;
            ins.push(' ');
            for &(s, _) in &non_blank {
                pieces.push(Piece {
                    pos: s + col,
                    del: 0,
                    ins: ins.clone(),
                });
            }
        }
        self.apply_pieces(pieces)
    }

    /// 블록 주석 토글: 영역마다(캐럿만이면 그 줄의 본문) 앞뒤 공백을 뺀 내용이 `open … close`로 감싸져 있으면 벗기고, 아니면
    /// `open ` + 내용 + ` close`로 감싼다. 영역이 여럿이면 각각.
    fn toggle_block_comment(&mut self, open: &str, close: &str) -> bool {
        let o: Vec<char> = open.chars().collect();
        let c: Vec<char> = close.chars().collect();
        if o.is_empty() || c.is_empty() {
            return false;
        }
        let ws = |ch: Option<char>| matches!(ch, Some(' ') | Some('\t') | Some('\n') | Some('\r'));
        let mut pieces = Vec::new();
        // 단일 선택이면 토글 뒤 선택을 감싼/벗긴 범위로 맞춘다(다음 토글이 되돌리게 · Sublime).
        let regions = self.regions();
        let mut resel: Option<(usize, usize)> = None;
        for (a, b) in regions.iter().copied() {
            // 캐럿만 = 그 줄(들여쓰기 뒤 본문).
            let (mut s, mut e) = if a == b {
                let line = self.buf.line_of(a);
                let ls = self.buf.line_start(line);
                let le = if line + 1 < self.buf.line_count() {
                    self.buf.line_start(line + 1) - 1
                } else {
                    self.buf.len()
                };
                (ls, le)
            } else {
                (a, b)
            };
            // 앞뒤 공백은 밖에 둔다.
            while s < e && ws(self.buf.get(s)) {
                s += 1;
            }
            while e > s && ws(self.buf.get(e - 1)) {
                e -= 1;
            }
            if s >= e {
                continue;
            }
            let wrapped = e - s >= o.len() + c.len()
                && self.buf.starts_with_at(s, &o)
                && self.buf.starts_with_at(e - c.len(), &c);
            if wrapped {
                let mut del_o = o.len();
                if ws(self.buf.get(s + del_o)) && s + del_o < e - c.len() {
                    del_o += 1;
                }
                let mut ce = e - c.len();
                let mut del_c = c.len();
                if ce > s + del_o && ws(self.buf.get(ce - 1)) {
                    ce -= 1;
                    del_c += 1;
                }
                pieces.push(Piece {
                    pos: s,
                    del: del_o,
                    ins: Vec::new(),
                });
                pieces.push(Piece {
                    pos: ce,
                    del: del_c,
                    ins: Vec::new(),
                });
                if regions.len() == 1 && a != b {
                    resel = Some((s, e - del_o - del_c));
                }
            } else {
                let mut ins_o = o.clone();
                ins_o.push(' ');
                let mut ins_c = vec![' '];
                ins_c.extend(c.iter().copied());
                pieces.push(Piece {
                    pos: s,
                    del: 0,
                    ins: ins_o,
                });
                pieces.push(Piece {
                    pos: e,
                    del: 0,
                    ins: ins_c,
                });
                if regions.len() == 1 && a != b {
                    resel = Some((s, e + o.len() + 1 + 1 + c.len()));
                }
            }
        }
        if pieces.is_empty() {
            return false;
        }
        let ok = self.apply_pieces(pieces);
        if let (true, Some((s, e))) = (ok, resel) {
            self.set_selection(s, e);
        }
        ok
    }

    fn indent_lines(&mut self, unit: &str) -> bool {
        let ins: Vec<char> = unit.chars().collect();
        if ins.is_empty() {
            return false;
        }
        let pieces: Vec<Piece> = self
            .blocks()
            .into_iter()
            .flat_map(|b| self.lines_in(b))
            .filter(|&(s, e)| e > s)
            .map(|(s, _)| Piece {
                pos: s,
                del: 0,
                ins: ins.clone(),
            })
            .collect();
        // 줄 시작에 있는 앵커/캐럿도 함께 밀려야 한다(순수 삽입 규칙 예외) — 블록 첫 줄 시작의 선택 시작점.
        let starts: Vec<usize> = pieces.iter().map(|p| p.pos).collect();
        let ok = self.apply_pieces(pieces);
        if ok {
            let n = ins.len();
            let bump = |p: usize| -> usize {
                // 밀리지 않은(= 여전히 옛 줄 시작에 있는) 점을 들여쓰기 뒤로.
                let mut shifted = 0usize;
                for &s in &starts {
                    let s2 = s + shifted;
                    if p == s2 {
                        return p + n;
                    }
                    if p < s2 {
                        break;
                    }
                    shifted += n;
                }
                p
            };
            let (c, a) = (bump(self.caret), self.anchor.map(bump));
            self.caret = c;
            self.anchor = a;
            self.extra = self
                .extra
                .iter()
                .map(|&(a, c)| (bump(a), bump(c)))
                .collect();
        }
        ok
    }

    fn unindent_lines(&mut self, tab_size: usize) -> bool {
        let ts = tab_size.max(1);
        let pieces: Vec<Piece> = self
            .blocks()
            .into_iter()
            .flat_map(|b| self.lines_in(b))
            .filter_map(|(s, e)| {
                let del = if self.buf.get(s) == Some('\t') {
                    1
                } else {
                    self.buf
                        .iter_from(s)
                        .take(e - s)
                        .take_while(|c| *c == ' ')
                        .count()
                        .min(ts)
                };
                (del > 0).then_some(Piece {
                    pos: s,
                    del,
                    ins: Vec::new(),
                })
            })
            .collect();
        self.apply_pieces(pieces)
    }

    fn select_lines(&mut self) -> bool {
        let regions: Vec<(usize, usize)> = self
            .regions()
            .into_iter()
            .map(|(a, b)| {
                let (s, e) = self.block_of(a, b);
                (s, (e + 1).min(self.buf.len()))
            })
            .collect();
        // 겹치는 줄 선택은 합친다.
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (s, e) in regions {
            match merged.last_mut() {
                Some(l) if s <= l.1 => l.1 = l.1.max(e),
                _ => merged.push((s, e)),
            }
        }
        self.set_regions(&merged);
        true
    }

    fn split_into_lines(&mut self) -> bool {
        let mut out: Vec<(usize, usize)> = Vec::new();
        for (a, b) in self.regions() {
            if a == b {
                out.push((a, a));
                continue;
            }
            let mut s = a;
            while s < b {
                let e = self.line_end_of(s).min(b);
                out.push((s, e));
                s = e + 1;
            }
        }
        if out.is_empty() {
            return false;
        }
        self.set_regions(&out);
        true
    }

    fn add_caret_vert(&mut self, up: bool) -> bool {
        // 가장 위/아래 캐럿에서 한 줄 더(Sublime은 마지막에 추가한 방향으로 늘린다).
        let carets = self.carets();
        let Some(&from) = (if up { carets.first() } else { carets.last() }) else {
            return false;
        };
        let ls = self.line_start_of(from);
        // 목표 열 = 캐럿들 중 가장 넓은 열(짧은 줄에서 줄 끝으로 잘린 캐럿을 지나도 원래 열을 기억하는 Sublime 동작).
        let col = carets
            .iter()
            .map(|&c| c - self.line_start_of(c))
            .max()
            .unwrap_or(from - ls);
        let target = if up {
            if ls == 0 {
                return false;
            }
            let ps = self.line_start_of(ls - 1);
            (ps + col).min(ls - 1)
        } else {
            let le = self.line_end_of(from);
            if le >= self.buf.len() {
                return false;
            }
            let ns = le + 1;
            (ns + col).min(self.line_end_of(ns))
        };
        if self.regions().contains(&(target, target)) {
            return false;
        }
        // 지금 캐럿(빈 것이라도)을 보존하고 새 캐럿을 주 캐럿으로(`add_selection`은 빈 주 캐럿을 버린다).
        let cur = (self.anchor.unwrap_or(self.caret), self.caret);
        self.extra.push(cur);
        self.anchor = None;
        self.caret = target;
        self.last_op = None;
        true
    }

    fn transform_case(&mut self, upper: bool) -> bool {
        let mut pieces = Vec::new();
        for (a, b) in self.regions() {
            let (a, b) = if a == b { self.word_bounds(a) } else { (a, b) };
            if b <= a {
                continue;
            }
            let src: String = self.buf.slice_string(a, b);
            let dst = if upper {
                src.to_uppercase()
            } else {
                src.to_lowercase()
            };
            if dst != src {
                pieces.push(Piece {
                    pos: a,
                    del: b - a,
                    ins: dst.chars().collect(),
                });
            }
        }
        self.apply_pieces(pieces)
    }

    /// 캐럿 밑 단어 경계(글자·숫자·`_`).
    fn word_bounds(&self, i: usize) -> (usize, usize) {
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let n = self.buf.len();
        let i = i.min(n);
        let mut a = i;
        while a > 0 && is_word(self.buf.at(a - 1)) {
            a -= 1;
        }
        let mut b = i;
        while b < n && is_word(self.buf.at(b)) {
            b += 1;
        }
        (a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(text: &str, caret: usize) -> EditState {
        let mut e = EditState::with_text(text, false);
        e.set_caret(caret, false);
        e
    }

    fn run(e: &mut EditState, c: EditCommand) -> bool {
        e.command(c, "    ", 4, Some("--"))
    }

    #[test]
    fn duplicate_line_and_selection() {
        let mut e = st("ab\ncd\nef", 4); // 'd' 앞
        assert!(run(&mut e, EditCommand::DuplicateLines));
        assert_eq!(e.text(), "ab\ncd\ncd\nef");
        assert_eq!(e.caret(), 7, "캐럿은 새 사본(아래쪽)에 남는다");
        // 마지막 줄(끝 '\n' 없음)도 된다.
        let mut e = st("ab\ncd", 5);
        assert!(run(&mut e, EditCommand::DuplicateLines));
        assert_eq!(e.text(), "ab\ncd\ncd");
        assert_eq!(e.caret(), 8);
        // 선택 복제 = 선택 바로 뒤 · 새 사본이 선택된다.
        let mut e = EditState::with_text("hello world", false);
        e.set_selection(0, 5);
        assert!(run(&mut e, EditCommand::DuplicateLines));
        assert_eq!(e.text(), "hellohello world");
        assert_eq!(e.selection(), Some((5, 10)));
        // 되돌리기 1번.
        assert!(e.undo());
        assert_eq!(e.text(), "hello world");
    }

    #[test]
    fn delete_lines_including_last_and_multi() {
        let mut e = st("a\nb\nc", 2);
        assert!(run(&mut e, EditCommand::DeleteLines));
        assert_eq!(e.text(), "a\nc");
        assert_eq!(e.caret(), 2, "다음 줄 시작");
        let mut e = st("a\nb\nc", 5);
        assert!(run(&mut e, EditCommand::DeleteLines));
        assert_eq!(e.text(), "a\nb", "마지막 줄은 앞 '\\n'과 함께");
        // 다중 캐럿: 두 줄 동시에.
        let mut e = st("a\nb\nc\nd", 0);
        e.set_regions(&[(0, 0), (4, 4)]);
        assert!(run(&mut e, EditCommand::DeleteLines));
        assert_eq!(e.text(), "b\nd");
        assert_eq!(e.carets(), vec![0, 2]);
    }

    #[test]
    fn join_lines_trims_leading_ws() {
        let mut e = st("select\n    a,\n    b", 0);
        assert!(run(&mut e, EditCommand::JoinLines));
        assert_eq!(e.text(), "select a,\n    b");
        // 여러 줄 선택이면 안의 줄바꿈 전부.
        let mut e = EditState::with_text("x\n  y\n  z\nw", false);
        e.set_selection(0, 8);
        assert!(run(&mut e, EditCommand::JoinLines));
        assert_eq!(e.text(), "x y z\nw");
        assert_eq!(
            e.selection(),
            Some((0, 4)),
            "선택 끝(z 앞)은 줄어든 길이만큼 당겨진다"
        );
    }

    #[test]
    fn swap_lines_up_down_moves_caret_with_line() {
        let mut e = st("1\n22\n333", 3); // '22' 안(인덱스 3)
        assert!(run(&mut e, EditCommand::SwapLinesUp));
        assert_eq!(e.text(), "22\n1\n333");
        assert_eq!(e.caret(), 1);
        assert!(!run(&mut e, EditCommand::SwapLinesUp), "맨 위는 못 올린다");
        assert!(run(&mut e, EditCommand::SwapLinesDown));
        assert_eq!(e.text(), "1\n22\n333");
        assert_eq!(e.caret(), 3);
        assert!(run(&mut e, EditCommand::SwapLinesDown));
        assert_eq!(e.text(), "1\n333\n22");
        assert_eq!(e.caret(), 7);
        assert!(
            !run(&mut e, EditCommand::SwapLinesDown),
            "맨 아래는 못 내린다"
        );
        // 여러 줄 선택 블록째 이동.
        let mut e = EditState::with_text("a\nb\nc\nd", false);
        e.set_selection(2, 5); // b..c
        assert!(run(&mut e, EditCommand::SwapLinesDown));
        assert_eq!(e.text(), "a\nd\nb\nc");
        assert_eq!(e.selection(), Some((4, 7)));
    }

    /// 블록 주석 자동 점검(nexa-sql 사용자 09-24 "로직 확인 자동화"): 여러 줄 선택 · 탭 들여쓰기 · 여러 영역 · 왕복 3회 · 부분 선택(중첩) ·
    /// 끝 줄바꿈 포함 선택 · 빈 선택 여러 줄 · 이미 감싼 안쪽 공백 없는 꼴(`/*x*/`)도 벗김.
    #[test]
    fn toggle_block_comment_matrix() {
        // 여러 줄 선택(탭 들여쓰기 · 선택이 줄바꿈까지 포함) — 앞뒤 공백은 밖.
        let src = "SELECT\n\tA.*\nFROM\n\tT A\n";
        let mut e = EditState::with_text(src, false);
        e.set_selection(0, src.len());
        assert!(run(&mut e, EditCommand::ToggleBlockComment));
        assert_eq!(e.text(), "/* SELECT\n\tA.*\nFROM\n\tT A */\n");
        for _ in 0..3 {
            assert!(run(&mut e, EditCommand::ToggleBlockComment));
            assert_eq!(e.text(), src, "왕복 = 원문");
            assert!(run(&mut e, EditCommand::ToggleBlockComment));
        }
        assert_eq!(e.text(), "/* SELECT\n\tA.*\nFROM\n\tT A */\n");
        // 여러 영역(캐럿 둘 · Ctrl+Alt+↓) = 각 줄을 따로.
        let mut e = EditState::with_text("a = 1\nb = 2", false);
        e.set_caret(1, false);
        assert!(run(&mut e, EditCommand::AddCaretDown));
        assert!(run(&mut e, EditCommand::ToggleBlockComment));
        assert_eq!(e.text(), "/* a = 1 */\n/* b = 2 */");
        // 부분 선택은 그 조각만(중첩 허용 — SQL 블록 주석은 중첩되지 않으므로 사용자가 판단).
        let mut e = EditState::with_text("x = 1 + 2", false);
        e.set_selection(4, 9);
        assert!(run(&mut e, EditCommand::ToggleBlockComment));
        assert_eq!(e.text(), "x = /* 1 + 2 */");
        // 안쪽 공백 없는 기존 주석도 벗긴다.
        let mut e = EditState::with_text("/*x*/", false);
        e.set_selection(0, 5);
        assert!(run(&mut e, EditCommand::ToggleBlockComment));
        assert_eq!(e.text(), "x");
        // 공백만 선택 = 아무것도 안 함.
        let mut e = EditState::with_text("a\n   \nb", false);
        e.set_selection(2, 5);
        assert!(!run(&mut e, EditCommand::ToggleBlockComment));
        assert_eq!(e.text(), "a\n   \nb");
    }

    /// 블록 주석(nexa-sql 09-24 `Ctrl+Shift+/`): 선택 = 감싸기 → 다시 = 벗기기 · 캐럿만 = 그 줄 본문 · 앞뒤 공백은 밖.
    #[test]
    fn toggle_block_comment_wrap_and_unwrap() {
        let mut e = EditState::with_text(
            "SELECT 1
FROM dual",
            false,
        );
        e.set_selection(0, 8);
        assert!(run(&mut e, EditCommand::ToggleBlockComment));
        assert_eq!(
            e.text(),
            "/* SELECT 1 */
FROM dual"
        );
        assert!(run(&mut e, EditCommand::ToggleBlockComment));
        assert_eq!(
            e.text(),
            "SELECT 1
FROM dual",
            "다시 = 벗김"
        );
        let mut e = st("  x = 1  ", 3);
        assert!(run(&mut e, EditCommand::ToggleBlockComment));
        assert_eq!(
            e.text(),
            "  /* x = 1 */  ",
            "캐럿만 = 그 줄 본문 · 공백은 밖"
        );
        assert!(run(&mut e, EditCommand::ToggleBlockComment));
        assert_eq!(e.text(), "  x = 1  ");
        let mut e = st("   ", 1);
        assert!(
            !run(&mut e, EditCommand::ToggleBlockComment),
            "빈 줄은 없음"
        );
    }

    #[test]
    fn toggle_comment_add_remove_min_indent() {
        let mut e = EditState::with_text("  a\n    b\n\n  c", false);
        e.set_selection(0, 14);
        assert!(run(&mut e, EditCommand::ToggleComment));
        assert_eq!(
            e.text(),
            "  -- a\n  --   b\n\n  -- c",
            "최소 들여쓰기 열에 · 빈 줄 건너뜀"
        );
        assert!(run(&mut e, EditCommand::ToggleComment));
        assert_eq!(e.text(), "  a\n    b\n\n  c", "전부 주석이면 제거");
        // 캐럿만: 그 줄.
        let mut e = st("x = 1", 2);
        assert!(run(&mut e, EditCommand::ToggleComment));
        assert_eq!(e.text(), "-- x = 1");
        assert_eq!(e.caret(), 5);
        assert!(run(&mut e, EditCommand::ToggleComment));
        assert_eq!(e.text(), "x = 1");
        assert_eq!(e.caret(), 2);
    }

    #[test]
    fn indent_unindent_blocks() {
        let mut e = EditState::with_text("a\nb\n\nc", false);
        e.set_selection(0, 4);
        assert!(run(&mut e, EditCommand::Indent));
        assert_eq!(e.text(), "    a\n    b\n\nc", "빈 줄은 건너뜀");
        assert_eq!(e.selection(), Some((4, 12)), "선택 시작은 들여쓰기 뒤로");
        assert!(run(&mut e, EditCommand::Unindent));
        assert_eq!(e.text(), "a\nb\n\nc");
        // 탭 들여쓰기 · 탭 내어쓰기 · 공백 2칸만 있으면 2칸만.
        let mut e = st("\tx\n  y", 0);
        e.set_selection(0, 5);
        assert!(run(&mut e, EditCommand::Unindent));
        assert_eq!(e.text(), "x\ny");
        assert!(
            !run(&mut e, EditCommand::Unindent),
            "더 뺄 것이 없으면 false"
        );
        let mut e = st("x", 1);
        assert!(e.command(EditCommand::Indent, "\t", 4, None));
        assert_eq!(e.text(), "\tx");
        assert_eq!(e.caret(), 2);
    }

    #[test]
    fn select_lines_and_split() {
        let mut e = st("ab\ncd\nef", 4);
        assert!(run(&mut e, EditCommand::SelectLines));
        assert_eq!(e.selection(), Some((3, 6)), "줄 전체 + 줄바꿈");
        assert!(run(&mut e, EditCommand::SelectLines));
        assert_eq!(
            e.selection(),
            Some((3, 6)),
            "다음 줄 첫 칸에서 끝나는 선택은 그 줄을 안 넣는다"
        );
        let mut e = EditState::with_text("ab\ncd\nef", false);
        e.set_selection(1, 7);
        assert!(run(&mut e, EditCommand::SplitIntoLines));
        assert_eq!(e.regions(), vec![(1, 2), (3, 5), (6, 7)]);
        assert!(e.has_multi());
    }

    #[test]
    fn add_caret_vertically_clamps_to_short_line() {
        let mut e = st("abcd\nef\nghij", 3);
        assert!(run(&mut e, EditCommand::AddCaretDown));
        assert_eq!(e.carets(), vec![3, 7], "짧은 줄은 줄 끝");
        assert!(run(&mut e, EditCommand::AddCaretDown));
        assert_eq!(
            e.carets(),
            vec![3, 7, 11],
            "짧은 줄을 지나도 원래 열(3)을 기억한다"
        );
        assert!(
            !run(&mut e, EditCommand::AddCaretDown),
            "마지막 줄 아래 없음"
        );
        e.insert('X');
        assert_eq!(e.text(), "abcXd\nefX\nghiXj");
    }

    #[test]
    fn case_transform_selection_or_word() {
        let mut e = EditState::with_text("select a from t", false);
        e.set_selection(0, 6);
        assert!(run(&mut e, EditCommand::UpperCase));
        assert_eq!(e.text(), "SELECT a from t");
        assert_eq!(e.selection(), Some((0, 6)));
        let mut e = st("select a from t", 10); // 'from' 안
        assert!(run(&mut e, EditCommand::UpperCase));
        assert_eq!(e.text(), "select a FROM t");
        assert_eq!(e.caret(), 10);
        assert!(run(&mut e, EditCommand::LowerCase));
        assert_eq!(e.text(), "select a from t");
        assert!(!run(&mut e, EditCommand::LowerCase), "이미 소문자");
    }

    #[test]
    fn helpers_line_index_and_span() {
        let e = EditState::with_text("a\nbb\nccc", false);
        assert_eq!(e.line_start_index(1), 0);
        assert_eq!(e.line_start_index(2), 2);
        assert_eq!(e.line_start_index(3), 5);
        assert_eq!(e.line_start_index(99), 5, "넘치면 마지막 줄");
        let mut e = EditState::with_text("a\nbb", false);
        e.set_selection(0, 3);
        assert!(e.selection_spans_lines());
        e.set_selection(2, 4);
        assert!(!e.selection_spans_lines());
    }
}

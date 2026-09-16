//! 텍스트 편집 모델 — 캐럿·선택·삽입·삭제([docs/12 §A] `nexa-gui/edit.rs` 이식).
//!
//! **순수 로직**(레이아웃·히트테스트 없음 — 그건 위젯이 폰트로 실측). 문자(`char`) 단위 버퍼라
//! 한글·이모지 경계가 자연히 지켜진다(UTF-8 바이트 인덱싱의 함정 회피). **IME 연결 지점**(M3-3):
//! 조합 확정 문자열은 [`EditState::insert_str`], 프리에딧 표시는 위젯이 이 상태 위에 얹는다.
//!
//! 원본에서 뺀 것: paint 캐시 기반 클릭 히트테스트(위젯이 폰트 실측으로 대체) · 드래그 선택.

mod ops;
pub use ops::EditCommand;

/// 이동/편집 키(플랫폼 중립 — [`crate::event::Key`]에서 번역).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKey {
    /// 캐럿 왼쪽(선택 있으면 왼쪽 가장자리로 접기).
    Left,
    /// 캐럿 오른쪽.
    Right,
    /// 줄 시작.
    Home,
    /// 줄 끝.
    End,
    /// 전체 선택.
    SelectAll,
    /// Delete(앞으로 삭제).
    DeleteForward,
}

/// 편집 상태 — 버퍼(char)·캐럿(`0..=len`)·선택(anchor↔caret).
#[derive(Clone, Debug, Default)]
pub struct EditState {
    buf: Vec<char>,
    caret: usize,
    anchor: Option<usize>,
    /// 추가 선택/캐럿(Sublime find_under_expand = Ctrl+D · 열 선택 · nexa-sql 09-15) — `(anchor, caret)`.
    /// 주 선택(`caret`/`anchor`)은 마지막에 추가된 것이다. 삽입·삭제는 전 구간에 함께 적용하고,
    /// 클릭·세로 이동(`set_caret`)·전체 선택은 이 목록을 비운다(하나로 접힘).
    extra: Vec<(usize, usize)>,
    /// ★ 되돌리기 히스토리(nexa-sql 사용자 09-15) — 변경 **직전** 스냅샷(버퍼·캐럿·앵커). 연속 타이핑/삭제는 한 묶음
    /// (공백·개행·선택 대체·캐럿 이동이 경계). 상한 [`Self::HISTORY_MAX`] · `set_text`(프로그램 교체)는 히스토리를 비운다.
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    last_op: Option<EditOp>,
    /// 직전에 삽입한 문자가 공백이었나 — 공백 뒤 첫 글자 = 새 단어 = 새 묶음(Sublime 단어 단위 되돌리기).
    last_ws: bool,
    /// IME 조합 중 문자열(M3-1e ① — TextBox·대화 입력 공용). **표시 전용**:
    /// 편집 버퍼(`buf`)에 들어가지 않고, `display_text`가 캐럿 자리에 끼워 보인다.
    /// 확정 문자는 `insert`로 버퍼에 들어오고 조합은 끝난다(호출측이 preedit 비움).
    preedit: String,
}

/// 히스토리 스냅샷.
#[derive(Clone, Debug)]
struct Snap {
    buf: Vec<char>,
    caret: usize,
    anchor: Option<usize>,
    extra: Vec<(usize, usize)>,
}

/// 편집 종류 — 같은 종류가 이어지면 한 묶음(경계가 없을 때).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditOp {
    Insert,
    Delete,
    Other,
}

impl EditState {
    /// 히스토리 상한(스냅샷 수).
    pub const HISTORY_MAX: usize = 500;

    fn snap(&self) -> Snap {
        Snap {
            buf: self.buf.clone(),
            caret: self.caret,
            anchor: self.anchor,
            extra: self.extra.clone(),
        }
    }

    /// 변경 직전 호출 — `boundary`거나 종류가 바뀌면 새 스냅샷, 아니면 직전 묶음에 이어 붙인다. redo는 버린다.
    fn record(&mut self, op: EditOp, boundary: bool) {
        if self.last_op == Some(op) && !boundary {
            return;
        }
        let s = self.snap();
        self.undo.push(s);
        if self.undo.len() > Self::HISTORY_MAX {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.last_op = Some(op);
    }

    fn restore(&mut self, s: Snap) {
        self.buf = s.buf;
        self.caret = s.caret.min(self.buf.len());
        self.anchor = s.anchor.map(|a| a.min(self.buf.len()));
        let n = self.buf.len();
        self.extra = s
            .extra
            .into_iter()
            .map(|(a, c)| (a.min(n), c.min(n)))
            .collect();
    }

    /// 실행 취소 — 되돌렸으면 `true`.
    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else {
            return false;
        };
        let cur = self.snap();
        self.redo.push(cur);
        self.restore(prev);
        self.last_op = None;
        true
    }

    /// 다시 실행 — 되살렸으면 `true`.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        let cur = self.snap();
        self.undo.push(cur);
        self.restore(next);
        self.last_op = None;
        true
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// 빈 상태.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 초기 텍스트(캐럿 끝). `select_all`이면 전체 선택으로 시작.
    #[must_use]
    pub fn with_text(text: &str, select_all: bool) -> Self {
        let buf: Vec<char> = text.chars().collect();
        let caret = buf.len();
        let anchor = (select_all && !buf.is_empty()).then_some(0);
        Self {
            buf,
            caret,
            anchor,
            extra: Vec::new(),
            preedit: String::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            last_op: None,
            last_ws: false,
        }
    }

    /// 현재 텍스트.
    #[must_use]
    pub fn text(&self) -> String {
        self.buf.iter().collect()
    }

    /// 비어 있는가.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// 캐럿 위치(문자 인덱스).
    #[must_use]
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// 선택 범위(정규화 `[a, b)`) — 없으면 `None`.
    #[must_use]
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        if a == self.caret {
            return None;
        }
        Some((a.min(self.caret), a.max(self.caret)))
    }

    /// 선택 텍스트(복사용).
    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selection()?;
        Some(self.buf[a..b].iter().collect())
    }

    /// 조합 중 문자열 지정(M3-1e ① 공용) — **H-25 규칙 내장**: 조합 시작(빈→비움
    /// 아님)에 선택이 있으면 그 선택을 삭제한다(OS 관례 — 선택 위 타이핑 = 대체 ·
    /// "선택 반전 + 조합 밑줄 병존"의 어리둥절한 화면 방지). 버퍼를 바꿨으면(선택
    /// 삭제) `true` — 호스트가 dirty 플래그를 갱신하는 근거.
    pub fn set_preedit(&mut self, text: &str) -> bool {
        let cut = if !text.is_empty() && self.selection().is_some() {
            self.delete_selection()
        } else {
            false
        };
        self.preedit = text.to_string();
        cut
    }

    /// 조합 중 문자열(표시·테스트).
    #[must_use]
    pub fn preedit(&self) -> &str {
        &self.preedit
    }

    /// **표시용 텍스트** — 조합 중 문자열을 캐럿 자리에 끼운 것(편집 버퍼 불변).
    /// 필드에 보이는 그대로가 필요한 곳(아바타 이니셜 미리보기 등)이 쓴다.
    #[must_use]
    pub fn display_text(&self) -> String {
        if self.preedit.is_empty() {
            return self.text();
        }
        let caret = self.caret.min(self.buf.len());
        let before: String = self.buf[..caret].iter().collect();
        let after: String = self.buf[caret..].iter().collect();
        format!("{before}{}{after}", self.preedit)
    }

    fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        self.buf.drain(a..b);
        self.caret = a;
        self.anchor = None;
        true
    }

    // ───────────────────────── 다중 선택(Ctrl+D · 열 선택 · Sublime) ─────────────────────────

    /// 추가 선택이 있는가(주 선택 외).
    #[must_use]
    pub fn has_multi(&self) -> bool {
        !self.extra.is_empty()
    }

    /// 모든 구간 `[start, end)`(주 선택 포함 · 시작 오름차순 · 빈 캐럿도 `start == end`로 들어온다).
    #[must_use]
    pub fn regions(&self) -> Vec<(usize, usize)> {
        let mut v: Vec<(usize, usize)> = self
            .extra
            .iter()
            .map(|&(a, c)| (a.min(c), a.max(c)))
            .collect();
        let (a, b) = match self.selection() {
            Some(r) => r,
            None => (self.caret, self.caret),
        };
        v.push((a, b));
        v.sort_unstable();
        v.dedup();
        v
    }

    /// 모든 캐럿 위치(주 캐럿 포함 · 오름차순).
    #[must_use]
    pub fn carets(&self) -> Vec<usize> {
        let mut v: Vec<usize> = self.extra.iter().map(|&(_, c)| c).collect();
        v.push(self.caret);
        v.sort_unstable();
        v.dedup();
        v
    }

    /// 구간을 추가로 선택한다 — 지금 선택은 추가 목록으로 내려가고 새 구간이 주 선택이 된다.
    /// 이미 선택된 구간이면 아무것도 하지 않는다(`false`).
    pub fn add_selection(&mut self, from: usize, to: usize) -> bool {
        let n = self.buf.len();
        let (from, to) = (from.min(n), to.min(n));
        let key = (from.min(to), from.max(to));
        if self.regions().contains(&key) {
            return false;
        }
        let cur = (self.anchor.unwrap_or(self.caret), self.caret);
        if cur.0 != cur.1 || self.has_multi() {
            self.extra.push(cur);
        }
        self.anchor = Some(from);
        self.caret = to;
        self.last_op = None;
        true
    }

    /// 구간 목록으로 선택을 통째로 바꾼다(열 선택 드래그) — 마지막 구간이 주 선택.
    pub fn set_regions(&mut self, regions: &[(usize, usize)]) {
        let n = self.buf.len();
        let mut v: Vec<(usize, usize)> =
            regions.iter().map(|&(a, c)| (a.min(n), c.min(n))).collect();
        let Some((a, c)) = v.pop() else { return };
        self.extra = v;
        self.anchor = (a != c).then_some(a);
        self.caret = c;
        self.last_op = None;
    }

    /// 추가 선택을 모두 지운다(Esc·클릭) — 지웠으면 `true`.
    pub fn clear_multi(&mut self) -> bool {
        let had = !self.extra.is_empty();
        self.extra.clear();
        had
    }

    /// 모든 구간의 텍스트(복사·잘라내기 — Sublime처럼 줄바꿈으로 잇는다).
    #[must_use]
    pub fn selected_text_multi(&self) -> Option<String> {
        if !self.has_multi() {
            return self.selected_text();
        }
        let parts: Vec<String> = self
            .regions()
            .into_iter()
            .filter(|(a, b)| b > a)
            .map(|(a, b)| self.buf[a..b].iter().collect::<String>())
            .collect();
        (!parts.is_empty()).then(|| parts.join("\n"))
    }

    /// 모든 구간에 같은 편집을 적용한다 — `ins`를 넣고, 빈 구간이면 `back`(앞 한 글자)·`fwd`(뒤 한 글자)를 지운다.
    /// 구간은 앞에서부터 처리하며 길이 변화를 누적 반영한다(뒤 구간 위치가 밀린다).
    fn edit_regions(&mut self, ins: &[char], back: bool, fwd: bool) {
        let regions = self.regions();
        let mut delta: isize = 0;
        let mut carets: Vec<usize> = Vec::with_capacity(regions.len());
        for (a, b) in regions {
            let mut a2 = ((a as isize + delta).max(0) as usize).min(self.buf.len());
            let b2 = ((b as isize + delta).max(0) as usize).min(self.buf.len());
            let mut removed = b2.saturating_sub(a2);
            if removed == 0 {
                if back && a2 > 0 {
                    a2 -= 1;
                    removed = 1;
                } else if fwd && a2 < self.buf.len() {
                    removed = 1;
                }
            }
            if removed > 0 {
                self.buf.drain(a2..a2 + removed);
            }
            for (i, c) in ins.iter().enumerate() {
                self.buf.insert(a2 + i, *c);
            }
            delta += ins.len() as isize - removed as isize;
            carets.push(a2 + ins.len());
        }
        // 마지막 구간을 주 캐럿으로(나머지는 추가 캐럿 · 선택은 접힌다 = Sublime).
        let last = carets.pop().unwrap_or(self.caret);
        self.extra = carets.into_iter().map(|c| (c, c)).collect();
        self.caret = last.min(self.buf.len());
        self.anchor = None;
    }

    /// 문자 하나 삽입(선택 있으면 대체 · 다중 선택이면 전부에).
    pub fn insert(&mut self, c: char) {
        // 묶음 경계 = 공백 뒤 첫 글자(새 단어) · 개행 · 선택 대체 → 단어 단위로 되돌린다(Sublime 관례).
        let boundary = (self.last_ws && !c.is_whitespace())
            || c == '\n'
            || self.anchor.is_some()
            || self.has_multi();
        self.record(EditOp::Insert, boundary);
        self.last_ws = c.is_whitespace();
        if self.has_multi() {
            self.edit_regions(&[c], false, false);
            return;
        }
        self.delete_selection();
        self.buf.insert(self.caret, c);
        self.caret += 1;
    }

    /// 문자열 삽입(붙여넣기·IME 확정 — 선택 있으면 대체). 제어문자 필터는 호출자 몫.
    pub fn insert_str(&mut self, s: &str) {
        self.record(EditOp::Other, true);
        if self.has_multi() {
            let ins: Vec<char> = s.chars().collect();
            self.edit_regions(&ins, false, false);
            return;
        }
        self.delete_selection();
        for c in s.chars() {
            self.buf.insert(self.caret, c);
            self.caret += 1;
        }
    }

    /// Backspace(선택 있으면 선택 삭제).
    pub fn backspace(&mut self) {
        if self.has_multi() {
            self.record(EditOp::Delete, true);
            self.edit_regions(&[], true, false);
            return;
        }
        if self.anchor.is_none() && self.caret == 0 {
            return;
        }
        self.record(EditOp::Delete, self.anchor.is_some());
        if !self.delete_selection() && self.caret > 0 {
            self.caret -= 1;
            self.buf.remove(self.caret);
        }
    }

    /// 잘라내기(선택 텍스트 반환 후 삭제).
    pub fn cut(&mut self) -> Option<String> {
        let t = self.selected_text_multi()?;
        self.record(EditOp::Other, true);
        if self.has_multi() {
            self.edit_regions(&[], false, false);
        } else {
            self.delete_selection();
        }
        Some(t)
    }

    /// 캐럿을 옮긴다 — `extend`면 기존 앵커를 유지해 범위가 늘어난다(드래그·Shift 이동).
    pub fn set_caret(&mut self, idx: usize, extend: bool) {
        self.last_op = None; // 캐럿 이동 = 타이핑 묶음 경계
        self.extra.clear(); // 클릭·세로 이동 = 다중 선택 접기(Sublime)
        let i = idx.min(self.buf.len());
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.caret);
            }
        } else {
            self.anchor = None;
        }
        self.caret = i;
    }

    /// 범위를 직접 선택한다(더블클릭 단어 선택 등).
    pub fn set_selection(&mut self, from: usize, to: usize) {
        self.extra.clear();
        let n = self.buf.len();
        self.anchor = Some(from.min(n));
        self.caret = to.min(n);
    }

    /// 전체 교체(캐럿 끝·선택 해제).
    pub fn set_text(&mut self, text: &str) {
        self.buf = text.chars().collect();
        self.caret = self.buf.len();
        self.anchor = None;
        self.extra.clear();
        // 프로그램적 교체 = 새 문서(히스토리 초기화).
        self.undo.clear();
        self.redo.clear();
        self.last_op = None;
    }

    /// 키 처리. 비Shift 이동 중 선택이 있으면 선택 가장자리로 접는다(표준 관례).
    pub fn key(&mut self, k: EditKey, shift: bool) {
        // 다중 캐럿에서는 ←/→가 모든 캐럿을 함께 옮긴다(Sublime) · 삭제도 전 구간에.
        if self.has_multi() {
            match k {
                EditKey::Left | EditKey::Right => {
                    let right = matches!(k, EditKey::Right);
                    let n = self.buf.len();
                    let step = |a: Option<usize>, c: usize| -> (Option<usize>, usize) {
                        let nc = if right {
                            (c + 1).min(n)
                        } else {
                            c.saturating_sub(1)
                        };
                        let na = if shift { Some(a.unwrap_or(c)) } else { None };
                        (na, nc)
                    };
                    let (na, nc) = step(self.anchor, self.caret);
                    self.anchor = na;
                    self.caret = nc;
                    let moved: Vec<(usize, usize)> = self
                        .extra
                        .iter()
                        .map(|&(a, c)| {
                            let (na, nc) = step((a != c).then_some(a), c);
                            (na.unwrap_or(nc), nc)
                        })
                        .collect();
                    self.extra = moved;
                    self.last_op = None;
                    return;
                }
                EditKey::DeleteForward => {
                    self.record(EditOp::Delete, true);
                    self.edit_regions(&[], false, true);
                    return;
                }
                EditKey::SelectAll | EditKey::Home | EditKey::End => {
                    self.extra.clear();
                }
            }
        }
        match k {
            EditKey::Left => {
                if let (false, Some((a, _))) = (shift, self.selection()) {
                    self.caret = a;
                    self.anchor = None;
                } else {
                    self.move_to(self.caret.saturating_sub(1), shift);
                }
            }
            EditKey::Right => {
                if let (false, Some((_, b))) = (shift, self.selection()) {
                    self.caret = b;
                    self.anchor = None;
                } else {
                    self.move_to((self.caret + 1).min(self.buf.len()), shift);
                }
            }
            EditKey::Home => self.move_to(0, shift),
            EditKey::End => self.move_to(self.buf.len(), shift),
            EditKey::SelectAll => {
                self.anchor = (!self.buf.is_empty()).then_some(0);
                self.caret = self.buf.len();
            }
            EditKey::DeleteForward => {
                if self.anchor.is_none() && self.caret >= self.buf.len() {
                    return;
                }
                self.record(EditOp::Delete, self.anchor.is_some());
                if !self.delete_selection() && self.caret < self.buf.len() {
                    self.buf.remove(self.caret);
                }
            }
        }
        if !matches!(k, EditKey::DeleteForward) {
            self.last_op = None; // 이동 키 = 묶음 경계
        }
    }

    fn move_to(&mut self, to: usize, shift: bool) {
        if shift {
            if self.anchor.is_none() {
                self.anchor = Some(self.caret);
            }
        } else {
            self.anchor = None;
        }
        self.caret = to;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_caret_advance() {
        let mut e = EditState::new();
        e.insert('h');
        e.insert('i');
        assert_eq!(e.text(), "hi");
        assert_eq!(e.caret(), 2);
    }

    #[test]
    fn hangul_is_char_wise_not_byte() {
        // UTF-8 3바이트 한글이 캐럿 1칸씩 — 바이트 인덱싱 함정 회피.
        let mut e = EditState::new();
        e.insert_str("한글");
        assert_eq!(e.caret(), 2);
        e.backspace();
        assert_eq!(e.text(), "한");
        assert_eq!(e.caret(), 1);
    }

    #[test]
    fn caret_move_left_right_home_end() {
        let mut e = EditState::with_text("abc", false);
        assert_eq!(e.caret(), 3);
        e.key(EditKey::Left, false);
        assert_eq!(e.caret(), 2);
        e.key(EditKey::Home, false);
        assert_eq!(e.caret(), 0);
        e.insert('X');
        assert_eq!(e.text(), "Xabc");
        e.key(EditKey::End, false);
        assert_eq!(e.caret(), 4);
    }

    #[test]
    fn shift_selection_then_type_replaces() {
        let mut e = EditState::with_text("hello", false);
        e.key(EditKey::Home, false);
        e.key(EditKey::Right, true); // select 'h'
        e.key(EditKey::Right, true); // select 'he'
        assert_eq!(e.selection(), Some((0, 2)));
        assert_eq!(e.selected_text().as_deref(), Some("he"));
        e.insert('X'); // 선택 대체
        assert_eq!(e.text(), "Xllo");
        assert_eq!(e.caret(), 1);
    }

    #[test]
    fn select_all_and_backspace_clears() {
        let mut e = EditState::with_text("data", false);
        e.key(EditKey::SelectAll, false);
        assert_eq!(e.selection(), Some((0, 4)));
        e.backspace();
        assert!(e.is_empty());
    }

    #[test]
    fn non_shift_left_collapses_selection_to_edge() {
        let mut e = EditState::with_text("abcd", false);
        e.key(EditKey::Home, false);
        e.key(EditKey::Right, true);
        e.key(EditKey::Right, true); // sel [0,2), caret=2
        e.key(EditKey::Left, false); // 비Shift Left = 선택 왼쪽 가장자리로 접기
        assert_eq!(e.caret(), 0);
        assert_eq!(e.selection(), None);
    }

    #[test]
    fn delete_forward_at_caret() {
        let mut e = EditState::with_text("abc", false);
        e.key(EditKey::Home, false);
        e.key(EditKey::DeleteForward, false);
        assert_eq!(e.text(), "bc");
        assert_eq!(e.caret(), 0);
    }

    #[test]
    fn insert_str_replaces_selection() {
        let mut e = EditState::with_text("world", true); // 전체 선택
        e.insert_str("hi"); // IME 확정 문자열이 선택 대체
        assert_eq!(e.text(), "hi");
    }
}

#[cfg(test)]
mod undo_tests {
    use super::*;

    #[test]
    fn typing_groups_by_word_and_undo_redo_round_trip() {
        let mut e = EditState::new();
        for c in "ab cd".chars() {
            e.insert(c);
        }
        assert_eq!(e.text(), "ab cd");
        assert!(e.undo(), "마지막 단어 묶음(cd)");
        assert_eq!(e.text(), "ab ");
        assert!(e.undo(), "첫 단어 + 공백 묶음");
        assert_eq!(e.text(), "");
        assert!(!e.undo(), "더 없음");
        assert!(e.redo());
        assert_eq!(e.text(), "ab ");
        assert!(e.redo());
        assert_eq!(e.text(), "ab cd");
        assert!(!e.redo());
        // 새 편집은 redo를 버린다.
        e.undo();
        e.insert('!');
        assert_eq!(e.text(), "ab !");
        assert!(!e.can_redo());
    }

    #[test]
    fn delete_paste_and_set_text_history_rules() {
        let mut e = EditState::with_text("hello", false);
        e.set_caret(5, false);
        e.backspace();
        e.backspace();
        assert_eq!(e.text(), "hel");
        assert!(e.undo(), "연속 백스페이스 = 한 묶음");
        assert_eq!(e.text(), "hello");
        e.insert_str(" world");
        assert!(e.undo());
        assert_eq!(e.text(), "hello");
        e.set_text("fresh");
        assert!(!e.can_undo(), "프로그램 교체 = 히스토리 초기화");
        assert!(!e.can_redo());
    }

    #[test]
    fn multi_selection_edits_every_region() {
        // Ctrl+D로 모은 구간 전부에 같은 타이핑이 들어간다(Sublime).
        let mut e = EditState::with_text("aa bb aa", false);
        e.set_selection(0, 2); // 첫 "aa"
        assert!(e.add_selection(6, 8)); // 두 번째 "aa"
        assert!(e.has_multi());
        assert_eq!(e.regions(), vec![(0, 2), (6, 8)]);
        e.insert('X');
        assert_eq!(e.text(), "X bb X");
        assert_eq!(
            e.carets(),
            vec![1, 6],
            "구간마다 캐럿이 남는다(두 번째 X 뒤)"
        );
        // 이어 타이핑하면 두 캐럿 모두에 들어간다.
        e.insert('Y');
        assert_eq!(e.text(), "XY bb XY");
        // Backspace도 전부.
        e.backspace();
        assert_eq!(e.text(), "X bb X");
        // 복사 텍스트는 줄바꿈으로 잇는다.
        e.set_selection(0, 1);
        assert!(e.add_selection(5, 6));
        assert_eq!(e.selected_text_multi().as_deref(), Some("X\nX"));
        // 클릭(= set_caret)은 다중 선택을 접는다.
        e.set_caret(0, false);
        assert!(!e.has_multi());
    }

    #[test]
    fn add_selection_skips_duplicates() {
        let mut e = EditState::with_text("aa aa", false);
        e.set_selection(0, 2);
        assert!(e.add_selection(3, 5));
        assert!(!e.add_selection(0, 2), "이미 선택된 구간은 추가하지 않는다");
    }

    #[test]
    fn set_regions_makes_column_block() {
        // 열 선택 드래그 — 줄마다 같은 열 구간(마지막이 주 선택).
        let mut e = EditState::with_text("abcd\nefgh\nijkl", false);
        e.set_regions(&[(1, 3), (6, 8), (11, 13)]);
        assert_eq!(e.regions(), vec![(1, 3), (6, 8), (11, 13)]);
        e.insert('.');
        assert_eq!(e.text(), "a.d\ne.h\ni.l");
    }
}

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
#[derive(Clone, Debug)]
pub struct EditState {
    buf: Vec<char>,
    /// ★ 본문 변경 세대(nexa-sql 09-19 성능): 버퍼가 바뀔 때마다 +1 — 페인트의 O(n) 계산(내용 폭·줄 분해)·호스트의
    /// "저장본과 다른가" 판정이 본문을 다시 훑는 대신 이 값으로 캐시를 판단한다.
    rev: u64,
    caret: usize,
    anchor: Option<usize>,
    /// 추가 선택/캐럿(Sublime find_under_expand = Ctrl+D · 열 선택 · nexa-sql 09-15) — `(anchor, caret)`.
    /// 주 선택(`caret`/`anchor`)은 마지막에 추가된 것이다. 삽입·삭제는 전 구간에 함께 적용하고,
    /// 클릭·세로 이동(`set_caret`)·전체 선택은 이 목록을 비운다(하나로 접힘).
    extra: Vec<(usize, usize)>,
    /// ★ 되돌리기 히스토리(nexa-sql 사용자 09-15) — 변경 **직전** 스냅샷(버퍼·캐럿·앵커). 연속 타이핑/삭제는 한 묶음
    /// (공백·개행·선택 대체·캐럿 이동이 경계). 상한 [`Self::history_max`](기본 [`Self::HISTORY_MAX`] · 호스트가 설정
    /// `editor.undo_max`로 [`Self::set_history_max`] · nexa-sql docs/39 T-90d) · `set_text`(프로그램 교체)는 히스토리를 비운다.
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    /// 되돌리기 스냅샷 상한(≥ 1).
    history_max: usize,
    last_op: Option<EditOp>,
    /// 직전에 삽입한 문자가 공백이었나 — 공백 뒤 첫 글자 = 새 단어 = 새 묶음(Sublime 단어 단위 되돌리기).
    last_ws: bool,
    /// IME 조합 중 문자열(M3-1e ① — TextBox·대화 입력 공용). **표시 전용**:
    /// 편집 버퍼(`buf`)에 들어가지 않고, `display_text`가 캐럿 자리에 끼워 보인다.
    /// 확정 문자는 `insert`로 버퍼에 들어오고 조합은 끝난다(호출측이 preedit 비움).
    preedit: String,
}

/// 히스토리 스냅샷의 본문 — **전체 복사는 맨 위 하나뿐**(nexa-sql 09-19 메모리 점검: 종전에는 묶음마다 `Vec<char>` 전체를
/// 복사해, 3.6 MB 스크립트면 단어 하나에 14.6 MB · 상한 1,000개면 수백 MB~GB까지 쌓였다).
///
/// 규약: 되돌리기 스택의 k번째 항목은 "그다음 상태에 적용하면 k번째 상태가 되는 **차이**"다. 다음 상태를 아직 모르는 맨 위
/// 항목만 `Full`이고, 새 묶음을 기록하는 순간(그때의 버퍼 = 다음 상태) 차이로 접는다. 다시 실행 스택은 늘 차이다.
#[derive(Clone, Debug)]
enum SnapBuf {
    Full(Vec<char>),
    /// 적용 대상의 `[pre, pre + del_len)`을 `ins`로 바꾼다.
    Delta {
        pre: usize,
        del_len: usize,
        ins: Vec<char>,
    },
}

impl SnapBuf {
    /// `from`에 적용하면 `to`가 되는 차이(공통 앞·뒤를 뺀 가운데).
    fn delta(from: &[char], to: &[char]) -> SnapBuf {
        let mut pre = 0;
        while pre < from.len() && pre < to.len() && from[pre] == to[pre] {
            pre += 1;
        }
        let mut suf = 0;
        while suf < from.len() - pre
            && suf < to.len() - pre
            && from[from.len() - 1 - suf] == to[to.len() - 1 - suf]
        {
            suf += 1;
        }
        SnapBuf::Delta {
            pre,
            del_len: from.len() - suf - pre,
            ins: to[pre..to.len() - suf].to_vec(),
        }
    }

    /// `buf`에 적용하고, **되돌리는 차이**(적용 뒤의 버퍼에 적용하면 적용 전이 된다)를 돌려준다.
    fn apply(self, buf: &mut Vec<char>) -> SnapBuf {
        match self {
            SnapBuf::Full(prev) => {
                let inverse = SnapBuf::delta(&prev, buf);
                *buf = prev;
                inverse
            }
            SnapBuf::Delta { pre, del_len, ins } => {
                let pre = pre.min(buf.len());
                let end = (pre + del_len).min(buf.len());
                let ins_len = ins.len();
                let removed: Vec<char> = buf.splice(pre..end, ins).collect();
                SnapBuf::Delta {
                    pre,
                    del_len: ins_len,
                    ins: removed,
                }
            }
        }
    }

    /// 이 항목이 쥐고 있는 글자 수(진단·테스트 — 메모리 = × 4바이트).
    fn held_chars(&self) -> usize {
        match self {
            SnapBuf::Full(v) => v.len(),
            SnapBuf::Delta { ins, .. } => ins.len(),
        }
    }
}

/// 히스토리 스냅샷.
#[derive(Clone, Debug)]
struct Snap {
    buf: SnapBuf,
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

impl Default for EditState {
    fn default() -> Self {
        EditState {
            buf: Vec::new(),
            rev: 0,
            caret: 0,
            anchor: None,
            extra: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            last_op: None,
            last_ws: false,
            preedit: String::new(),
            history_max: Self::HISTORY_MAX,
        }
    }
}

impl EditState {
    /// 히스토리 상한 기본값(스냅샷 수) — 호스트가 바꾸지 않으면 이 값.
    pub const HISTORY_MAX: usize = 500;

    /// 되돌리기 스냅샷 상한을 바꾼다(0은 1로) — 넘치는 오래된 스냅샷은 즉시 버린다(메모리 상한 = 설정 즉시 반영).
    pub fn set_history_max(&mut self, n: usize) {
        self.history_max = n.max(1);
        if self.undo.len() > self.history_max {
            let drop_n = self.undo.len() - self.history_max;
            self.undo.drain(..drop_n);
        }
        if self.redo.len() > self.history_max {
            let drop_n = self.redo.len() - self.history_max;
            self.redo.drain(..drop_n);
        }
    }

    /// 현재 되돌리기 상한.
    #[must_use]
    pub fn history_max(&self) -> usize {
        self.history_max
    }

    /// 쌓인 되돌리기 스냅샷 수(진단·테스트).
    #[must_use]
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// 히스토리가 쥐고 있는 글자 수(되돌리기 + 다시 실행 · 진단·테스트 — 메모리 ≈ × 4바이트).
    #[must_use]
    pub fn history_chars(&self) -> usize {
        self.undo
            .iter()
            .chain(self.redo.iter())
            .map(|s| s.buf.held_chars())
            .sum()
    }

    /// 히스토리를 비운다(호스트의 메모리 회수 — 보이지 않는 큰 탭 · 닫기 직전).
    pub fn clear_history(&mut self) {
        self.undo = Vec::new();
        self.redo = Vec::new();
        self.last_op = None;
    }

    fn snap_with(&self, buf: SnapBuf) -> Snap {
        Snap {
            buf,
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
        // 지금 버퍼 = 앞선 묶음의 "다음 상태" → 맨 위의 전체 복사를 차이로 접는다(전체 복사는 늘 하나 이하).
        if let Some(top) = self.undo.last_mut() {
            if let SnapBuf::Full(prev) = &top.buf {
                top.buf = SnapBuf::delta(&self.buf, prev);
            }
        }
        let s = self.snap_with(SnapBuf::Full(self.buf.clone()));
        self.undo.push(s);
        if self.undo.len() > self.history_max {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.last_op = Some(op);
    }

    /// 스냅샷을 적용하고, 반대 방향 스냅샷(적용 직전의 캐럿·선택 + 되돌리는 차이)을 돌려준다.
    fn restore(&mut self, s: Snap) -> Snap {
        let mut back = self.snap_with(SnapBuf::Full(Vec::new()));
        self.rev = self.rev.wrapping_add(1);
        back.buf = s.buf.apply(&mut self.buf);
        self.caret = s.caret.min(self.buf.len());
        self.anchor = s.anchor.map(|a| a.min(self.buf.len()));
        let n = self.buf.len();
        self.extra = s
            .extra
            .into_iter()
            .map(|(a, c)| (a.min(n), c.min(n)))
            .collect();
        back
    }

    /// 실행 취소 — 되돌렸으면 `true`.
    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else {
            return false;
        };
        let back = self.restore(prev);
        self.redo.push(back);
        self.last_op = None;
        true
    }

    /// 다시 실행 — 되살렸으면 `true`.
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        let back = self.restore(next);
        self.undo.push(back);
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
            ..Self::default()
        }
    }

    /// 현재 텍스트.
    #[must_use]
    pub fn text(&self) -> String {
        self.buf.iter().collect()
    }

    /// 본문 변경 세대 — 같은 값이면 본문이 같다(캐시 키).
    #[must_use]
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// 본문 글자 슬라이스(복사 0) — 캐럿 이동·줄 계산이 `text()`로 String을 다시 만들지 않게.
    #[must_use]
    pub fn chars(&self) -> &[char] {
        &self.buf
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

    /// 주 선택이 **거꾸로**(뒤에서 앞으로 드래그 → 캐럿이 앞)인가 — 선택이 없으면 false.
    #[must_use]
    pub fn selection_reversed(&self) -> bool {
        self.anchor.is_some_and(|a| a > self.caret)
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
        self.rev = self.rev.wrapping_add(1);
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

    /// 캐럿 하나를 **더한다**(Ctrl+클릭 · Sublime) — 지금 주 선택/캐럿은 추가 목록으로 내려가고 새 자리가 주 캐럿.
    /// 같은 자리에 이미 캐럿이 있으면 **뺀다**(토글 · 마지막 하나는 남긴다). 반환 = 더했으면 true.
    pub fn toggle_caret(&mut self, idx: usize) -> bool {
        let n = self.buf.len();
        let idx = idx.min(n);
        let cur = (self.anchor.unwrap_or(self.caret), self.caret);
        // 이미 있는 캐럿(빈 구간) 제거.
        if let Some(i) = self.extra.iter().position(|&(a, c)| a == c && c == idx) {
            self.extra.remove(i);
            self.last_op = None;
            return false;
        }
        if cur.0 == cur.1 && cur.1 == idx {
            if let Some((a, c)) = self.extra.pop() {
                self.anchor = (a != c).then_some(a);
                self.caret = c;
            }
            self.last_op = None;
            return false;
        }
        self.extra.push(cur);
        self.anchor = None;
        self.caret = idx;
        self.last_op = None;
        true
    }

    /// 단어 경계(Sublime `words`/`word_ends`): 문자 부류 = 공백 · 단어(영숫자·`_`·비ASCII 문자) · 구분자(그 밖).
    /// 왼쪽 = 공백을 건너뛴 뒤 같은 부류 런의 시작 · 오른쪽 = 공백을 건너뛴 뒤 같은 부류 런의 끝.
    pub fn word_boundary(&self, from: usize, right: bool) -> usize {
        word_boundary(&self.buf, from, right)
    }

    /// 서브워드 경계(Sublime `subwords`/`subword_ends`): 단어 안에서 `_`·소문자→대문자·글자↔숫자 전환도 경계.
    pub fn subword_boundary(&self, from: usize, right: bool) -> usize {
        subword_boundary(&self.buf, from, right)
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
                self.rev = self.rev.wrapping_add(1);
                self.buf.drain(a2..a2 + removed);
            }
            for (i, c) in ins.iter().enumerate() {
                self.rev = self.rev.wrapping_add(1);
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
        self.rev = self.rev.wrapping_add(1);
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
            self.rev = self.rev.wrapping_add(1);
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
            self.rev = self.rev.wrapping_add(1);
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
        self.rev = self.rev.wrapping_add(1);
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
                    self.rev = self.rev.wrapping_add(1);
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Space,
    Word,
    Sep,
}

fn class_of(c: char) -> CharClass {
    if c.is_whitespace() {
        CharClass::Space
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Sep
    }
}

/// 단어 경계(Sublime · 줄바꿈은 공백으로 취급하되 줄을 넘지 않는다).
pub fn word_boundary(buf: &[char], from: usize, right: bool) -> usize {
    let n = buf.len();
    let mut i = from.min(n);
    if right {
        // 공백 건너뛰기(줄바꿈 하나는 넘는다 · 그 뒤 첫 런의 끝).
        while i < n && buf[i].is_whitespace() && buf[i] != '\n' {
            i += 1;
        }
        if i < n && buf[i] == '\n' {
            return i + 1;
        }
        if i >= n {
            return n;
        }
        let k = class_of(buf[i]);
        while i < n && class_of(buf[i]) == k {
            i += 1;
        }
        i
    } else {
        while i > 0 && buf[i - 1].is_whitespace() && buf[i - 1] != '\n' {
            i -= 1;
        }
        if i > 0 && buf[i - 1] == '\n' {
            return i - 1;
        }
        if i == 0 {
            return 0;
        }
        let k = class_of(buf[i - 1]);
        while i > 0 && class_of(buf[i - 1]) == k {
            i -= 1;
        }
        i
    }
}

/// 서브워드 경계 — 단어 런 안에서 `_` 양쪽 · 소문자→대문자 · 대문자 연속→마지막 대문자+소문자(`HTMLParser` → `HTML|Parser`) ·
/// 글자↔숫자 전환에서 멈춘다. 단어 밖(공백·구분자)은 단어 경계와 같다.
pub fn subword_boundary(buf: &[char], from: usize, right: bool) -> usize {
    let n = buf.len();
    let i = from.min(n);
    let is_sub_break = |a: char, b: char| -> bool {
        // a = 앞 글자 · b = 뒤 글자(경계는 a|b 사이)
        if a == '_' || b == '_' {
            return true;
        }
        (a.is_lowercase() && b.is_uppercase())
            || (a.is_alphabetic() && b.is_ascii_digit())
            || (a.is_ascii_digit() && b.is_alphabetic())
    };
    if right {
        if i >= n || class_of(buf[i]) != CharClass::Word {
            return word_boundary(buf, i, true);
        }
        let mut j = i + 1;
        // `_` 바로 위면 그 런을 통째로 넘긴다.
        if buf[i] == '_' {
            while j < n && buf[j] == '_' {
                j += 1;
            }
            return j;
        }
        while j < n && class_of(buf[j]) == CharClass::Word {
            if is_sub_break(buf[j - 1], buf[j]) {
                break;
            }
            // 대문자 연속 뒤 소문자: `HTMLParser` → `HTML|Parser`(경계 = 마지막 대문자 앞)
            if buf[j - 1].is_uppercase()
                && buf[j].is_uppercase()
                && j + 1 < n
                && buf[j + 1].is_lowercase()
            {
                break;
            }
            j += 1;
        }
        j
    } else {
        if i == 0 || class_of(buf[i - 1]) != CharClass::Word {
            return word_boundary(buf, i, false);
        }
        let mut j = i - 1;
        if buf[j] == '_' {
            while j > 0 && buf[j - 1] == '_' {
                j -= 1;
            }
            return j;
        }
        while j > 0 && class_of(buf[j - 1]) == CharClass::Word {
            if is_sub_break(buf[j - 1], buf[j]) {
                break;
            }
            if buf[j - 1].is_uppercase()
                && buf[j].is_uppercase()
                && j < i
                && j + 1 < n
                && buf[j + 1].is_lowercase()
            {
                break;
            }
            j -= 1;
        }
        j
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 되돌리기 = 차이 저장(nexa-sql 09-19): 큰 본문에서 여러 묶음을 쳐도 히스토리가 쥔 글자는 **전체 복사 하나 + 친 만큼** ·
    /// 되돌리기/다시 실행을 끝까지 오가도 본문·캐럿이 정확히 돌아온다.
    #[test]
    fn undo_history_stores_deltas_not_full_copies() {
        let base: String = (0..2000).map(|i| format!("line {i}\n")).collect();
        let n = base.chars().count();
        let mut e = EditState::with_text(&base, false);
        e.set_caret(5, false);
        let mut states = vec![e.text()];
        for w in ["alpha", "beta", "gamma", "delta"] {
            for c in w.chars() {
                e.insert(c);
            }
            e.insert(' '); // 공백 뒤 첫 글자 = 새 묶음
            states.push(e.text());
        }
        assert!(e.undo_len() >= 4);
        // 전체 복사는 맨 위 하나뿐 — 묶음 수 × 전체가 아니다.
        assert!(
            e.history_chars() < n + 200,
            "history holds {} chars for a {n}-char buffer",
            e.history_chars()
        );
        // 끝까지 되돌렸다가 끝까지 다시 실행.
        let mut back = 0;
        while e.undo() {
            back += 1;
        }
        assert_eq!(e.text(), base);
        assert!(e.history_chars() < 200, "되돌린 뒤에는 차이만 남는다");
        for _ in 0..back {
            assert!(e.redo());
        }
        assert_eq!(e.text(), *states.last().unwrap_or(&String::new()));
        // 중간에서 새로 치면 다시 실행은 버려지고, 되돌리면 직전 상태로.
        assert!(e.undo());
        let mid = e.text();
        e.insert('Z');
        assert!(!e.can_redo());
        assert!(e.undo());
        assert_eq!(e.text(), mid);
        e.clear_history();
        assert_eq!((e.undo_len(), e.history_chars()), (0, 0));
    }

    /// Sublime 단어/서브워드 경계 · Ctrl+클릭 캐럿 토글(nexa-sql 사용자 09-17).
    #[test]
    fn word_subword_boundaries_and_caret_toggle() {
        let b: Vec<char> = "sales_customer.HTMLParser  x1y".chars().collect();
        // words: 오른쪽 = 런 끝 · 왼쪽 = 런 시작
        assert_eq!(word_boundary(&b, 0, true), 14, "sales_customer|");
        assert_eq!(word_boundary(&b, 14, true), 15, "구분자 `.` 런");
        assert_eq!(word_boundary(&b, 15, true), 25, "HTMLParser|");
        assert_eq!(word_boundary(&b, 25, true), 30, "공백 건너뛰고 x1y|");
        assert_eq!(word_boundary(&b, 30, false), 27, "|x1y");
        assert_eq!(word_boundary(&b, 14, false), 0);
        // subwords
        assert_eq!(subword_boundary(&b, 0, true), 5, "sales|_");
        assert_eq!(subword_boundary(&b, 5, true), 6, "_|customer");
        assert_eq!(subword_boundary(&b, 6, true), 14);
        assert_eq!(subword_boundary(&b, 15, true), 19, "HTML|Parser");
        assert_eq!(subword_boundary(&b, 19, true), 25);
        assert_eq!(subword_boundary(&b, 25, false), 19, "HTML|Parser 왼쪽");
        assert_eq!(subword_boundary(&b, 14, false), 6, "_|customer 왼쪽");
        assert_eq!(subword_boundary(&b, 30, false), 29, "x1|y");
        // caret toggle
        let mut e = EditState::new();
        e.set_text("ab cd");
        e.set_caret(1, false);
        assert!(e.toggle_caret(4));
        assert_eq!(e.carets(), vec![1, 4]);
        assert!(!e.toggle_caret(4), "같은 자리 = 제거");
        assert_eq!(e.carets(), vec![1]);
        assert!(e.toggle_caret(3));
        assert!(!e.toggle_caret(1), "추가 목록의 캐럿 제거");
        assert_eq!(e.carets(), vec![3]);
    }

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

    /// T-90d(nexa-sql docs/39 §3-6): 되돌리기 깊이 상한 — 경계마다 새 스냅샷 · 상한을 넘으면 오래된 것부터 버린다 · 줄이면 즉시 잘린다.
    #[test]
    fn history_max_bounds_undo_depth() {
        let mut e = EditState::new();
        assert_eq!(e.history_max(), EditState::HISTORY_MAX);
        e.set_history_max(3);
        for _ in 0..10 {
            e.insert('a');
            e.insert(' '); // 공백 = 묶음 경계 → 다음 글자가 새 스냅샷
        }
        assert!(e.undo_len() <= 3, "상한 3을 넘지 않는다: {}", e.undo_len());
        assert_eq!(e.undo_len(), 3);
        e.set_history_max(1);
        assert_eq!(e.undo_len(), 1, "줄이면 즉시 잘린다");
        assert!(e.undo());
        assert!(!e.undo(), "스냅샷 하나만 남았었다");
        e.set_history_max(0);
        assert_eq!(e.history_max(), 1, "0은 1로");
    }
}

//! 텍스트 편집 모델 — 캐럿·선택·삽입·삭제([docs/12 §A] `nexa-gui/edit.rs` 이식).
//!
//! **순수 로직**(레이아웃·히트테스트 없음 — 그건 위젯이 폰트로 실측). 좌표는 문자(`char`) 인덱스라
//! 한글·이모지 경계가 자연히 지켜진다 — 저장은 UTF-8 갭 버퍼 + 줄 표([`TextBuf`] · nexa-sql T-142)이고
//! 바이트 오프셋은 그 안에서만 쓴다. **IME 연결 지점**(M3-3):
//! 조합 확정 문자열은 [`EditState::insert_str`], 프리에딧 표시는 위젯이 이 상태 위에 얹는다.
//!
//! 원본에서 뺀 것: paint 캐시 기반 클릭 히트테스트(위젯이 폰트 실측으로 대체) · 드래그 선택.

mod ops;
mod textbuf;
pub use ops::EditCommand;
pub use textbuf::{CharSeq, Hash64, LineChange, TextBuf};

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
    buf: TextBuf,
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
    undo: std::collections::VecDeque<Txn>,
    redo: Vec<Txn>,
    /// 되돌리기 단계 수 상한(≥ 1).
    history_max: usize,
    /// 히스토리 **바이트 예산**(되돌리기 + 다시 실행) — 넘으면 오래된 단계부터 버린다(맨 위 단계는 남긴다).
    history_budget: usize,
    history_bytes: usize,
    /// 상태 식별자 발급기 · 맨 아래(버려진 단계들까지 적용된) 상태의 식별자 · 저장 지점.
    serial: u64,
    base_id: u64,
    saved_id: Option<u64>,
    /// 묶음 API 깊이(`begin_group`) · 그 묶음에서 이미 단계를 열었는가.
    group_depth: u32,
    group_open: bool,
    /// **긴 정지 뒤에는 새 묶음**(nexa-sql docs/60 D-131): 이어 붙이던 타이핑·삭제라도 직전 편집에서 이만큼 쉬었으면 끊는다 —
    /// 되돌리기 단위가 "한 번에 친 만큼"에 가까워진다. `None` = 끔. 단어 경계 규칙의 **보조**다(단독 기준이 아니다).
    group_pause: Option<std::time::Duration>,
    last_edit_at: Option<std::time::Instant>,
    /// **거대 편집 확인**(nexa-sql docs/60 D-130): 한 번에 이만큼(바이트) 이상을 지우는 편집은 되돌리기 기록이 그만큼을
    /// 들어야 한다 — 예산을 넘겨 드는 대신 **두 번 눌러야** 하고, 하고 나면 이 문서의 히스토리를 비운다(Emacs `undo-outer-limit`).
    /// 0 = 끔. 확인은 **지우는 동작**(Delete · Backspace · 잘라내기 · 편집 명령 · 모두 바꾸기)을 3초 안에 되풀이하는 것이고,
    /// 타이핑·붙여넣기·조합으로 덮어쓰는 것은 확인 수단이 아니다(빠르게 두 글자를 치는 것이 확인이 되면 안 된다).
    giant_limit: usize,
    giant_armed_until: Option<std::time::Instant>,
    /// 방금 통과시킨 거대 편집 — 끝나면 히스토리를 비운다.
    giant_pass: bool,
    /// 막힌 거대 편집(바이트 · 되풀이하면 되는가) — 호스트가 꺼내 안내한다(1회성).
    giant_blocked: Option<(usize, bool)>,
    /// **읽기 전용**(nexa-sql docs/59 — 큰 파일을 보기만 · 일부만 열기): 본문을 바꾸는 모든 길이 단일 통로
    /// ([`Self::splice_rec`] · `replace_many_inner`)를 지나므로 거기서 막는다. 캐럿 이동·선택·복사는 그대로 된다.
    read_only: bool,
    last_op: Option<EditOp>,
    /// 직전에 삽입한 문자가 공백이었나 — 공백 뒤 첫 글자 = 새 단어 = 새 묶음(Sublime 단어 단위 되돌리기).
    last_ws: bool,
    /// IME 조합 중 문자열(M3-1e ① — TextBox·대화 입력 공용). **표시 전용**:
    /// 편집 버퍼(`buf`)에 들어가지 않고, `display_text`가 캐럿 자리에 끼워 보인다.
    /// 확정 문자는 `insert`로 버퍼에 들어오고 조합은 끝난다(호출측이 preedit 비움).
    preedit: String,
}

/// **되돌리기 연산 하나**(nexa-sql docs/60 · 09-19 재설계): "적용 = `pos`에서 `remove_n` 글자를 지우고 `insert`를 넣는다".
/// 적용하면 그 **역연산**이 나온다(지운 글자가 역연산의 `insert`가 된다) — 되돌리기와 다시 실행이 같은 모양을 주고받는다.
/// 저장하는 것은 **지워진 글자뿐**이다: 타이핑은 "k글자 지우기"(글자 없음 · 버퍼에 있으니까) · 삭제는 그 글자들.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Op {
    pos: usize,
    remove_n: usize,
    insert: String,
    /// `insert`의 글자 수(인덱스 계산용 — 매번 세지 않게).
    insert_n: usize,
}

/// 되돌리기 한 단계(묶음) — 연산들 + 되돌렸을 때 돌아갈 캐럿·선택.
///
/// 규약: `ops`는 **뒤에서부터 앞으로** 적용한다. 각 연산의 `pos`는 "자기보다 뒤의 연산이 모두 적용된 시점"의 좌표다.
/// 위치가 오름차순이고 겹치지 않으면(여러 곳을 한 번에 고친 묶음 — 다중 캐럿 · 모두 바꾸기) 본문을 **한 번만 훑어** 적용한다.
#[derive(Clone, Debug)]
struct Txn {
    /// 상태 식별자 — "이 묶음까지 적용된 상태"의 이름(저장 지점 비교용 · 되돌리기/다시 실행을 오가도 같은 값).
    id: u64,
    ops: Vec<Op>,
    caret: usize,
    anchor: Option<usize>,
    extra: Vec<(usize, usize)>,
    /// 이 묶음이 쥔 바이트(글자 + 연산당 고정비) — 예산 회계.
    bytes: usize,
}

/// 연산·묶음의 고정비 어림(바이트) — 예산이 "글자 0인 연산 수만 개"로도 차도록.
const OP_OVERHEAD: usize = 48;
const TXN_OVERHEAD: usize = 96;

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
            buf: TextBuf::new(),
            rev: 0,
            caret: 0,
            anchor: None,
            extra: Vec::new(),
            undo: std::collections::VecDeque::new(),
            redo: Vec::new(),
            history_budget: Self::HISTORY_BUDGET,
            history_bytes: 0,
            serial: 0,
            base_id: 0,
            saved_id: Some(0),
            group_depth: 0,
            group_open: false,
            group_pause: None,
            last_edit_at: None,
            giant_limit: 0,
            giant_armed_until: None,
            giant_pass: false,
            giant_blocked: None,
            read_only: false,
            last_op: None,
            last_ws: false,
            preedit: String::new(),
            history_max: Self::HISTORY_MAX,
        }
    }
}

impl EditState {
    /// 거대 편집을 되풀이로 확인하는 시간(초).
    pub const GIANT_CONFIRM_SECS: u64 = 3;

    /// 여러 곳 편집을 "본문을 새로 짓는 한 번 훑기"로 넘기는 건수 — 이하면 한 곳 바꾸기를 되풀이한다.
    /// (한 곳 바꾸기 = O(편집 + 뒤따르는 줄 수) · 70만 줄에서 건당 ≈ 0.3 ms → 64건 ≈ 20 ms · 새로 짓기 ≈ 100 ms.)
    const BULK_MIN: usize = 64;

    /// 히스토리 상한 기본값(스냅샷 수) — 호스트가 바꾸지 않으면 이 값.
    pub const HISTORY_MAX: usize = 500;

    /// 히스토리 바이트 예산 기본값(64 MB) — 호스트가 파일 크기에 맞춰 [`Self::set_history_budget`]으로 바꾼다.
    pub const HISTORY_BUDGET: usize = 64 << 20;

    /// 되돌리기 단계 수 상한을 바꾼다(0은 1로) — 넘치는 오래된 단계는 즉시 버린다.
    pub fn set_history_max(&mut self, n: usize) {
        self.history_max = n.max(1);
        self.evict();
    }

    /// 히스토리 바이트 예산(되돌리기 + 다시 실행) — 넘치는 오래된 단계는 즉시 버린다(맨 위 하나는 남긴다).
    pub fn set_history_budget(&mut self, bytes: usize) {
        self.history_budget = bytes.max(1 << 16);
        self.evict();
    }

    /// 현재 되돌리기 상한.
    #[must_use]
    pub fn history_max(&self) -> usize {
        self.history_max
    }

    /// 쌓인 되돌리기 단계 수(진단·테스트).
    #[must_use]
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// 히스토리가 쥐고 있는 글자 수(되돌리기 + 다시 실행 · 진단·테스트).
    #[must_use]
    pub fn history_chars(&self) -> usize {
        self.undo
            .iter()
            .chain(self.redo.iter())
            .flat_map(|t| t.ops.iter())
            .map(|o| o.insert_n)
            .sum()
    }

    /// 히스토리가 쥔 바이트(어림 · 예산 회계와 같은 값).
    #[must_use]
    pub fn history_bytes(&self) -> usize {
        self.history_bytes
    }

    /// 히스토리를 비운다(호스트의 메모리 회수 · 닫기 직전) — 저장 지점 판정은 그대로 유효하다(지금 상태가 새 바닥).
    pub fn clear_history(&mut self) {
        self.base_id = self.state_id();
        self.undo = std::collections::VecDeque::new();
        self.redo = Vec::new();
        self.history_bytes = 0;
        self.last_op = None;
    }

    /// **비밀 값 지우기**(nexa-sql 09-21): 본문 · 되돌리기/다시 실행 기록이 쥔 글 · 조합 중 글을 0으로 덮어쓰고 비운다.
    pub fn wipe(&mut self) {
        let zero = |s: &mut String| {
            let mut v = std::mem::take(s).into_bytes();
            v.fill(0);
            std::hint::black_box(&v);
        };
        for txn in self.undo.iter_mut().chain(self.redo.iter_mut()) {
            for op in &mut txn.ops {
                zero(&mut op.insert);
            }
        }
        zero(&mut self.preedit);
        self.buf.wipe();
        self.set_text("");
    }

    /// 거대 편집 확인의 기준(바이트 · 0 = 끔).
    pub fn set_giant_limit(&mut self, bytes: usize) {
        self.giant_limit = bytes;
    }

    /// 막힌 거대 편집을 꺼낸다(1회성) — `(지우려던 바이트, 같은 동작을 되풀이하면 진행되는가)`.
    pub fn take_giant_blocked(&mut self) -> Option<(usize, bool)> {
        self.giant_blocked.take()
    }

    /// 지금 선택(모든 구간)이 쥔 바이트.
    fn selected_bytes(&self) -> usize {
        if self.anchor.is_none() && self.extra.is_empty() {
            return 0;
        }
        self.regions()
            .into_iter()
            .map(|(a, b)| self.buf.byte_len(a, b))
            .sum()
    }

    /// 거대 편집 문지기 — 막았으면 `true`(호출자는 아무것도 하지 않는다). `confirmable` = 되풀이가 확인이 되는 동작인가.
    fn giant_refused(&mut self, bytes: usize, confirmable: bool) -> bool {
        if self.giant_limit == 0 || bytes < self.giant_limit || self.read_only {
            return false;
        }
        let now = std::time::Instant::now();
        if confirmable && self.giant_armed_until.is_some_and(|t| now <= t) {
            self.giant_armed_until = None;
            self.giant_pass = true;
            return false;
        }
        self.giant_armed_until =
            confirmable.then(|| now + std::time::Duration::from_secs(Self::GIANT_CONFIRM_SECS));
        self.giant_blocked = Some((bytes, confirmable));
        true
    }

    /// 통과시킨 거대 편집이 끝났다 — 그 기록(지운 글 전체)을 들지 않고 히스토리를 비운다(되돌릴 수 없다고 이미 알렸다).
    fn giant_done(&mut self) {
        if std::mem::take(&mut self.giant_pass) {
            self.clear_history();
        }
    }

    /// 긴 정지 뒤 묶음 끊기(밀리초 · 0 = 끔) — 기본은 끔(호스트가 설정으로 켠다).
    pub fn set_group_pause_ms(&mut self, ms: u64) {
        self.group_pause = (ms > 0).then(|| std::time::Duration::from_millis(ms));
    }

    /// 읽기 전용 켜기/끄기 — 켜져 있으면 입력·붙여넣기·삭제·편집 명령·되돌리기가 본문을 바꾸지 못한다(`set_text`는 된다).
    pub fn set_read_only(&mut self, on: bool) {
        self.read_only = on;
    }

    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    // ───────────── 기록 파일(재시작 뒤에도 남는 되돌리기 · nexa-sql docs/60 D-129) ─────────────

    const HISTORY_MAGIC: &'static [u8; 4] = b"NSQU";
    /// 형식 판(좌표 단위 = 글자 인덱스). 버퍼 좌표의 뜻이 바뀌면 올린다 — 옛 파일은 조용히 버려진다.
    const HISTORY_VERSION: u8 = 1;

    /// 되돌리기·다시 실행 기록을 바이트열로 내보낸다(Vim `undofile`과 같은 쓰임 — 호스트가 **저장 직후**에 부른다).
    /// 머리에 본문의 길이·해시를 적어 두므로, 다시 읽을 때 본문이 한 글자라도 다르면 통째로 버려진다.
    /// `max_bytes`를 넘으면 **오래된 단계부터** 뺀다(다시 실행은 다 못 넣으면 전부 뺀다). 기록이 비었으면 `None`.
    #[must_use]
    pub fn export_history(&self, max_bytes: usize) -> Option<Vec<u8>> {
        if self.undo.is_empty() && self.redo.is_empty() {
            return None;
        }
        fn txn_len(t: &Txn) -> usize {
            8 * 3
                + 4
                + t.extra.len() * 16
                + 4
                + t.ops
                    .iter()
                    .map(|o| 8 * 3 + 4 + o.insert.len())
                    .sum::<usize>()
        }
        // 최신 단계부터 예산이 되는 만큼.
        let mut room = max_bytes.saturating_sub(96);
        let mut first = self.undo.len();
        for (i, t) in self.undo.iter().enumerate().rev() {
            let n = txn_len(t);
            if n > room {
                break;
            }
            room -= n;
            first = i;
        }
        let redo_len: usize = self.redo.iter().map(txn_len).sum();
        let redo: &[Txn] = if redo_len <= room { &self.redo } else { &[] };
        if first == self.undo.len() && redo.is_empty() {
            return None;
        }
        // 빠진 단계들까지 적용된 상태가 새 바닥이다(`evict`와 같은 규칙).
        let base_id = if first == 0 {
            self.base_id
        } else {
            self.undo[first - 1].id
        };
        let mut out: Vec<u8> = Vec::with_capacity(max_bytes.min(1 << 20));
        out.extend_from_slice(Self::HISTORY_MAGIC);
        out.extend_from_slice(&[Self::HISTORY_VERSION, 0, 0, 0]);
        let put = |out: &mut Vec<u8>, v: u64| out.extend_from_slice(&v.to_le_bytes());
        put(&mut out, self.buf.len() as u64);
        put(&mut out, self.buf.len_bytes() as u64);
        put(&mut out, self.buf.content_hash());
        put(&mut out, self.serial);
        put(&mut out, base_id);
        put(&mut out, self.saved_id.unwrap_or(u64::MAX));
        put(&mut out, (self.undo.len() - first) as u64);
        put(&mut out, redo.len() as u64);
        let write_txn = |out: &mut Vec<u8>, t: &Txn| {
            put(out, t.id);
            put(out, t.caret as u64);
            put(out, t.anchor.map_or(u64::MAX, |a| a as u64));
            out.extend_from_slice(&(t.extra.len() as u32).to_le_bytes());
            for &(a, c) in &t.extra {
                put(out, a as u64);
                put(out, c as u64);
            }
            out.extend_from_slice(&(t.ops.len() as u32).to_le_bytes());
            for o in &t.ops {
                put(out, o.pos as u64);
                put(out, o.remove_n as u64);
                put(out, o.insert_n as u64);
                out.extend_from_slice(&(o.insert.len() as u32).to_le_bytes());
                out.extend_from_slice(o.insert.as_bytes());
            }
        };
        for t in self.undo.iter().skip(first) {
            write_txn(&mut out, t);
        }
        for t in redo {
            write_txn(&mut out, t);
        }
        // 꼬리 = 앞 전체의 해시(잘린 파일 · 깨진 파일을 버린다).
        let mut h = Hash64::new();
        h.write(&out);
        put(&mut out, h.finish());
        Some(out)
    }

    /// [`Self::export_history`]가 만든 기록을 들인다 — **방금 읽은 본문**(기록 없음)에만, 그리고 본문의 길이·해시가 기록의 것과
    /// 같을 때만. 하나라도 어긋나면 아무것도 바꾸지 않고 `false`(부분 복구는 하지 않는다 — Vim · VS Code와 같은 규칙).
    pub fn import_history(&mut self, bytes: &[u8]) -> bool {
        if !self.undo.is_empty() || !self.redo.is_empty() || bytes.len() < 8 + 8 * 8 + 8 {
            return false;
        }
        let (body, tail) = bytes.split_at(bytes.len() - 8);
        let mut h = Hash64::new();
        h.write(body);
        let mut tail8 = [0u8; 8];
        tail8.copy_from_slice(tail);
        if u64::from_le_bytes(tail8) != h.finish()
            || &body[..4] != Self::HISTORY_MAGIC
            || body[4] != Self::HISTORY_VERSION
        {
            return false;
        }
        struct Rd<'a>(&'a [u8]);
        impl Rd<'_> {
            fn u64(&mut self) -> Option<u64> {
                let (a, b) = self.0.split_at_checked(8)?;
                self.0 = b;
                let mut w = [0u8; 8];
                w.copy_from_slice(a);
                Some(u64::from_le_bytes(w))
            }
            fn u32(&mut self) -> Option<u32> {
                let (a, b) = self.0.split_at_checked(4)?;
                self.0 = b;
                let mut w = [0u8; 4];
                w.copy_from_slice(a);
                Some(u32::from_le_bytes(w))
            }
            fn idx(&mut self) -> Option<usize> {
                usize::try_from(self.u64()?).ok()
            }
            fn text(&mut self, n: usize) -> Option<String> {
                let (a, b) = self.0.split_at_checked(n)?;
                self.0 = b;
                String::from_utf8(a.to_vec()).ok()
            }
        }
        let mut rd = Rd(&body[8..]);
        /// (발급기, 바닥 id, 저장 지점, 되돌리기, 다시 실행)
        type Parsed = (u64, u64, Option<u64>, Vec<Txn>, Vec<Txn>);
        let parse = |rd: &mut Rd<'_>| -> Option<Parsed> {
            let (serial, base_id, saved) = (rd.u64()?, rd.u64()?, rd.u64()?);
            let (n_undo, n_redo) = (rd.idx()?, rd.idx()?);
            let read_txn = |rd: &mut Rd<'_>| -> Option<Txn> {
                let id = rd.u64()?;
                let caret = rd.idx()?;
                let anchor = match rd.u64()? {
                    u64::MAX => None,
                    a => Some(usize::try_from(a).ok()?),
                };
                let mut extra = Vec::new();
                for _ in 0..rd.u32()? {
                    extra.push((rd.idx()?, rd.idx()?));
                }
                let mut ops = Vec::new();
                let mut bytes = TXN_OVERHEAD;
                for _ in 0..rd.u32()? {
                    let (pos, remove_n, insert_n) = (rd.idx()?, rd.idx()?, rd.idx()?);
                    let len = rd.u32()? as usize;
                    let insert = rd.text(len)?;
                    // 글자 수가 기록과 맞아야 한다(좌표 계산의 전제).
                    if insert.chars().count() != insert_n {
                        return None;
                    }
                    bytes += insert.len() + OP_OVERHEAD;
                    ops.push(Op {
                        pos,
                        remove_n,
                        insert,
                        insert_n,
                    });
                }
                Some(Txn {
                    id,
                    ops,
                    caret,
                    anchor,
                    extra,
                    bytes,
                })
            };
            let mut undo = Vec::new();
            for _ in 0..n_undo {
                undo.push(read_txn(rd)?);
            }
            let mut redo = Vec::new();
            for _ in 0..n_redo {
                redo.push(read_txn(rd)?);
            }
            rd.0.is_empty().then_some((
                serial,
                base_id,
                (saved != u64::MAX).then_some(saved),
                undo,
                redo,
            ))
        };
        let (Some(n_chars), Some(n_bytes), Some(hash)) = (rd.idx(), rd.idx(), rd.u64()) else {
            return false;
        };
        if n_chars != self.buf.len()
            || n_bytes != self.buf.len_bytes()
            || hash != self.buf.content_hash()
        {
            return false;
        }
        let Some((serial, base_id, saved_id, undo, redo)) = parse(&mut rd) else {
            return false;
        };
        self.history_bytes = undo.iter().chain(redo.iter()).map(|t| t.bytes).sum();
        self.undo = undo.into();
        self.redo = redo;
        self.serial = self.serial.max(serial);
        self.base_id = base_id;
        self.saved_id = saved_id;
        self.last_op = None;
        self.evict();
        true
    }

    // ───────────── 저장 지점(O(1) 더러움 판정) ─────────────

    /// 지금 상태의 식별자 — 맨 위 단계의 id(없으면 바닥 id).
    fn state_id(&self) -> u64 {
        self.undo.back().map_or(self.base_id, |t| t.id)
    }

    /// **저장 지점**: 지금 상태를 "저장된 상태"로 표시한다(진행 중인 타이핑 묶음은 여기서 끊는다).
    pub fn mark_saved(&mut self) {
        self.last_op = None;
        self.saved_id = Some(self.state_id());
    }

    /// 저장 지점과 같은 상태인가 — **O(1)**(본문 비교 없음). 되돌리기로 저장 지점에 돌아오면 다시 true.
    /// 저장 지점의 단계가 예산으로 버려지고 그 아래로 내려갈 수 없게 되면 false로 남는다(영구 더러움).
    #[must_use]
    pub fn is_saved(&self) -> bool {
        self.saved_id == Some(self.state_id())
    }

    // ───────────── 묶음(여러 편집 = 되돌리기 한 단계) ─────────────

    /// 묶음 시작 — [`Self::end_group`]까지의 모든 편집이 **되돌리기 한 단계**가 된다(중첩 가능 · 모두 바꾸기 · 서식 정리).
    pub fn begin_group(&mut self) {
        if self.group_depth == 0 {
            self.group_open = false;
        }
        self.group_depth += 1;
    }

    pub fn end_group(&mut self) {
        self.group_depth = self.group_depth.saturating_sub(1);
        if self.group_depth == 0 {
            self.group_open = false;
            self.last_op = None;
        }
    }

    // ───────────── 기록 · 적용 ─────────────

    fn open_txn(&mut self) {
        self.serial += 1;
        self.undo.push_back(Txn {
            id: self.serial,
            ops: Vec::new(),
            caret: self.caret,
            anchor: self.anchor,
            extra: self.extra.clone(),
            bytes: TXN_OVERHEAD,
        });
        self.history_bytes += TXN_OVERHEAD;
        for t in self.redo.drain(..) {
            self.history_bytes = self.history_bytes.saturating_sub(t.bytes);
        }
        self.evict();
    }

    /// 예산·개수 상한을 넘으면 **오래된 단계부터** 버린다(맨 위 하나는 남긴다 — 방금 한 일은 늘 되돌릴 수 있다).
    fn evict(&mut self) {
        while self.undo.len() > self.history_max
            || (self.history_bytes > self.history_budget && self.undo.len() > 1)
        {
            let Some(t) = self.undo.pop_front() else {
                break;
            };
            self.history_bytes = self.history_bytes.saturating_sub(t.bytes);
            self.base_id = t.id;
        }
        while self.redo.len() > self.history_max {
            let t = self.redo.remove(0);
            self.history_bytes = self.history_bytes.saturating_sub(t.bytes);
        }
    }

    /// 변경 직전 호출 — `boundary`거나 종류가 바뀌면 새 단계를 열고, 아니면 맨 위 단계에 이어 붙인다. 다시 실행은 버린다.
    fn record(&mut self, op: EditOp, boundary: bool) {
        if self.read_only {
            return;
        }
        if self.group_depth > 0 {
            if !self.group_open {
                self.group_open = true;
                self.open_txn();
            }
            self.last_op = Some(op);
            return;
        }
        // 긴 정지 = 묶음 경계(같은 종류가 이어지더라도).
        let now = std::time::Instant::now();
        let paused = match (self.group_pause, self.last_edit_at) {
            (Some(p), Some(at)) => now.duration_since(at) >= p,
            _ => false,
        };
        self.last_edit_at = Some(now);
        if self.last_op == Some(op) && !boundary && !paused && !self.undo.is_empty() {
            // 이어 붙이는 중에도 다시 실행은 무효다(새 편집이 끼었다).
            for t in self.redo.drain(..) {
                self.history_bytes = self.history_bytes.saturating_sub(t.bytes);
            }
            return;
        }
        self.open_txn();
        self.last_op = Some(op);
    }

    /// **모든 본문 변경의 단 하나의 통로**: `pos`에서 `del_n` 글자를 지우고 `ins`를 넣는다 + 맨 위 단계에 역연산을 적는다.
    /// (버퍼 표현 = [`TextBuf`] — 본문을 바꾸는 곳은 여기와 [`Self::apply_ops`] · `replace_many_inner` 셋뿐이다.)
    fn splice_rec(&mut self, pos: usize, del_n: usize, ins: &[char]) {
        let s: String = ins.iter().collect();
        self.splice_rec_str(pos, del_n, &s, ins.len());
    }

    /// [`Self::splice_rec`]의 문자열 판(`ins_n` = `ins`의 글자 수 — 붙여넣기처럼 큰 글을 글자 배열로 풀지 않는다).
    fn splice_rec_str(&mut self, pos: usize, del_n: usize, ins: &str, ins_n: usize) {
        if self.read_only {
            return;
        }
        let pos = pos.min(self.buf.len());
        let end = (pos + del_n).min(self.buf.len());
        let del_n = end - pos;
        if del_n == 0 && ins.is_empty() {
            return;
        }
        let removed = self.buf.splice(pos, del_n, ins);
        self.rev = self.rev.wrapping_add(1);
        self.note_op(pos, ins_n, removed, del_n);
    }

    /// 역연산 기록 — 이웃한 타이핑·Backspace·Delete는 한 연산으로 합친다(글자마다 연산을 만들지 않는다).
    fn note_op(&mut self, pos: usize, ins_n: usize, removed: String, del_n: usize) {
        let Some(t) = self.undo.back_mut() else {
            return;
        };
        let mut grew = removed.len();
        let merged = match t.ops.last_mut() {
            // 타이핑 이어 붙이기: 직전 = "k글자 지우기"이고 그 끝에 이어서 넣었다.
            Some(l)
                if del_n == 0
                    && l.insert_n == 0
                    && l.insert.is_empty()
                    && l.pos + l.remove_n == pos =>
            {
                l.remove_n += ins_n;
                true
            }
            // Backspace 이어 붙이기: 직전 = "글자 되살리기"이고 그 바로 앞을 또 지웠다.
            Some(l) if ins_n == 0 && l.remove_n == 0 && pos + del_n == l.pos => {
                l.insert.insert_str(0, &removed);
                l.insert_n += del_n;
                l.pos = pos;
                true
            }
            // Delete(앞으로 지우기) 이어 붙이기: 같은 자리에서 또 지웠다.
            Some(l) if ins_n == 0 && l.remove_n == 0 && pos == l.pos => {
                l.insert.push_str(&removed);
                l.insert_n += del_n;
                true
            }
            _ => false,
        };
        if !merged {
            t.ops.push(Op {
                pos,
                remove_n: ins_n,
                insert: removed,
                insert_n: del_n,
            });
            grew += OP_OVERHEAD;
        }
        t.bytes += grew;
        self.history_bytes += grew;
        if self.history_bytes > self.history_budget {
            self.evict();
        }
    }

    /// 연산 목록을 **뒤에서부터** 적용하고 역연산 목록(같은 규약)을 돌려준다. 위치가 오름차순·비겹침이고 **많으면**
    /// 본문을 한 번만 훑어 새로 짓는다(O(본문)) — 몇 곳 안 되면 한 곳 바꾸기를 되풀이하는 쪽이 싸다(O(편집) · 줄별 캐시 유지).
    fn apply_ops(&mut self, ops: Vec<Op>) -> Vec<Op> {
        self.rev = self.rev.wrapping_add(1);
        let bulk = ops.len() > Self::BULK_MIN
            && ops.windows(2).all(|w| w[0].pos + w[0].remove_n <= w[1].pos);
        if !bulk {
            let mut inv = Vec::with_capacity(ops.len());
            for op in ops.into_iter().rev() {
                let pos = op.pos.min(self.buf.len());
                let end = (pos + op.remove_n).min(self.buf.len());
                let removed = self.buf.splice(pos, end - pos, &op.insert);
                inv.push(Op {
                    pos,
                    remove_n: op.insert_n,
                    insert: removed,
                    insert_n: end - pos,
                });
            }
            return inv;
        }
        // 한 번 훑기: 모든 `pos`는 지금 버퍼 좌표다(뒤의 연산은 앞의 위치를 밀지 않는다).
        let edits: Vec<(usize, usize, &str)> = ops
            .iter()
            .map(|o| (o.pos, o.pos + o.remove_n, o.insert.as_str()))
            .collect();
        self.buf
            .replace_many(&edits)
            .into_iter()
            .map(|(pos, ins_n, removed, del_n)| Op {
                // 역연산의 좌표 = 앞쪽 연산이 모두 적용된 결과 버퍼에서의 자리.
                pos,
                remove_n: ins_n,
                insert: removed,
                insert_n: del_n,
            })
            .collect()
    }

    /// 단계 하나를 적용하고 반대 방향 단계(지금의 캐럿·선택 + 역연산)를 돌려준다.
    fn apply_txn(&mut self, t: Txn) -> Txn {
        let mut back = Txn {
            id: t.id,
            ops: Vec::new(),
            caret: self.caret,
            anchor: self.anchor,
            extra: self.extra.clone(),
            bytes: TXN_OVERHEAD,
        };
        back.ops = self.apply_ops(t.ops);
        back.bytes += back
            .ops
            .iter()
            .map(|o| o.insert.len() + OP_OVERHEAD)
            .sum::<usize>();
        let n = self.buf.len();
        self.caret = t.caret.min(n);
        self.anchor = t.anchor.map(|a| a.min(n));
        self.extra = t
            .extra
            .into_iter()
            .map(|(a, c)| (a.min(n), c.min(n)))
            .collect();
        back
    }

    /// 실행 취소 — 되돌렸으면 `true`.
    pub fn undo(&mut self) -> bool {
        if self.read_only {
            return false;
        }
        let Some(t) = self.undo.pop_back() else {
            return false;
        };
        self.history_bytes = self.history_bytes.saturating_sub(t.bytes);
        let back = self.apply_txn(t);
        self.history_bytes += back.bytes;
        self.redo.push(back);
        self.last_op = None;
        true
    }

    /// 다시 실행 — 되살렸으면 `true`.
    pub fn redo(&mut self) -> bool {
        if self.read_only {
            return false;
        }
        let Some(t) = self.redo.pop() else {
            return false;
        };
        self.history_bytes = self.history_bytes.saturating_sub(t.bytes);
        let back = self.apply_txn(t);
        self.history_bytes += back.bytes;
        self.undo.push_back(back);
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
        let buf = TextBuf::from_string(text.to_string());
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
        self.buf.to_string()
    }

    /// 본문 변경 세대 — 같은 값이면 본문이 같다(캐시 키).
    #[must_use]
    pub fn rev(&self) -> u64 {
        self.rev
    }

    /// 본문 버퍼(읽기) — 글자·구간·줄 조회([`TextBuf`] · 복사 0). 종전의 `chars() -> &[char]`를 대신한다.
    #[must_use]
    pub fn buf(&self) -> &TextBuf {
        &self.buf
    }

    /// 본문 전체를 글자 배열로(**O(본문)** — 드문 명령·테스트용. 자주 도는 길은 [`Self::buf`]의 조회를 쓴다).
    #[must_use]
    pub fn chars_vec(&self) -> Vec<char> {
        self.buf.iter_from(0).collect()
    }

    /// 글자 수.
    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
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
        Some(self.buf.slice_string(a, b))
    }

    /// 조합 중 문자열 지정(M3-1e ① 공용) — **H-25 규칙 내장**: 조합 시작(빈→비움
    /// 아님)에 선택이 있으면 그 선택을 삭제한다(OS 관례 — 선택 위 타이핑 = 대체 ·
    /// "선택 반전 + 조합 밑줄 병존"의 어리둥절한 화면 방지). 버퍼를 바꿨으면(선택
    /// 삭제) `true` — 호스트가 dirty 플래그를 갱신하는 근거.
    pub fn set_preedit(&mut self, text: &str) -> bool {
        // 거대 선택 위에서 조합을 시작하면 선택을 지우지 않는다(조합은 확인 수단이 아니다).
        let giant = !text.is_empty() && {
            let n = self.selected_bytes();
            self.giant_refused(n, false)
        };
        let cut = if !giant && !text.is_empty() && self.selection().is_some() {
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
        let (before, after) = (
            self.buf.slice(0, caret),
            self.buf.slice(caret, self.buf.len()),
        );
        format!("{before}{}{after}", self.preedit)
    }

    fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        self.splice_rec(a, b - a, &[]);
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

    /// 추가 목록에서 구간 `[a, b)` 하나를 뺀다(순서 무관 · 주 선택은 건드리지 않는다) — 뺐으면 `true`.
    /// Sublime `find_under_expand_skip`(Ctrl+K,Ctrl+D)의 재료: 방금 주 선택이던 구간을 버릴 때.
    pub fn remove_region(&mut self, a: usize, b: usize) -> bool {
        let key = (a.min(b), a.max(b));
        let Some(i) = self
            .extra
            .iter()
            .rposition(|&(x, y)| (x.min(y), x.max(y)) == key)
        else {
            return false;
        };
        self.extra.remove(i);
        true
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
            .map(|(a, b)| self.buf.slice_string(a, b))
            .collect();
        (!parts.is_empty()).then(|| parts.join("\n"))
    }

    /// 모든 구간에 같은 편집을 적용한다 — `ins`를 넣고, 빈 구간이면 `back`(앞 한 글자)·`fwd`(뒤 한 글자)를 지운다.
    /// 구간은 앞에서부터 처리하며 길이 변화를 누적 반영한다(뒤 구간 위치가 밀린다).
    fn edit_regions(&mut self, ins: &[char], back: bool, fwd: bool) {
        // 구간마다 지울 범위(원래 좌표) — 빈 구간은 Backspace/Delete가 한 글자를 먹는다. 앞 구간과 겹치면 뒤로 민다.
        let mut edits: Vec<(usize, usize)> = Vec::new();
        let mut floor = 0usize;
        for (a, b) in self.regions() {
            let (mut a, mut b) = (a.min(self.buf.len()), b.min(self.buf.len()));
            if a == b {
                if back && a > 0 {
                    a -= 1;
                } else if fwd && b < self.buf.len() {
                    b += 1;
                }
            }
            let a = a.max(floor);
            let b = b.max(a);
            floor = b;
            edits.push((a, b));
        }
        let ins_s: String = ins.iter().collect();
        let list: Vec<(usize, usize, &str)> =
            edits.iter().map(|&(a, b)| (a, b, ins_s.as_str())).collect();
        let mut carets = self.replace_many_inner(&list);
        // 마지막 구간을 주 캐럿으로(나머지는 추가 캐럿 · 선택은 접힌다 = Sublime).
        let last = carets.pop().unwrap_or(self.caret);
        self.extra = carets.into_iter().map(|c| (c, c)).collect();
        self.caret = last.min(self.buf.len());
        self.anchor = None;
    }

    /// 여러 곳을 바꾼다(`edits` = 원래 좌표의 오름차순·비겹침 `(from, to, 새 글)`) — 많으면 본문을 **한 번 훑어** 새로 짓고,
    /// 몇 곳이면 뒤에서부터 한 곳씩(앞 좌표가 밀리지 않는다). 돌려주는 값 = 구간별 새 글의 끝 위치(결과 좌표).
    /// 되돌리기 기록은 맨 위 단계에 구간당 연산 하나.
    fn replace_many_inner(&mut self, edits: &[(usize, usize, &str)]) -> Vec<usize> {
        if edits.is_empty() || self.read_only {
            // 읽기 전용 = 아무것도 바꾸지 않는다(캐럿 자리는 구간의 끝 그대로).
            return edits.iter().map(|e| e.1).collect();
        }
        self.rev = self.rev.wrapping_add(1);
        // (결과 좌표의 시작, 넣은 글자 수, 지운 글, 지운 글자 수)
        let notes: Vec<(usize, usize, String, usize)> = if edits.len() > Self::BULK_MIN {
            self.buf.replace_many(edits)
        } else {
            // 구간을 다듬고(범위·겹침) 결과 좌표를 앞에서부터 센 다음, 뒤에서부터 바꾼다.
            let n = self.buf.len();
            let mut plan: Vec<(usize, usize, &str, usize, usize)> = Vec::with_capacity(edits.len());
            let (mut at, mut delta) = (0usize, 0isize);
            for &(a, b, text) in edits {
                let a = a.min(n).max(at);
                let b = b.min(n).max(a);
                let ins_n = text.chars().count();
                plan.push((a, b, text, ins_n, (a as isize + delta) as usize));
                delta += ins_n as isize - (b - a) as isize;
                at = b;
            }
            let mut notes: Vec<(usize, usize, String, usize)> = Vec::with_capacity(plan.len());
            for &(a, b, text, ins_n, new_pos) in plan.iter().rev() {
                let removed = self.buf.splice(a, b - a, text);
                notes.push((new_pos, ins_n, removed, b - a));
            }
            notes.reverse();
            notes
        };
        let ends: Vec<usize> = notes.iter().map(|n| n.0 + n.1).collect();
        // 연산은 저마다 하나(이웃 타이핑 합치기는 한 곳 편집에만 의미가 있다 — 여기서는 합쳐지면 좌표 규약이 깨진다).
        let live: Vec<(usize, usize, String, usize)> =
            notes.into_iter().filter(|n| n.1 > 0 || n.3 > 0).collect();
        let many = live.len() > 1;
        for (pos, ins_n, removed, del_n) in live {
            if many {
                self.push_op_raw(pos, ins_n, removed, del_n);
            } else {
                self.note_op(pos, ins_n, removed, del_n);
            }
        }
        ends
    }

    fn push_op_raw(&mut self, pos: usize, ins_n: usize, removed: String, del_n: usize) {
        let Some(t) = self.undo.back_mut() else {
            return;
        };
        let grew = removed.len() + OP_OVERHEAD;
        t.ops.push(Op {
            pos,
            remove_n: ins_n,
            insert: removed,
            insert_n: del_n,
        });
        t.bytes += grew;
        self.history_bytes += grew;
    }

    /// **여러 곳 바꾸기**(모두 바꾸기 · 서식 정리의 최소 편집) — 되돌리기 **한 단계** · 본문을 한 번만 훑는다.
    /// `edits` = 지금 좌표의 오름차순·비겹침 `(from, to, 새 글)`. 캐럿은 마지막으로 바꾼 곳의 끝 · 선택은 접는다.
    pub fn replace_many(&mut self, edits: &[(usize, usize, &str)]) {
        if edits.is_empty() {
            return;
        }
        let bytes: usize = edits.iter().map(|e| self.buf.byte_len(e.0, e.1)).sum();
        if self.giant_refused(bytes, true) {
            return;
        }
        self.replace_many_checked(edits);
        self.giant_done();
    }

    fn replace_many_checked(&mut self, edits: &[(usize, usize, &str)]) {
        self.record(EditOp::Other, true);
        let ends = self.replace_many_inner(edits);
        self.extra.clear();
        self.anchor = None;
        if let Some(&e) = ends.last() {
            self.caret = e.min(self.buf.len());
        }
        self.last_op = None;
    }

    /// 문자 하나 삽입(선택 있으면 대체 · 다중 선택이면 전부에).
    pub fn insert(&mut self, c: char) {
        let n = self.selected_bytes();
        if self.giant_refused(n, false) {
            return;
        }
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
        let at = self.caret;
        self.splice_rec(at, 0, &[c]);
        if !self.read_only {
            self.caret += 1;
        }
    }

    /// 문자열 삽입(붙여넣기·IME 확정 — 선택 있으면 대체). 제어문자 필터는 호출자 몫.
    pub fn insert_str(&mut self, s: &str) {
        // 빈 글을 넣는 것 = 선택 지우기(호스트의 프로그램적 삭제) — 되풀이로 확인된다. 그 밖(붙여넣기)은 아니다.
        let n = self.selected_bytes();
        if self.giant_refused(n, s.is_empty()) {
            return;
        }
        self.insert_str_checked(s);
        self.giant_done();
    }

    fn insert_str_checked(&mut self, s: &str) {
        self.record(EditOp::Other, true);
        if self.has_multi() {
            let ins: Vec<char> = s.chars().collect();
            self.edit_regions(&ins, false, false);
            return;
        }
        self.delete_selection();
        // ★ 한 번에 끼운다(종전 = 글자마다 `Vec::insert` — 3 MB 본문에 100 KB 붙여넣기가 34초였다 · 제곱 시간).
        let n = s.chars().count();
        let at = self.caret;
        self.splice_rec_str(at, 0, s, n);
        if !self.read_only {
            self.caret += n;
        }
    }

    /// Backspace(선택 있으면 선택 삭제).
    pub fn backspace(&mut self) {
        let n = self.selected_bytes();
        if self.giant_refused(n, true) {
            return;
        }
        self.backspace_checked();
        self.giant_done();
    }

    fn backspace_checked(&mut self) {
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
            let at = self.caret;
            self.splice_rec(at, 1, &[]);
        }
    }

    /// 잘라내기(선택 텍스트 반환 후 삭제).
    pub fn cut(&mut self) -> Option<String> {
        let n = self.selected_bytes();
        if self.giant_refused(n, true) {
            return None;
        }
        let t = self.selected_text_multi()?;
        self.record(EditOp::Other, true);
        if self.has_multi() {
            self.edit_regions(&[], false, false);
        } else {
            self.delete_selection();
        }
        self.giant_done();
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
        self.buf.set_string(text.to_string());
        self.caret = self.buf.len();
        self.anchor = None;
        self.extra.clear();
        // 프로그램적 교체 = 새 문서(히스토리 초기화 · 새 바닥 상태 — 저장 지점은 호출자가 `mark_saved`로 찍는다).
        self.undo.clear();
        self.redo.clear();
        self.history_bytes = 0;
        self.serial += 1;
        self.base_id = self.serial;
        self.saved_id = None;
        self.group_depth = 0;
        self.group_open = false;
        self.last_op = None;
    }

    /// [`Self::set_text`]의 소유권 판 — 이미 만들어 둔 버퍼(본문 + 줄 표)를 **그대로** 본문으로 삼는다(복사·변환 0).
    /// 큰 파일을 작업 스레드에서 준비한 뒤 UI 스레드는 옮겨 받기만 하려는 것(nexa-sql 09-20). 캐럿 = 문서 처음.
    pub fn set_buf(&mut self, buf: TextBuf) {
        self.set_text("");
        self.buf.adopt(buf);
        self.caret = 0;
    }

    /// 키 처리. 비Shift 이동 중 선택이 있으면 선택 가장자리로 접는다(표준 관례).
    pub fn key(&mut self, k: EditKey, shift: bool) {
        if k == EditKey::DeleteForward {
            let n = self.selected_bytes();
            if self.giant_refused(n, true) {
                return;
            }
        }
        self.key_checked(k, shift);
        self.giant_done();
    }

    fn key_checked(&mut self, k: EditKey, shift: bool) {
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
                    let at = self.caret;
                    self.splice_rec(at, 1, &[]);
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
pub fn word_boundary<B: CharSeq + ?Sized>(buf: &B, from: usize, right: bool) -> usize {
    let n = buf.len();
    let mut i = from.min(n);
    if right {
        // 공백 건너뛰기(줄바꿈 하나는 넘는다 · 그 뒤 첫 런의 끝).
        while i < n && buf.at(i).is_whitespace() && buf.at(i) != '\n' {
            i += 1;
        }
        if i < n && buf.at(i) == '\n' {
            return i + 1;
        }
        if i >= n {
            return n;
        }
        let k = class_of(buf.at(i));
        while i < n && class_of(buf.at(i)) == k {
            i += 1;
        }
        i
    } else {
        while i > 0 && buf.at(i - 1).is_whitespace() && buf.at(i - 1) != '\n' {
            i -= 1;
        }
        if i > 0 && buf.at(i - 1) == '\n' {
            return i - 1;
        }
        if i == 0 {
            return 0;
        }
        let k = class_of(buf.at(i - 1));
        while i > 0 && class_of(buf.at(i - 1)) == k {
            i -= 1;
        }
        i
    }
}

/// 서브워드 경계 — 단어 런 안에서 `_` 양쪽 · 소문자→대문자 · 대문자 연속→마지막 대문자+소문자(`HTMLParser` → `HTML|Parser`) ·
/// 글자↔숫자 전환에서 멈춘다. 단어 밖(공백·구분자)은 단어 경계와 같다.
pub fn subword_boundary<B: CharSeq + ?Sized>(buf: &B, from: usize, right: bool) -> usize {
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
        if i >= n || class_of(buf.at(i)) != CharClass::Word {
            return word_boundary(buf, i, true);
        }
        let mut j = i + 1;
        // `_` 바로 위면 그 런을 통째로 넘긴다.
        if buf.at(i) == '_' {
            while j < n && buf.at(j) == '_' {
                j += 1;
            }
            return j;
        }
        while j < n && class_of(buf.at(j)) == CharClass::Word {
            if is_sub_break(buf.at(j - 1), buf.at(j)) {
                break;
            }
            // 대문자 연속 뒤 소문자: `HTMLParser` → `HTML|Parser`(경계 = 마지막 대문자 앞)
            if buf.at(j - 1).is_uppercase()
                && buf.at(j).is_uppercase()
                && j + 1 < n
                && buf.at(j + 1).is_lowercase()
            {
                break;
            }
            j += 1;
        }
        j
    } else {
        if i == 0 || class_of(buf.at(i - 1)) != CharClass::Word {
            return word_boundary(buf, i, false);
        }
        let mut j = i - 1;
        if buf.at(j) == '_' {
            while j > 0 && buf.at(j - 1) == '_' {
                j -= 1;
            }
            return j;
        }
        while j > 0 && class_of(buf.at(j - 1)) == CharClass::Word {
            if is_sub_break(buf.at(j - 1), buf.at(j)) {
                break;
            }
            if buf.at(j - 1).is_uppercase()
                && buf.at(j).is_uppercase()
                && j < i
                && j + 1 < n
                && buf.at(j + 1).is_lowercase()
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

    /// 의존 없는 난수(xorshift64*).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
    }

    /// ★ 속성 테스트(docs/60 §5): 난수 편집(타이핑 · 붙여넣기 · Backspace/Delete · 선택 대체 · 다중 캐럿 · 여러 곳 바꾸기 ·
    /// 묶음 · 되돌리기/다시 실행 · 저장)을 **상태 스냅샷 모델**과 대조한다 — ① 되돌릴 때마다 그 단계 직전 본문과 같다
    /// ② 끝까지 되돌리면 원본 · 끝까지 다시 실행하면 최종 ③ `is_saved()`가 참이면 본문이 저장본과 같다 ④ 회계 바이트 ≥ 0.
    #[test]
    fn random_edits_match_snapshot_model() {
        for seed in 1..=40u64 {
            let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let base = "select a, b\n  from 한글표 t\n where x = 1;\n".repeat(3);
            let mut e = EditState::with_text(&base, false);
            e.mark_saved();
            let mut saved = base.clone();
            // 모델: 되돌리기 단계 수가 늘 때마다 "그 단계 직전 본문"을 쌓는다.
            let mut before: Vec<String> = Vec::new();
            for _ in 0..120 {
                let n = e.len();
                let (depth, text0) = (e.undo_len(), e.text());
                match r.below(12) {
                    0..=3 => {
                        let at = r.below(n + 1);
                        e.set_caret(at, false);
                        for c in ["ab", "x ", "가나", "\n", "q"][r.below(5)].chars() {
                            e.insert(c);
                        }
                    }
                    4 => {
                        e.set_caret(r.below(n + 1), false);
                        e.insert_str(["paste", "두 줄\n붙임\n", ""][r.below(3)]);
                    }
                    5 => {
                        e.set_caret(r.below(n + 1), false);
                        for _ in 0..r.below(4) {
                            e.backspace();
                        }
                    }
                    6 => {
                        e.set_caret(r.below(n + 1), false);
                        for _ in 0..r.below(4) {
                            e.key(EditKey::DeleteForward, false);
                        }
                    }
                    7 => {
                        let (a, b) = (r.below(n + 1), r.below(n + 1));
                        e.set_selection(a.min(b), a.max(b));
                        e.insert('S');
                    }
                    8 => {
                        // 다중 캐럿 3곳에 같은 글자.
                        let mut ps = [r.below(n + 1), r.below(n + 1), r.below(n + 1)];
                        ps.sort_unstable();
                        e.set_regions(&[(ps[0], ps[0]), (ps[1], ps[1]), (ps[2], ps[2])]);
                        e.insert('M');
                        if r.below(2) == 0 {
                            // 두 번째 원시 편집 = 새 단계 — 모델을 먼저 맞춘 뒤 이어 간다.
                            for _ in depth..e.undo_len() {
                                before.push(text0.clone());
                            }
                            let (d1, t1) = (e.undo_len(), e.text());
                            e.backspace();
                            for _ in d1..e.undo_len() {
                                before.push(t1.clone());
                            }
                        }
                        e.set_caret(0, false);
                    }
                    9 => {
                        // 여러 곳 바꾸기(오름차순·비겹침).
                        let mut cuts: Vec<usize> = (0..6).map(|_| r.below(n + 1)).collect();
                        cuts.sort_unstable();
                        let edits: Vec<(usize, usize, &str)> = cuts
                            .chunks(2)
                            .map(|c| (c[0], c[1], ["", "R", "긴 교체 글"][c[0] % 3]))
                            .collect();
                        e.replace_many(&edits);
                    }
                    10 => {
                        e.begin_group();
                        for _ in 0..3 {
                            let m = e.len();
                            e.set_selection(r.below(m + 1), r.below(m + 1));
                            e.insert_str("G");
                        }
                        e.end_group();
                    }
                    _ => {
                        if r.below(3) == 0 {
                            e.mark_saved();
                            saved = e.text();
                        } else if r.below(2) == 0 && e.undo() {
                            let want = before.pop().unwrap_or_default();
                            assert_eq!(e.text(), want, "seed {seed}: undo = 그 단계 직전 본문");
                            // 다시 실행하면 되돌리기 전으로.
                            if r.below(2) == 0 {
                                assert!(e.redo());
                                assert_eq!(e.text(), text0, "seed {seed}: redo = 되돌리기 전");
                                before.push(want);
                            }
                        }
                    }
                }
                // 새 단계가 열렸으면 그 직전 본문을 모델에 쌓는다(이어 붙인 타이핑은 단계가 늘지 않는다).
                for _ in before.len()..e.undo_len() {
                    before.push(text0.clone());
                }
                before.truncate(e.undo_len());
                if e.is_saved() {
                    assert_eq!(
                        e.text(),
                        saved,
                        "seed {seed}: 저장 지점 = 저장본과 같은 본문"
                    );
                }
            }
            let last = e.text();
            let mut steps = 0;
            while e.undo() {
                steps += 1;
            }
            assert_eq!(e.text(), base, "seed {seed}: 끝까지 되돌리면 원본");
            for _ in 0..steps {
                assert!(e.redo());
            }
            assert_eq!(e.text(), last, "seed {seed}: 끝까지 다시 실행하면 최종");
        }
    }

    /// 예산: 넘치면 오래된 단계부터 버리고(맨 위는 남긴다) · 저장 지점이 버려진 아래에 있으면 영구 더러움 · 회계가 맞는다.
    #[test]
    fn history_budget_evicts_oldest_and_keeps_accounting() {
        let mut e = EditState::with_text("base\n", false);
        e.mark_saved();
        e.set_history_budget(1 << 16);
        let chunk = "x".repeat(20_000);
        for _ in 0..10 {
            let end = e.len();
            e.set_selection(0, end.min(20_000));
            e.insert_str(&chunk); // 단계마다 ≈ 20 KB(지워진 글자)를 쥔다
        }
        assert!(
            e.history_bytes() <= (1 << 16) + 30_000,
            "{}",
            e.history_bytes()
        );
        assert!(e.undo_len() < 10 && e.undo_len() >= 1);
        while e.undo() {}
        assert!(!e.is_saved(), "저장 지점까지 내려갈 수 없다 = 더러움 유지");
        let sum: usize = e.redo.iter().map(|t| t.bytes).sum();
        assert_eq!(e.history_bytes(), sum, "회계 = 묶음 바이트의 합");
        // 하나가 예산보다 커도 그 단계는 되돌릴 수 있다.
        let mut e = EditState::with_text(&"y".repeat(200_000), false);
        e.set_history_budget(1 << 16);
        e.key(EditKey::SelectAll, false);
        e.backspace();
        assert_eq!(e.text(), "");
        assert!(e.undo());
        assert_eq!(e.len(), 200_000);
    }

    /// 기록 파일 왕복(D-129): 저장 직후 내보낸 기록을 **같은 본문을 새로 읽은** 편집 상태에 들이면 되돌리기·다시 실행이 그대로
    /// 이어지고(끝까지 되돌리면 원문) · 들인 직후는 "저장됨"이다 · 본문이 한 글자라도 다르거나 · 파일이 잘렸거나 · 한 바이트가
    /// 뒤집혔거나 · 이미 기록이 있으면 거부한다 · 예산이 작으면 오래된 단계부터 빠진다.
    #[test]
    fn history_file_round_trip_and_rejections() {
        let original = "select 1;\n-- 한글 주석\nselect 2;\n";
        let mut e = EditState::with_text(original, false);
        e.mark_saved();
        e.set_caret(9, false);
        e.insert_str(" -- 끝");
        e.set_caret(0, false);
        for c in "with x as (".chars() {
            e.insert(c);
        }
        e.set_regions(&[(0, 0), (20, 20)]);
        e.insert('#');
        e.set_caret(3, false);
        e.backspace();
        assert!(e.undo(), "다시 실행 한 단계를 남긴다");
        e.mark_saved();
        let saved_text = e.text();
        let (n_undo, can_redo) = (e.undo_len(), e.can_redo());
        let file = e.export_history(1 << 20).expect("history");

        let mut f = EditState::with_text(&saved_text, false);
        assert!(f.import_history(&file));
        assert!(f.is_saved(), "들인 직후 = 디스크와 같다");
        assert_eq!((f.undo_len(), f.can_redo()), (n_undo, can_redo));
        assert_eq!(f.history_bytes(), e.history_bytes());
        assert!(f.redo());
        assert!(!f.is_saved());
        assert!(f.undo());
        assert!(f.is_saved());
        while f.undo() {}
        assert_eq!(f.text(), original, "끝까지 되돌리면 원문");
        while f.redo() {}
        let mut g = e.clone();
        while g.redo() {}
        assert_eq!(f.text(), g.text());

        // 거부: 다른 본문 · 잘림 · 한 바이트 뒤집힘 · 이미 기록 있음 · 빈 기록.
        let mut other = EditState::with_text(&format!("{saved_text} "), false);
        assert!(!other.import_history(&file) && !other.can_undo());
        let mut fresh = EditState::with_text(&saved_text, false);
        assert!(!fresh.import_history(&file[..file.len() - 3]));
        let mut bad = file.clone();
        bad[40] ^= 1;
        assert!(!fresh.import_history(&bad) && !fresh.can_undo());
        assert!(!fresh.import_history(&[]));
        fresh.insert('x');
        assert!(
            !fresh.import_history(&file),
            "기록이 이미 있으면 들이지 않는다"
        );
        assert!(EditState::with_text("x", false)
            .export_history(1 << 20)
            .is_none());

        // 예산: 작으면 최신 단계만 · 그래도 들이면 그만큼은 되돌려진다.
        let small = e.export_history(260).expect("some");
        assert!(small.len() <= 260 && small.len() < file.len());
        let mut s = EditState::with_text(&saved_text, false);
        assert!(s.import_history(&small));
        assert!(s.undo_len() < n_undo && s.undo_len() >= 1);
        assert!(s.is_saved());
    }

    /// 거대 편집 확인(D-130): 기준 이상을 지우는 편집은 처음엔 막히고(본문·선택 그대로) 안내가 나온다 · **지우는 동작을 되풀이**하면
    /// 진행되고 히스토리가 빈다(되돌릴 수 없다) · 타이핑·붙여넣기는 몇 번을 해도 확인이 되지 않는다 · 기준 아래는 평소대로.
    #[test]
    fn giant_edit_needs_a_repeated_delete() {
        let body = "select 1;\n".repeat(200); // 2,000 B
        let mut e = EditState::with_text(&body, false);
        e.mark_saved();
        e.set_giant_limit(1000);
        e.key(EditKey::SelectAll, false);
        // 덮어쓰는 입력은 확인 수단이 아니다.
        for _ in 0..3 {
            e.insert('x');
            assert_eq!(e.take_giant_blocked(), Some((2000, false)));
            e.insert_str("pasted");
            assert_eq!(e.take_giant_blocked(), Some((2000, false)));
        }
        assert_eq!(e.text(), body);
        assert_eq!(e.selection(), Some((0, 2000)), "막혀도 선택은 그대로");
        assert!(e.cut().is_none());
        assert_eq!(
            e.take_giant_blocked(),
            Some((2000, true)),
            "지우는 동작 = 되풀이하면 된다"
        );
        assert_eq!(e.text(), body);
        // 되풀이 = 진행 + 히스토리 비움 + 더러움.
        e.backspace();
        assert_eq!(e.take_giant_blocked(), None);
        assert_eq!(e.text(), "");
        assert!(!e.can_undo() && !e.is_saved());
        assert_eq!(e.history_bytes(), 0);
        // 기준 아래 = 평소대로(되돌려진다).
        let mut e = EditState::with_text("select 1;", false);
        e.set_giant_limit(1000);
        e.key(EditKey::SelectAll, false);
        e.insert('x');
        assert_eq!(e.text(), "x");
        assert!(e.undo());
        assert_eq!(e.text(), "select 1;");
        // 모두 바꾸기도 같은 문지기(지우는 양의 합).
        let mut e = EditState::with_text(&body, false);
        e.set_giant_limit(1000);
        let edits: Vec<(usize, usize, &str)> =
            (0..200).map(|i| (i * 10, i * 10 + 9, "x")).collect();
        e.replace_many(&edits);
        assert_eq!(e.text(), body);
        assert_eq!(e.take_giant_blocked(), Some((1800, true)));
        e.replace_many(&edits);
        assert_eq!(e.text(), "x\n".repeat(200));
        assert!(!e.can_undo());
    }

    /// 긴 정지 뒤의 타이핑은 새 묶음(D-131) — 끄면(기본) 같은 단어는 한 묶음 그대로.
    #[test]
    fn long_pause_starts_a_new_undo_group() {
        let mut e = EditState::new();
        for c in "abc".chars() {
            e.insert(c);
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
        for c in "def".chars() {
            e.insert(c);
        }
        assert_eq!(e.undo_len(), 1, "끔 = 쉬어도 한 단어는 한 묶음");
        let mut e = EditState::new();
        e.set_group_pause_ms(5);
        for c in "abc".chars() {
            e.insert(c);
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
        for c in "def".chars() {
            e.insert(c);
        }
        assert_eq!(e.undo_len(), 2, "쉰 뒤 = 새 묶음");
        assert!(e.undo());
        assert_eq!(e.text(), "abc");
        assert!(e.undo());
        assert_eq!(e.text(), "");
    }

    /// 읽기 전용: 어떤 편집 길로도 본문이 바뀌지 않는다 · 캐럿·선택은 움직인다 · 끄면 다시 편집된다.
    #[test]
    fn read_only_blocks_every_mutation_path() {
        let mut e = EditState::with_text("select 1;\nselect 2;", false);
        e.set_read_only(true);
        let before = e.text();
        e.set_caret(3, false);
        e.insert('x');
        e.insert_str("paste");
        e.backspace();
        e.key(EditKey::DeleteForward, false);
        e.set_selection(0, 6);
        e.insert('y');
        e.set_selection(0, 6);
        assert!(
            e.cut().is_some(),
            "잘라내기 = 글은 돌려주지만 지우지 않는다"
        );
        e.replace_many(&[(0, 3, "zzz")]);
        e.set_regions(&[(0, 0), (5, 5)]);
        e.insert('m');
        assert_eq!(e.text(), before);
        assert!(!e.undo() && !e.can_undo());
        e.set_caret(4, false);
        assert_eq!(e.caret(), 4);
        e.set_read_only(false);
        e.insert('!');
        assert_ne!(e.text(), before);
    }

    /// 묶음 + 여러 곳 바꾸기 = 되돌리기 한 단계 · 타이핑은 글자를 저장하지 않는다(지운 글자만 저장).
    #[test]
    fn groups_and_typing_storage() {
        let mut e = EditState::with_text("aaa bbb aaa bbb aaa", false);
        e.replace_many(&[(0, 3, "X"), (8, 11, "X"), (16, 19, "X")]);
        assert_eq!(e.text(), "X bbb X bbb X");
        assert_eq!(e.undo_len(), 1);
        assert!(e.undo());
        assert_eq!(e.text(), "aaa bbb aaa bbb aaa");
        assert!(e.redo());
        assert_eq!(e.text(), "X bbb X bbb X");
        let mut e = EditState::new();
        for c in "typing".chars() {
            e.insert(c);
        }
        assert_eq!(
            (e.undo_len(), e.history_chars()),
            (1, 0),
            "타이핑 = 글자 0 저장"
        );
        e.backspace();
        e.backspace();
        assert_eq!(e.history_chars(), 2, "지운 글자만 저장");
        assert!(e.undo() && e.undo());
        assert_eq!(e.text(), "");
    }

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
        assert_eq!(word_boundary(b.as_slice(), 0, true), 14, "sales_customer|");
        assert_eq!(word_boundary(b.as_slice(), 14, true), 15, "구분자 `.` 런");
        assert_eq!(word_boundary(b.as_slice(), 15, true), 25, "HTMLParser|");
        assert_eq!(
            word_boundary(b.as_slice(), 25, true),
            30,
            "공백 건너뛰고 x1y|"
        );
        assert_eq!(word_boundary(b.as_slice(), 30, false), 27, "|x1y");
        assert_eq!(word_boundary(b.as_slice(), 14, false), 0);
        // subwords
        assert_eq!(subword_boundary(b.as_slice(), 0, true), 5, "sales|_");
        assert_eq!(subword_boundary(b.as_slice(), 5, true), 6, "_|customer");
        assert_eq!(subword_boundary(b.as_slice(), 6, true), 14);
        assert_eq!(subword_boundary(b.as_slice(), 15, true), 19, "HTML|Parser");
        assert_eq!(subword_boundary(b.as_slice(), 19, true), 25);
        assert_eq!(
            subword_boundary(b.as_slice(), 25, false),
            19,
            "HTML|Parser 왼쪽"
        );
        assert_eq!(
            subword_boundary(b.as_slice(), 14, false),
            6,
            "_|customer 왼쪽"
        );
        assert_eq!(subword_boundary(b.as_slice(), 30, false), 29, "x1|y");
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

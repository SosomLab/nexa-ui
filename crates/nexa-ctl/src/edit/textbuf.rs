//! **편집 버퍼** — UTF-8 갭 버퍼 + 줄 시작 표(nexa-sql docs/59 §4 2단계 · T-142 · D-126).
//!
//! 종전 `Vec<char>`(글자당 4 B)는 65 MB 파일에서 260 MB였고, 글자 하나를 넣을 때마다 그리기 쪽이 본문 전체를
//! 문자열로 다시 모으고 줄 표를 다시 만들었다(입력 ≈ 190 ms). 이 버퍼는:
//!
//! - **글자당 1 B(ASCII) ~ 3 B(한글)** — 파일 크기 그대로 + 갭.
//! - **편집 = O(편집 크기 + 갭 이동)** — 같은 자리 근처의 연속 편집은 갭 이동이 없다.
//! - **줄 표를 편집과 함께 고친다**(줄마다 `바이트 시작 · 글자 시작`) — 줄 ↔ 글자 인덱스가 이분 탐색이고,
//!   그리기는 보이는 줄만 꺼내 쓴다. ASCII만 있는 줄은 글자 → 바이트가 O(1).
//! - **변경 기록**(어느 줄들이 몇 줄로 바뀌었나) — 줄별 캐시(폭 · 구문 상태)를 가진 쪽이 바뀐 줄만 고친다.
//!
//! **좌표는 글자 인덱스 그대로다**(캐럿 · 선택 · 되돌리기 기록 · 호스트의 찾기/실행 범위/오류 줄이 모두 글자 단위) —
//! 바이트 오프셋은 이 파일 밖으로 나가지 않는다. 그래서 되돌리기(`Op`)와 호스트는 바뀌지 않는다.
//!
//! 불변식: 갭의 양 끝은 늘 글자 경계 · `lines[0] = (0, 0)` · `lines[k]` = k번째 `'\n'` 바로 뒤 ·
//! 논리 바이트열(갭을 뺀 것)은 늘 온전한 UTF-8. `unsafe` 없음(문자열로 꺼낼 때는 검증을 거친다 — 보이는 줄 몇십 개라 싸다).

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::VecDeque;

/// 줄 시작(논리 바이트 오프셋 · 글자 인덱스).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LineStart {
    byte: usize,
    ch: usize,
}

/// 줄 변경 한 건 — `first`번째 줄부터 `removed`줄이 `inserted`줄로 바뀌었다(그 뒤의 줄은 번호만 밀렸다).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineChange {
    pub first: usize,
    pub removed: usize,
    pub inserted: usize,
}

/// 변경 기록 보관 상한 — 넘으면 앞에서 버린다(그만큼 뒤처진 소비자는 전체를 다시 만든다).
const JOURNAL_MAX: usize = 512;
/// 갭을 새로 잡을 때의 여유(바이트) 하한·상한.
const GAP_MIN: usize = 4 << 10;
const GAP_MAX: usize = 4 << 20;
/// 위치 캐시에서 걸어갈 최대 글자 수(넘으면 줄 표로 찾는다).
const WALK_MAX: usize = 64;

/// UTF-8 갭 버퍼 + 줄 시작 표. 글자 인덱스로 말한다.
#[derive(Debug)]
pub struct TextBuf {
    /// 물리 바이트열 — `[..gap_start]` + 갭 + `[gap_end..]`.
    data: Vec<u8>,
    gap_start: usize,
    gap_end: usize,
    lines: Vec<LineStart>,
    n_chars: usize,
    /// 변경마다 +1(위치 캐시의 열쇠).
    stamp: u64,
    /// 통째 교체(새 문서 · 여러 곳 한 번에 바꾸기)마다 +1 — 다르면 줄별 캐시는 전부 다시.
    epoch: u64,
    /// 변경 기록의 첫 항목 번호 · 항목들(번호 = `journal_first + i`).
    journal_first: u64,
    journal: VecDeque<LineChange>,
    /// (stamp, 글자 인덱스, 논리 바이트) — 마지막으로 찾은 자리. 이웃한 조회는 여기서 걸어간다.
    pos_cache: Cell<(u64, usize, usize)>,
}

impl Default for TextBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for TextBuf {
    fn clone(&self) -> Self {
        Self::from_string(self.to_string())
    }
}

impl PartialEq for TextBuf {
    fn eq(&self, other: &Self) -> bool {
        self.len_bytes() == other.len_bytes() && self.bytes().eq(other.bytes())
    }
}

/// 글자 배열처럼 읽을 수 있는 것 — 단어 경계 같은 순수 함수가 `[char]`와 [`TextBuf`] 양쪽에서 돌게.
pub trait CharSeq {
    fn len(&self) -> usize;
    /// `i`번째 글자(범위 밖이면 `'\0'`).
    fn at(&self, i: usize) -> char;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl CharSeq for [char] {
    fn len(&self) -> usize {
        <[char]>::len(self)
    }
    fn at(&self, i: usize) -> char {
        self.get(i).copied().unwrap_or('\0')
    }
}

impl CharSeq for TextBuf {
    fn len(&self) -> usize {
        self.n_chars
    }
    fn at(&self, i: usize) -> char {
        self.get(i).unwrap_or('\0')
    }
}

/// 64비트 흐름 해시(8바이트 낱말 단위 · 조각 경계와 무관) — 암호학적 해시가 아니다(우연한 불일치만 막는다).
#[derive(Clone, Copy, Debug)]
pub struct Hash64 {
    h: u64,
    word: [u8; 8],
    n: usize,
    len: u64,
}

impl Default for Hash64 {
    fn default() -> Self {
        Self::new()
    }
}

impl Hash64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;

    #[must_use]
    pub fn new() -> Self {
        Hash64 {
            h: 0xcbf2_9ce4_8422_2325,
            word: [0; 8],
            n: 0,
            len: 0,
        }
    }

    fn mix(&mut self, w: u64) {
        self.h = (self.h.rotate_left(5) ^ w).wrapping_mul(Self::K);
    }

    pub fn write(&mut self, mut bytes: &[u8]) {
        self.len += bytes.len() as u64;
        if self.n > 0 {
            let take = (8 - self.n).min(bytes.len());
            self.word[self.n..self.n + take].copy_from_slice(&bytes[..take]);
            self.n += take;
            bytes = &bytes[take..];
            if self.n < 8 {
                return;
            }
            let w = u64::from_le_bytes(self.word);
            self.mix(w);
            self.n = 0;
        }
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            let mut w = [0u8; 8];
            w.copy_from_slice(c);
            self.mix(u64::from_le_bytes(w));
        }
        let rest = chunks.remainder();
        self.word[..rest.len()].copy_from_slice(rest);
        self.n = rest.len();
    }

    #[must_use]
    pub fn finish(mut self) -> u64 {
        if self.n > 0 {
            for b in &mut self.word[self.n..] {
                *b = 0;
            }
            let w = u64::from_le_bytes(self.word);
            self.mix(w);
        }
        let len = self.len;
        self.mix(len);
        self.h ^ (self.h >> 29)
    }
}

/// 바이트열 → 글(불변식상 늘 온전한 UTF-8 — 깨졌으면 빈 글).
fn ok(s: &[u8]) -> &str {
    std::str::from_utf8(s).unwrap_or_default()
}

/// UTF-8 첫 바이트 → 그 글자의 바이트 수.
fn width_of(lead: u8) -> usize {
    match lead {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

fn is_cont(b: u8) -> bool {
    b & 0xC0 == 0x80
}

/// 바이트열의 (글자 수, 개행 수).
fn count_chars_nl(bytes: &[u8]) -> (usize, usize) {
    let mut ch = 0usize;
    let mut nl = 0usize;
    for &b in bytes {
        ch += usize::from(!is_cont(b));
        nl += usize::from(b == b'\n');
    }
    (ch, nl)
}

impl TextBuf {
    #[must_use]
    pub fn new() -> Self {
        TextBuf {
            data: Vec::new(),
            gap_start: 0,
            gap_end: 0,
            lines: vec![LineStart { byte: 0, ch: 0 }],
            n_chars: 0,
            stamp: 0,
            epoch: 0,
            journal_first: 0,
            journal: VecDeque::new(),
            pos_cache: Cell::new((u64::MAX, 0, 0)),
        }
    }

    /// 문자열을 **그대로** 본문으로(바이트 복사 0 · 갭은 첫 편집 때 잡는다) + 줄 표를 한 번 훑어 만든다.
    /// `Send`라 작업 스레드에서 만들어 옮길 수 있다(큰 파일 적재).
    #[must_use]
    pub fn from_string(text: String) -> Self {
        let data = text.into_bytes();
        let n = data.len();
        let mut b = TextBuf {
            data,
            gap_start: n,
            gap_end: n,
            ..Self::new()
        };
        b.rebuild_lines();
        b
    }

    /// 본문을 통째로 바꾼다(새 문서) — 통째 교체 세대는 **이어서** 오른다(줄별 캐시가 "같은 세대"로 착각하지 않게).
    pub fn set_string(&mut self, text: String) {
        self.adopt(Self::from_string(text));
    }

    /// **비밀 값 지우기**(nexa-sql 09-21 — 일회성 비밀번호): 물리 바이트열을 0으로 덮어쓴 뒤 빈 본문으로 바꾼다.
    /// `black_box`로 덮어쓰기가 최적화로 사라지지 않게 한다(`unsafe` 없음). 버퍼가 자라며 옮겨 간 **옛 할당**은 이미 반환된
    /// 메모리라 닿지 않는다 — 가린 입력란은 짧아서 재할당이 드물다.
    pub fn wipe(&mut self) {
        self.data.fill(0);
        std::hint::black_box(&self.data);
        self.set_string(String::new());
    }

    /// 밖에서(작업 스레드에서) 만든 버퍼를 받아들인다 — 복사 0 · 세대는 이어서 오른다.
    pub fn adopt(&mut self, mut other: TextBuf) {
        other.epoch = self.epoch.max(other.epoch) + 1;
        other.stamp = self.stamp.wrapping_add(1);
        other.journal_first = self.change_seq();
        other.journal.clear();
        *self = other;
    }

    // ───────────────────────── 크기 · 줄 ─────────────────────────

    /// 글자 수.
    #[must_use]
    pub fn len(&self) -> usize {
        self.n_chars
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.n_chars == 0
    }

    /// 본문 바이트 수(UTF-8 · 갭 제외).
    #[must_use]
    pub fn len_bytes(&self) -> usize {
        self.data.len() - (self.gap_end - self.gap_start)
    }

    /// 논리 줄 수(빈 본문 = 1 · 끝이 개행이면 그 뒤의 빈 줄도 한 줄).
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// 줄의 첫 글자 인덱스(넘치면 본문 길이).
    #[must_use]
    pub fn line_start(&self, line: usize) -> usize {
        self.lines.get(line).map_or(self.n_chars, |l| l.ch)
    }

    /// 줄의 끝 = 그 줄의 `'\n'` 자리(마지막 줄이면 본문 길이) — 개행은 포함하지 않는다.
    #[must_use]
    pub fn line_end(&self, line: usize) -> usize {
        match self.lines.get(line + 1) {
            Some(next) => next.ch - 1,
            None => self.n_chars,
        }
    }

    /// 글자 인덱스가 속한 줄(개행은 자기 줄에 속한다 · 본문 끝 = 마지막 줄).
    #[must_use]
    pub fn line_of(&self, ch: usize) -> usize {
        self.lines.partition_point(|l| l.ch <= ch).saturating_sub(1)
    }

    /// `i`가 속한 줄의 시작 글자 인덱스.
    #[must_use]
    pub fn line_start_of(&self, i: usize) -> usize {
        self.line_start(self.line_of(i.min(self.n_chars)))
    }

    /// `i`가 속한 줄의 끝(`'\n'` 자리 또는 본문 길이).
    #[must_use]
    pub fn line_end_of(&self, i: usize) -> usize {
        self.line_end(self.line_of(i.min(self.n_chars)))
    }

    /// 줄의 (논리 바이트 시작, 끝 — 개행 제외).
    fn line_bytes(&self, line: usize) -> (usize, usize) {
        let Some(l) = self.lines.get(line) else {
            let n = self.len_bytes();
            return (n, n);
        };
        let end = match self.lines.get(line + 1) {
            Some(next) => next.byte - 1,
            None => self.len_bytes(),
        };
        (l.byte, end)
    }

    /// 줄의 글(개행 제외) — 갭에 걸친 줄 하나만 새로 만들고 나머지는 빌린다.
    #[must_use]
    pub fn line_text(&self, line: usize) -> Cow<'_, str> {
        let (a, b) = self.line_bytes(line);
        self.bytes_text(a, b)
    }

    /// 줄이 ASCII만인가(글자 수 == 바이트 수) — 글자 ↔ 바이트가 O(1)인 줄.
    #[must_use]
    pub fn line_is_ascii(&self, line: usize) -> bool {
        let (a, b) = self.line_bytes(line);
        b - a == self.line_end(line) - self.line_start(line)
    }

    // ───────────────────────── 읽기 ─────────────────────────

    fn phys(&self, logical: usize) -> usize {
        if logical < self.gap_start {
            logical
        } else {
            logical + (self.gap_end - self.gap_start)
        }
    }

    fn byte_at(&self, logical: usize) -> u8 {
        self.data[self.phys(logical)]
    }

    /// 갭을 뺀 바이트열(앞 절반 뒤에 뒤 절반).
    fn bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.data[..self.gap_start]
            .iter()
            .chain(self.data[self.gap_end..].iter())
            .copied()
    }

    /// 논리 바이트 `[a, b)`의 글(글자 경계여야 한다).
    fn bytes_text(&self, a: usize, b: usize) -> Cow<'_, str> {
        let (a, b) = (a.min(b), b.min(self.len_bytes()));
        if b <= self.gap_start {
            Cow::Borrowed(ok(&self.data[a..b]))
        } else if a >= self.gap_start {
            let g = self.gap_end - self.gap_start;
            Cow::Borrowed(ok(&self.data[a + g..b + g]))
        } else {
            let mut s = String::with_capacity(b - a);
            s.push_str(ok(&self.data[a..self.gap_start]));
            s.push_str(ok(
                &self.data[self.gap_end..self.gap_end + (b - self.gap_start)]
            ));
            Cow::Owned(s)
        }
    }

    /// 논리 바이트 `b`에서 시작하는 글자와 그 바이트 수.
    fn decode_at(&self, b: usize) -> (char, usize) {
        let p = self.phys(b);
        let w = width_of(self.data[p]).min(self.data.len() - p);
        let c = std::str::from_utf8(&self.data[p..p + w])
            .ok()
            .and_then(|s| s.chars().next())
            .unwrap_or('\u{FFFD}');
        (c, w)
    }

    /// 글자 인덱스 → 논리 바이트 오프셋. 이웃한 조회는 직전 자리에서 걸어가고, 멀면 줄 표(ASCII 줄은 O(1)).
    fn byte_of(&self, ch: usize) -> usize {
        if ch >= self.n_chars {
            return self.len_bytes();
        }
        let (st, cc, cb) = self.pos_cache.get();
        let (from_c, from_b) = if st == self.stamp && ch.abs_diff(cc) <= WALK_MAX {
            (cc, cb)
        } else {
            let line = self.line_of(ch);
            let l = self.lines[line];
            let (la, lb) = self.line_bytes(line);
            if lb - la == self.line_end(line) - l.ch {
                let b = l.byte + (ch - l.ch);
                self.pos_cache.set((self.stamp, ch, b));
                return b;
            }
            (l.ch, l.byte)
        };
        let (mut c, mut b) = (from_c, from_b);
        while c < ch {
            b += width_of(self.byte_at(b));
            c += 1;
        }
        while c > ch {
            b -= 1;
            while is_cont(self.byte_at(b)) {
                b -= 1;
            }
            c -= 1;
        }
        self.pos_cache.set((self.stamp, ch, b));
        b
    }

    /// `i`번째 글자.
    #[must_use]
    pub fn get(&self, i: usize) -> Option<char> {
        (i < self.n_chars).then(|| self.decode_at(self.byte_of(i)).0)
    }

    /// `[a, b)`의 글.
    #[must_use]
    pub fn slice(&self, a: usize, b: usize) -> Cow<'_, str> {
        let (a, b) = (a.min(self.n_chars), b.min(self.n_chars));
        if b <= a {
            return Cow::Borrowed("");
        }
        self.bytes_text(self.byte_of(a), self.byte_of(b))
    }

    /// `[a, b)`의 UTF-8 바이트 수(글을 만들지 않는다).
    #[must_use]
    pub fn byte_len(&self, a: usize, b: usize) -> usize {
        let (a, b) = (a.min(self.n_chars), b.min(self.n_chars));
        if b <= a {
            return 0;
        }
        self.byte_of(b) - self.byte_of(a)
    }

    /// `[a, b)`의 글(소유).
    #[must_use]
    pub fn slice_string(&self, a: usize, b: usize) -> String {
        self.slice(a, b).into_owned()
    }

    /// `[a, b)`의 글자들 — 짧은 구간을 글자 배열로 다루는 명령용(줄 몇 개 · 선택 한 덩이).
    #[must_use]
    pub fn slice_vec(&self, a: usize, b: usize) -> Vec<char> {
        self.slice(a, b).chars().collect()
    }

    /// `i`부터 앞으로 글자들.
    pub fn iter_from(&self, i: usize) -> impl Iterator<Item = char> + '_ {
        let mut b = self.byte_of(i.min(self.n_chars));
        let end = self.len_bytes();
        std::iter::from_fn(move || {
            (b < end).then(|| {
                let (c, w) = self.decode_at(b);
                b += w;
                c
            })
        })
    }

    /// `i` **앞**의 글자들을 뒤로 가며(`i-1`, `i-2`, …).
    pub fn iter_rev_from(&self, i: usize) -> impl Iterator<Item = char> + '_ {
        let mut b = self.byte_of(i.min(self.n_chars));
        std::iter::from_fn(move || {
            (b > 0).then(|| {
                b -= 1;
                while is_cont(self.byte_at(b)) {
                    b -= 1;
                }
                self.decode_at(b).0
            })
        })
    }

    /// `[a, b)` 안에 `c`가 있는가.
    #[must_use]
    pub fn contains_in(&self, a: usize, b: usize, c: char) -> bool {
        if c == '\n' {
            let (a, b) = (a.min(self.n_chars), b.min(self.n_chars));
            return b > a && self.line_of(a) != self.line_of(b);
        }
        self.iter_from(a).take(b.saturating_sub(a)).any(|x| x == c)
    }

    /// `i`에서 `pat`으로 시작하는가.
    #[must_use]
    pub fn starts_with_at(&self, i: usize, pat: &[char]) -> bool {
        i + pat.len() <= self.n_chars && self.iter_from(i).zip(pat).all(|(a, b)| a == *b)
    }

    /// 논리 바이트 오프셋(글자 경계) → 글자 인덱스 — 줄 표로 줄을 찾고 그 줄 안에서만 센다.
    fn char_of_byte(&self, byte: usize) -> usize {
        let byte = byte.min(self.len_bytes());
        let line = self
            .lines
            .partition_point(|l| l.byte <= byte)
            .saturating_sub(1);
        let l = self.lines[line];
        let (la, lb) = self.line_bytes(line);
        if lb - la == self.line_end(line) - l.ch {
            return l.ch + (byte - l.byte);
        }
        l.ch + (l.byte..byte)
            .filter(|&b| !is_cont(self.byte_at(b)))
            .count()
    }

    /// 글자 인덱스 `from`부터 `needle`의 첫 출현(글자 인덱스) — 본문을 문자열로 만들지 않고 두 절반을 바이트로 훑는다.
    #[must_use]
    pub fn find(&self, from: usize, needle: &str) -> Option<usize> {
        let nb = needle.as_bytes();
        let (&first, m) = (nb.first()?, nb.len());
        let len = self.len_bytes();
        let start = self.byte_of(from.min(self.n_chars));
        if start + m > len {
            return None;
        }
        let gs = self.gap_start;
        // 한 절반 안에서 찾기(`lo..hi` = 논리 바이트 · 그 절반에 온전히 들어 있는 출현만).
        let scan = |lo: usize, hi: usize, shift: usize| -> Option<usize> {
            if hi < lo + m {
                return None;
            }
            let hay = &self.data[lo + shift..hi + shift];
            let mut at = 0usize;
            while at + m <= hay.len() {
                let p = hay[at..=hay.len() - m].iter().position(|&b| b == first)?;
                let i = at + p;
                if hay[i..i + m] == *nb {
                    return Some(lo + i);
                }
                at = i + 1;
            }
            None
        };
        let g = self.gap_end - gs;
        let hit = None
            .or_else(|| (start < gs).then(|| scan(start, gs.min(len), 0)).flatten())
            // 갭에 걸친 출현 — 걸칠 수 있는 짧은 구간만 이어 붙여 본다.
            .or_else(|| {
                let lo = start.max(gs.saturating_sub(m - 1));
                let hi = (gs + m - 1).min(len);
                if m < 2 || lo >= gs || hi <= gs {
                    return None;
                }
                let mut w: Vec<u8> = Vec::with_capacity(hi - lo);
                self.push_bytes(&mut w, lo, hi);
                w.windows(m).position(|x| x == nb).map(|i| lo + i)
            })
            .or_else(|| scan(start.max(gs), len, g));
        hit.map(|b| self.char_of_byte(b))
    }

    /// 본문 전체(갭을 뺀 두 절반을 이어 붙인 새 문자열).
    fn collect_string(&self) -> String {
        let mut s = String::with_capacity(self.len_bytes());
        s.push_str(&self.bytes_text(0, self.gap_start.min(self.len_bytes())));
        s.push_str(&self.bytes_text(self.gap_start, self.len_bytes()));
        s
    }

    /// 본문의 64비트 해시 — 갭이 어디에 있든 같은 본문이면 같은 값(되돌리기 기록 파일이 "같은 본문인가"를 볼 때).
    #[must_use]
    pub fn content_hash(&self) -> u64 {
        let mut h = Hash64::new();
        h.write(&self.data[..self.gap_start]);
        h.write(&self.data[self.gap_end..]);
        h.finish()
    }

    /// 본문이 `s`와 같은가(문자열을 만들지 않는다).
    #[must_use]
    pub fn eq_str(&self, s: &str) -> bool {
        s.len() == self.len_bytes() && self.bytes().eq(s.bytes())
    }

    /// 쥐고 있는 바이트(본문 + 갭 + 줄 표).
    #[must_use]
    pub fn approx_bytes(&self) -> usize {
        self.data.capacity() + self.lines.capacity() * std::mem::size_of::<LineStart>()
    }

    // ───────────────────────── 변경 기록 ─────────────────────────

    /// 통째 교체 세대 — 값이 다르면 줄별 캐시는 전부 다시 만든다.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// 지금까지의 줄 변경 건수(다음 항목의 번호).
    #[must_use]
    pub fn change_seq(&self) -> u64 {
        self.journal_first + self.journal.len() as u64
    }

    /// `seq` 뒤의 줄 변경들 — 기록이 이미 버려졌으면 `None`(전부 다시 만들 것).
    #[must_use]
    pub fn changes_since(&self, seq: u64) -> Option<impl Iterator<Item = LineChange> + '_> {
        let skip = seq.checked_sub(self.journal_first)?;
        let skip = usize::try_from(skip).ok()?;
        (skip <= self.journal.len()).then(|| self.journal.iter().skip(skip).copied())
    }

    fn note_change(&mut self, c: LineChange) {
        self.journal.push_back(c);
        while self.journal.len() > JOURNAL_MAX {
            self.journal.pop_front();
            self.journal_first += 1;
        }
    }

    fn reset_journal(&mut self) {
        self.epoch += 1;
        self.journal_first = self.change_seq();
        self.journal.clear();
    }

    // ───────────────────────── 쓰기 ─────────────────────────

    /// 줄 표를 본문에서 다시 만든다(통째 교체 뒤) + 글자 수.
    fn rebuild_lines(&mut self) {
        let mut lines = Vec::with_capacity(self.lines.capacity().max(16));
        lines.push(LineStart { byte: 0, ch: 0 });
        let (mut byte, mut ch) = (0usize, 0usize);
        for half in [&self.data[..self.gap_start], &self.data[self.gap_end..]] {
            for &b in half {
                byte += 1;
                ch += usize::from(!is_cont(b));
                if b == b'\n' {
                    lines.push(LineStart { byte, ch });
                }
            }
        }
        lines.shrink_to_fit();
        self.lines = lines;
        self.n_chars = ch;
        self.stamp = self.stamp.wrapping_add(1);
        self.reset_journal();
    }

    /// 갭을 논리 바이트 `at`으로 옮기고 `need`바이트 이상을 확보한다.
    fn prepare_gap(&mut self, at: usize, need: usize) {
        let gap = self.gap_end - self.gap_start;
        if gap < need {
            let len = self.len_bytes();
            let room = need + (len / 16).clamp(GAP_MIN, GAP_MAX);
            let mut data = Vec::with_capacity(len + room);
            data.extend_from_slice(&self.data[..self.gap_start]);
            data.resize(self.gap_start + room, 0);
            data.extend_from_slice(&self.data[self.gap_end..]);
            self.gap_end = self.gap_start + room;
            self.data = data;
        }
        if at < self.gap_start {
            let n = self.gap_start - at;
            self.data.copy_within(at..self.gap_start, self.gap_end - n);
            self.gap_start = at;
            self.gap_end -= n;
        } else if at > self.gap_start {
            let n = at - self.gap_start;
            self.data
                .copy_within(self.gap_end..self.gap_end + n, self.gap_start);
            self.gap_start += n;
            self.gap_end += n;
        }
    }

    /// **한 곳 바꾸기** — `pos`에서 `del_n` 글자를 지우고 `ins`를 넣는다. 돌려주는 값 = 지운 글.
    /// 줄 표는 바뀐 줄만 고치고 그 뒤는 밀기만 한다 · 변경 기록 한 건.
    pub fn splice(&mut self, pos: usize, del_n: usize, ins: &str) -> String {
        let pos = pos.min(self.n_chars);
        let end = (pos + del_n).min(self.n_chars);
        if end == pos && ins.is_empty() {
            return String::new();
        }
        let (b0, b1) = (self.byte_of(pos), self.byte_of(end));
        let (l0, l1) = (self.line_of(pos), self.line_of(end));
        let removed = self.bytes_text(b0, b1).into_owned();
        // 본문.
        self.prepare_gap(b0, ins.len());
        self.gap_end += b1 - b0;
        self.data[self.gap_start..self.gap_start + ins.len()].copy_from_slice(ins.as_bytes());
        self.gap_start += ins.len();
        // 줄 표: 지워진 개행 뒤의 줄 시작들을 빼고, 넣은 개행 뒤의 줄 시작들을 넣고, 그 뒤를 민다.
        let (ins_chars, ins_nl) = count_chars_nl(ins.as_bytes());
        let mut fresh: Vec<LineStart> = Vec::with_capacity(ins_nl);
        if ins_nl > 0 {
            let mut ch = pos;
            for (i, &b) in ins.as_bytes().iter().enumerate() {
                ch += usize::from(!is_cont(b));
                if b == b'\n' {
                    fresh.push(LineStart {
                        byte: b0 + i + 1,
                        ch,
                    });
                }
            }
        }
        let tail_from = l0 + 1 + fresh.len();
        self.lines.splice(l0 + 1..=l1, fresh);
        let (db, dc) = (
            ins.len() as isize - (b1 - b0) as isize,
            ins_chars as isize - (end - pos) as isize,
        );
        if db != 0 || dc != 0 {
            for l in &mut self.lines[tail_from..] {
                l.byte = (l.byte as isize + db) as usize;
                l.ch = (l.ch as isize + dc) as usize;
            }
        }
        self.n_chars = (self.n_chars as isize + dc) as usize;
        self.stamp = self.stamp.wrapping_add(1);
        self.note_change(LineChange {
            first: l0,
            removed: l1 - l0 + 1,
            inserted: ins_nl + 1,
        });
        removed
    }

    /// **여러 곳을 한 번 훑어 바꾸기** — `edits` = 지금 좌표의 오름차순·비겹침 `(from, to, 새 글)`.
    /// 돌려주는 값 = 구간마다 `(결과 좌표의 시작, 넣은 글자 수, 지운 글, 지운 글자 수)`.
    /// 본문을 새로 짓고 줄 표도 다시 만든다(O(본문) · 통째 교체 세대 +1) — 몇 곳 안 되면 [`Self::splice`]가 싸다.
    pub fn replace_many(
        &mut self,
        edits: &[(usize, usize, &str)],
    ) -> Vec<(usize, usize, String, usize)> {
        let grow: usize = edits.iter().map(|e| e.2.len()).sum();
        let len = self.len_bytes();
        let mut out: Vec<u8> = Vec::with_capacity(len + grow);
        let mut notes = Vec::with_capacity(edits.len());
        let (mut at_b, mut at_c) = (0usize, 0usize);
        // `out`에 쌓인 글자 수(결과 좌표).
        let mut out_c = 0usize;
        for &(a, b, text) in edits {
            let a = a.min(self.n_chars).max(at_c);
            let b = b.min(self.n_chars).max(a);
            let (ba, bb) = (self.byte_of(a), self.byte_of(b));
            self.push_bytes(&mut out, at_b, ba);
            out_c += a - at_c;
            let ins_n = text.chars().count();
            notes.push((out_c, ins_n, self.bytes_text(ba, bb).into_owned(), b - a));
            out.extend_from_slice(text.as_bytes());
            out_c += ins_n;
            (at_b, at_c) = (bb, b);
        }
        self.push_bytes(&mut out, at_b, len);
        let n = out.len();
        self.data = out;
        self.gap_start = n;
        self.gap_end = n;
        self.rebuild_lines();
        notes
    }

    /// 논리 바이트 `[a, b)`를 `out`에 덧붙인다.
    fn push_bytes(&self, out: &mut Vec<u8>, a: usize, b: usize) {
        if b <= a {
            return;
        }
        let g = self.gap_end - self.gap_start;
        if b <= self.gap_start {
            out.extend_from_slice(&self.data[a..b]);
        } else if a >= self.gap_start {
            out.extend_from_slice(&self.data[a + g..b + g]);
        } else {
            out.extend_from_slice(&self.data[a..self.gap_start]);
            out.extend_from_slice(&self.data[self.gap_end..b + g]);
        }
    }

    /// 갭을 없애 쥔 메모리를 본문 크기로 줄인다(메모리 회수 — 보이지 않는 탭).
    pub fn shrink(&mut self) {
        if self.gap_end == self.gap_start && self.data.capacity() == self.data.len() {
            return;
        }
        let len = self.len_bytes();
        self.prepare_gap(len, 0);
        self.data.truncate(len);
        self.data.shrink_to_fit();
        self.gap_start = len;
        self.gap_end = len;
        self.lines.shrink_to_fit();
    }
}

impl std::fmt::Display for TextBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.collect_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 단순 모델(`Vec<char>`)과 모든 조회를 대조한다.
    fn check(b: &TextBuf, m: &[char]) {
        let text: String = m.iter().collect();
        assert_eq!(b.to_string(), text);
        assert_eq!(b.len(), m.len());
        assert_eq!(b.len_bytes(), text.len());
        assert!(b.eq_str(&text));
        // 줄 표 = 본문에서 새로 센 것.
        let mut starts = vec![0usize];
        starts.extend(
            m.iter()
                .enumerate()
                .filter(|(_, c)| **c == '\n')
                .map(|(i, _)| i + 1),
        );
        assert_eq!(b.line_count(), starts.len());
        for (l, &s) in starts.iter().enumerate() {
            assert_eq!(b.line_start(l), s, "line {l}");
            let e = starts.get(l + 1).map_or(m.len(), |n| n - 1);
            assert_eq!(b.line_end(l), e);
            let lt: String = m[s..e].iter().collect();
            assert_eq!(b.line_text(l), lt);
            assert_eq!(b.line_is_ascii(l), lt.is_ascii());
        }
        for (i, &c) in m.iter().enumerate() {
            assert_eq!(b.get(i), Some(c), "char {i}");
            let l = starts.partition_point(|s| *s <= i) - 1;
            assert_eq!(b.line_of(i), l);
        }
        assert_eq!(b.get(m.len()), None);
        assert_eq!(b.line_of(m.len()), starts.len() - 1);
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % (n.max(1) as u64)) as usize
        }
    }

    const ALPHABET: [&str; 12] = [
        "a",
        "B",
        " ",
        "\n",
        "한",
        "글",
        "é",
        "😀",
        "\t",
        "select",
        "\n\n",
        "값 = 1;\n",
    ];

    /// 난수 편집(한 곳 · 여러 곳 · 앞/뒤/가운데 · 한글·이모지·개행)을 단순 모델과 대조 + 변경 기록으로 줄 수를 따라간다.
    #[test]
    fn random_edits_match_vec_model() {
        for seed in 1..=30u64 {
            let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            let mut b = TextBuf::from_string("첫 줄\nsecond line\n".into());
            let mut m: Vec<char> = b.to_string().chars().collect();
            let (mut epoch, mut seq, mut lines) = (b.epoch(), b.change_seq(), b.line_count());
            for step in 0..150 {
                if rng.below(10) == 0 {
                    // 여러 곳 한 번에.
                    let mut edits: Vec<(usize, usize, String)> = Vec::new();
                    let mut at = 0usize;
                    for _ in 0..rng.below(5) + 1 {
                        let a = at + rng.below(m.len().saturating_sub(at) + 1);
                        let e = (a + rng.below(4)).min(m.len());
                        edits.push((a, e, ALPHABET[rng.below(ALPHABET.len())].to_string()));
                        at = e;
                    }
                    let list: Vec<(usize, usize, &str)> =
                        edits.iter().map(|(a, e, s)| (*a, *e, s.as_str())).collect();
                    let notes = b.replace_many(&list);
                    let mut out: Vec<char> = Vec::new();
                    let mut at = 0usize;
                    for (k, (a, e, s)) in edits.iter().enumerate() {
                        out.extend_from_slice(&m[at..*a]);
                        assert_eq!(notes[k].0, out.len(), "seed {seed} step {step}");
                        assert_eq!(notes[k].2, m[*a..*e].iter().collect::<String>());
                        assert_eq!((notes[k].1, notes[k].3), (s.chars().count(), e - a));
                        out.extend(s.chars());
                        at = *e;
                    }
                    out.extend_from_slice(&m[at..]);
                    m = out;
                } else {
                    let pos = rng.below(m.len() + 1);
                    let del = rng.below(6).min(m.len() - pos);
                    let mut ins = String::new();
                    for _ in 0..rng.below(4) {
                        ins.push_str(ALPHABET[rng.below(ALPHABET.len())]);
                    }
                    let removed = b.splice(pos, del, &ins);
                    assert_eq!(removed, m[pos..pos + del].iter().collect::<String>());
                    m.splice(pos..pos + del, ins.chars());
                }
                // 변경 기록을 따라가면 줄 수가 맞는다(통째 교체면 새로 읽는다).
                if b.epoch() == epoch {
                    for c in b.changes_since(seq).expect("journal") {
                        lines = lines - c.removed + c.inserted;
                    }
                } else {
                    epoch = b.epoch();
                    lines = b.line_count();
                }
                seq = b.change_seq();
                assert_eq!(lines, b.line_count(), "seed {seed} step {step}");
                if step % 10 == 0 {
                    check(&b, &m);
                }
            }
            check(&b, &m);
            b.shrink();
            check(&b, &m);
        }
    }

    #[test]
    fn empty_and_edges() {
        let mut b = TextBuf::new();
        check(&b, &[]);
        assert_eq!(b.splice(0, 5, ""), "");
        b.splice(0, 0, "\n");
        check(&b, &['\n']);
        b.splice(1, 0, "끝");
        check(&b, &['\n', '끝']);
        assert_eq!(b.splice(0, 99, ""), "\n끝");
        check(&b, &[]);
        // 범위 밖 조회는 조용히 잘린다.
        assert_eq!(b.slice(3, 9), "");
        assert_eq!(b.line_start(7), 0);
    }

    #[test]
    fn iterators_slices_and_queries() {
        let text = "ab\n한글 cd\n\nxyz";
        let mut b = TextBuf::from_string(text.into());
        // 갭을 가운데에 만들어 걸친 조회를 본다.
        b.splice(5, 0, "_");
        b.splice(5, 1, "");
        let m: Vec<char> = text.chars().collect();
        for a in 0..=m.len() {
            for e in a..=m.len() {
                assert_eq!(b.slice(a, e), m[a..e].iter().collect::<String>());
            }
            assert_eq!(b.iter_from(a).collect::<Vec<_>>(), m[a..].to_vec());
            let mut rev = m[..a].to_vec();
            rev.reverse();
            assert_eq!(b.iter_rev_from(a).collect::<Vec<_>>(), rev);
            assert_eq!(
                b.line_start_of(a),
                m[..a].iter().rposition(|c| *c == '\n').map_or(0, |p| p + 1)
            );
            assert_eq!(
                b.line_end_of(a),
                m[a..]
                    .iter()
                    .position(|c| *c == '\n')
                    .map_or(m.len(), |p| a + p)
            );
        }
        assert!(b.contains_in(0, 3, '\n') && !b.contains_in(0, 2, '\n'));
        assert!(b.contains_in(3, 8, '글') && !b.contains_in(3, 4, '글'));
        assert!(b.starts_with_at(3, &['한', '글']) && !b.starts_with_at(12, &['y', 'z', '!']));
        assert_eq!(b.slice_vec(3, 5), vec!['한', '글']);
        assert_eq!(CharSeq::at(&b, 3), '한');
        assert_eq!(CharSeq::at(&b, 999), '\0');
    }

    /// 내용 해시: 갭이 어디에 있든 같다 · 조각을 어떻게 나눠 넣어도 같다 · 본문이 다르면 다르다.
    #[test]
    fn content_hash_ignores_gap_position() {
        let text = "select 값 from 표;\n-- 주석 0123456789 abcdefg\n";
        let base = TextBuf::from_string(text.into()).content_hash();
        for at in 0..text.chars().count() {
            let mut b = TextBuf::from_string(text.into());
            b.splice(at, 0, "#");
            assert_ne!(b.content_hash(), base);
            b.splice(at, 1, "");
            assert_eq!(b.content_hash(), base, "gap at {at}");
        }
        let mut h = Hash64::new();
        for piece in text.as_bytes().chunks(3) {
            h.write(piece);
        }
        assert_eq!(h.finish(), base);
        assert_ne!(
            TextBuf::new().content_hash(),
            TextBuf::from_string("\0".into()).content_hash()
        );
    }

    /// 찾기: 모든 시작점에서 `str::find`와 같은 답(갭의 앞 · 뒤 · 걸침 · 한글 · 없는 글).
    #[test]
    fn find_matches_str_find() {
        let text = "select 값 from 표 where 값 = '값값'; -- select\nselect 1;";
        let m: Vec<char> = text.chars().collect();
        for gap_at in [0usize, 9, 10, 25, m.len()] {
            let mut b = TextBuf::from_string(text.into());
            b.splice(gap_at, 0, "#");
            b.splice(gap_at, 1, "");
            for needle in [
                "select",
                "값",
                "값값",
                "표 where",
                "; -- s",
                "zzz",
                "\nselect 1;",
            ] {
                for from in 0..=m.len() {
                    let tail: String = m[from..].iter().collect();
                    let want = tail
                        .find(needle)
                        .map(|bi| from + tail[..bi].chars().count());
                    assert_eq!(
                        b.find(from, needle),
                        want,
                        "gap {gap_at} {needle:?} from {from}"
                    );
                }
            }
            assert_eq!(b.find(0, ""), None);
        }
    }

    /// 변경 기록: 한 줄 안 = (줄, 1, 1) · 개행 넣기 = +1줄 · 줄을 걸쳐 지우기 = 걸친 줄 수 · 기록이 넘치면 None.
    #[test]
    fn journal_describes_line_changes() {
        let mut b = TextBuf::from_string("a\nb\nc\nd".into());
        let seq = b.change_seq();
        b.splice(2, 0, "x");
        b.splice(3, 0, "\ny\n");
        b.splice(0, 5, "");
        let got: Vec<LineChange> = b.changes_since(seq).expect("journal").collect();
        assert_eq!(
            got,
            vec![
                LineChange {
                    first: 1,
                    removed: 1,
                    inserted: 1
                },
                LineChange {
                    first: 1,
                    removed: 1,
                    inserted: 3
                },
                LineChange {
                    first: 0,
                    removed: 3,
                    inserted: 1
                },
            ]
        );
        assert_eq!(b.changes_since(b.change_seq()).expect("empty").count(), 0);
        for _ in 0..JOURNAL_MAX + 3 {
            b.splice(0, 0, "z");
        }
        assert!(b.changes_since(seq).is_none(), "버려진 기록 = 전부 다시");
        let e = b.epoch();
        b.replace_many(&[(0, 1, "Q")]);
        assert_ne!(b.epoch(), e);
    }

    /// 큰 본문에서 먼 곳을 오가도 조회가 맞다(위치 캐시 · ASCII 줄 지름길 · 한글 줄 걷기).
    #[test]
    fn far_lookups_on_long_text() {
        let mut text = String::new();
        for i in 0..3000 {
            if i % 3 == 0 {
                text.push_str(&format!("-- 한글 주석 {i} 줄\n"));
            } else {
                text.push_str(&format!("select {i} from dual;\n"));
            }
        }
        let m: Vec<char> = text.chars().collect();
        let b = TextBuf::from_string(text);
        let mut rng = Rng(42);
        for _ in 0..4000 {
            let i = rng.below(m.len());
            assert_eq!(b.get(i), Some(m[i]));
        }
        assert_eq!(b.line_count(), 3001);
        assert_eq!(b.line_text(3), "-- 한글 주석 3 줄");
    }
}

//! **타입어헤드 부품** — 목록/트리에서 글자를 치면 그 접두로 시작하는 항목으로 점프(nexa-beep `typeahead.rs` 이식 ·
//! nexa-sql 오브젝트 탐색기 사용자 09-19 "beep의 장점을 모두 반영 · 다른 영역에도 재사용").
//!
//! 나뉜 책임(재사용의 핵심):
//! - [`TypeAhead`] = **버퍼 + 한글 조합기 + 타임아웃**(시각 주입 → 순수 로직 · 전 플랫폼 테스트). 매칭은 안 한다.
//! - [`TypeAheadFilter`] = 어떤 글자를 버퍼에 넣는가(한/영/숫자 항상 · 공백·특수문자는 설정).
//! - [`find_prefix`]/[`find_prefix_rev`] = 라벨 접두 매칭·순환(소비 위젯이 라벨 함수를 준다 — 목록마다 매칭 대상이 다르다).
//! - [`HudPos`] + [`paint_hud`] = 입력 중 접두를 보여 주는 작은 카드(3×3 위치).
//!
//! 규칙(nexa-beep 사용자 확정): 마지막 활동 뒤 `timeout_ms`가 지나면 초기화 · **↑/↓는 접두 매치 안에서만 순환**하고 그동안
//! 타임아웃 기준을 되돌린다(`touch`) · 같은 키 반복도 **누적**(자동 순환 없음 — 순환은 ↑/↓ 전용) · Backspace = 조합 중이면
//! 자모 단위, 아니면 글자 단위 · Esc = 즉시 초기화. 한글은 [`crate::hangul::Composer`]가 앱 안에서 조합한다(IME 탈피).

use crate::draw::{DrawCtx, FontSlot};
use crate::geom::Rect;
use crate::hangul::Composer;
use crate::theme::Theme;

/// 기본 타임아웃(ms) — nexa-beep 사용자 확정 2000.
pub const TYPEAHEAD_TIMEOUT_MS: u64 = 2000;

/// 입력 결과 — 검색 접두사와 시작점 규칙.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Query {
    /// 검색 접두사(확정 글자 + 조합 중 글자).
    pub prefix: String,
    /// `true` = 접두사 확장(지금 행 포함해 재평가) · `false` = 새 입력(다음 행부터).
    pub include_caret: bool,
}

/// 버퍼 + 조합기 + 타임아웃.
#[derive(Debug)]
pub struct TypeAhead {
    buf: String,
    composer: Composer,
    last_ms: u64,
    timeout_ms: u64,
}

impl Default for TypeAhead {
    fn default() -> Self {
        Self::new(TYPEAHEAD_TIMEOUT_MS)
    }
}

impl TypeAhead {
    /// 타임아웃(ms)으로 생성.
    #[must_use]
    pub fn new(timeout_ms: u64) -> Self {
        TypeAhead {
            buf: String::new(),
            composer: Composer::new(),
            last_ms: 0,
            timeout_ms: timeout_ms.max(1),
        }
    }

    /// 확정 버퍼(테스트·내부용).
    #[must_use]
    pub fn text(&self) -> &str {
        &self.buf
    }

    /// 확정 버퍼 + 조합 중 글자 = HUD 표시·매칭 접두사. 빈 값 = 비활성.
    #[must_use]
    pub fn composing(&self) -> String {
        let mut s = self.buf.clone();
        if let Some(p) = self.composer.preview() {
            s.push(p);
        }
        s
    }

    /// 입력 중인가(버퍼나 조합이 살아 있음).
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.buf.is_empty() || self.composer.is_composing()
    }

    /// 타임아웃 변경(설정).
    pub fn set_timeout(&mut self, ms: u64) {
        self.timeout_ms = ms.max(1);
    }

    /// 활동 갱신 — 살아 있으면 타임아웃 기준을 지금으로(↑/↓ 순환 중 유지).
    pub fn touch(&mut self, now_ms: u64) {
        if self.is_active() {
            self.last_ms = now_ms;
        }
    }

    /// 즉시 초기화(Esc · 포커스 이탈).
    pub fn clear(&mut self) {
        self.buf.clear();
        self.composer.reset();
    }

    /// 글자 입력. 타임아웃이 지났으면 새 접두사로. 자모는 조합기로(완성 글자만 버퍼에) · 그 외는 그대로 누적.
    pub fn push(&mut self, c: char, now_ms: u64) -> Query {
        let was_empty = !self.is_active();
        let expired = was_empty || now_ms.saturating_sub(self.last_ms) > self.timeout_ms;
        self.last_ms = now_ms;
        if expired {
            self.clear();
        }
        self.buf.push_str(&self.composer.feed(c));
        Query {
            prefix: self.composing(),
            include_caret: !expired,
        }
    }

    /// Backspace — 접두사 축소 뒤 재평가. 비었으면(또는 타임아웃 뒤) `None`.
    pub fn backspace(&mut self, now_ms: u64) -> Option<Query> {
        if now_ms.saturating_sub(self.last_ms) > self.timeout_ms {
            self.clear();
            return None;
        }
        self.last_ms = now_ms;
        if !self.composer.backspace() {
            self.buf.pop();
        }
        let p = self.composing();
        if p.is_empty() {
            None
        } else {
            Some(Query {
                prefix: p,
                include_caret: true,
            })
        }
    }

    /// 주기 점검 — 타임아웃이 지났으면 초기화하고 `true`(HUD 소거 → 다시 그리기).
    pub fn tick(&mut self, now_ms: u64) -> bool {
        if self.is_active() && now_ms.saturating_sub(self.last_ms) > self.timeout_ms {
            self.clear();
            true
        } else {
            false
        }
    }
}

/// 어떤 글자를 버퍼에 넣는가 — 한글·영문·숫자는 항상 · 공백/특수문자는 설정(nexa-beep `ui.typeahead_space/special`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TypeAheadFilter {
    pub space: bool,
    pub special: bool,
}

impl Default for TypeAheadFilter {
    fn default() -> Self {
        Self {
            space: true,
            special: true,
        }
    }
}

impl TypeAheadFilter {
    /// 이 글자를 받는가(제어 문자는 항상 거른다).
    #[must_use]
    pub fn accepts(&self, c: char) -> bool {
        if c.is_control() {
            return false;
        }
        if c == ' ' {
            return self.space;
        }
        if c.is_alphanumeric() || crate::hangul::is_jamo(c) {
            return true;
        }
        self.special
    }
}

/// HUD 위치(3×3).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HudPos {
    TopLeft,
    TopCenter,
    TopRight,
    MidLeft,
    Center,
    MidRight,
    #[default]
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl HudPos {
    /// 설정 문자열(`top_left` …) → 위치(모르면 기본).
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "top_left" => Self::TopLeft,
            "top_center" => Self::TopCenter,
            "top_right" => Self::TopRight,
            "mid_left" => Self::MidLeft,
            "center" => Self::Center,
            "mid_right" => Self::MidRight,
            "bottom_center" => Self::BottomCenter,
            "bottom_right" => Self::BottomRight,
            _ => Self::BottomLeft,
        }
    }

    /// 짧은 코드(`tl…br` · [`crate::controls::PositionPicker`]/[`crate::controls::PositionDropdown`] 값 체계) → 위치.
    #[must_use]
    pub fn from_code(code: &str) -> Self {
        match code.trim() {
            "tl" => Self::TopLeft,
            "tc" => Self::TopCenter,
            "tr" => Self::TopRight,
            "ml" => Self::MidLeft,
            "c" => Self::Center,
            "mr" => Self::MidRight,
            "bc" => Self::BottomCenter,
            "br" => Self::BottomRight,
            _ => Self::BottomLeft,
        }
    }

    /// 짧은 코드.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::TopLeft => "tl",
            Self::TopCenter => "tc",
            Self::TopRight => "tr",
            Self::MidLeft => "ml",
            Self::Center => "c",
            Self::MidRight => "mr",
            Self::BottomLeft => "bl",
            Self::BottomCenter => "bc",
            Self::BottomRight => "br",
        }
    }

    /// 설정 문자열.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TopLeft => "top_left",
            Self::TopCenter => "top_center",
            Self::TopRight => "top_right",
            Self::MidLeft => "mid_left",
            Self::Center => "center",
            Self::MidRight => "mid_right",
            Self::BottomLeft => "bottom_left",
            Self::BottomCenter => "bottom_center",
            Self::BottomRight => "bottom_right",
        }
    }
}

/// 접두 매치 — `from`부터 **앞으로** 순환(대소문자 무시 · 라벨은 소비 위젯이 준다).
pub fn find_prefix(
    n: usize,
    from: usize,
    prefix: &str,
    label: impl Fn(usize) -> String,
) -> Option<usize> {
    if n == 0 || prefix.is_empty() {
        return None;
    }
    let p = prefix.to_lowercase();
    (0..n)
        .map(|k| (from + k) % n)
        .find(|&i| label(i).to_lowercase().starts_with(&p))
}

/// 접두 매치 — `from`부터 **뒤로** 순환(↑ 순환용).
pub fn find_prefix_rev(
    n: usize,
    from: usize,
    prefix: &str,
    label: impl Fn(usize) -> String,
) -> Option<usize> {
    if n == 0 || prefix.is_empty() {
        return None;
    }
    let p = prefix.to_lowercase();
    (0..n)
        .map(|k| (from % n + n - (k % n)) % n)
        .find(|&i| label(i).to_lowercase().starts_with(&p))
}

/// 입력 중 접두를 보여 주는 HUD 카드(nexa-beep 목록과 같은 모양 · 기본 글꼴 · accent 글자 · 배율 `scale`).
pub fn paint_hud(
    ctx: &mut dyn DrawCtx,
    bounds: Rect,
    scale: f32,
    pos: HudPos,
    text: &str,
    theme: &Theme,
) {
    if text.is_empty() {
        return;
    }
    let s = |v: f32| (v * scale).round() as i32;
    ctx.select_font(FontSlot::Base, false);
    let w = ctx.text_width(text) + s(16.0);
    let hh = s(20.0);
    let m = s(8.0);
    let left = bounds.x + m;
    let cx = bounds.x + (bounds.w - w) / 2;
    let right = bounds.right() - w - m;
    let topy = bounds.y + m;
    let midy = bounds.y + (bounds.h - hh) / 2;
    let boty = bounds.bottom() - hh - m;
    let (hx, hy) = match pos {
        HudPos::TopLeft => (left, topy),
        HudPos::TopCenter => (cx, topy),
        HudPos::TopRight => (right, topy),
        HudPos::MidLeft => (left, midy),
        HudPos::Center => (cx, midy),
        HudPos::MidRight => (right, midy),
        HudPos::BottomLeft => (left, boty),
        HudPos::BottomCenter => (cx, boty),
        HudPos::BottomRight => (right, boty),
    };
    let hud = Rect::new(hx, hy, w, hh);
    ctx.fill_round_rect(hud, s(6.0), theme.field_bg);
    ctx.stroke_round_rect(hud, s(6.0), theme.border, 1.0);
    ctx.text(hud.x + s(8.0), hud.y + s(3.0), hud, text, theme.accent);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accumulates_within_timeout_and_resets_after() {
        let mut t = TypeAhead::new(1000);
        assert_eq!(
            t.push('r', 0),
            Query {
                prefix: "r".into(),
                include_caret: false
            }
        );
        assert_eq!(
            t.push('e', 500),
            Query {
                prefix: "re".into(),
                include_caret: true
            }
        );
        // 1000ms 초과 → 새 접두사(다음 행부터).
        assert_eq!(
            t.push('x', 1600),
            Query {
                prefix: "x".into(),
                include_caret: false
            }
        );
    }

    #[test]
    fn hangul_composes_live_and_mixes_with_latin() {
        let mut t = TypeAhead::new(1000);
        assert_eq!(t.push('ㄱ', 0).prefix, "ㄱ");
        assert_eq!(t.push('ㅣ', 50).prefix, "기");
        assert_eq!(t.push('ㅁ', 100).prefix, "김");
        assert_eq!(
            t.push('ㅊ', 150).prefix,
            "김ㅊ",
            "받침 뒤 자음 = 새 글자 시작"
        );
        assert_eq!(t.push('ㅗ', 200).prefix, "김초");
        assert_eq!(t.push('ㅣ', 250).prefix, "김최");
        assert_eq!(
            t.push('_', 300).prefix,
            "김최_",
            "특수문자 = 조합 확정 뒤 그대로"
        );
        assert_eq!(t.push('a', 350).prefix, "김최_a");
        assert_eq!(t.text(), "김최_a");
    }

    #[test]
    fn repeated_key_accumulates_no_auto_cycle() {
        let mut t = TypeAhead::new(1000);
        t.push('b', 0);
        let q = t.push('b', 300);
        assert_eq!(q.prefix, "bb");
        assert!(q.include_caret, "누적 = 확장 매치");
    }

    #[test]
    fn backspace_is_jamo_wise_then_char_wise_then_ends() {
        let mut t = TypeAhead::new(1000);
        t.push('a', 0);
        t.push('ㄱ', 50);
        t.push('ㅣ', 100);
        assert_eq!(t.composing(), "a기");
        assert_eq!(
            t.backspace(150).unwrap().prefix,
            "aㄱ",
            "조합 중 = 자모 단위"
        );
        assert_eq!(t.backspace(200).unwrap().prefix, "a");
        assert_eq!(t.backspace(250), None, "비면 끝");
        assert_eq!(t.backspace(300), None);
        t.push('x', 400);
        assert_eq!(t.backspace(2000), None, "타임아웃 뒤 Backspace = 초기화");
        assert!(!t.is_active());
    }

    #[test]
    fn touch_keeps_buffer_alive_and_tick_clears_after_timeout() {
        let mut t = TypeAhead::new(1000);
        t.push('a', 0);
        assert!(!t.tick(900));
        t.touch(900); // ↑/↓ 순환 = 활동
        assert!(!t.tick(1800), "순환이 기준을 옮겼다");
        assert!(t.tick(2000));
        assert_eq!(t.text(), "");
        assert!(!t.tick(3000));
    }

    #[test]
    fn filter_rules() {
        let f = TypeAheadFilter::default();
        assert!(f.accepts('a') && f.accepts('7') && f.accepts('김') && f.accepts('ㄱ'));
        assert!(f.accepts(' ') && f.accepts('_') && f.accepts('*'));
        assert!(!f.accepts('\u{8}'), "제어 문자는 늘 거른다");
        let f = TypeAheadFilter {
            space: false,
            special: false,
        };
        assert!(!f.accepts(' ') && !f.accepts('-') && f.accepts('Z'));
    }

    #[test]
    fn prefix_search_cycles_both_ways_case_insensitive() {
        let labels = ["Alpha", "beta", "Bravo", "gamma", "delta"];
        let label = |i: usize| labels[i].to_string();
        assert_eq!(find_prefix(5, 0, "b", label), Some(1));
        assert_eq!(find_prefix(5, 2, "b", label), Some(2), "지금 행 포함");
        assert_eq!(find_prefix(5, 3, "b", label), Some(1), "끝 지나 순환");
        assert_eq!(find_prefix(5, 2, "br", label), Some(2));
        assert_eq!(find_prefix_rev(5, 0, "b", label), Some(2), "뒤로 순환");
        assert_eq!(find_prefix_rev(5, 1, "b", label), Some(1));
        assert_eq!(find_prefix(5, 0, "zz", label), None);
        assert_eq!(find_prefix(0, 0, "a", label), None);
        assert_eq!(find_prefix(5, 0, "", label), None);
    }

    #[test]
    fn hud_pos_roundtrip() {
        for p in [
            HudPos::TopLeft,
            HudPos::TopCenter,
            HudPos::TopRight,
            HudPos::MidLeft,
            HudPos::Center,
            HudPos::MidRight,
            HudPos::BottomLeft,
            HudPos::BottomCenter,
            HudPos::BottomRight,
        ] {
            assert_eq!(HudPos::parse(p.as_str()), p);
            assert_eq!(HudPos::from_code(p.code()), p);
        }
        assert_eq!(HudPos::parse("bogus"), HudPos::BottomLeft);
    }
}

//! ★ **디자인 토큰** — [docs/25](../../../docs/25-design-system.md)의 수치를 코드로 못 박는다.
//!
//! **Material에서 규칙을, macOS에서 느낌을**([DR-27](../../../docs/10-decision-record.md)).
//! Material은 *얼마나 띄우고 얼마나 크게*를, macOS는 *얼마나 둥글고 부드러운가*를 정한다.
//!
//! ## 왜 상수로 두는가
//!
//! beep과의 격차 진단([docs/25 §1](../../../docs/25-design-system.md))에서 *"간격 리듬이 산발"* 이
//! 나왔다. **"3px만 더"가 쌓이면 리듬이 무너진다** — 눈대중을 없애려면 값에 이름을 줘야 한다.

use crate::theme::Color;

/// 간격 — **4px 그리드**. 이 여섯 개 밖의 값을 쓰지 않는다.
pub mod space {
    /// 4 — 아이콘과 글자 사이 같은 최소 간격.
    pub const XS: i32 = 4;
    /// 8 — 컨트롤 사이.
    pub const S: i32 = 8;
    /// 12 — 컨트롤 안쪽 좌우 여백 · 창 가장자리.
    pub const M: i32 = 12;
    /// 16 — 묶음 사이.
    pub const L: i32 = 16;
    /// 24 — 섹션 사이.
    pub const XL: i32 = 24;
    /// 32 — 큰 구역 사이.
    pub const XXL: i32 = 32;

    /// 4의 배수로 맞춘다 — 계산 결과가 리듬에서 벗어나지 않게.
    #[must_use]
    pub const fn snap(v: i32) -> i32 {
        (v + 2) / 4 * 4
    }
}

/// 코너 반경 — **macOS 쪽**(넉넉하게).
pub mod radius {
    /// 12 — 창·팝업.
    pub const WINDOW: i32 = 12;
    /// 10 — 패널·카드·시트.
    pub const PANEL: i32 = 10;
    /// 6 — 버튼·필드·콤보.
    pub const CONTROL: i32 = 6;
    /// 배지·칩(pill) — 높이의 절반을 쓰라는 뜻의 큰 값.
    pub const PILL: i32 = 999;
}

/// 타입 스케일 — 1.0배 기준 크기(px)와 굵기.
pub mod type_scale {
    /// 창 제목 · 설정 카테고리.
    pub const TITLE: (f32, bool) = (15.0, true);
    /// ★ 목록 항목 본문 — **가장 많이 쓰인다**.
    pub const BODY: (f32, bool) = (13.0, false);
    /// 버튼 · 툴바.
    pub const LABEL: (f32, bool) = (12.0, false);
    /// 출처 앱 · 시각 · 설명.
    pub const CAPTION: (f32, bool) = (11.0, false);
    /// 코드·해시·경로(고정폭 슬롯과 함께).
    pub const MONO: (f32, bool) = (12.5, false);

    /// 행 높이 = 크기 × 1.45, **4의 배수로 스냅**.
    #[must_use]
    pub fn line_height(size_px: f32) -> i32 {
        super::space::snap((size_px * 1.45).round() as i32)
    }
}

/// ★ **상태 레이어** — Material의 핵심 아이디어.
///
/// **색을 새로 만들지 않는다.** 기존 색 위에 **같은 색을 알파로 덮는다**
/// ([docs/23 A-5](../../../docs/23-alpha-rendering.md)).
/// 이 표 하나가 *"손에 닿는 느낌"* 의 대부분이고, 구현 비용은 `fill_rect_alpha` 한 줄이다.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum State {
    /// 평상.
    #[default]
    Rest,
    /// 커서가 올라와 있다.
    Hover,
    /// 눌려 있다.
    Pressed,
    /// 선택돼 있다.
    Selected,
    /// 선택 + 커서.
    SelectedHover,
    /// 비활성(전경을 흐리게 — 배경 오버레이가 아니다).
    Disabled,
}

impl State {
    /// 배경 위에 덮을 오버레이 불투명도. `Disabled`는 배경을 안 덮는다(0.0).
    #[must_use]
    pub fn overlay_alpha(self) -> f32 {
        match self {
            State::Rest => 0.0,
            State::Hover => 0.08,
            State::Pressed => 0.12,
            State::Selected => 0.16,
            State::SelectedHover => 0.20,
            State::Disabled => 0.0,
        }
    }

    /// 전경(글자·아이콘) 불투명도 — 비활성만 낮춘다.
    #[must_use]
    pub fn content_alpha(self) -> f32 {
        if matches!(self, State::Disabled) {
            0.38
        } else {
            1.0
        }
    }

    /// 커서·눌림을 상태로 합친다(위젯이 매번 분기하지 않게).
    #[must_use]
    pub fn of(selected: bool, hover: bool, pressed: bool, enabled: bool) -> State {
        if !enabled {
            State::Disabled
        } else if pressed {
            State::Pressed
        } else if selected && hover {
            State::SelectedHover
        } else if selected {
            State::Selected
        } else if hover {
            State::Hover
        } else {
            State::Rest
        }
    }
}

/// 엘리베이션 — **그림자로 층을 말한다**.
///
/// 두 겹으로 그린다(가까운 진한 것 + 먼 옅은 것) — **한 겹은 딱딱해 보인다**.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum Elevation {
    /// 창 배경 — 그림자 없음.
    #[default]
    Flat,
    /// 목록 카드 · 툴바.
    Low,
    /// 팝오버 · 드롭다운.
    Mid,
    /// 시트 · 모달.
    High,
}

/// 그림자 한 겹 — `(y 오프셋, 번짐, 알파)`.
pub type ShadowLayer = (i32, i32, f32);

impl Elevation {
    /// 이 층이 그릴 그림자 겹들(먼 것 → 가까운 것 순 — **먼저 그린 위에 덮는다**).
    #[must_use]
    pub fn layers(self) -> &'static [ShadowLayer] {
        match self {
            Elevation::Flat => &[],
            Elevation::Low => &[(2, 4, 0.06), (1, 2, 0.10)],
            Elevation::Mid => &[(6, 12, 0.10), (2, 4, 0.16)],
            Elevation::High => &[(12, 24, 0.14), (4, 8, 0.22)],
        }
    }
}

/// 모션 — 지속(ms). ★ **팝업은 120ms를 넘기지 않는다**(3초 예산에서 애니메이션은 순수 비용).
pub mod motion {
    /// 상태 변화(hover·press).
    pub const STATE_MS: u32 = 90;
    /// 팝업 등장.
    pub const POPUP_MS: u32 = 120;
    /// 패널 열기/닫기.
    pub const PANEL_MS: u32 = 160;
    /// ★ **커서를 올렸을 때 밝아지는 시간**(사용자 확정 08-26 — *"서서히 밝아지는 듯하게"*).
    ///
    /// 스플리터에서 시작해 **hover 전체로 확장**했다(사용자 요청 08-26 2차 —
    /// *"Hover 에 대해서도 스플리터처럼"*). [`STATE_MS`]보다 11배 길다 — **의도된 예외**다.
    ///
    /// | | 왜 |
    /// |---|---|
    /// | **느리게 들어온다** | hover는 *"여기 뭔가 있다"* 를 **조용히** 알리는 신호다. 90ms로 번쩍이면 마우스가 지나가기만 해도 화면이 소란스러워진다 |
    /// | **빠르게 나간다** | 나가는 쪽까지 느리면 **잔상**이 남아 *"아직 거기 있나?"* 로 읽힌다 |
    ///
    /// ⚠️ 눌림(`Pressed`)은 이 페이드를 쓰지 않는다 — **누른 건 즉시 보여야 한다**.
    pub const HOVER_IN_MS: u32 = 1000;
    /// 커서가 떠난 뒤 꺼지는 시간 — 들어올 때보다 **빨리**(잔상 방지).
    pub const HOVER_OUT_MS: u32 = 220;

    /// `reduce_motion`이면 전부 0으로 — 접근성 설정을 존중한다.
    #[must_use]
    pub const fn effective(ms: u32, reduce_motion: bool) -> u32 {
        if reduce_motion {
            0
        } else {
            ms
        }
    }
}

/// ★ **hover 오버레이 알파** — 선택 여부와 진행도(0~1)로 정해지는 단일 원천.
///
/// 컨트롤마다 알파를 손으로 고르면 같은 hover가 곳마다 다르게 보인다. 여기 한 군데서 정한다.
///
/// ⚠️ **선택된 행은 이미 진한 배경이 깔려 있다** — 그 위에 [`State::SelectedHover`] 전량을
/// 얹으면 두 번 칠해진다. 그래서 **차이분만** 준다.
#[must_use]
pub fn hover_alpha(selected: bool, progress: f32) -> f32 {
    let (to, from) = if selected {
        (State::SelectedHover, State::Selected)
    } else {
        (State::Hover, State::Rest)
    };
    (to.overlay_alpha() - from.overlay_alpha()).max(0.0) * progress.clamp(0.0, 1.0)
}

/// ★ **서서히 변하는 0.0~1.0 값** — hover 페이드의 공용 부품.
///
/// [`motion::HOVER_IN_MS`] 동안 켜지고 [`motion::HOVER_OUT_MS`] 동안 꺼진다.
/// **들어오는 쪽과 나가는 쪽의 속도가 다르다**(비대칭) — 그게 이 타입이 존재하는 이유다.
///
/// ## 쓰는 법
///
/// ```ignore
/// self.fade.set(cursor_is_over);          // 목표만 바꾼다(진행도는 유지)
/// if self.fade.tick(now_ms) { redraw(); } // 매 프레임 — 값이 변했으면 참
/// let a = self.fade.value();              // 0.0~1.0
/// ```
///
/// ★ **목표를 바꿔도 진행도는 유지된다** — 반쯤 밝아진 상태에서 커서가 나가면
/// 100%까지 갔다가 꺼지는 게 아니라 **그 자리에서** 꺼진다.
#[derive(Clone, Copy, Debug)]
pub struct Fade {
    p: f32,
    on: bool,
    /// 마지막 틱 시각 — `None`이면 아직 한 번도 안 돌았다(첫 틱의 거대한 dt 방지).
    at: Option<u64>,
    in_ms: u32,
    out_ms: u32,
}

impl Default for Fade {
    fn default() -> Self {
        Self::hover()
    }
}

impl Fade {
    /// 들어오고 나가는 시간을 직접 정한다.
    #[must_use]
    pub const fn new(in_ms: u32, out_ms: u32) -> Self {
        Self {
            p: 0.0,
            on: false,
            at: None,
            in_ms,
            out_ms,
        }
    }

    /// hover 기본값 — [`motion::HOVER_IN_MS`] / [`motion::HOVER_OUT_MS`].
    #[must_use]
    pub const fn hover() -> Self {
        Self::new(motion::HOVER_IN_MS, motion::HOVER_OUT_MS)
    }

    /// 목표만 바꾼다 — **지금 진행도는 그대로 둔다**.
    pub fn set(&mut self, on: bool) {
        self.on = on;
    }

    /// 애니메이션 없이 목표로 튄다(`reduce_motion` · 창이 다시 뜰 때).
    pub fn jump(&mut self, on: bool) {
        self.on = on;
        self.p = if on { 1.0 } else { 0.0 };
    }

    /// 현재 값 0.0~1.0.
    #[must_use]
    pub fn value(self) -> f32 {
        self.p
    }

    /// 아직 움직이는 중인가(호스트가 다음 프레임을 예약할지 판단).
    #[must_use]
    pub fn is_animating(self) -> bool {
        let target = if self.on { 1.0 } else { 0.0 };
        (self.p - target).abs() > f32::EPSILON
    }

    /// 시간을 흘린다 — **값이 변했으면 `true`**(그때만 다시 그린다).
    pub fn tick(&mut self, now_ms: u64) -> bool {
        let prev_at = self.at.replace(now_ms);
        let target = if self.on { 1.0 } else { 0.0 };
        if (self.p - target).abs() <= f32::EPSILON {
            return false;
        }
        // 첫 틱은 기준 시각만 잡는다 — dt가 프로그램 시작부터로 잡히면 한 프레임에 끝난다.
        let Some(prev) = prev_at else { return false };
        #[allow(clippy::cast_precision_loss)]
        let dt = now_ms.saturating_sub(prev) as f32;
        let dur = if self.on { self.in_ms } else { self.out_ms };
        if dur == 0 {
            self.p = target;
            return true;
        }
        let step = dt / f32::from(u16::try_from(dur.min(65_535)).unwrap_or(u16::MAX));
        self.p = if self.on {
            (self.p + step).min(1.0)
        } else {
            (self.p - step).max(0.0)
        };
        true
    }
}

/// ★ **한 번에 하나만 hover된다** — 그 성질을 자료구조로 만든 것.
///
/// 목록에서 커서가 A→B로 옮겨가면 **A는 꺼지는 중, B는 켜지는 중**이다. 동시에 움직이는
/// 것은 항상 **둘뿐**이므로 항목 수와 무관하게 [`Fade`] 두 개면 충분하다
/// (행이 1,000개여도 상태는 두 개다).
#[derive(Clone, Copy, Debug, Default)]
pub struct HoverFade {
    cur: Option<usize>,
    cur_f: Fade,
    prev: Option<usize>,
    prev_f: Fade,
}

impl HoverFade {
    /// 지금 커서가 올라간 항목(없으면 `None`).
    pub fn set(&mut self, idx: Option<usize>) {
        if idx == self.cur {
            return;
        }
        // 되돌아온 경우 — 꺼지던 자리에서 **이어서** 밝아진다(0부터 다시 시작하지 않는다).
        let mut next = if idx.is_some() && idx == self.prev {
            self.prev_f
        } else {
            Fade::hover()
        };
        next.set(true);
        let mut leaving = self.cur_f;
        leaving.set(false);
        self.prev = self.cur;
        self.prev_f = leaving;
        self.cur = idx;
        self.cur_f = next;
    }

    /// 지금 hover 중인 항목.
    #[must_use]
    pub fn current(&self) -> Option<usize> {
        self.cur
    }

    /// 항목 `idx`의 밝기 0.0~1.0.
    #[must_use]
    pub fn value(&self, idx: usize) -> f32 {
        if self.cur == Some(idx) {
            self.cur_f.value()
        } else if self.prev == Some(idx) {
            self.prev_f.value()
        } else {
            0.0
        }
    }

    /// 아직 움직이는 중인가.
    #[must_use]
    pub fn is_animating(&self) -> bool {
        (self.cur.is_some() && self.cur_f.is_animating())
            || (self.prev.is_some() && self.prev_f.is_animating())
    }

    /// 시간을 흘린다 — 값이 변했으면 `true`.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        let a = self.cur.is_some() && self.cur_f.tick(now_ms);
        let b = self.prev.is_some() && self.prev_f.tick(now_ms);
        // 다 꺼진 이전 항목은 놓아준다 — 안 그러면 value()가 계속 그 자리를 본다.
        if self.prev.is_some() && self.prev_f.value() <= 0.0 && !self.prev_f.is_animating() {
            self.prev = None;
        }
        a || b
    }
}

/// 상태 오버레이에 쓸 색을 고른다 — **강조 계열이면 accent, 아니면 전경색**.
///
/// 색을 새로 만들지 않는다는 규칙([docs/25 §3-4])의 구현 지점이다.
#[must_use]
pub fn overlay_color(theme: &crate::theme::Theme, on_accent: bool) -> Color {
    if on_accent {
        theme.accent
    } else {
        theme.text
    }
}

/// ★ **hover 의도(intent) 코얼레싱** — 큐 없이 "마지막 의도 1개"만(nexa-clip 28 §hover · dir2 20 승계 · 09-04).
///
/// 사건(커서 이동·휠)은 [`set`](Self::set)으로 목표를 **덮어쓰기**만 한다 — 이전 의도는 어디에도 남지 않아
/// 폐기 비용 0. 수행은 [`take_due`](Self::take_due)가 의도 나이 ≥ [`INTENT_MS`](Self::INTENT_MS)일 때 **1회**
/// 내놓는다. 빠르게 지나간 목표들은 수행 단계에 닿지 못한다. 휠·스크롤바는 [`settle`](Self::settle)로
/// [`SETTLE_MS`](Self::SETTLE_MS) 동안 수행을 보류한다(멈춘 뒤 커서 아래 목표만).
///
/// 같은 목표가 다시 `set`되면 시계를 **다시 재지 않는다** — 한 행 안에서 잔움직임이 계속돼도 70ms 뒤엔 켜진다.
#[derive(Clone, Copy, Debug)]
pub struct HoverIntent<T: Copy + PartialEq> {
    pending: Option<(T, u64)>,
    settle_until: u64,
}

impl<T: Copy + PartialEq> Default for HoverIntent<T> {
    fn default() -> Self {
        Self {
            pending: None,
            settle_until: 0,
        }
    }
}

impl<T: Copy + PartialEq> HoverIntent<T> {
    /// "머문다"로 보는 최소 시간(ms) — 60~100ms 관례(hoverIntent 류). 100↑ 굼뜸 · 40↓ 꼬리.
    pub const INTENT_MS: u64 = 70;
    /// 휠·스크롤바 뒤 안정 대기(ms) — 휠 노치 간격(30~80)보다 길고 멈춤 체감(≈150)보다 짧게.
    pub const SETTLE_MS: u64 = 120;

    /// 의도 등록(덮어쓰기). 같은 목표면 등록 시각을 유지한다.
    pub fn set(&mut self, target: T, now_ms: u64) {
        if self.pending.is_some_and(|(t, _)| t == target) {
            return;
        }
        self.pending = Some((target, now_ms));
    }

    /// 스크롤 사건 — 안정 대기를 재무장한다.
    pub fn settle(&mut self, now_ms: u64) {
        self.settle_until = now_ms + Self::SETTLE_MS;
    }

    /// 만료된 의도를 1회 내놓는다(안정 대기 중이면 보류).
    pub fn take_due(&mut self, now_ms: u64) -> Option<T> {
        if now_ms < self.settle_until {
            return None;
        }
        let (t, at) = self.pending?;
        if now_ms.saturating_sub(at) < Self::INTENT_MS {
            return None;
        }
        self.pending = None;
        Some(t)
    }

    /// 의도·대기 전부 버린다(커서 이탈 · 세대 교체).
    pub fn clear(&mut self) {
        self.pending = None;
        self.settle_until = 0;
    }

    /// 아직 기다리는 게 있나(박동을 촘촘히 유지할지).
    #[must_use]
    pub fn is_waiting(&self, now_ms: u64) -> bool {
        self.pending.is_some() || now_ms < self.settle_until
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★ 간격은 전부 4의 배수여야 한다 — 리듬의 근거.
    #[test]
    fn spacing_is_on_four_grid() {
        for v in [
            space::XS,
            space::S,
            space::M,
            space::L,
            space::XL,
            space::XXL,
        ] {
            assert_eq!(v % 4, 0, "{v}는 4의 배수가 아니다");
        }
    }

    #[test]
    fn snap_rounds_to_grid() {
        assert_eq!(space::snap(1), 0);
        assert_eq!(space::snap(2), 4);
        assert_eq!(space::snap(13), 12);
        assert_eq!(space::snap(14), 16);
    }

    /// ★ 상태가 진해질수록 오버레이도 진해야 한다 — 역전되면 눌린 게 덜 눌려 보인다.
    #[test]
    fn overlay_alpha_is_monotonic() {
        let a = State::Rest.overlay_alpha();
        let b = State::Hover.overlay_alpha();
        let c = State::Pressed.overlay_alpha();
        let d = State::Selected.overlay_alpha();
        let e = State::SelectedHover.overlay_alpha();
        assert!(a < b && b < c, "rest < hover < pressed");
        assert!(d < e, "selected < selected+hover");
    }

    #[test]
    fn disabled_dims_content_not_background() {
        assert_eq!(State::Disabled.overlay_alpha(), 0.0);
        assert!(State::Disabled.content_alpha() < 1.0);
    }

    /// 상태 합성 우선순위 — 비활성이 가장 세고, 눌림이 선택보다 앞선다.
    #[test]
    fn state_of_priority() {
        assert_eq!(State::of(true, true, true, false), State::Disabled);
        assert_eq!(State::of(true, true, true, true), State::Pressed);
        assert_eq!(State::of(true, true, false, true), State::SelectedHover);
        assert_eq!(State::of(true, false, false, true), State::Selected);
        assert_eq!(State::of(false, true, false, true), State::Hover);
        assert_eq!(State::of(false, false, false, true), State::Rest);
    }

    /// ★ 그림자는 **두 겹**이다 — 한 겹은 딱딱해 보인다.
    #[test]
    fn elevation_has_two_layers_when_raised() {
        assert!(Elevation::Flat.layers().is_empty());
        for e in [Elevation::Low, Elevation::Mid, Elevation::High] {
            assert_eq!(e.layers().len(), 2, "{e:?}는 두 겹이어야 한다");
        }
    }

    /// 층이 높을수록 더 멀리·더 진하게.
    #[test]
    fn elevation_grows_with_level() {
        let near = |e: Elevation| e.layers().last().copied().unwrap_or((0, 0, 0.0));
        assert!(near(Elevation::Mid).2 > near(Elevation::Low).2);
        assert!(near(Elevation::High).2 > near(Elevation::Mid).2);
    }

    /// ★ 팝업 애니메이션은 120ms를 넘지 않는다(3초 예산 보호).
    #[test]
    fn popup_motion_is_capped() {
        const _: () = assert!(
            motion::POPUP_MS <= 120,
            "팝업 애니메이션이 3초 예산을 잠식한다"
        );
        assert_eq!(
            motion::effective(motion::POPUP_MS, true),
            0,
            "reduce_motion이면 0"
        );
    }

    /// ★ 들어오는 쪽이 나가는 쪽보다 **느리다** — 이 비대칭이 hover 페이드의 핵심이다.
    #[test]
    fn fade_in_is_slower_than_fade_out() {
        const _: () = assert!(
            motion::HOVER_IN_MS > motion::HOVER_OUT_MS,
            "빠르게 들어오고 느리게 나가면 잔상이 남는다"
        );
    }

    /// 첫 틱은 기준 시각만 잡는다 — 시작 시각이 크면 한 프레임에 끝나 버린다.
    #[test]
    fn first_tick_only_anchors_time() {
        let mut f = Fade::hover();
        f.set(true);
        assert!(!f.tick(50_000), "첫 틱은 값을 움직이지 않는다");
        assert_eq!(f.value(), 0.0);
        assert!(f.tick(50_100));
        assert!(f.value() > 0.0);
    }

    /// 정해진 시간이면 끝까지 간다(1000ms · 16ms 프레임 가정).
    #[test]
    fn fade_reaches_one_within_in_ms() {
        let mut f = Fade::hover();
        f.set(true);
        let mut t = 0;
        f.tick(t);
        while t < u64::from(motion::HOVER_IN_MS) + 32 {
            t += 16;
            f.tick(t);
        }
        assert!((f.value() - 1.0).abs() < 1e-6, "{}", f.value());
        assert!(!f.is_animating());
    }

    /// ★ 나가는 쪽이 실제로 더 빠르다 — 같은 시간에 더 많이 움직인다.
    #[test]
    fn out_moves_faster_than_in() {
        let mut fin = Fade::hover();
        fin.set(true);
        fin.tick(0);
        fin.tick(100);
        let gained = fin.value();

        let mut fout = Fade::hover();
        fout.jump(true);
        fout.set(false);
        fout.tick(0);
        fout.tick(100);
        let left = 1.0 - fout.value();

        assert!(
            left > gained,
            "나갈 때가 더 빨라야 한다: {left} vs {gained}"
        );
    }

    /// ★ 목표를 되돌리면 **그 자리에서** 방향만 바뀐다(끝까지 갔다 오지 않는다).
    #[test]
    fn reversing_keeps_progress() {
        let mut f = Fade::hover();
        f.set(true);
        f.tick(0);
        f.tick(300);
        let mid = f.value();
        assert!(mid > 0.0 && mid < 1.0);
        f.set(false);
        assert_eq!(f.value(), mid, "방향만 바뀌고 값은 유지된다");
    }

    /// ★ hover는 배타적이다 — 항목이 몇 개든 움직이는 것은 둘뿐.
    #[test]
    fn hover_fade_tracks_only_two() {
        let mut h = HoverFade::default();
        h.set(Some(3));
        h.tick(0);
        h.tick(300);
        assert!(h.value(3) > 0.0);
        h.set(Some(7));
        h.tick(310);
        h.tick(320);
        assert!(h.value(7) > 0.0, "새 항목이 켜진다");
        assert!(h.value(3) > 0.0, "떠난 항목은 꺼지는 중");
        assert_eq!(h.value(99), 0.0, "나머지는 전부 0");
    }

    /// 되돌아오면 꺼지던 자리에서 이어서 밝아진다(0부터 다시 시작하지 않는다).
    #[test]
    fn returning_resumes_from_where_it_left() {
        let mut h = HoverFade::default();
        h.set(Some(1));
        h.tick(0);
        h.tick(500);
        let peak = h.value(1);
        h.set(Some(2));
        h.tick(510);
        let leaving = h.value(1);
        assert!(leaving > 0.0 && leaving < peak);
        h.set(Some(1));
        assert!(h.value(1) >= leaving - 1e-6, "이어서 올라간다");
    }

    /// 다 꺼진 이전 항목은 놓아준다(값이 계속 남으면 목록이 얼룩진다).
    #[test]
    fn faded_out_row_is_released() {
        let mut h = HoverFade::default();
        h.set(Some(1));
        h.tick(0);
        h.tick(200);
        h.set(None);
        let mut t = 200;
        while t < 200 + u64::from(motion::HOVER_OUT_MS) + 64 {
            t += 16;
            h.tick(t);
        }
        assert_eq!(h.value(1), 0.0);
        assert!(!h.is_animating());
    }

    /// ★ 선택된 행에는 **차이분만** 얹는다 — 아니면 두 번 칠해져 과하게 밝아진다.
    #[test]
    fn hover_alpha_accounts_for_selection() {
        let plain = hover_alpha(false, 1.0);
        let sel = hover_alpha(true, 1.0);
        assert!((plain - State::Hover.overlay_alpha()).abs() < 1e-6);
        assert!(
            sel < plain,
            "선택 행은 이미 깔린 만큼 덜 얹는다: {sel} vs {plain}"
        );
        assert_eq!(
            hover_alpha(false, 0.0),
            0.0,
            "진행도 0이면 아무것도 안 얹는다"
        );
        assert!(hover_alpha(false, 0.5) < plain, "중간은 중간만큼만");
    }

    /// 행 높이도 4의 배수로 떨어진다.
    #[test]
    fn line_height_snaps() {
        for (size, _) in [type_scale::BODY, type_scale::TITLE, type_scale::CAPTION] {
            assert_eq!(type_scale::line_height(size) % 4, 0);
        }
    }

    /// ★ 의도 코얼레싱(28 §hover): 5행을 10ms 간격으로 지나면 수행 0회, 마지막 행에서 70ms 뒤 1회.
    #[test]
    fn hover_intent_keeps_only_last() {
        let mut hi = HoverIntent::<usize>::default();
        let mut fired = Vec::new();
        for k in 0..5u64 {
            hi.set(k as usize, 1_000 + k * 10);
            if let Some(t) = hi.take_due(1_000 + k * 10) {
                fired.push(t);
            }
        }
        assert!(fired.is_empty(), "{fired:?}");
        assert_eq!(hi.take_due(1_040 + 69), None, "아직 69ms");
        assert_eq!(hi.take_due(1_040 + 70), Some(4));
        assert_eq!(hi.take_due(1_200), None, "1회만");
        // 같은 목표 재등록은 시계를 다시 재지 않는다.
        hi.set(7, 2_000);
        hi.set(7, 2_060);
        assert_eq!(hi.take_due(2_070), Some(7));
        // 스크롤 안정 대기 중엔 만료돼도 보류 · 지나면 내놓는다.
        hi.set(9, 3_000);
        hi.settle(3_050);
        assert_eq!(hi.take_due(3_100), None);
        assert!(hi.is_waiting(3_100));
        assert_eq!(
            hi.take_due(3_050 + HoverIntent::<usize>::SETTLE_MS),
            Some(9)
        );
        hi.set(1, 4_000);
        hi.clear();
        assert_eq!(hi.take_due(5_000), None);
        assert!(!hi.is_waiting(5_000));
    }
}

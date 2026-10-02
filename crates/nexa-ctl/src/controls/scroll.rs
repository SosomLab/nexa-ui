//! 오버레이 스크롤바 — macOS식 **반투명 오버레이**(사용자 확정 08-08).
//!
//! - 콘텐츠가 넘쳐도 **스크롤 전엔 보이지 않는다**(식별 안 됨).
//! - 스크롤(휠)·바 근처 접근·드래그 중엔 **콘텐츠 위에 겹쳐** 위치·비율을 보여준다(세로·가로).
//! - 바 위에 마우스가 오면 **더 두껍게** + 클릭 드래그 가능.
//! - 항상 **반투명**.
//! - **축은 따로 논다**(nexa-sql 사용자 09-14): 세로 휠은 세로 막대만, 가로 휠은 가로 막대만 깨운다 · 숨김 시각도 축별.
//!   `show()`(프로그램적)만 둘 다 깨운다.
//!
//! 상태(hover/drag/표시)만 보유하고 **오프셋은 호스트가 소유**한다 — [`ScrollBars::on_event`]에
//! 현재 오프셋을 넣으면 갱신된 오프셋을 돌려준다(스크롤 가능한 어떤 뷰에도 재사용: 갤러리·트리·그리드).

use crate::draw::{DrawCtx, FontSlot};
use crate::event::InputEvent;
use crate::geom::{Point, Rect};
use crate::theme::Theme;
use crate::typeahead::HudPos;

// 레이아웃 상수(논리 px).
const THIN: i32 = 6;
const THICK: i32 = 11;
const MARGIN: i32 = 2;
const MIN_THUMB: i32 = 28;
/// 반투명도 — 항상 은은하게.
const ALPHA_IDLE: f32 = 0.35;
const ALPHA_HOT: f32 = 0.6;

/// 자동 숨김까지의 기본 지연(ms) — 사용자 확정 08-10. 설정에서 바꾼다.
pub const DEFAULT_HIDE_MS: u64 = 2000;

/// 전역 자동 숨김 지연 — 설정 변경이 **모든 스크롤 영역에 즉시** 반영되도록 프로세스 전역에 둔다
/// (스크롤바는 목록·트리·갤러리·대화·설정에 흩어져 있어, 값을 일일이 들고 다니면
/// 한 군데만 옛 값으로 남는다 — 핫스왑 원칙).
static HIDE_MS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(DEFAULT_HIDE_MS);

/// 자동 숨김 지연을 바꾼다(설정 즉시 적용). 0이면 **숨기지 않는다**(항상 표시).
pub fn set_hide_delay_ms(ms: u64) {
    HIDE_MS.store(ms, core::sync::atomic::Ordering::Relaxed);
}

/// 현재 자동 숨김 지연(ms).
#[must_use]
pub fn hide_delay_ms() -> u64 {
    HIDE_MS.load(core::sync::atomic::Ordering::Relaxed)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Axis {
    V,
    H,
}
/// ★ **고속 스크롤 전역 설정**(nexa-sql 09-30 · 설정 `scroll.*` · 핫스왑 원칙 = 값을 들고 다니지 않고 한 곳에서 읽는다).
/// 호스트가 설정이 바뀔 때 [`set_fast_scroll`]로 넣고, 모든 [`ScrollBars`]·[`ScrollAccel`]·[`SpeedHud`]가 그때그때 읽는다.
/// 영역별 예외(결과 그리드 = 한 단계 더 빠르게)는 [`ScrollBars::set_fast_override`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FastScroll {
    /// 고속 스크롤 사용.
    pub enabled: bool,
    /// 연속 N번마다 배수 +1.
    pub step: u32,
    /// 배수 상한.
    pub max: i32,
    /// 이 간격(ms) 안에 이어진 사건만 연속으로 본다.
    pub window_ms: u64,
    /// 속도 HUD 표시.
    pub hud: bool,
    /// HUD 자리(영역 안 9자리 · 기본 우상단).
    pub hud_pos: HudPos,
    /// 마지막 가속 사건 뒤 HUD가 그대로 보이는 시간.
    pub hud_hold_ms: u64,
    /// 그 뒤 서서히 사라지는 시간.
    pub hud_fade_ms: u64,
}

impl Default for FastScroll {
    /// 부품 기본 = **끔**(호스트가 설정으로 켠다 — 기존 스크롤 동작·시험을 바꾸지 않는다).
    fn default() -> Self {
        Self {
            enabled: false,
            step: 5,
            max: 8,
            window_ms: 160,
            hud: true,
            hud_pos: HudPos::TopRight,
            hud_hold_ms: 250,
            hud_fade_ms: 600,
        }
    }
}

static FAST: std::sync::RwLock<FastScroll> = std::sync::RwLock::new(FastScroll {
    enabled: false,
    step: 5,
    max: 8,
    window_ms: 160,
    hud: true,
    hud_pos: HudPos::TopRight,
    hud_hold_ms: 250,
    hud_fade_ms: 600,
});

/// 전역 고속 스크롤 설정을 바꾼다(설정 즉시 적용).
pub fn set_fast_scroll(cfg: FastScroll) {
    if let Ok(mut g) = FAST.write() {
        *g = cfg;
    }
}

/// 현재 전역 고속 스크롤 설정.
#[must_use]
pub fn fast_scroll() -> FastScroll {
    FAST.read().map(|g| *g).unwrap_or_default()
}

/// ★ **속도 HUD 부품**(nexa-sql 09-30 "가속 중이면 영역 우상단에 속도를 플래시로 · 멈추면 지정 시간에 서서히"): 가속 사건마다
/// `note(k)` — 배수 > 1이면 `×k`를 보이고, 마지막 사건 뒤 `hud_hold_ms`가 지나면(또는 배수가 1로 돌아오면) `hud_fade_ms` 동안
/// 제곱 감속으로 사라진다. 스크롤 자체에는 관성이 없다(사건마다 즉시 이동 · 멈추면 바로 멈춤) — HUD만 늦게 사라진다.
#[derive(Debug, Clone, Default)]
pub struct SpeedHud {
    factor: i32,
    last: Option<std::time::Instant>,
    fade_from: Option<std::time::Instant>,
}

impl SpeedHud {
    /// 가속 사건 하나(배수 `k`).
    pub fn note(&mut self, k: i32, cfg: &FastScroll) {
        self.note_at(k, cfg, std::time::Instant::now());
    }

    pub fn note_at(&mut self, k: i32, cfg: &FastScroll, now: std::time::Instant) {
        if !cfg.hud || !cfg.enabled {
            self.clear();
            return;
        }
        if k > 1 {
            self.factor = k;
            self.last = Some(now);
            self.fade_from = None;
        } else if self.last.is_some() && self.fade_from.is_none() {
            // 가속이 끊겼다(1배로) → 지금부터 사라진다.
            self.fade_from = Some(now);
        }
    }

    pub fn clear(&mut self) {
        self.last = None;
        self.fade_from = None;
    }

    #[must_use]
    pub fn visible(&self) -> bool {
        self.last.is_some()
    }

    /// 지금의 세기 0..=1(없으면 None). 순수 — 상태를 바꾸지 않는다.
    #[must_use]
    pub fn alpha_at(&self, now: std::time::Instant, cfg: &FastScroll) -> Option<f32> {
        let last = self.last?;
        let fade_from = match self.fade_from {
            Some(f) => f,
            None => {
                let held = now.saturating_duration_since(last).as_millis() as u64;
                if held < cfg.hud_hold_ms {
                    return Some(1.0);
                }
                last + std::time::Duration::from_millis(cfg.hud_hold_ms)
            }
        };
        let f = now.saturating_duration_since(fade_from).as_millis() as u64;
        let fade = cfg.hud_fade_ms.max(1);
        if f >= fade {
            return None;
        }
        let t = f as f32 / fade as f32;
        Some(1.0 - t * t)
    }

    /// 호스트 tick — 사라지는 중이면 `true`(다시 그려야 한다) · 다 사라지면 상태를 지우고 `true` 한 번.
    pub fn tick(&mut self, now: std::time::Instant, cfg: &FastScroll) -> bool {
        if self.last.is_none() {
            return false;
        }
        match self.alpha_at(now, cfg) {
            None => {
                self.clear();
                true
            }
            Some(a) => a < 1.0,
        }
    }

    /// 그리기 — `area` 안 `cfg.hud_pos` 자리에 캡슐 `×k`(세기만큼 투명).
    pub fn paint(
        &self,
        ctx: &mut dyn DrawCtx,
        theme: &Theme,
        area: Rect,
        scale: f32,
        cfg: &FastScroll,
    ) {
        let Some(a) = self.alpha_at(std::time::Instant::now(), cfg) else {
            return;
        };
        if a < 0.08 {
            return;
        }
        let label = format!("×{}", self.factor);
        // ★ 크기(사용자 09-30) — 처음 50 %로 줄였다가 다시 50 % 키움 = 호출자 글꼴 높이의 75 %.
        let base_h = ctx.text_height();
        // (09-30 "50 % 키워서") 글꼴 = 호출자 높이의 75 % · 여백 6/3px · 가장자리 8px.
        // ★ 단위(10-02 맥 Retina에서 HUD가 작게 보임): `text_height()` = 물리 px · `select_font_sized`의 증분 = 논리 px(뒤에
        //   배율이 다시 곱해진다) → 배율로 나눠 넘긴다. 나누지 않으면 줄이는 양이 배율만큼 커져 2배율에서 50 %가 된다.
        let delta = -(base_h as f32 / scale.max(0.01)) * 0.25;
        ctx.select_font_sized(FontSlot::Base, false, delta);
        let th_txt = ctx.text_height();
        let tw = ctx.text_width(&label);
        let (px, py) = (sc(6, scale), sc(3, scale));
        let (w, h) = (tw + px * 2, th_txt + py * 2);
        let m = sc(8, scale);
        let (l, c, r) = (area.x + m, area.x + (area.w - w) / 2, area.right() - m - w);
        let (t, mid, b) = (area.y + m, area.y + (area.h - h) / 2, area.bottom() - m - h);
        let (x, y) = match cfg.hud_pos {
            HudPos::TopLeft => (l, t),
            HudPos::TopCenter => (c, t),
            HudPos::TopRight => (r, t),
            HudPos::MidLeft => (l, mid),
            HudPos::MidRight => (r, mid),
            HudPos::BottomLeft => (l, b),
            HudPos::BottomCenter => (c, b),
            HudPos::BottomRight => (r, b),
            _ => (c, mid),
        };
        let rect = Rect::new(x.max(area.x), y.max(area.y), w, h);
        // 배경 60 % 투명(= 40 % 불투명 · 페이드 세기 곱) · 글자 = **테마 글자색**(라이트 = 어두운 글자 · 다크 = 밝은 글자 · 09-30
        //   "흰색이라 식별이 어렵다 · 투명도 더 낮게") — 페이드 후반에만 캡슐 색 쪽으로 섞어 사라진다.
        ctx.fill_round_rect_alpha(rect, h / 2, theme.accent, 0.4 * a);
        if a > 0.35 {
            let fg = if a >= 0.85 {
                theme.text
            } else {
                theme
                    .accent
                    .lerp(theme.text, ((a - 0.35) / 0.5).clamp(0.0, 1.0))
            };
            ctx.text(rect.x + px, rect.y + py, rect, &label, fg);
        }
        ctx.select_font(FontSlot::Base, false);
    }
}

/// ★ **고속 스크롤 가속 부품**(nexa-sql 09-30 "상/하 이동시 고속 스크롤") — 같은 방향의 사건이 짧은 간격으로 이어지면
/// 한 번의 이동량을 배수로 키운다(휠 틱 · 키 자동 반복 공통). 사건마다 `factor(dir)` 하나만 부른다 — 큐·타이머 없음.
///
/// 규칙: 이전 사건과 방향이 같고 간격이 [`ScrollAccel::WINDOW_MS`] 안이면 연속 횟수 +1, 아니면 0으로. 배수 =
/// `1 + 연속/STEP`(최대 [`ScrollAccel::MAX`]). 처음 몇 번은 그대로(정확한 한 줄 이동이 먼저) · 오래 누르거나 빨리 돌릴 때만 빨라진다.
#[derive(Debug, Clone)]
pub struct ScrollAccel {
    last: Option<std::time::Instant>,
    dir: i32,
    streak: u32,
}

impl Default for ScrollAccel {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollAccel {
    pub fn new() -> Self {
        Self {
            last: None,
            dir: 0,
            streak: 0,
        }
    }

    /// 사건 하나(`dir` = 부호만 본다 · 0 = 리셋) → 이번 이동에 곱할 배수(1 = 가속 없음). 전역 설정([`fast_scroll`])을 쓴다.
    pub fn factor(&mut self, dir: i32) -> i32 {
        let cfg = fast_scroll();
        self.factor_cfg(dir, &cfg)
    }

    /// 설정을 넘기는 판(영역별 override).
    pub fn factor_cfg(&mut self, dir: i32, cfg: &FastScroll) -> i32 {
        self.factor_at(dir, std::time::Instant::now(), cfg)
    }

    /// 시각·설정을 밖에서 주는 판(시험 · 순수).
    pub fn factor_at(&mut self, dir: i32, now: std::time::Instant, cfg: &FastScroll) -> i32 {
        let d = dir.signum();
        if d == 0 || !cfg.enabled {
            self.reset();
            return 1;
        }
        let quick = self
            .last
            .is_some_and(|t| now.duration_since(t).as_millis() <= u128::from(cfg.window_ms));
        if quick && d == self.dir {
            self.streak = self.streak.saturating_add(1);
        } else {
            self.streak = 0;
        }
        self.dir = d;
        self.last = Some(now);
        (1 + (self.streak / cfg.step.max(1)) as i32).min(cfg.max.max(1))
    }

    /// 연속을 끊는다(방향 전환·포커스 이탈 등).
    pub fn reset(&mut self) {
        self.last = None;
        self.dir = 0;
        self.streak = 0;
    }
}

/// 오버레이 스크롤바(세로+가로).
#[derive(Clone, Debug, Default)]
pub struct ScrollBars {
    /// ★ 고속 스크롤(전역 [`fast_scroll`] · 영역별 override) — 휠 가속기 + 속도 HUD.
    accel: ScrollAccel,
    hud: SpeedHud,
    fast_override: Option<FastScroll>,
    hover: Option<Axis>,
    /// 드래그 중: (축, 잡은 지점 오프셋 = 커서 - 썸 시작).
    drag: Option<(Axis, i32)>,
    /// 축별 — 스크롤/접근/드래그로 활성화되어 보이는가(1·2단계). [세로, 가로].
    active: [bool; 2],
    /// 축별 — 이 시각(ms)이 지나면 숨긴다(1→0단계). 활동마다 뒤로 민다.
    hide_at_ms: [u64; 2],
    /// 축별 — 활동이 있었다 — 다음 [`ScrollBars::tick`]에서 마감 시각을 다시 잡는다.
    /// (`on_event`는 시계를 모른다. 시각 주입은 호스트가 하는 `tick` 한 곳으로 모은다.)
    bumped: [bool; 2],
    /// 모습이 바뀌어 다시 그려야 한다(표시 전환·호버 두께) — `tick`이 호스트에 알린다.
    dirty: bool,
}

impl Axis {
    const fn idx(self) -> usize {
        match self {
            Axis::V => 0,
            Axis::H => 1,
        }
    }
}

/// px 헬퍼.
fn sc(v: i32, scale: f32) -> i32 {
    (v as f32 * scale).round() as i32
}

impl ScrollBars {
    /// **프로그램적 표시** — 사용자 입력이 아니라 코드가 스크롤을 옮겼을 때 부른다
    /// (타이핑으로 가로 스크롤이 따라붙는 경우 등). 이걸 부르지 않으면 막대가
    /// `on_event` 전까지 숨어 있어 "스크롤이 생기지 않는다"로 보인다(08-10 지적).
    /// 이 영역만의 고속 스크롤 설정(None = 전역 [`fast_scroll`]). 예 = 결과 그리드 한 단계 더 빠르게.
    pub fn set_fast_override(&mut self, cfg: Option<FastScroll>) {
        self.fast_override = cfg;
        self.accel.reset();
        self.hud.clear();
    }

    /// 지금 이 영역에 적용되는 고속 스크롤 설정.
    #[must_use]
    pub fn fast_cfg(&self) -> FastScroll {
        self.fast_override.unwrap_or_else(fast_scroll)
    }

    /// 키 등 휠 밖의 가속 사건을 HUD에 알린다(배수 `k`).
    pub fn note_fast(&mut self, k: i32) {
        let cfg = self.fast_cfg();
        self.hud.note(k, &cfg);
    }

    /// 속도 HUD(호스트가 자기 영역에 직접 그릴 때).
    #[must_use]
    pub fn hud(&self) -> &SpeedHud {
        &self.hud
    }

    pub fn show(&mut self) {
        self.wake(Axis::V);
        self.wake(Axis::H);
    }

    /// 새 스크롤바.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 지금 화면에 보이는가(자동 숨김 전 단계).
    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.active[0] || self.active[1]
    }

    fn is_active(&self, axis: Axis) -> bool {
        self.active[axis.idx()]
    }

    /// 가로 썸 rect(호스트 검증·테스트용) — 필요 없으면 `None`. 두께는 잡기 쉬운 THICK 기준.
    #[must_use]
    pub fn h_thumb_for_test(vp: Rect, content_w: i32, off_x: i32, scale: f32) -> Option<Rect> {
        Self::h_thumb(vp, content_w, off_x, scale, sc(THICK, scale))
    }

    fn v_needed(vp: Rect, content_h: i32) -> bool {
        content_h > vp.h
    }
    fn h_needed(vp: Rect, content_w: i32) -> bool {
        content_w > vp.w
    }

    /// 세로 썸 rect(현재 오프셋 기준). 불필요하면 `None`. `width`는 두께.
    fn v_thumb(vp: Rect, content_h: i32, off_y: i32, scale: f32, width: i32) -> Option<Rect> {
        if !Self::v_needed(vp, content_h) {
            return None;
        }
        let track = vp.h;
        let thumb = (track * vp.h / content_h)
            .max(sc(MIN_THUMB, scale))
            .min(track);
        let scrollable = (content_h - vp.h).max(1);
        let travel = (track - thumb).max(0);
        let ty = vp.y + off_y.clamp(0, scrollable) * travel / scrollable;
        let x = vp.right() - width - sc(MARGIN, scale);
        Some(Rect::new(x, ty, width, thumb))
    }

    /// 가로 썸 rect. `height`는 두께.
    fn h_thumb(vp: Rect, content_w: i32, off_x: i32, scale: f32, height: i32) -> Option<Rect> {
        if !Self::h_needed(vp, content_w) {
            return None;
        }
        let track = vp.w;
        let thumb = (track * vp.w / content_w)
            .max(sc(MIN_THUMB, scale))
            .min(track);
        let scrollable = (content_w - vp.w).max(1);
        let travel = (track - thumb).max(0);
        let tx = vp.x + off_x.clamp(0, scrollable) * travel / scrollable;
        let y = vp.bottom() - height - sc(MARGIN, scale);
        Some(Rect::new(tx, y, thumb, height))
    }

    fn clamp(off_x: i32, off_y: i32, vp: Rect, content_w: i32, content_h: i32) -> (i32, i32) {
        (
            off_x.clamp(0, (content_w - vp.w).max(0)),
            off_y.clamp(0, (content_h - vp.h).max(0)),
        )
    }

    /// 이벤트 처리 — 갱신된 `(off_x, off_y, consumed)`. `consumed`면 호스트는 그 이벤트를
    /// 자기 콘텐츠에 다시 쓰지 않는다(드래그가 행 선택으로 새지 않도록).
    #[allow(clippy::too_many_arguments)]
    pub fn on_event(
        &mut self,
        ev: &InputEvent,
        vp: Rect,
        content_w: i32,
        content_h: i32,
        off_x: i32,
        off_y: i32,
        scale: f32,
    ) -> (i32, i32, bool) {
        let thick = sc(THICK, scale);
        let (mut ox, mut oy) = (off_x, off_y);
        match *ev {
            InputEvent::Wheel { delta } => {
                // ★ 고속 스크롤: 같은 방향의 틱이 빨리 이어지면 배수(전역 설정 또는 영역 override) + HUD.
                let cfg = self.fast_cfg();
                let k = self.accel.factor_cfg(-delta, &cfg);
                self.hud.note(k, &cfg);
                oy -= delta / 3 * k;
                self.wake(Axis::V); // 0/1→1단계 + 카운트다운 리셋 — 세로만
                let (ox, oy) = Self::clamp(ox, oy, vp, content_w, content_h);
                (ox, oy, Self::v_needed(vp, content_h))
            }
            InputEvent::HWheel { delta } => {
                ox += delta / 3;
                self.wake(Axis::H);
                let (ox, oy) = Self::clamp(ox, oy, vp, content_w, content_h);
                (ox, oy, Self::h_needed(vp, content_w))
            }
            InputEvent::MouseDown { x, y, .. } => {
                let p = Point { x, y };
                // 보이는 축의 썸만 잡힌다.
                if self.is_active(Axis::V) {
                    if let Some(t) = Self::v_thumb(vp, content_h, oy, scale, thick) {
                        if t.contains(p) {
                            self.drag = Some((Axis::V, y - t.y));
                            self.wake(Axis::V);
                            return (ox, oy, true);
                        }
                    }
                }
                if self.is_active(Axis::H) {
                    if let Some(t) = Self::h_thumb(vp, content_w, ox, scale, thick) {
                        if t.contains(p) {
                            self.drag = Some((Axis::H, x - t.x));
                            self.wake(Axis::H);
                            return (ox, oy, true);
                        }
                    }
                }
                // ★ 썸 밖 **트랙** 클릭 = 그 자리로 썸 중심을 옮기고 드래그 시작 · 소비(nexa-sql 09-16: 가로 스크롤바 트랙을
                //   누르고 끌면 클릭이 아래 그리드로 흘러 셀 드래그 선택이 됐다). 보이는 축만.
                if self.is_active(Axis::V) && Self::v_needed(vp, content_h) {
                    let track = Rect::new(vp.right() - thick, vp.y, thick, vp.h);
                    if track.contains(p) {
                        if let Some(t) = Self::v_thumb(vp, content_h, oy, scale, thick) {
                            let travel = (vp.h - t.h).max(1);
                            let scrollable = (content_h - vp.h).max(0);
                            oy = (y - t.h / 2 - vp.y) * scrollable / travel;
                            self.drag = Some((Axis::V, t.h / 2));
                            self.wake(Axis::V);
                            let (ox, oy) = Self::clamp(ox, oy, vp, content_w, content_h);
                            return (ox, oy, true);
                        }
                    }
                }
                if self.is_active(Axis::H) && Self::h_needed(vp, content_w) {
                    let track = Rect::new(vp.x, vp.bottom() - thick, vp.w, thick);
                    if track.contains(p) {
                        if let Some(t) = Self::h_thumb(vp, content_w, ox, scale, thick) {
                            let travel = (vp.w - t.w).max(1);
                            let scrollable = (content_w - vp.w).max(0);
                            ox = (x - t.w / 2 - vp.x) * scrollable / travel;
                            self.drag = Some((Axis::H, t.w / 2));
                            self.wake(Axis::H);
                            let (ox, oy) = Self::clamp(ox, oy, vp, content_w, content_h);
                            return (ox, oy, true);
                        }
                    }
                }
                (ox, oy, false)
            }
            InputEvent::MouseMove { x, y } => {
                let p = Point { x, y };
                // 드래그 중이면 오프셋 갱신.
                if let Some((axis, grab)) = self.drag {
                    match axis {
                        Axis::V => {
                            if let Some(t) = Self::v_thumb(vp, content_h, oy, scale, thick) {
                                let travel = (vp.h - t.h).max(1);
                                let scrollable = (content_h - vp.h).max(0);
                                oy = (y - grab - vp.y) * scrollable / travel;
                            }
                        }
                        Axis::H => {
                            if let Some(t) = Self::h_thumb(vp, content_w, ox, scale, thick) {
                                let travel = (vp.w - t.w).max(1);
                                let scrollable = (content_w - vp.w).max(0);
                                ox = (x - grab - vp.x) * scrollable / travel;
                            }
                        }
                    }
                    self.wake(axis);
                    let (ox, oy) = Self::clamp(ox, oy, vp, content_w, content_h);
                    return (ox, oy, true);
                }
                // ★ 가장자리 접근 = 그 축의 바를 드러낸다(모듈 머리의 "바 근처 접근" — 09-22까지 빠져 있어 가로 휠·Shift+휠이
                //   없는 환경에서는 가로 막대를 볼 수도 끌 수도 없었다 · nexa-sql 사용자 "편집기 가로 스크롤이 동작하지 않는다").
                //   축은 따로(09-14 규칙): 아래 띠 = 가로만 · 오른쪽 띠 = 세로만 · 그 축이 필요할 때만.
                if vp.contains(p) {
                    let edge = thick + sc(MARGIN, scale);
                    if Self::h_needed(vp, content_w) && y >= vp.bottom() - edge {
                        self.wake(Axis::H);
                    }
                    if Self::v_needed(vp, content_h) && x >= vp.right() - edge {
                        self.wake(Axis::V);
                    }
                }
                // 호버 판정(썸 위 = 2단계 두껍게). **그 축의** 바가 보일 때만 판정한다
                // (0단계에선 바 위가 아닌 접근으로는 뜨지 않는다 — 스크롤 또는 위의 가장자리 접근으로만).
                let was_hover = self.hover;
                self.hover = None;
                if self.is_active(Axis::V) {
                    if let Some(t) = Self::v_thumb(vp, content_h, oy, scale, thick) {
                        if t.contains(p) {
                            self.hover = Some(Axis::V);
                        }
                    }
                }
                if self.hover.is_none() && self.is_active(Axis::H) {
                    if let Some(t) = Self::h_thumb(vp, content_w, ox, scale, thick) {
                        if t.contains(p) {
                            self.hover = Some(Axis::H);
                        }
                    }
                }
                // 호버가 바뀌면 두께가 바뀐다 — 다시 그려야 보인다.
                if self.hover != was_hover {
                    self.dirty = true;
                }
                (ox, oy, false)
            }
            InputEvent::MouseUp { .. } => {
                let was = self.drag.take();
                if let Some((axis, _)) = was {
                    self.wake(axis); // 놓는 순간부터 다시 카운트 — 곧바로 사라지지 않는다
                }
                (ox, oy, was.is_some())
            }
            _ => (ox, oy, false),
        }
    }

    /// 그 축의 스크롤/드래그 활동 → 표시(1단계) + 숨김 마감 연기.
    fn wake(&mut self, axis: Axis) {
        let i = axis.idx();
        if !self.active[i] {
            self.dirty = true; // 숨김 → 표시 전환은 다시 그려야 보인다
        }
        self.active[i] = true;
        self.bumped[i] = true;
    }

    /// 호스트가 호출 — `now_ms`가 마감을 넘겼고 호버/드래그가 아니면 숨긴다(1→0단계).
    /// 표시 상태가 바뀌면 `true`(재그리기 필요).
    ///
    /// ★ **시간 기반이어야 한다** — 예전에는 호출 횟수를 셌는데, 호스트는 유휴 시 5Hz지만
    /// **이벤트가 들어오면 그때마다** 부른다. 그래서 드래그 중에는 초당 수십 번 깎여
    /// 막대가 0.2초 만에 사라졌다(08-10 지적: "드래그하면 잠깐 보였다 금방 사라짐").
    /// 벽시계로 재면 호출 빈도와 무관하게 항상 설정된 시간만큼 보인다.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        let delay = hide_delay_ms();
        let mut redraw = core::mem::take(&mut self.dirty);
        // 속도 HUD가 사라지는 중이면 계속 그린다.
        let cfg = self.fast_cfg();
        redraw |= self.hud.tick(std::time::Instant::now(), &cfg);
        for axis in [Axis::V, Axis::H] {
            let i = axis.idx();
            let engaged = matches!(self.hover, Some(a) if a == axis)
                || matches!(self.drag, Some((a, _)) if a == axis);
            // 그 축에 활동이 있었거나 호버/드래그 중(2단계)이면 마감을 계속 뒤로 민다.
            if self.bumped[i] || engaged {
                self.bumped[i] = false;
                self.hide_at_ms[i] = now_ms.saturating_add(delay);
                continue;
            }
            // delay 0 = 자동 숨김 안 함(사용자가 항상 보이길 택한 경우).
            if self.active[i] && delay != 0 && now_ms >= self.hide_at_ms[i] {
                self.active[i] = false;
                redraw = true;
            }
        }
        redraw
    }

    /// 오버레이 렌더 — `active`일 때만 그린다(스크롤 전엔 보이지 않는다).
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &self,
        ctx: &mut dyn DrawCtx,
        theme: &Theme,
        vp: Rect,
        content_w: i32,
        content_h: i32,
        off_x: i32,
        off_y: i32,
        scale: f32,
    ) {
        // 속도 HUD(막대 표시 여부와 무관 · 영역 = 뷰포트).
        self.hud.paint(ctx, theme, vp, scale, &self.fast_cfg());
        if !self.is_visible() {
            return;
        }
        let thin = sc(THIN, scale);
        let thick = sc(THICK, scale);
        let radius = thin / 2;
        // 세로 — 이 축이 깨어 있을 때만.
        if let Some(hit) =
            Self::v_thumb(vp, content_h, off_y, scale, thick).filter(|_| self.is_active(Axis::V))
        {
            let hot =
                matches!(self.hover, Some(Axis::V)) || matches!(self.drag, Some((Axis::V, _)));
            let w = if hot { thick } else { thin };
            let x = vp.right() - w - sc(MARGIN, scale);
            let thumb = Rect::new(x, hit.y, w, hit.h);
            let a = if hot { ALPHA_HOT } else { ALPHA_IDLE };
            ctx.fill_round_rect_alpha(thumb, radius, theme.text_dim, a);
        }
        // 가로 — 이 축이 깨어 있을 때만.
        if let Some(hit) =
            Self::h_thumb(vp, content_w, off_x, scale, thick).filter(|_| self.is_active(Axis::H))
        {
            let hot =
                matches!(self.hover, Some(Axis::H)) || matches!(self.drag, Some((Axis::H, _)));
            let h = if hot { thick } else { thin };
            let y = vp.bottom() - h - sc(MARGIN, scale);
            let thumb = Rect::new(hit.x, y, hit.w, h);
            let a = if hot { ALPHA_HOT } else { ALPHA_IDLE };
            ctx.fill_round_rect_alpha(thumb, radius, theme.text_dim, a);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vp() -> Rect {
        Rect::new(0, 0, 200, 100)
    }
    fn wheel(d: i32) -> InputEvent {
        InputEvent::Wheel { delta: d }
    }
    fn down(x: i32, y: i32) -> InputEvent {
        InputEvent::MouseDown {
            x,
            y,
            shift: false,
            primary: false,
        }
    }
    fn mv(x: i32, y: i32) -> InputEvent {
        InputEvent::MouseMove { x, y }
    }
    fn up() -> InputEvent {
        InputEvent::MouseUp { x: 0, y: 0 }
    }

    /// 숨김 지연은 **프로세스 전역**이라, 값을 바꾸는 테스트와 시간에 의존하는 테스트가
    /// 동시에 돌면 서로를 흔든다. 그 테스트들만 이 잠금을 잡는다.
    static DELAY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    fn lock_delay() -> std::sync::MutexGuard<'static, ()> {
        DELAY_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn track_click_outside_thumb_is_consumed_and_jumps() {
        // nexa-sql 09-16: 트랙(썸 밖) 클릭이 아래로 흘러 셀 드래그 선택이 되던 것.
        let mut b = ScrollBars::new();
        let vp = Rect::new(0, 0, 200, 100);
        let (cw, ch) = (1000, 100);
        // 가로 휠로 깨워 가로 바를 보이게.
        let _ = b.on_event(&InputEvent::HWheel { delta: 30 }, vp, cw, ch, 0, 0, 1.0);
        let thick = sc(THICK, 1.0);
        let (ox, _, consumed) = b.on_event(
            &InputEvent::MouseDown {
                x: 180,
                y: vp.bottom() - thick / 2,
                shift: false,
                primary: false,
            },
            vp,
            cw,
            ch,
            10,
            0,
            1.0,
        );
        assert!(consumed, "트랙 클릭은 소비된다");
        assert!(ox > 10, "썸이 클릭 자리로 옮겨진다: {ox}");
        let (_, _, up) = b.on_event(
            &InputEvent::MouseUp { x: 180, y: 95 },
            vp,
            cw,
            ch,
            ox,
            0,
            1.0,
        );
        assert!(up);
    }

    /// 가장자리 접근: 아래 띠에 오면 가로 막대만, 오른쪽 띠에 오면 세로 막대만 깨어난다 · 그 축이 필요 없으면(내용이 들어가면) 안 깨어난다 ·
    /// 가운데 이동은 아무것도 깨우지 않는다(09-22 가로 스크롤 결함).
    #[test]
    fn approaching_an_edge_reveals_that_axis_only() {
        let vp = Rect::new(0, 0, 200, 100);
        let mut b = ScrollBars::new();
        b.on_event(
            &InputEvent::MouseMove { x: 100, y: 50 },
            vp,
            800,
            400,
            0,
            0,
            1.0,
        );
        assert!(!b.is_visible(), "가운데 이동은 깨우지 않는다");
        b.on_event(
            &InputEvent::MouseMove { x: 100, y: 96 },
            vp,
            800,
            400,
            0,
            0,
            1.0,
        );
        assert!(
            b.is_active(Axis::H) && !b.is_active(Axis::V),
            "아래 띠 = 가로만"
        );
        let mut c = ScrollBars::new();
        c.on_event(
            &InputEvent::MouseMove { x: 196, y: 50 },
            vp,
            800,
            400,
            0,
            0,
            1.0,
        );
        assert!(
            c.is_active(Axis::V) && !c.is_active(Axis::H),
            "오른쪽 띠 = 세로만"
        );
        let mut d = ScrollBars::new();
        d.on_event(
            &InputEvent::MouseMove { x: 100, y: 96 },
            vp,
            150,
            400,
            0,
            0,
            1.0,
        );
        assert!(!d.is_active(Axis::H), "가로가 필요 없으면 안 깨어난다");
        // 드러난 가로 막대는 썸을 잡아 끌 수 있다(가로 휠 없이도 가로 스크롤).
        let t = ScrollBars::h_thumb_for_test(vp, 800, 0, 1.0).expect("썸");
        let (ox, _, consumed) = b.on_event(
            &InputEvent::MouseDown {
                x: t.x + 2,
                y: t.y + 2,
                shift: false,
                primary: true,
            },
            vp,
            800,
            400,
            0,
            0,
            1.0,
        );
        assert!(consumed && ox == 0);
        let (ox, _, _) = b.on_event(
            &InputEvent::MouseMove {
                x: t.x + 52,
                y: t.y + 2,
            },
            vp,
            800,
            400,
            0,
            0,
            1.0,
        );
        assert!(ox > 0, "끌면 오프셋이 는다: {ox}");
    }

    #[test]
    fn hidden_until_scrolled() {
        let sb = ScrollBars::new();
        assert!(!sb.is_visible(), "스크롤 전엔 비활성(안 보임)");
    }

    #[test]
    fn wheel_wakes_only_its_axis() {
        let _g = lock_delay();
        // 세로 휠 = 세로 막대만(가로 콘텐츠가 넘쳐도 가로 막대는 안 뜬다 — nexa-sql 사용자 09-14).
        let mut sb = ScrollBars::new();
        sb.on_event(&wheel(-100), vp(), 800, 400, 0, 0, 1.0);
        assert!(sb.is_active(Axis::V) && !sb.is_active(Axis::H));
        // 가로 휠 = 가로도 깨어난다 · 숨김은 축별.
        sb.tick(0);
        sb.on_event(
            &InputEvent::HWheel { delta: 300 },
            vp(),
            800,
            400,
            0,
            0,
            1.0,
        );
        sb.tick(1000); // 가로 마감 = 3000 · 세로 마감 = 2000
        assert!(sb.is_active(Axis::V) && sb.is_active(Axis::H));
        assert!(sb.tick(2000), "세로 먼저 숨김");
        assert!(!sb.is_active(Axis::V) && sb.is_active(Axis::H));
        assert!(sb.tick(3000) && !sb.is_visible(), "가로도 숨김");
        // 프로그램적 표시는 둘 다.
        sb.show();
        assert!(sb.is_active(Axis::V) && sb.is_active(Axis::H));
    }

    #[test]
    fn wheel_scrolls_and_activates_and_clamps() {
        let mut sb = ScrollBars::new();
        let (_ox, oy, consumed) = sb.on_event(&wheel(-300), vp(), 200, 400, 0, 0, 1.0);
        assert!(consumed, "세로 스크롤 소비");
        assert_eq!(oy, 100, "delta/3=100");
        assert!(sb.is_visible(), "스크롤 시 표시");
        // 과도 스크롤 클램프(content_h 400 - vp.h 100 = 300).
        let (_ox, oy, _) = sb.on_event(&wheel(-100_000), vp(), 200, 400, oy, 0, 1.0);
        assert_eq!(oy, 300);
    }

    #[test]
    fn drag_thumb_updates_offset() {
        let mut sb = ScrollBars::new();
        sb.on_event(&wheel(-1), vp(), 200, 400, 0, 0, 1.0); // 보일 때만 썸이 잡힌다
                                                            // v_thumb at off 0: thumb top = vp.y = 0. 두께 THICK=11. 썸 폭 안 x=200-11-2=187.
        let t = ScrollBars::v_thumb(vp(), 400, 0, 1.0, 11).unwrap();
        let (_ox, _oy, consumed) = sb.on_event(&down(t.x + 2, t.y + 2), vp(), 200, 400, 0, 0, 1.0);
        assert!(consumed && sb.drag.is_some(), "썸 클릭 = 드래그 시작");
        // 아래로 드래그.
        let (_ox, oy, consumed) = sb.on_event(&mv(t.x + 2, t.y + 40), vp(), 200, 400, 0, 0, 1.0);
        assert!(consumed);
        assert!(oy > 0, "드래그로 오프셋 증가: {oy}");
        // 해제.
        let (_ox, _oy, consumed) = sb.on_event(&up(), vp(), 200, 400, oy, 0, 1.0);
        assert!(consumed && sb.drag.is_none(), "해제 = 드래그 종료");
    }

    #[test]
    fn no_bar_when_content_fits() {
        assert!(ScrollBars::v_thumb(vp(), 80, 0, 1.0, 11).is_none());
        assert!(ScrollBars::h_thumb(vp(), 150, 0, 1.0, 11).is_none());
    }

    #[test]
    fn fades_after_the_configured_delay_not_before() {
        let _g = lock_delay();
        let mut sb = ScrollBars::new();
        sb.on_event(&wheel(-100), vp(), 200, 400, 0, 0, 1.0);
        assert!(sb.is_visible(), "스크롤 = 1단계 표시");
        sb.tick(0); // 마감 = 0 + 2000ms
        assert!(!sb.tick(1999) && sb.is_visible(), "지연 이전엔 유지");
        assert!(sb.tick(2000) && !sb.is_visible(), "지연이 지나면 숨김");
    }

    #[test]
    fn frequent_ticks_do_not_shorten_the_delay() {
        let _g = lock_delay();
        // ★ 회귀: 예전 구현은 tick **횟수**를 셌다. 호스트는 이벤트마다 tick을 부르므로
        //   드래그 중 초당 수십 번 불려 막대가 0.2초 만에 사라졌다(08-10 지적).
        let mut sb = ScrollBars::new();
        sb.on_event(&wheel(-100), vp(), 200, 400, 0, 0, 1.0);
        sb.tick(0);
        for i in 0..500 {
            // 500번 불러도 시계가 1.5초 안이면 살아 있어야 한다.
            assert!(!sb.tick(i * 3), "호출 횟수로 사라지면 안 된다(t={})", i * 3);
        }
        assert!(sb.is_visible());
    }

    #[test]
    fn hide_delay_is_configurable_and_zero_means_always_on() {
        let _g = lock_delay();
        let mut sb = ScrollBars::new();
        set_hide_delay_ms(500);
        sb.on_event(&wheel(-100), vp(), 200, 400, 0, 0, 1.0);
        sb.tick(0);
        assert!(!sb.tick(499));
        assert!(sb.tick(500) && !sb.is_visible(), "설정한 500ms에 숨는다");
        // 0 = 자동 숨김 없음.
        set_hide_delay_ms(0);
        sb.on_event(&wheel(-100), vp(), 200, 400, 0, 0, 1.0);
        sb.tick(0);
        assert!(!sb.tick(u64::MAX) && sb.is_visible(), "0이면 숨기지 않는다");
        set_hide_delay_ms(DEFAULT_HIDE_MS); // 전역이라 되돌린다
    }

    #[test]
    fn hover_keeps_visible_until_unhover_and_is_thicker() {
        let _g = lock_delay();
        let mut sb = ScrollBars::new();
        sb.on_event(&wheel(-100), vp(), 200, 400, 0, 0, 1.0);
        // 세로 썸 위로 호버(2단계) — 시간이 아무리 흘러도 유지.
        let t = ScrollBars::v_thumb(vp(), 400, 0, 1.0, 11).unwrap();
        sb.on_event(&mv(t.x + 2, t.y + 2), vp(), 200, 400, 0, 0, 1.0);
        assert_eq!(sb.hover, Some(Axis::V), "썸 위 = 호버");
        for i in 0..50 {
            sb.tick(i * 1000);
        }
        assert!(sb.is_visible(), "호버 중(2단계)엔 유지 — 사라지지 않는다");
        // 마지막 호버 틱이 t=49_000이었으니 마감은 51_000.
        // 썸 밖으로 이동(1단계) → 남은 지연을 채운 뒤에야 숨는다(즉시 사라지지 않는다).
        sb.on_event(&mv(0, 0), vp(), 200, 400, 0, 0, 1.0);
        assert_eq!(sb.hover, None);
        sb.tick(50_000);
        assert!(sb.is_visible(), "언호버 직후엔 아직 지연이 남아 있다");
        assert!(
            sb.tick(51_000) && !sb.is_visible(),
            "언호버 후 지연 경과 → 숨김"
        );
    }

    #[test]
    fn hover_change_requests_a_redraw() {
        let _g = lock_delay();
        // 두께가 바뀌는데 다시 그리지 않으면 사용자 눈엔 아무 일도 안 일어난다.
        let mut sb = ScrollBars::new();
        sb.on_event(&wheel(-100), vp(), 200, 400, 0, 0, 1.0);
        sb.tick(0);
        let t = ScrollBars::v_thumb(vp(), 400, 0, 1.0, 11).unwrap();
        sb.on_event(&mv(t.x + 2, t.y + 2), vp(), 200, 400, 0, 0, 1.0);
        assert!(sb.tick(1), "호버 진입 = 재그리기 요청");
        sb.on_event(&mv(0, 0), vp(), 200, 400, 0, 0, 1.0);
        assert!(sb.tick(2), "호버 이탈 = 재그리기 요청");
    }

    /// ★ 가속 부품(설정 넘김 · 순수): 처음 step번은 1배 · 같은 방향이 빨리 이어지면 배수 증가 · 방향 바꾸면 리셋 · 간격이 길면
    /// 리셋 · 상한 max · 꺼져 있으면 늘 1.
    #[test]
    fn scroll_accel_streak_rules() {
        use std::time::{Duration, Instant};
        let cfg = FastScroll {
            enabled: true,
            step: 5,
            max: 8,
            ..FastScroll::default()
        };
        let mut a = ScrollAccel::new();
        let t0 = Instant::now();
        let mut t = t0;
        let mut seen = Vec::new();
        for _ in 0..15 {
            seen.push(a.factor_at(1, t, &cfg));
            t += Duration::from_millis(40);
        }
        assert_eq!(&seen[..5], &[1; 5], "처음은 그대로: {seen:?}");
        assert_eq!(seen[5], 2, "{seen:?}");
        assert_eq!(seen[10], 3, "{seen:?}");
        assert_eq!(a.factor_at(-1, t, &cfg), 1, "방향 전환 = 1");
        for _ in 0..5 {
            t += Duration::from_millis(40);
            a.factor_at(-1, t, &cfg);
        }
        assert_eq!(a.factor_at(-1, t + Duration::from_millis(40), &cfg), 2);
        assert_eq!(
            a.factor_at(-1, t + Duration::from_millis(cfg.window_ms + 500), &cfg),
            1,
            "간격이 길면 리셋"
        );
        let mut b = ScrollAccel::new();
        let mut t = t0;
        let mut last = 1;
        for _ in 0..200 {
            last = b.factor_at(1, t, &cfg);
            t += Duration::from_millis(10);
        }
        assert_eq!(last, cfg.max, "상한");
        let off = FastScroll::default();
        let mut c = ScrollAccel::new();
        for _ in 0..30 {
            assert_eq!(c.factor_at(1, t, &off), 1, "꺼짐 = 늘 1");
            t += Duration::from_millis(10);
        }
    }

    /// ScrollBars 고속 휠(영역 override): 빠른 틱이 이어지면 이동량이 커진다 · 끄면 늘 delta/3 · HUD = 가속 중 1.0 → hold 뒤
    /// fade 동안 줄다가 사라짐 · HUD 끔이면 안 보인다.
    #[test]
    fn scrollbars_fast_wheel_multiplies() {
        use std::time::{Duration, Instant};
        let vp = Rect::new(0, 0, 100, 100);
        let mut slow = ScrollBars::new();
        let mut fast = ScrollBars::new();
        fast.set_fast_override(Some(FastScroll {
            enabled: true,
            ..FastScroll::default()
        }));
        let (mut ys, mut yf) = (0, 0);
        for _ in 0..30 {
            ys = slow
                .on_event(
                    &InputEvent::Wheel { delta: -120 },
                    vp,
                    100,
                    100_000,
                    0,
                    ys,
                    1.0,
                )
                .1;
            yf = fast
                .on_event(
                    &InputEvent::Wheel { delta: -120 },
                    vp,
                    100,
                    100_000,
                    0,
                    yf,
                    1.0,
                )
                .1;
        }
        assert_eq!(ys, 30 * 40);
        assert!(yf > ys, "fast {yf} > slow {ys}");
        assert!(fast.hud().visible());
        let cfg = fast.fast_cfg();
        let now = Instant::now();
        assert_eq!(fast.hud().alpha_at(now, &cfg), Some(1.0));
        let mid = now + Duration::from_millis(cfg.hud_hold_ms + cfg.hud_fade_ms / 2);
        let a = fast.hud().alpha_at(mid, &cfg).expect("fading");
        assert!(a > 0.0 && a < 1.0, "{a}");
        let end = now + Duration::from_millis(cfg.hud_hold_ms + cfg.hud_fade_ms + 5);
        assert_eq!(fast.hud().alpha_at(end, &cfg), None);
        let mut h = SpeedHud::default();
        h.note_at(
            4,
            &FastScroll {
                enabled: true,
                hud: false,
                ..FastScroll::default()
            },
            now,
        );
        assert!(!h.visible());
    }

    /// 속도 HUD의 크기는 배율에 비례한다(10-02 맥 Retina: 물리 px 높이를 논리 px 증분으로 넘겨 2배율에서 글꼴이
    /// 75 %가 아니라 50 %가 되던 결함) — 배율 1과 2에서 캡슐 크기가 정확히 2배.
    #[test]
    fn speed_hud_size_scales_with_dpi() {
        use crate::theme::Color;
        /// 실제 래스터와 같은 단위 모델: 슬롯 크기·증분 = 논리 px · 측정값 = 물리 px(크기 × 배율).
        struct ScaleCtx {
            scale: f32,
            size: f32,
            capsules: Vec<Rect>,
        }
        impl DrawCtx for ScaleCtx {
            fn select_font(&mut self, _slot: FontSlot, _bold: bool) {
                self.size = 16.0;
            }
            fn select_font_sized(&mut self, slot: FontSlot, bold: bool, delta_px: f32) {
                self.select_font(slot, bold);
                self.size = (self.size + delta_px).max(1.0);
            }
            fn fill_rect(&mut self, _r: Rect, _c: Color) {}
            fn text_opaque(&mut self, _x: i32, _y: i32, _c: Rect, _t: &str, _f: Color, _b: Color) {}
            fn text(&mut self, _x: i32, _y: i32, _c: Rect, _t: &str, _f: Color) {}
            fn text_width(&mut self, text: &str) -> i32 {
                (text.chars().count() as f32 * self.size * self.scale * 0.5).round() as i32
            }
            fn text_height(&mut self) -> i32 {
                (self.size * self.scale).round() as i32
            }
            fn fill_round_rect(&mut self, rect: Rect, _radius: i32, _color: Color) {
                self.capsules.push(rect);
            }
        }
        let cfg = FastScroll {
            enabled: true,
            hud: true,
            hud_hold_ms: 60_000,
            ..FastScroll::default()
        };
        let mut hud = SpeedHud::default();
        hud.note(12, &cfg);
        let capsule = |scale: f32| {
            let mut ctx = ScaleCtx {
                scale,
                size: 16.0,
                capsules: Vec::new(),
            };
            let area = Rect::new(0, 0, sc(400, scale), sc(300, scale));
            hud.paint(&mut ctx, &Theme::dark(), area, scale, &cfg);
            assert_eq!(ctx.capsules.len(), 1, "scale {scale}");
            ctx.capsules[0]
        };
        let (a, b, c) = (capsule(1.0), capsule(2.0), capsule(1.5));
        // 배율 1: 글꼴 16 → 12(75 %) · 캡슐 높이 = 12 + 3×2.
        assert_eq!(a.h, 18);
        assert_eq!((b.w, b.h), (a.w * 2, a.h * 2), "1x {a:?} · 2x {b:?}");
        // 배율 1.5: 글꼴 18(물리) + 여백 round(4.5) = 5 × 2.
        assert_eq!(c.h, 28, "1.5x {c:?}");
    }
}

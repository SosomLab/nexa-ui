//! **고속 스크롤**(10-02 사용자 — nexa-sql 09-30 "상/하 이동시 고속 스크롤" 이식.
//! 원본 = `nexa-ui/crates/nexa-ctl/src/controls/scroll.rs`의 `FastScroll`·`ScrollAccel`·`SpeedHud`).
//!
//! - 같은 방향의 사건(휠 노치·키 자동 반복)이 짧은 간격으로 이어지면 한 번의 이동량을 배수로
//!   키운다. 사건마다 `factor(dir)` 하나만 — 큐·타이머·관성 없음(멈추면 즉시 멈춤).
//! - 배수 = `1 + 연속/step`(상한 `max`). 처음 몇 번은 그대로(정확한 한 줄 이동이 먼저).
//! - 가속 중이면 영역 안 `hud_pos`(3×3 — 타입어헤드 배지와 같은 행우선 0..9) 자리에 `×N`
//!   배지를 보이고, 마지막 사건 뒤 `hud_hold_ms` 유지 → `hud_fade_ms` 동안 제곱 감속으로 사라진다.
//! - **Windows 정밀 터치패드 제외**: 노치(120) 미만 delta는 OS가 이미 가속·관성을 넣어 주므로
//!   [`FastScroller::wheel`]은 |delta| ≥ 노치일 때만 배수를 적용한다(분수 누적은 `WheelAccum`).
//! - 설정은 프로세스 전역([`set_fast_scroll`]) — 핫스왑 원칙: 8개 스크롤 영역이 값을 들고
//!   다니지 않고 사건마다 읽는다.

use std::time::Instant;

use crate::draw::DrawCtx;
use crate::event::WHEEL_DELTA;
use crate::geom::Rect;
use crate::theme::Theme;
use crate::widget::Invalidations;

/// 고속 스크롤 설정(설정 창 `scroll.*`).
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
    /// 속도 배지 표시.
    pub hud: bool,
    /// 배지 자리(3×3 행우선 0..9 — 0=좌상 · 2=우상 · 4=가운데 · 6=좌하 · 8=우하).
    pub hud_pos: u8,
    /// 마지막 가속 사건 뒤 배지가 그대로 보이는 시간(ms).
    pub hud_hold_ms: u64,
    /// 그 뒤 서서히 사라지는 시간(ms).
    pub hud_fade_ms: u64,
}

impl Default for FastScroll {
    /// 기본 = **켬**, 수치 = nexa-sql 설정 기본값(`scroll.fast_speed = fast` → step 3·max 16 —
    /// 사용자 10-02 "nexa-sql과 동일한 값").
    fn default() -> Self {
        Self {
            enabled: true,
            step: 3,
            max: 16,
            window_ms: 160,
            hud: true,
            hud_pos: 2,
            hud_hold_ms: 250,
            hud_fade_ms: 600,
        }
    }
}

static FAST: std::sync::RwLock<FastScroll> = std::sync::RwLock::new(FastScroll {
    enabled: true,
    step: 3,
    max: 16,
    window_ms: 160,
    hud: true,
    hud_pos: 2,
    hud_hold_ms: 250,
    hud_fade_ms: 600,
});

/// 전역 설정 교체(설정 창 즉시 적용).
pub fn set_fast_scroll(cfg: FastScroll) {
    if let Ok(mut g) = FAST.write() {
        *g = cfg;
    }
}

/// 현재 전역 설정.
#[must_use]
pub fn fast_scroll() -> FastScroll {
    FAST.read().map(|g| *g).unwrap_or_default()
}

/// 파일 그리드 전용 **한 단계 더 빠른** 설정(nexa-sql `scroll.fast_grid_extra` — 결과 그리드 규약:
/// 한 번 먼저 오르고(step-1) 상한 두 배). None = 전역과 동일. 호스트가 설정 적용 때 넣는다.
static FAST_GRID: std::sync::RwLock<Option<FastScroll>> = std::sync::RwLock::new(None);

pub fn set_fast_scroll_grid(cfg: Option<FastScroll>) {
    if let Ok(mut g) = FAST_GRID.write() {
        *g = cfg;
    }
}

/// 그리드 설정(없으면 전역).
#[must_use]
pub fn fast_scroll_grid() -> FastScroll {
    FAST_GRID
        .read()
        .ok()
        .and_then(|g| *g)
        .unwrap_or_else(fast_scroll)
}

/// 전역에서 "한 단계 더 빠른" 그리드 설정 파생(nexa-sql 규약).
#[must_use]
pub fn grid_extra_of(base: &FastScroll) -> FastScroll {
    FastScroll {
        step: base.step.saturating_sub(1).max(1),
        max: base.max.saturating_mul(2),
        ..*base
    }
}

/// 가속기 — 같은 방향 연속 사건 횟수로 배수를 낸다.
#[derive(Debug, Clone, Default)]
pub struct ScrollAccel {
    last: Option<Instant>,
    dir: i32,
    streak: u32,
}

impl ScrollAccel {
    /// 사건 하나(`dir` 부호만 · 0 = 리셋) → 이번 이동에 곱할 배수(1 = 가속 없음). 전역 설정 사용.
    pub fn factor(&mut self, dir: i32) -> i32 {
        self.factor_at(dir, Instant::now(), &fast_scroll())
    }

    /// 시각·설정을 밖에서 주는 판(시험 · 순수).
    pub fn factor_at(&mut self, dir: i32, now: Instant, cfg: &FastScroll) -> i32 {
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

/// 속도 배지 — 가속 사건마다 `note(k)`, 호스트 `tick`으로 유지·페이드.
#[derive(Debug, Clone, Default)]
pub struct SpeedHud {
    factor: i32,
    last: Option<Instant>,
    fade_from: Option<Instant>,
    /// 마지막으로 그린 배지 rect(무효화용 — paint가 채움).
    drawn: std::cell::Cell<Rect>,
}

impl SpeedHud {
    pub fn note(&mut self, k: i32, cfg: &FastScroll) {
        self.note_at(k, cfg, Instant::now());
    }

    pub fn note_at(&mut self, k: i32, cfg: &FastScroll, now: Instant) {
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

    /// 지금의 세기 0..=1(없으면 None). 순수.
    #[must_use]
    pub fn alpha_at(&self, now: Instant, cfg: &FastScroll) -> Option<f32> {
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

    /// 호스트 틱 — 보이는 동안 `true`(다시 그릴 것·틱 재요청). 다 사라지면 상태를 지우고
    /// 마지막 한 번 `true`.
    pub fn tick(&mut self, now: Instant, cfg: &FastScroll) -> bool {
        if self.last.is_none() {
            return false;
        }
        match self.alpha_at(now, cfg) {
            None => {
                self.clear();
                true
            }
            Some(_) => true,
        }
    }

    /// 배지 rect(영역 `area` 안 `cfg.hud_pos` 자리 · 캡슐 = 글자 폭 + 여백).
    fn place(area: Rect, w: i32, h: i32, pad: i32, pos: u8) -> Rect {
        let x = match pos % 3 {
            0 => area.x + pad,
            1 => area.x + (area.w - w) / 2,
            _ => area.right() - w - pad,
        };
        let y = match pos / 3 {
            0 => area.y + pad,
            1 => area.y + (area.h - h) / 2,
            _ => area.bottom() - h - pad,
        };
        Rect::new(x.max(area.x), y.max(area.y), w, h)
    }

    /// 그리기 — `area` 안 `cfg.hud_pos` 자리에 캡슐 `×k`(세기만큼 투명). 높이 = `row_h`.
    pub fn paint(
        &self,
        ctx: &mut dyn DrawCtx,
        theme: &Theme,
        area: Rect,
        row_h: i32,
        pad_x: i32,
        cfg: &FastScroll,
    ) {
        let Some(a) = self.alpha_at(Instant::now(), cfg) else {
            self.drawn.set(Rect::default());
            return;
        };
        if a < 0.08 || area.w <= 0 || area.h <= 0 {
            return;
        }
        let label = format!("×{}", self.factor);
        let tw = ctx.text_width(&label);
        let (w, h) = (tw + pad_x * 2, row_h);
        let rect = Self::place(area, w, h, pad_x.max(4), cfg.hud_pos.min(8));
        self.drawn.set(rect);
        // 캡슐 = accent 40% × 세기 · 글자 = 테마 글자색(라이트·다크 모두 식별 — nexa-sql 09-30)
        ctx.fill_round_rect_alpha(rect, h / 2, theme.accent, (102.0 * a) as u8);
        if a > 0.35 {
            let ty = rect.y + (h - (h * 4) / 5) / 2;
            ctx.text(rect.x + pad_x, ty, rect, &label, theme.text);
        }
    }

    /// 마지막 그린 배지 rect(무효화용).
    #[must_use]
    pub fn drawn_rect(&self) -> Rect {
        self.drawn.get()
    }
}

/// 위젯용 묶음 — 가속기 + 배지. 휠·키 사건 → 이동량 배수, 틱 → 배지 페이드.
/// `grid` = 파일 그리드(한 단계 더 빠른 [`fast_scroll_grid`] 사용).
#[derive(Debug, Clone, Default)]
pub struct FastScroller {
    accel: ScrollAccel,
    hud: SpeedHud,
    grid: bool,
}

impl FastScroller {
    /// 파일 그리드용(설정 `fast_scroll_grid_extra` 반영).
    #[must_use]
    pub fn for_grid() -> Self {
        Self {
            grid: true,
            ..Self::default()
        }
    }

    fn cfg(&self) -> FastScroll {
        if self.grid {
            fast_scroll_grid()
        } else {
            fast_scroll()
        }
    }

    /// 휠 사건: `delta`(원시, 부호 = 방향)로 가속 판정 후 `units`(이미 노치 환산된 이동량)에
    /// 배수를 곱해 돌려준다. **노치 미만 delta(정밀 터치패드)는 배수 1** — OS 가속에 맡긴다.
    pub fn wheel(&mut self, delta: i32, units: i32) -> i32 {
        if units == 0 {
            return 0;
        }
        let cfg = self.cfg();
        if delta.abs() < WHEEL_DELTA {
            self.accel.reset();
            self.hud.note(1, &cfg);
            return units;
        }
        let k = self.accel.factor_at(delta, Instant::now(), &cfg);
        self.hud.note(k, &cfg);
        units.saturating_mul(k)
    }

    /// 키 자동 반복 사건(`dir` 부호) → 한 번에 옮길 행 수(= 배수).
    pub fn key(&mut self, dir: i32) -> i32 {
        let cfg = self.cfg();
        let k = self.accel.factor_at(dir, Instant::now(), &cfg);
        self.hud.note(k, &cfg);
        k
    }

    /// 연속 끊기(포커스 이탈·내용 교체).
    pub fn reset(&mut self) {
        self.accel.reset();
    }

    /// 호스트 틱 — 배지가 보이는 동안 그 영역을 무효화하고 틱을 재요청한다.
    pub fn tick(&mut self, area: Rect, inv: &mut Invalidations) {
        if !self.hud.visible() {
            return;
        }
        let cfg = self.cfg();
        if self.hud.tick(Instant::now(), &cfg) {
            let r = self.hud.drawn_rect();
            inv.push(if r.w > 0 { r } else { area });
            if self.hud.visible() {
                inv.request_tick();
            }
        }
    }

    /// 배지 그리기(위젯 paint 끝에서 — 내용 위 마지막) + 보이는 동안 틱 요청은 호스트가
    /// `tick`으로 이어 간다. 사건 직후 첫 틱은 `note` 호출자(위젯)가 `inv.request_tick()`.
    pub fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme, area: Rect, row_h: i32, pad_x: i32) {
        if !self.hud.visible() {
            return;
        }
        self.hud.paint(ctx, theme, area, row_h, pad_x, &self.cfg());
    }

    /// 배지가 보이는가(호출자가 사건 직후 틱을 요청할지 판단).
    #[must_use]
    pub fn hud_visible(&self) -> bool {
        self.hud.visible()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn cfg() -> FastScroll {
        FastScroll::default()
    }

    #[test]
    fn factor_grows_every_step_within_window_and_caps() {
        let mut a = ScrollAccel::default();
        let t0 = Instant::now();
        let c = cfg();
        // step 3: 첫 3번(연속 0..2) = ×1, 4번째(연속 3) = ×2 … 상한 16
        let mut ks = Vec::new();
        for i in 0..80 {
            ks.push(a.factor_at(-120, t0 + Duration::from_millis(50 * i), &c));
        }
        assert_eq!(&ks[..4], &[1, 1, 1, 2]);
        assert_eq!(ks[6], 3);
        assert_eq!(*ks.last().unwrap(), 16, "상한");
        let g = grid_extra_of(&c);
        assert_eq!((g.step, g.max), (2, 32), "그리드 = 한 단계 먼저·상한 두 배");
    }

    #[test]
    fn direction_change_or_gap_resets() {
        let mut a = ScrollAccel::default();
        let t0 = Instant::now();
        let c = cfg();
        for i in 0..4 {
            a.factor_at(1, t0 + Duration::from_millis(50 * i), &c);
        }
        assert_eq!(a.factor_at(1, t0 + Duration::from_millis(200), &c), 2);
        assert_eq!(
            a.factor_at(-1, t0 + Duration::from_millis(350), &c),
            1,
            "방향 전환 = 리셋"
        );
        for i in 0..4 {
            a.factor_at(-1, t0 + Duration::from_millis(400 + 50 * i), &c);
        }
        assert_eq!(
            a.factor_at(-1, t0 + Duration::from_millis(1000), &c),
            1,
            "간격 초과 = 리셋"
        );
        let off = FastScroll {
            enabled: false,
            ..c
        };
        assert_eq!(a.factor_at(-1, t0, &off), 1, "끔 = 항상 1");
    }

    #[test]
    fn hud_holds_then_fades_and_clears() {
        let mut h = SpeedHud::default();
        let c = cfg();
        let t0 = Instant::now();
        h.note_at(3, &c, t0);
        assert_eq!(h.alpha_at(t0 + Duration::from_millis(100), &c), Some(1.0));
        let mid = h
            .alpha_at(t0 + Duration::from_millis(250 + 300), &c)
            .unwrap();
        assert!(mid > 0.0 && mid < 1.0, "페이드 중 {mid}");
        assert_eq!(h.alpha_at(t0 + Duration::from_millis(250 + 600), &c), None);
        assert!(
            h.tick(t0 + Duration::from_millis(900), &c),
            "마지막 한 번 true"
        );
        assert!(!h.visible());
        assert!(!h.tick(t0 + Duration::from_millis(901), &c));
        // 1배로 돌아오면 그 시점부터 페이드
        h.note_at(2, &c, t0);
        h.note_at(1, &c, t0 + Duration::from_millis(10));
        assert!(h.alpha_at(t0 + Duration::from_millis(20), &c).unwrap() < 1.0);
        // hud 꺼진 설정 = 즉시 소거
        let off = FastScroll { hud: false, ..c };
        h.note_at(5, &off, t0);
        assert!(!h.visible());
    }

    #[test]
    fn hud_placement_follows_3x3_index() {
        let area = Rect::new(100, 200, 400, 300);
        let (w, h, pad) = (40, 20, 6);
        assert_eq!(
            SpeedHud::place(area, w, h, pad, 0),
            Rect::new(106, 206, 40, 20)
        );
        assert_eq!(
            SpeedHud::place(area, w, h, pad, 2),
            Rect::new(454, 206, 40, 20)
        );
        assert_eq!(
            SpeedHud::place(area, w, h, pad, 4),
            Rect::new(280, 340, 40, 20)
        );
        assert_eq!(
            SpeedHud::place(area, w, h, pad, 8),
            Rect::new(454, 474, 40, 20)
        );
    }

    #[test]
    fn scroller_ignores_sub_notch_trackpad_deltas() {
        let mut f = FastScroller::default();
        // 노치 미만 = 배수 없음(그대로)
        for _ in 0..30 {
            assert_eq!(f.wheel(-8, 1), 1);
        }
        assert!(!f.hud_visible());
        // 노치 연타 = 배수(연속 3 넘으면 ×2 · 6 = ×3)
        let mut last = 0;
        for _ in 0..8 {
            last = f.wheel(-120, 3);
        }
        assert_eq!(last, 9);
        assert!(f.hud_visible());
    }
}

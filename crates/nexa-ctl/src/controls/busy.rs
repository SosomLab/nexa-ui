//! ★ **진행 표시 링(혜성)** — 입력 상자 둘레를 도는 밝은 선 + 완료 깜빡임(nexa-sql 84 §8 · 사용자 10-07 "모든 검색에 · 컨트롤 속성으로 상속").
//!
//! - **속성으로 상속**: 모든 [`crate::controls::TextBox`]가 [`BusyRing`]을 갖는다 — 호스트는 상자에 `set_busy(Running/Done/Idle)`만 알리고
//!   그리기·시계·깜빡임은 상자가 한다(필터 틀 · 팔레트 · 찾기 막대 · 어느 상자든 같은 모양).
//! - **스타일 = 전역 토큰**([`BusyStyle`] · [`set_busy_style`]): 사용 여부 · 머리 두께 · 색(없으면 테마 강조색) · 한 바퀴 시간 ·
//!   최소 표시(짧게 끝나도 이만큼은 돈다) · 완료 테두리 유지(0 = 상자를 만질 때까지). 앱 설정 한 벌이 모든 상자에 바로 적용된다.
//! - **성능**: 꺼져 있으면(`enabled = false` · 성능 향상 모드) 상태만 기억하고 **그리지도, 프레임을 요구하지도** 않는다 — 비용 0.
//!
//! 상태 기계: `Idle` → `Running`(혜성) → `Done`(두 번 깜빡임 + 안쪽 플래시 → 완료 테두리) → 상자를 만지면(`dismiss_done`) `Idle`.
//! `Running`이 최소 표시보다 빨리 끝나면 때가 될 때까지 혜성을 유지한다(0.2 s 열거가 깜빡 지나가면 "안 돈다"로 보인다).

use crate::draw::DrawCtx;
use crate::geom::Rect;
use crate::theme::{Color, Theme};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// 진행 상태.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BusyState {
    /// 아무것도 안 그린다.
    #[default]
    Idle,
    /// 진행 중 = 혜성.
    Running,
    /// 끝남 = 깜빡임 뒤 완료 테두리.
    Done,
}

/// 전역 스타일(앱 설정 한 벌 · 모든 상자가 상속).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BusyStyle {
    /// 사용 여부(끄면 비용 0).
    pub enabled: bool,
    /// 혜성 머리 두께(논리 px · 꼬리는 절반).
    pub width: f32,
    /// 색(`None` = 테마 강조색).
    pub color: Option<Color>,
    /// 한 바퀴(ms).
    pub lap_ms: u32,
    /// 최소 표시(ms · 0 = 없음).
    pub hold_ms: u32,
    /// 완료 테두리 유지(ms · 0 = 상자를 만질 때까지).
    pub done_ms: u32,
}

impl Default for BusyStyle {
    fn default() -> Self {
        BusyStyle {
            enabled: true,
            width: 3.0,
            color: None,
            lap_ms: 1200,
            hold_ms: 600,
            done_ms: 0,
        }
    }
}

static BUSY_ENABLED: AtomicBool = AtomicBool::new(true);
static BUSY_WIDTH_X10: AtomicU32 = AtomicU32::new(30);
/// 0 = 테마 강조색 · 그 밖 = RGBA(알파 0이면 없음으로 본다 → 0xFF 알파로 저장).
static BUSY_COLOR: AtomicU32 = AtomicU32::new(0);
static BUSY_LAP_MS: AtomicU32 = AtomicU32::new(1200);
static BUSY_HOLD_MS: AtomicU32 = AtomicU32::new(600);
static BUSY_DONE_MS: AtomicU32 = AtomicU32::new(0);

/// 전역 스타일 지정(앱 설정 적용 때 · 다음 프레임부터 모든 상자).
pub fn set_busy_style(st: BusyStyle) {
    BUSY_ENABLED.store(st.enabled, Ordering::Relaxed);
    BUSY_WIDTH_X10.store(
        (st.width.clamp(0.5, 20.0) * 10.0).round() as u32,
        Ordering::Relaxed,
    );
    BUSY_COLOR.store(st.color.map_or(0, |c| c.0 | 0xFF00_0000), Ordering::Relaxed);
    BUSY_LAP_MS.store(st.lap_ms.max(100), Ordering::Relaxed);
    BUSY_HOLD_MS.store(st.hold_ms, Ordering::Relaxed);
    BUSY_DONE_MS.store(st.done_ms, Ordering::Relaxed);
}

/// 지금 전역 스타일.
#[must_use]
pub fn busy_style() -> BusyStyle {
    let c = BUSY_COLOR.load(Ordering::Relaxed);
    BusyStyle {
        enabled: BUSY_ENABLED.load(Ordering::Relaxed),
        width: BUSY_WIDTH_X10.load(Ordering::Relaxed) as f32 / 10.0,
        color: (c != 0).then_some(Color(c)),
        lap_ms: BUSY_LAP_MS.load(Ordering::Relaxed),
        hold_ms: BUSY_HOLD_MS.load(Ordering::Relaxed),
        done_ms: BUSY_DONE_MS.load(Ordering::Relaxed),
    }
}

/// 선 길이(둘레 비율) · 혜성 꼬리 단계 수 · 완료 깜빡임 한 위상(ms) · 위상 수(켬·끔·켬·끔 = 두 번) · 안쪽 플래시(ms).
const SEG_FRACTION: f32 = 0.22;
const COMET_STEPS: usize = 8;
pub const BLINK_MS: u64 = 130;
pub const BLINK_PHASES: u64 = 4;
const FLASH_MS: u64 = 520;

/// 상자 하나의 진행 표시 상태(상자가 소유 · 시계 = 호스트 `tick(now_ms)`).
#[derive(Debug, Default)]
pub struct BusyRing {
    state: BusyState,
    /// 완료 깜빡임 시작 시각(ms) — Running → Done 전환 뒤 첫 tick에 잡는다.
    blink_start: Option<u64>,
    blink_pending: bool,
    now: u64,
    /// 완료 시그니처를 사용자가 "봤다"(상자 재포커스 · 글 변경) → 다음 Running까지 Done을 안 켠다.
    done_seen: bool,
    /// 최소 표시: Running 시작 시각(첫 tick에 잡음) · Done 예약.
    run_start: Option<u64>,
    run_pending: bool,
    done_pending: bool,
}

impl BusyRing {
    /// 시계 전진 — 돌려주는 값 = 아직 움직이는 그림이 있다(호스트가 프레임을 이어 간다).
    pub fn tick(&mut self, now_ms: u64) -> bool {
        self.now = now_ms;
        if self.run_pending {
            self.run_pending = false;
            self.run_start = Some(now_ms);
        }
        let hold = u64::from(BUSY_HOLD_MS.load(Ordering::Relaxed));
        if self.done_pending
            && self
                .run_start
                .is_some_and(|t| now_ms.saturating_sub(t) >= hold)
        {
            self.done_pending = false;
            self.finish_done();
        }
        if self.blink_pending {
            self.blink_pending = false;
            self.blink_start = Some(now_ms);
        }
        // 완료 테두리 자동 해제(설정 · 0 = 만질 때까지).
        let done_ms = u64::from(BUSY_DONE_MS.load(Ordering::Relaxed));
        if done_ms > 0
            && self.state == BusyState::Done
            && self
                .blink_start
                .is_some_and(|t| now_ms.saturating_sub(t) >= done_ms.max(BLINK_MS * BLINK_PHASES))
        {
            self.state = BusyState::Idle;
            self.blink_start = None;
        }
        self.animating()
    }

    fn finish_done(&mut self) {
        // 완료 = 두 번 깜빡임(시각은 다음 tick에서).
        self.blink_pending = true;
        self.blink_start = None;
        self.state = BusyState::Done;
    }

    /// 호스트가 상태를 알려 준다(진행 = Running · 끝 = Done · 검색어 없음 = Idle).
    pub fn set_state(&mut self, st: BusyState) {
        // 완료 시그니처 뒤 사용자가 상자를 만졌으면 다음 Running까지 Done = 그리지 않음.
        let st = if st == BusyState::Done && self.done_seen {
            BusyState::Idle
        } else {
            st
        };
        if st == BusyState::Running {
            self.done_seen = false;
            // 끝나려던 참에 다시 진행 = 예약 취소(혜성 계속).
            self.done_pending = false;
        }
        if self.state == st {
            return;
        }
        if self.state == BusyState::Running && st == BusyState::Done {
            let hold = u64::from(BUSY_HOLD_MS.load(Ordering::Relaxed));
            let short = hold > 0
                && (self.run_pending
                    || self
                        .run_start
                        .is_some_and(|t| self.now.saturating_sub(t) < hold));
            if short {
                // 최소 표시 전 = Running 유지 · tick이 때가 되면 Done으로.
                self.done_pending = true;
                return;
            }
            self.finish_done();
            return;
        }
        if st != BusyState::Done {
            self.blink_start = None;
            self.blink_pending = false;
        }
        if st == BusyState::Running {
            self.run_pending = true;
            self.run_start = None;
        }
        if st == BusyState::Idle {
            self.done_pending = false;
        }
        self.state = st;
    }

    #[must_use]
    pub fn state(&self) -> BusyState {
        self.state
    }

    /// 완료 시그니처 원복(상자 재포커스 · 글 변경 · 초기화) → 그리지 않음(다음 Running에서 다시 살아난다).
    pub fn dismiss_done(&mut self) {
        self.done_seen = true;
        if self.state == BusyState::Done {
            self.state = BusyState::Idle;
            self.blink_start = None;
            self.blink_pending = false;
        }
    }

    fn blinking(&self) -> bool {
        self.blink_pending
            || self
                .blink_start
                .is_some_and(|t0| self.now.saturating_sub(t0) < BLINK_MS * BLINK_PHASES)
    }

    /// 움직이는 그림이 있는가 — 꺼져 있으면 늘 `false`(프레임 요구 0).
    #[must_use]
    pub fn animating(&self) -> bool {
        BUSY_ENABLED.load(Ordering::Relaxed)
            && (self.state == BusyState::Running || self.blinking())
    }

    /// 그리기 — `fb` = 상자 테두리 사각형 · `r` = 모서리 반지름(px) · `scale` = 배율(두께). 꺼져 있으면 아무것도 안 그린다.
    pub fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme, fb: Rect, r: i32, scale: f32) {
        if !BUSY_ENABLED.load(Ordering::Relaxed) {
            return;
        }
        let st = busy_style();
        let accent = st.color.unwrap_or(theme.accent);
        match self.state {
            BusyState::Idle => {}
            BusyState::Running => {
                paint_comet(
                    ctx,
                    theme,
                    accent,
                    fb,
                    r,
                    self.now,
                    st.width * scale,
                    st.lap_ms,
                );
            }
            BusyState::Done => {
                if self.blinking() {
                    let since = self.blink_start.map_or(0, |t0| self.now.saturating_sub(t0));
                    paint_done_flash(ctx, accent, fb, r, since);
                } else {
                    ctx.stroke_round_rect(fb, r, theme.ok, 1.0);
                }
            }
        }
    }
}

/// 혜성: 테두리 전체를 강조색 절반으로 물들이고 꼬리(배경에 가깝게) → 머리(강조색) 8단계 그라데이션이 한 바퀴 돈다.
#[allow(clippy::too_many_arguments)]
fn paint_comet(
    ctx: &mut dyn DrawCtx,
    theme: &Theme,
    accent: Color,
    fb: Rect,
    r: i32,
    now_ms: u64,
    head_w: f32,
    lap_ms: u32,
) {
    let path = round_rect_path(fb, r);
    let total = path_len(&path);
    if total <= 0.0 {
        return;
    }
    ctx.stroke_round_rect(fb, r, accent.lerp(theme.border, 0.5), 1.0);
    let lap = u64::from(lap_ms.max(100));
    let t = (now_ms % lap) as f32 / lap as f32;
    let seg = total * SEG_FRACTION;
    let start = t * total;
    let step = seg / COMET_STEPS as f32;
    let head = head_w.max(1.0);
    for i in 0..COMET_STEPS {
        let k = (i + 1) as f32 / COMET_STEPS as f32; // 0 = 꼬리 끝 · 1 = 머리
        let color = accent.lerp(theme.field_bg, 0.85 * (1.0 - k));
        let width = head * 0.5 + head * 0.5 * k;
        for pts in path_window(&path, total, start + step * i as f32, step + 0.5) {
            ctx.polyline(&pts, color, width);
        }
    }
}

/// 완료 플래시: 안쪽 채움 0.45 → 0(`FLASH_MS`) + 테두리 깜빡임(`BLINK_MS` 위상) · `since` = 완료 뒤 ms.
fn paint_done_flash(ctx: &mut dyn DrawCtx, accent: Color, fb: Rect, r: i32, since: u64) {
    if since < FLASH_MS {
        let k = 1.0 - since as f32 / FLASH_MS as f32;
        let inner = Rect::new(fb.x + 1, fb.y + 1, (fb.w - 2).max(0), (fb.h - 2).max(0));
        ctx.fill_round_rect_alpha(inner, r, accent, 0.45 * k);
    }
    if (since / BLINK_MS) % 2 == 0 {
        ctx.stroke_round_rect(fb, r, accent, 2.0);
    }
}

/// 둥근 사각형 둘레의 폴리라인(시작 = 위쪽 변 왼쪽 끝 · 시계 방향 · 모서리는 호를 6분할).
#[must_use]
pub fn round_rect_path(fb: Rect, r: i32) -> Vec<(f32, f32)> {
    let r = r.max(0).min(fb.w / 2).min(fb.h / 2) as f32;
    let (x0, y0, x1, y1) = (
        fb.x as f32,
        fb.y as f32,
        fb.right() as f32,
        fb.bottom() as f32,
    );
    let mut pts = Vec::with_capacity(32);
    let arc = |pts: &mut Vec<(f32, f32)>, cx: f32, cy: f32, a0: f32, a1: f32| {
        let n = 6;
        for i in 0..=n {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            pts.push((cx + r * a.cos(), cy + r * a.sin()));
        }
    };
    use core::f32::consts::PI;
    pts.push((x0 + r, y0));
    pts.push((x1 - r, y0));
    arc(&mut pts, x1 - r, y0 + r, -PI / 2.0, 0.0);
    pts.push((x1, y1 - r));
    arc(&mut pts, x1 - r, y1 - r, 0.0, PI / 2.0);
    pts.push((x0 + r, y1));
    arc(&mut pts, x0 + r, y1 - r, PI / 2.0, PI);
    pts.push((x0, y0 + r));
    arc(&mut pts, x0 + r, y0 + r, PI, 1.5 * PI);
    pts
}

#[must_use]
pub fn path_len(path: &[(f32, f32)]) -> f32 {
    path.windows(2)
        .map(|w| ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt())
        .sum()
}

/// 둘레 위 `[start, start+len)` 구간의 점들(끝점 보간 · 한 바퀴를 넘으면 둘로 나눠 돌려준다).
#[must_use]
pub fn path_window(path: &[(f32, f32)], total: f32, start: f32, len: f32) -> Vec<Vec<(i32, i32)>> {
    if total <= 0.0 || len <= 0.0 || path.len() < 2 {
        return Vec::new();
    }
    let s = start.rem_euclid(total);
    let e = s + len.min(total);
    let mut out = Vec::new();
    if e <= total {
        out.push(path_slice(path, s, e));
    } else {
        out.push(path_slice(path, s, total));
        out.push(path_slice(path, 0.0, e - total));
    }
    out.retain(|v| v.len() >= 2);
    out
}

fn path_slice(path: &[(f32, f32)], a: f32, b: f32) -> Vec<(i32, i32)> {
    let mut out: Vec<(i32, i32)> = Vec::new();
    let mut acc = 0.0f32;
    let push = |p: (f32, f32), out: &mut Vec<(i32, i32)>| {
        let q = (p.0.round() as i32, p.1.round() as i32);
        if out.last() != Some(&q) {
            out.push(q);
        }
    };
    for w in path.windows(2) {
        let d = ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
        let (sa, sb) = (acc, acc + d);
        if sb >= a && sa <= b && d > 0.0 {
            let ta = ((a - sa) / d).clamp(0.0, 1.0);
            let tb = ((b - sa) / d).clamp(0.0, 1.0);
            let lerp = |t: f32| {
                (
                    w[0].0 + (w[1].0 - w[0].0) * t,
                    w[0].1 + (w[1].1 - w[0].1) * t,
                )
            };
            push(lerp(ta), &mut out);
            push(lerp(tb), &mut out);
        }
        acc = sb;
        if acc > b {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 둘레 폴리라인 = 네 변 + 네 호(≈ 2πr) · 창은 한 바퀴를 넘으면 둘로 · 길이 0 = 없음.
    #[test]
    fn round_rect_path_len_and_window_wrap() {
        let fb = Rect::new(10, 20, 200, 30);
        let r = 6;
        let path = round_rect_path(fb, r);
        let total = path_len(&path);
        let expect = 2.0 * (200.0 + 30.0) - 8.0 * r as f32 + 2.0 * core::f32::consts::PI * r as f32;
        assert!((total - expect).abs() < 1.5, "둘레 {total} ≈ {expect}");
        assert_eq!(path_window(&path, total, 10.0, 40.0).len(), 1);
        assert_eq!(
            path_window(&path, total, total - 5.0, 20.0).len(),
            2,
            "한 바퀴를 넘으면 둘로"
        );
        assert!(path_window(&path, total, 0.0, 0.0).is_empty());
        assert_eq!(path_window(&path, total, -3.0, 10.0).len(), 2);
    }

    /// Running → Done = 두 번 깜빡임 뒤 완료 테두리 · Idle = 취소 · 본 시그니처는 다시 안 켜짐 · 최소 표시 · 꺼지면 프레임 0.
    #[test]
    fn state_machine_blink_hold_and_disabled() {
        set_busy_style(BusyStyle {
            hold_ms: 0,
            ..BusyStyle::default()
        });
        let mut a = BusyRing::default();
        assert!(!a.animating());
        a.set_state(BusyState::Running);
        assert!(a.animating());
        assert!(a.tick(1000));
        a.set_state(BusyState::Done);
        assert!(a.tick(1010), "첫 tick에 깜빡임 시작");
        assert!(a.tick(1010 + BLINK_MS * BLINK_PHASES - 1));
        assert!(
            !a.tick(1010 + BLINK_MS * BLINK_PHASES + 1),
            "끝나면 멈춘다(완료 테두리는 정적)"
        );
        assert_eq!(a.state(), BusyState::Done);
        a.dismiss_done();
        assert_eq!(a.state(), BusyState::Idle, "만지면 원복");
        a.set_state(BusyState::Done);
        assert_eq!(a.state(), BusyState::Idle, "본 시그니처는 다시 안 켜진다");
        a.set_state(BusyState::Running);
        a.set_state(BusyState::Done);
        assert_eq!(a.state(), BusyState::Done, "새 진행이 끝나면 다시");
        // 최소 표시 600: 금방 끝나도 때까지 Running 유지.
        set_busy_style(BusyStyle {
            hold_ms: 600,
            ..BusyStyle::default()
        });
        let mut h = BusyRing::default();
        h.set_state(BusyState::Running);
        assert!(h.tick(5000));
        h.set_state(BusyState::Done);
        assert_eq!(h.state(), BusyState::Running);
        h.tick(5300);
        assert_eq!(h.state(), BusyState::Running);
        h.tick(5600);
        assert_eq!(h.state(), BusyState::Done);
        // 완료 테두리 자동 해제.
        set_busy_style(BusyStyle {
            hold_ms: 0,
            done_ms: 1000,
            ..BusyStyle::default()
        });
        let mut d = BusyRing::default();
        d.set_state(BusyState::Running);
        d.tick(10);
        d.set_state(BusyState::Done);
        d.tick(20);
        d.tick(1100);
        assert_eq!(d.state(), BusyState::Idle, "유지 시간 뒤 사라진다");
        // 꺼짐 = 상태는 기억하되 프레임 요구 0.
        set_busy_style(BusyStyle {
            enabled: false,
            ..BusyStyle::default()
        });
        let mut off = BusyRing::default();
        off.set_state(BusyState::Running);
        assert!(!off.animating());
        assert_eq!(off.state(), BusyState::Running);
        set_busy_style(BusyStyle::default());
    }
}

//! 토스트 `Toasts` — 우측 하단 쌓임(최대 5) · 설정 수명 뒤 사라짐 · 반투명 카드 · 남은 시간 막대 · 클릭 닫기 · 클릭 동작(`push_action` → `take_action`).
//!
//! ★ 범용 승격(114차 · 10-03 · 원장 UIK-107/213 · nexa-dir3 T-32): nexa-sql `toast.rs`(09-16 · 09-22 · 09-30)에서 SQL 오류 분류부를 뺀 카드 관리자를
//! nexa-dir3가 `toast.rs`로 복사해 쓰던 것을 nexa-ctl로 올렸다 — 앱 도메인 의존 0(설정 값은 호스트가 `configure*`로 주입 · 문자열은 호스트 i18n).
//! nexa-sql은 자기 `toast.rs`(실행 상태 카드 `runtoast` 포함)를 그대로 쓴다(이관은 nexa-sql 쪽 결정).
//!
//! 카드는 `alpha`(기본 85%)로 그려 아래 내용이 비친다 · 수명 `ttl`(기본 3000 ms) · 마지막 300ms는 페이드 아웃 · 클릭하면 바로 사라진다 ·
//! 최대 5장(오래된 것부터 밀려남). 그리기는 창의 맨 마지막 층(팝업 규칙).
//!
//! ★ 남은 시간 표시(nexa-sql 사용자 09-22 "진척에 따라 더 투명해지며 사라지는 느낌" → "왼쪽 세로선을 그대로 활용 · 위에서 아래로" → "지나간 부분은
//! 70% 투명한 같은 색"): **왼쪽 색 막대**의 남은 시간만큼이 진하고([`bar_remaining`] · 아래 고정) 지나간 위쪽은 같은 색을 `spent`(기본 30%)로 ·
//! 카드 불투명도는 [`life_alpha`] 곡선(제곱 이징 — 앞부분은 또렷하고 끝으로 갈수록 빨리 `fade_to`까지) → 마지막 300ms에 0.
//! 새 타이머 없음 — 카드가 있는 동안 도는 30ms 틱이 그대로 그린다(`tick`/`animating`).

use crate::draw::DrawCtx;
use crate::geom::{Point, Rect};
use crate::theme::Theme;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    /// 오류(테마 `danger`).
    Error,
    /// 경고(테마 `warn`).
    Warn,
    /// 안내(테마 `accent`).
    Info,
}

#[derive(Debug)]
struct Toast {
    kind: ToastKind,
    title: String,
    body: String,
    born: Instant,
    rect: Rect,
    /// 클릭하면 실행할 명령 id(예 북마크 제거 되돌리기 · docs/69 C-28).
    action: Option<String>,
}

const MAX_TOASTS: usize = 5;
const FADE_MS: u64 = 300;

/// 토스트가 붙는 모서리(214차 · nexa-dir3 10-18 "전역 = 창 기준 · 패널 작업 = 그 패널 범위" — 호스트 설정 `ui.toast_position`). 아래 모서리는
/// 위로, 위 모서리는 아래로 쌓인다(최신 카드가 모서리에 가장 가깝다).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToastCorner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

impl ToastCorner {
    /// 설정 문자열(`top-left` · `top-right` · `bottom-left` · `bottom-right`) → 모서리(모르는 값 = 우하단).
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "top-left" => ToastCorner::TopLeft,
            "top-right" => ToastCorner::TopRight,
            "bottom-left" => ToastCorner::BottomLeft,
            _ => ToastCorner::BottomRight,
        }
    }
}

/// 수명 진척 `p`(0 = 방금 · 1 = 만료)에 따른 불투명도 배율 — 1.0에서 `fade_to`(0..1)까지 **제곱 이징**(초반은 거의 그대로, 뒤로 갈수록
/// 빨리 투명해진다 = 읽을 시간은 지키고 "사라지는 느낌"은 뒤에 몰아 준다). `progress`가 꺼져 있으면 늘 1.0(종전 = 마지막 300ms만).
/// 마지막 300ms의 0으로 가는 페이드는 이 값에 곱한다(`paint`).
pub fn life_alpha(p: f32, fade_to: f32, progress: bool) -> f32 {
    if !progress {
        return 1.0;
    }
    let p = p.clamp(0.0, 1.0);
    let fade_to = fade_to.clamp(0.0, 1.0);
    1.0 - (1.0 - fade_to) * p * p
}

/// 왼쪽 막대의 남은(진한) 높이 — 진척 `life`(0..1)이 있으면 `full × (1 − life)`(반올림 · 끝에서는 0) · 없으면(진행 중 · 수동 닫기 · 꺼짐) 가득.
/// 호출자는 이 높이를 **아래에 붙여** 그리고 나머지는 옅은 같은 색으로(위에서부터 옅어지는 느낌).
pub fn bar_remaining(full: i32, life: Option<f32>) -> i32 {
    match life {
        Some(p) => ((1.0 - p.clamp(0.0, 1.0)) * full as f32).round() as i32,
        None => full,
    }
    .clamp(0, full)
}

#[derive(Debug)]
pub struct Toasts {
    items: Vec<Toast>,
    ttl: Duration,
    /// 카드 불투명도(0.3~1.0).
    alpha: f32,
    /// 남은 시간 막대 + 진척 페이드(`ui.toast_progress`).
    progress: bool,
    /// 수명 끝의 불투명도 비율(`ui.toast_fade_to` % → 0..1).
    fade_to: f32,
    /// 지나간 부분의 막대 불투명도 비율(`ui.toast_bar_spent` % → 0..1).
    spent: f32,
    /// 클릭된 토스트의 동작 id(1회성 · `take_action`).
    taken: Option<String>,
}

impl Default for Toasts {
    fn default() -> Self {
        Self::new()
    }
}

impl Toasts {
    /// 기본값: 3초 · 85% · 진척 막대 켬 · 끝 35% · 지나간 막대 30%.
    #[must_use]
    pub fn new() -> Self {
        Toasts {
            items: Vec::new(),
            ttl: Duration::from_secs(3),
            alpha: 0.85,
            progress: true,
            fade_to: 0.35,
            spent: 0.3,
            taken: None,
        }
    }

    /// 수명(ms · 100~60000) · 카드 불투명도(% · 30~100) — 호스트 설정(nexa-dir3 `ui.toast_ms`/`ui.toast_alpha`).
    pub fn configure(&mut self, ms: i64, alpha_pct: i64) {
        self.ttl = Duration::from_millis(ms.clamp(100, 60_000) as u64);
        self.alpha = (alpha_pct.clamp(30, 100) as f32) / 100.0;
    }

    /// 남은 시간 막대 + 진척 페이드 켜기 · 수명 끝 불투명도(%) · 지나간 막대 불투명도(%) — 호스트 설정(`ui.toast_progress`/`ui.toast_fade_to`/`ui.toast_bar_spent`).
    pub fn configure_progress(&mut self, on: bool, fade_to_pct: i64, spent_pct: i64) {
        self.progress = on;
        self.fade_to = (fade_to_pct.clamp(0, 100) as f32) / 100.0;
        self.spent = (spent_pct.clamp(0, 100) as f32) / 100.0;
    }

    /// 카드 추가(최대 5장 · 오래된 것부터 밀려남).
    pub fn push(&mut self, kind: ToastKind, title: impl Into<String>, body: impl Into<String>) {
        self.items.push(Toast {
            kind,
            title: title.into(),
            body: body.into(),
            born: Instant::now(),
            rect: Rect::default(),
            action: None,
        });
        while self.items.len() > MAX_TOASTS {
            self.items.remove(0);
        }
    }

    /// 클릭하면 `action`(명령 id)을 실행하는 토스트 — 호스트가 `take_action`으로 거둔다.
    pub fn push_action(
        &mut self,
        kind: ToastKind,
        title: impl Into<String>,
        body: impl Into<String>,
        action: &str,
    ) {
        self.push(kind, title, body);
        if let Some(t) = self.items.last_mut() {
            t.action = Some(action.to_string());
        }
    }

    /// 남아 있는 카드 수(214차 · 호스트 시험 · 영역별 토스트 배분 확인).
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 마지막 그리기에서 카드가 차지한 사각형들(214차 · 호스트가 겹침·영역 안 판정에 쓴다 · 그리기 전 = 빈 사각형).
    pub fn rects(&self) -> impl Iterator<Item = Rect> + '_ {
        self.items.iter().map(|t| t.rect)
    }

    /// 클릭된 카드의 동작 id(1회성).
    pub fn take_action(&mut self) -> Option<String> {
        self.taken.take()
    }

    /// 만료 제거 — 남은 카드가 있으면(페이드 애니메이션) true = 30ms 틱 유지.
    pub fn tick(&mut self, now: Instant) -> bool {
        let ttl = self.ttl;
        self.items.retain(|t| now.duration_since(t.born) < ttl);
        !self.items.is_empty()
    }

    /// 카드가 남아 있으면 true(호스트는 30ms 틱 유지).
    #[must_use]
    pub fn animating(&self) -> bool {
        !self.items.is_empty()
    }

    /// 클릭으로 닫기 — 카드 위였으면 true(호출자는 그 클릭을 아래로 흘리지 않는다).
    pub fn click(&mut self, p: Point) -> bool {
        if let Some(i) = self.items.iter().position(|t| t.rect.contains(p)) {
            let t = self.items.remove(i);
            if t.action.is_some() {
                self.taken = t.action;
            }
            return true;
        }
        false
    }

    /// 우측 하단부터 위로 쌓아 그린다(`bottom` = 상태줄 위 y · `right` = 창 오른쪽) = [`Self::paint_in`](창 전체 · 우하단).
    pub fn paint(&mut self, dc: &mut dyn DrawCtx, th: &Theme, right: i32, bottom: i32, s: f32) {
        self.paint_in(
            dc,
            th,
            Rect::new(0, 0, right, bottom),
            ToastCorner::BottomRight,
            s,
        );
    }

    /// `area`의 `corner`에 붙여 쌓아 그린다(214차): 아래 모서리 = 위로 · 위 모서리 = 아래로 · 카드 폭은 영역 폭을 넘지 않는다(좁은 패널).
    pub fn paint_in(
        &mut self,
        dc: &mut dyn DrawCtx,
        th: &Theme,
        area: Rect,
        corner: ToastCorner,
        s: f32,
    ) {
        if self.items.is_empty() {
            return;
        }
        let px = |v: f32| (v * s).round() as i32;
        let (pad, gap, bar) = (px(10.0), px(6.0), px(4.0));
        let w = px(340.0).min((area.w - gap * 2).max(px(120.0)));
        let (right, bottom) = (area.right(), area.bottom());
        let (top_side, left_side) = match corner {
            ToastCorner::TopLeft => (true, true),
            ToastCorner::TopRight => (true, false),
            ToastCorner::BottomLeft => (false, true),
            ToastCorner::BottomRight => (false, false),
        };
        let x = if left_side {
            area.x + gap
        } else {
            right - gap - w
        };

        let lh = dc.text_height();
        let now = Instant::now();
        let ttl_ms = self.ttl.as_millis().max(1) as f32;
        let mut y = if top_side { area.y + gap } else { bottom - gap };
        for t in self.items.iter_mut().rev() {
            let age = now.duration_since(t.born);
            let left = self.ttl.saturating_sub(age).as_millis() as u64;
            let fade = if left < FADE_MS {
                left as f32 / FADE_MS as f32
            } else {
                1.0
            };
            let life = (age.as_millis() as f32 / ttl_ms).clamp(0.0, 1.0);
            let a = self.alpha * life_alpha(life, self.fade_to, self.progress) * fade;
            let h = pad * 2 + lh * 2 + px(2.0);
            let r = if top_side {
                Rect::new(x, y, w, h)
            } else {
                Rect::new(x, y - h, w, h)
            };
            t.rect = r;
            dc.fill_round_rect_alpha(r, px(6.0), th.panel_bg, a);
            dc.stroke_round_rect_alpha(r, px(6.0), th.border, 1.0, a);
            let color = match t.kind {
                ToastKind::Error => th.danger,
                ToastKind::Warn => th.warn,
                ToastKind::Info => th.accent,
            };
            // 왼쪽 색 막대 = 남은 시간(아래 고정 · 위에서부터 옅어짐 · 지나간 부분 = 같은 색 `ui.toast_bar_spent`) · 꺼져 있으면 가득.
            let full = h - pad * 2;
            let bar_h = bar_remaining(full, self.progress.then_some(life));
            let bar_x = r.x + px(6.0);
            if bar_h < full {
                dc.fill_round_rect_alpha(
                    Rect::new(bar_x, r.y + pad, bar, full),
                    px(2.0),
                    color,
                    a * self.spent,
                );
            }
            if bar_h > 0 {
                dc.fill_round_rect_alpha(
                    Rect::new(bar_x, r.y + pad + (full - bar_h), bar, bar_h),
                    px(2.0),
                    color,
                    a,
                );
            }
            let tx = r.x + px(6.0) + bar + pad;
            let clip = Rect::new(r.x, r.y, r.w - pad, r.h);
            dc.text(tx, r.y + pad, clip, &t.title, color);
            dc.text(tx, r.y + pad + lh + px(2.0), clip, &t.body, th.text);
            y = if top_side {
                r.bottom() + gap
            } else {
                r.y - gap
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 모서리별 배치(214차): 우하단 = 영역 오른쪽·아래에 붙어 위로 쌓임 · 좌상단 = 왼쪽·위에 붙어 아래로 쌓임 · 카드는 영역 안 · 좁은 영역 = 폭 축소 ·
    /// `len`/`rects`/`parse`.
    #[test]
    fn paint_in_places_cards_at_each_corner() {
        use crate::controls::RecordCtx;
        let th = Theme::default();
        let mut t = Toasts::new();
        t.push(ToastKind::Info, "a", "1");
        t.push(ToastKind::Warn, "b", "2");
        assert_eq!(t.len(), 2);
        let area = Rect::new(100, 50, 600, 400);
        let mut dc = RecordCtx::with_surface(800, 600);
        t.paint_in(&mut dc, &th, area, ToastCorner::BottomRight, 1.0);
        let r: Vec<Rect> = t.rects().collect();
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].right(), area.right() - 6, "최신 카드 = 모서리");
        assert_eq!(r[1].bottom(), area.bottom() - 6);
        assert!(r[0].bottom() < r[1].y, "위로 쌓임");
        t.paint_in(&mut dc, &th, area, ToastCorner::TopLeft, 1.0);
        let r: Vec<Rect> = t.rects().collect();
        assert_eq!((r[1].x, r[1].y), (area.x + 6, area.y + 6));
        assert!(r[0].y > r[1].bottom(), "아래로 쌓임");
        let narrow = Rect::new(0, 0, 200, 300);
        t.paint_in(&mut dc, &th, narrow, ToastCorner::BottomLeft, 1.0);
        assert!(
            t.rects()
                .all(|r| r.x >= narrow.x && r.right() <= narrow.right()),
            "좁은 영역 = 폭 축소"
        );
        assert_eq!(ToastCorner::parse("top-right"), ToastCorner::TopRight);
        assert_eq!(ToastCorner::parse("??"), ToastCorner::BottomRight);
    }

    /// 수명 곡선: 시작 1.0 · 끝 `fade_to` · 단조 감소 · 초반은 완만(제곱 이징 — 절반 지점에서 4분의 1만 내려옴) · 꺼져 있으면 늘 1.0.
    #[test]
    fn life_alpha_curve() {
        assert_eq!(life_alpha(0.0, 0.35, true), 1.0);
        assert!((life_alpha(1.0, 0.35, true) - 0.35).abs() < 1e-6);
        let half = life_alpha(0.5, 0.35, true);
        assert!((half - (1.0 - 0.65 * 0.25)).abs() < 1e-6, "{half}");
        let mut prev = 1.0;
        for i in 1..=20 {
            let v = life_alpha(i as f32 / 20.0, 0.35, true);
            assert!(v <= prev, "{i}: {v} > {prev}");
            prev = v;
        }
        assert_eq!(life_alpha(2.0, 0.35, true), life_alpha(1.0, 0.35, true));
        assert_eq!(life_alpha(0.9, 0.35, false), 1.0);
        assert_eq!(life_alpha(1.0, 0.0, true), 0.0);
    }

    /// 막대의 진한 부분 높이: 진척 없음 = 가득 · 0 = 가득 · 0.5 = 절반 · 1 = 0 · 범위 밖은 잘라 준다.
    #[test]
    fn bar_remaining_shrinks_with_progress() {
        assert_eq!(bar_remaining(40, None), 40);
        assert_eq!(bar_remaining(40, Some(0.0)), 40);
        assert_eq!(bar_remaining(40, Some(0.5)), 20);
        assert_eq!(bar_remaining(40, Some(1.0)), 0);
        assert_eq!(bar_remaining(40, Some(3.0)), 0);
        assert_eq!(bar_remaining(40, Some(-1.0)), 40);
    }

    #[test]
    fn toasts_expire_and_click_dismisses() {
        let mut ts = Toasts::new();
        ts.configure(1000, 85);
        ts.push(ToastKind::Error, "a", "b");
        assert!(ts.animating());
        assert!(ts.tick(Instant::now()));
        assert!(!ts.tick(Instant::now() + Duration::from_secs(2)));
        ts.push(ToastKind::Error, "a", "b");
        ts.items[0].rect = Rect::new(0, 0, 10, 10);
        assert!(ts.click(Point { x: 5, y: 5 }));
        assert!(!ts.animating());
    }
}

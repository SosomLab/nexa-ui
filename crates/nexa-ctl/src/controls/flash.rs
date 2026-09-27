//! ★ **순간 메시지(Flash)** 부품(nexa-sql 09-27 · 사용자 "인스턴트 플로팅 메시지를 별도 컨트롤로"): 클릭 복사 "복사됨" 같은 짧은 알림을
//! **앵커(가리면 안 되는 사각형) 옆**에 띄우고 **`hold_ms` 동안 그대로 보인 뒤 `fade_ms` 동안** 색이 배경으로 녹아 사라진다(유지 → 페이드아웃 · 사용자 09-27). 타이머·큐 없음 — 호스트는
//! [`Flash::paint`]가 `true`를 돌려주는 동안 다시 그리기만 하면 된다(진행 중 = 프레임마다 · 끝나면 스스로 지운다).
//!
//! 자리 규칙(사용자 09-27): 메시지의 **좌하단 = 앵커의 우상단**(앵커 위·오른쪽에 떠서 앵커를 덮지 않음) → 오른쪽이 모자라면 왼쪽으로 밀고 →
//! 위가 모자라면 앵커 **아래**(좌상단 = 앵커 우하단) → 마지막으로 호스트 안으로. 배경 상자(`with_background` · 기본 켬)로 글이 돋보이게 하고
//! 호스트는 **창의 맨 마지막**에 그려 다른 컨트롤이 덮지 않게 한다(최상위 Z-order · 팝업 배치 규칙 = nexa-sql docs/61 §2-2).

use std::time::Instant;

use crate::draw::DrawCtx;
use crate::geom::Rect;
use crate::theme::Theme;
use nexa_gfx::Color;

/// 메시지 색조.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlashTone {
    #[default]
    Ok,
    Warn,
    Info,
}

/// 순간 메시지 상태(컨트롤 하나 = 메시지 한 줄 · 새로 `show`하면 앞 것을 대체).
#[derive(Debug, Default)]
pub struct Flash {
    text: String,
    tone: FlashTone,
    at: Option<Instant>,
    /// 유지(그대로 보이는) 시간 · 페이드아웃 시간(ms).
    hold_ms: u64,
    fade_ms: u64,
    /// 배경 상자(색조 배경 + 테두리 · 글이 돋보이게 · 기본 켬).
    background: bool,
}

impl Flash {
    #[must_use]
    pub fn new() -> Self {
        Self {
            background: true,
            ..Self::default()
        }
    }

    /// 배경 상자 켜기/끄기(빌더).
    #[must_use]
    pub fn with_background(mut self, on: bool) -> Self {
        self.background = on;
        self
    }

    pub fn set_background(&mut self, on: bool) {
        self.background = on;
    }

    /// 보이기 — `ms` 동안(최소 200) 서서히 사라진다.
    /// `hold_ms` 동안 그대로 보인 뒤 `fade_ms`(최소 200) 동안 서서히 사라진다.
    pub fn show(&mut self, text: impl Into<String>, tone: FlashTone, hold_ms: u64, fade_ms: u64) {
        self.text = text.into();
        self.tone = tone;
        self.hold_ms = hold_ms;
        self.fade_ms = fade_ms.max(200);
        self.at = Some(Instant::now());
    }

    pub fn clear(&mut self) {
        self.at = None;
    }

    #[must_use]
    pub fn active(&self) -> bool {
        self.at.is_some()
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 남은 세기 0..=1(1 = 방금 · 제곱 감속) · 끝났으면 `None`(스스로 지운다).
    pub fn strength_at(&mut self, now: Instant) -> Option<f32> {
        let at = self.at?;
        let el = now.saturating_duration_since(at).as_millis() as u64;
        if el < self.hold_ms {
            return Some(1.0);
        }
        let f = el - self.hold_ms;
        if f >= self.fade_ms {
            self.at = None;
            return None;
        }
        let t = f as f32 / self.fade_ms as f32;
        Some(1.0 - t * t)
    }

    /// 자리(순수): 메시지 좌하단 = 앵커 우상단 → 오른쪽이 모자라면 왼쪽으로 → 위가 모자라면 앵커 아래(좌상단 = 앵커 우하단) → 호스트 안으로.
    /// `w`·`h`는 배경 상자까지 포함한 크기 · `gap`은 앵커와의 틈.
    #[must_use]
    pub fn place(anchor: Rect, w: i32, h: i32, host: Rect, gap: i32) -> Rect {
        let x = (anchor.right() + gap).min(host.right() - w).max(host.x);
        let above = Rect::new(x, anchor.y - gap - h, w, h);
        if above.y >= host.y {
            return above;
        }
        let below = Rect::new(x, anchor.bottom() + gap, w, h);
        if below.bottom() <= host.bottom() {
            return below;
        }
        // 위·아래 다 모자라면 앵커 오른쪽 같은 줄(호스트 안으로 클램프).
        Rect::new(
            x,
            anchor.y.clamp(host.y, (host.bottom() - h).max(host.y)),
            w,
            h,
        )
    }

    /// 그리기 — `anchor` = 가리지 말아야 할 사각형(링크 글자 등) · `host` = 창 영역. 돌려주는 값 = 아직 진행 중(호스트가 다시 그린다).
    pub fn paint(&mut self, dc: &mut dyn DrawCtx, th: &Theme, anchor: Rect, host: Rect) -> bool {
        let Some(a) = self.strength_at(Instant::now()) else {
            return false;
        };
        let base = match self.tone {
            FlashTone::Ok => th.ok,
            FlashTone::Warn => th.warn,
            FlashTone::Info => th.text,
        };
        let color: Color = th.panel_bg.lerp(base, a);
        let th_txt = dc.text_height();
        let gap = (th_txt / 3).max(3);
        let (px, py) = if self.background {
            ((th_txt / 2).max(6), (th_txt / 4).max(3))
        } else {
            (0, 0)
        };
        let max_w = (host.w - gap * 2 - px * 2).max(1);
        let text = crate::draw::ellipsize_middle(dc, &self.text, max_w);
        let tw = dc.text_width(&text);
        let r = Self::place(anchor, tw + px * 2, th_txt + py * 2, host, gap);
        if self.background {
            // 글이 돋보이는 배경: 색조를 살짝 띤 어두운/밝은 상자 + 같은 색조 테두리(모두 세기 `a`로 함께 사라짐).
            let bg = th.panel_bg.lerp(th.panel_bg_alt, a).lerp(base, 0.18 * a);
            let border = th.panel_bg.lerp(base, 0.6 * a);
            dc.fill_rect(r, bg);
            dc.fill_rect(Rect::new(r.x, r.y, r.w, 1), border);
            dc.fill_rect(Rect::new(r.x, r.bottom() - 1, r.w, 1), border);
            dc.fill_rect(Rect::new(r.x, r.y, 1, r.h), border);
            dc.fill_rect(Rect::new(r.right() - 1, r.y, 1, r.h), border);
        }
        dc.text(r.x + px, r.y + py, r, &text, color);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn place_bottom_left_at_anchor_top_right_then_shift_then_below() {
        let host = Rect::new(0, 0, 400, 300);
        let anchor = Rect::new(20, 100, 100, 16);
        let r = Flash::place(anchor, 120, 20, host, 4);
        assert_eq!((r.x, r.bottom()), (124, 96), "좌하단 = 앵커 우상단(틈 4)");
        assert!(!r.intersects(&anchor));
        // 오른쪽이 모자라면 왼쪽으로 민다(호스트 안).
        let r = Flash::place(anchor, 300, 20, host, 4);
        assert_eq!(r.right(), host.right());
        assert!(r.bottom() <= anchor.y && !r.intersects(&anchor));
        // 위가 모자라면 앵커 아래.
        let top = Rect::new(20, 10, 100, 16);
        let r = Flash::place(top, 120, 20, host, 4);
        assert_eq!((r.x, r.y), (124, 30));
        assert!(!r.intersects(&top));
        // 위·아래 다 모자라면 오른쪽 같은 줄 · 호스트 안.
        let small = Rect::new(0, 0, 400, 30);
        let r = Flash::place(Rect::new(20, 5, 100, 20), 120, 20, small, 4);
        assert!(r.y >= small.y && r.bottom() <= small.bottom() && r.x == 124);
    }

    #[test]
    fn strength_fades_and_ends() {
        let mut f = Flash::new();
        assert!(!f.active());
        f.show("copied", FlashTone::Ok, 500, 1000);
        let t0 = f.at.expect("shown");
        assert!((f.strength_at(t0).expect("s") - 1.0).abs() < 1e-6);
        assert!(
            (f.strength_at(t0 + Duration::from_millis(499))
                .expect("hold")
                - 1.0)
                .abs()
                < 1e-6,
            "유지 중 = 1"
        );
        let mid = f
            .strength_at(t0 + Duration::from_millis(1000))
            .expect("mid");
        assert!(
            (mid - 0.75).abs() < 1e-6,
            "유지 뒤 페이드 절반 = 제곱 감속 · {mid}"
        );
        assert!(f.strength_at(t0 + Duration::from_millis(1500)).is_none());
        assert!(!f.active(), "끝나면 스스로 지운다");
        f.show("x", FlashTone::Warn, 0, 10);
        assert_eq!(f.fade_ms, 200, "페이드 최소 200ms");
        assert!(Flash::new().background, "배경 기본 켬");
        assert!(!Flash::new().with_background(false).background);
    }
}

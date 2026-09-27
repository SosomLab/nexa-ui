//! ★ **순간 메시지(Flash)** 부품(nexa-sql 09-27 · 사용자 "인스턴트 플로팅 메시지를 별도 컨트롤로"): 클릭 복사 "복사됨" 같은 짧은 알림을
//! **앵커(가리면 안 되는 사각형) 옆**에 띄우고 `ms` 동안 색이 배경으로 녹아 사라진다. 타이머·큐 없음 — 호스트는
//! [`Flash::paint`]가 `true`를 돌려주는 동안 다시 그리기만 하면 된다(진행 중 = 프레임마다 · 끝나면 스스로 지운다).
//!
//! 자리 규칙(순서): 앵커 **오른쪽** 같은 줄 → 자리 없으면 앵커 **아래** → 그것도 없으면 앵커 **위** → 마지막으로 호스트 안으로 밀어 넣는다.
//! 어느 경우에도 앵커를 덮지 않는다(팝업 배치 규칙 = nexa-sql docs/61 §2-2).

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
    ms: u64,
}

impl Flash {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 보이기 — `ms` 동안(최소 200) 서서히 사라진다.
    pub fn show(&mut self, text: impl Into<String>, tone: FlashTone, ms: u64) {
        self.text = text.into();
        self.tone = tone;
        self.ms = ms.max(200);
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
        if el >= self.ms {
            self.at = None;
            return None;
        }
        let t = el as f32 / self.ms as f32;
        Some(1.0 - t * t)
    }

    /// 자리(순수): 앵커 오른쪽 → 아래 → 위 → 호스트 안으로.
    #[must_use]
    pub fn place(anchor: Rect, w: i32, h: i32, host: Rect, gap: i32) -> Rect {
        let right = Rect::new(anchor.right() + gap, anchor.y, w, h);
        if right.right() <= host.right() && right.bottom() <= host.bottom() {
            return right;
        }
        let below = Rect::new(
            anchor.x.min(host.right() - w).max(host.x),
            anchor.bottom() + gap,
            w,
            h,
        );
        if below.bottom() <= host.bottom() {
            return below;
        }
        let above = Rect::new(below.x, anchor.y - gap - h, w, h);
        if above.y >= host.y {
            return above;
        }
        // 어디에도 온전히 안 들어가면 호스트 안으로 밀어 넣되 앵커와 겹치지 않는 쪽(오른쪽 끝)에.
        Rect::new(
            (host.right() - w).max(host.x),
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
        let h = dc.text_height();
        let gap = (h / 2).max(4);
        let max_w = (host.w - gap * 2).max(1);
        let text = crate::draw::ellipsize_middle(dc, &self.text, max_w);
        let w = dc.text_width(&text);
        let r = Self::place(anchor, w, h, host, gap);
        dc.text(r.x, r.y, r, &text, color);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn place_prefers_right_then_below_then_above() {
        let host = Rect::new(0, 0, 400, 300);
        let anchor = Rect::new(20, 100, 100, 16);
        let r = Flash::place(anchor, 120, 16, host, 8);
        assert_eq!((r.x, r.y), (128, 100), "오른쪽");
        assert!(!r.intersects(&anchor));
        let r = Flash::place(anchor, 300, 16, host, 8);
        assert_eq!((r.x, r.y), (20, 124), "오른쪽에 못 들어가면 아래");
        assert!(!r.intersects(&anchor));
        let low = Rect::new(20, 280, 100, 16);
        let r = Flash::place(low, 300, 16, host, 8);
        assert_eq!((r.x, r.y), (20, 256), "아래도 없으면 위");
        assert!(!r.intersects(&low));
        // 왼쪽 끝을 넘지 않게 x를 당긴다.
        let r = Flash::place(Rect::new(350, 100, 40, 16), 100, 16, host, 8);
        assert!(r.right() <= host.right() && r.x >= host.x);
    }

    #[test]
    fn strength_fades_and_ends() {
        let mut f = Flash::new();
        assert!(!f.active());
        f.show("copied", FlashTone::Ok, 1000);
        let t0 = f.at.expect("shown");
        assert!((f.strength_at(t0).expect("s") - 1.0).abs() < 1e-6);
        let mid = f.strength_at(t0 + Duration::from_millis(500)).expect("mid");
        assert!((mid - 0.75).abs() < 1e-6, "제곱 감속 · {mid}");
        assert!(f.strength_at(t0 + Duration::from_millis(1000)).is_none());
        assert!(!f.active(), "끝나면 스스로 지운다");
        f.show("x", FlashTone::Warn, 10);
        assert_eq!(f.ms, 200, "최소 200ms");
    }
}

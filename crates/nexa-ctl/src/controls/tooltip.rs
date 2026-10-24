//! ★ **툴팁 관리자**(229차 · dir3 UIK-208 · 10-25): "어디에 머물면 · 얼마 뒤에 · 어떤 글을" 한 상태기계로. 종전에는 컨트롤마다(툴바 · 로그 창
//! 스위치 · 탭 바) hover 시각 · 보임 깃발 · 지연 상수를 따로 들고 같은 규칙을 복제했다. 그리기는 [`crate::draw::draw_tooltip_in_dir`] 그대로
//! (역상 캡슐 · 가로 클램프 · 위/아래 접기) — 이 부품은 **언제 보일지**와 **무엇을 어디에**만 안다. 타이머 없음: 호스트가 [`Tooltip::tick`]을
//! 유휴 틱에서 부르고 `true`면 다시 그린다 · [`Tooltip::next_wake`]로 깨울 시각을 받는다(dir3 `WaitUntil` 규약).
//!
//! 규칙: 머문 자리(anchor)가 바뀌면 지연을 다시 센다 · 떠나면 바로 감춘다(사라진 자리는 호스트가 무효화 — `hover(None)`이 `true`) ·
//! 보이는 중에 같은 자리는 그대로 · 클릭 등으로 `dismiss`하면 떠날 때까지 다시 안 보인다.

use std::time::{Duration, Instant};

use crate::draw::DrawCtx;
use crate::geom::Rect;
use crate::theme::Theme;

/// 기본 지연(ms · 로그 창 스위치 600 · 탐색기 관례 500~700).
pub const DEFAULT_DELAY_MS: u64 = 600;

/// 툴팁 상태기계 — 컨트롤 하나(또는 창 하나)에 하나.
#[derive(Debug)]
pub struct Tooltip {
    delay: Duration,
    /// 머문 자리 · 글 · 머문 시각.
    hover: Option<(Rect, String, Instant)>,
    shown: bool,
    /// 떠날 때까지 다시 보이지 않음(`dismiss`).
    dismissed: bool,
    /// 세로 방향: `None` = 전역 기본([`crate::draw::tooltip_above`]) · `Some(true)` = 기준 위 · `Some(false)` = 아래.
    above: Option<bool>,
}

impl Default for Tooltip {
    fn default() -> Self {
        Self::new()
    }
}

impl Tooltip {
    #[must_use]
    pub fn new() -> Self {
        Tooltip {
            delay: Duration::from_millis(DEFAULT_DELAY_MS),
            hover: None,
            shown: false,
            dismissed: false,
            above: None,
        }
    }

    pub fn set_delay_ms(&mut self, ms: u64) {
        self.delay = Duration::from_millis(ms);
    }

    /// 세로 방향 고정(`None` = 전역 기본을 따른다).
    pub fn set_above(&mut self, above: Option<bool>) {
        self.above = above;
    }

    /// 머문 자리가 바뀌었다(`None` = 떠남). 돌려주는 값 = **보이던 툴팁이 사라졌다**(호스트가 그 자리를 무효화해야 한다).
    /// 같은 자리(anchor 같음)면 글만 갱신하고 지연은 다시 세지 않는다.
    pub fn hover(&mut self, target: Option<(Rect, String)>) -> bool {
        let was_shown = self.shown;
        match target {
            None => {
                self.hover = None;
                self.shown = false;
                self.dismissed = false;
            }
            Some((anchor, text)) => match &mut self.hover {
                Some((a, t, _)) if *a == anchor => {
                    *t = text;
                    return false;
                }
                _ => {
                    self.hover = Some((anchor, text, Instant::now()));
                    self.shown = false;
                    self.dismissed = false;
                }
            },
        }
        was_shown
    }

    /// 유휴 틱 — 지연이 지나 **처음 보이게 되면** `true`(한 번 · 호스트는 다시 그리기).
    pub fn tick(&mut self) -> bool {
        if self.shown || self.dismissed {
            return false;
        }
        match &self.hover {
            Some((_, t, since)) if !t.is_empty() && since.elapsed() >= self.delay => {
                self.shown = true;
                true
            }
            _ => false,
        }
    }

    /// 아직 보이지 않았지만 기다리는 중(호스트의 타이머 유지 조건).
    #[must_use]
    pub fn pending(&self) -> bool {
        !self.shown && !self.dismissed && self.hover.as_ref().is_some_and(|(_, t, _)| !t.is_empty())
    }

    /// 지금 보이는가(= `paint`가 그린다).
    #[must_use]
    pub fn visible(&self) -> bool {
        self.shown && self.hover.is_some()
    }

    /// 다음에 깨울 시각(기다리는 중일 때만).
    #[must_use]
    pub fn next_wake(&self) -> Option<Instant> {
        if !self.pending() {
            return None;
        }
        self.hover.as_ref().map(|(_, _, since)| *since + self.delay)
    }

    /// 보이던 툴팁을 감춘다(클릭 · 키 입력) — 떠날 때까지 다시 안 보인다. 돌려주는 값 = 감춘 것이 있었다.
    pub fn dismiss(&mut self) -> bool {
        self.dismissed = true;
        std::mem::take(&mut self.shown)
    }

    /// 머문 자리의 글(비면 None).
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.hover
            .as_ref()
            .map(|(_, t, _)| t.as_str())
            .filter(|t| !t.is_empty())
    }

    /// 팝업 층에서 그린다 — `clamp_x` = 가로 한계 · `surface_bottom` = 아래 한계(기준 아래로 넘치면 위로 접는 판정은 그리기 함수가 한다).
    pub fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme, clamp_x: (i32, i32), scale: f32) {
        if !self.visible() {
            return;
        }
        let Some((anchor, text, _)) = &self.hover else {
            return;
        };
        let above = self.above.unwrap_or_else(crate::draw::tooltip_above);
        crate::draw::draw_tooltip_in_dir(ctx, theme, *anchor, clamp_x, text, scale, above);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 상태기계: 머묾 → 지연 전 = 대기 · 지연 뒤 틱 = 처음 보임(한 번만) · 같은 자리 글 갱신은 지연 안 다시 셈 · 다른 자리 = 다시 대기 ·
    /// 떠남 = 사라짐(true 1회) · dismiss = 떠날 때까지 안 보임 · 빈 글 = 안 보임 · next_wake.
    #[test]
    fn tooltip_shows_after_delay_and_hides_on_leave() {
        let mut t = Tooltip::new();
        t.set_delay_ms(30);
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(20, 0, 10, 10);
        assert!(
            !t.hover(Some((a, "A".to_string()))),
            "처음엔 사라질 것이 없다"
        );
        assert!(t.pending() && !t.visible() && !t.tick());
        assert!(t.next_wake().is_some());
        std::thread::sleep(Duration::from_millis(40));
        assert!(t.tick(), "지연 뒤 처음 보임");
        assert!(t.visible() && !t.pending() && t.next_wake().is_none());
        assert!(!t.tick(), "두 번째 틱 = 변화 없음");
        assert!(!t.hover(Some((a, "A2".to_string()))), "같은 자리 = 글만");
        assert_eq!(t.text(), Some("A2"));
        assert!(t.visible());
        assert!(
            t.hover(Some((b, "B".to_string()))),
            "다른 자리 = 보이던 것 사라짐"
        );
        assert!(t.pending() && !t.visible());
        std::thread::sleep(Duration::from_millis(40));
        assert!(t.tick() && t.visible());
        assert!(t.dismiss(), "감춤");
        assert!(
            !t.visible() && !t.pending() && !t.tick(),
            "떠날 때까지 안 보임"
        );
        assert!(!t.hover(None), "이미 감춰져 사라질 것 없음");
        assert!(
            !t.hover(Some((a, "".to_string()))) && !t.pending(),
            "빈 글 = 기다리지 않음"
        );
        std::thread::sleep(Duration::from_millis(40));
        assert!(!t.tick());
        t.hover(Some((a, "A".to_string())));
        std::thread::sleep(Duration::from_millis(40));
        assert!(t.tick());
        assert!(t.hover(None), "떠남 = 사라짐 통지");
        assert!(!t.visible() && t.next_wake().is_none());
    }
}

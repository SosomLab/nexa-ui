//! 스플리터 — 두 영역 사이의 가는 경계선 + 드래그로 크기 조절(nexa-sql 사용자 09-16 "좌측 뷰와 편집기 사이 ·
//! 편집기와 결과 사이"). 설정 창의 인라인 스플리터(nexa-sql `prefs_win`)를 공용 부품으로 올렸다.
//!
//! - **hover = 서서히 진해지는 손잡이**([`IntentFade`] `Slow` = 전역 hover 진입 시간 · 기본 1초 · 마지막 위치만) ·
//!   드래그 중 = 즉시 최대.
//! - 기하는 호스트가 준다: [`set_rect`](Splitter::set_rect) = 잡히는 띠(물리 px · 보통 6~8 논리 px) ·
//!   [`on_event`](Splitter::on_event)는 축 방향 좌표를 돌려주고 호스트가 자기 배치 값(폭·비율)으로 바꾼다.
//! - 커서 모양(↔ / ↕)은 호스트 몫([`is_hover`](Splitter::is_hover) · [`axis`](Splitter::axis)).

use crate::draw::DrawCtx;
use crate::event::InputEvent;
use crate::geom::{Point, Rect};
use crate::theme::Theme;
use crate::tokens::{hover_alpha, FadeSpeed, IntentFade};

/// 경계선 방향 — `Vertical` = 세로선(좌우 분할 · x 이동) · `Horizontal` = 가로선(상하 분할 · y 이동).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitAxis {
    Vertical,
    Horizontal,
}

/// [`Splitter::on_event`] 결과.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitEvent {
    /// 이 스플리터와 무관.
    None,
    /// hover 상태만 바뀜(다시 그리기).
    Hover,
    /// 드래그 시작(호스트는 포커스 이동 등을 막는다).
    Start,
    /// 드래그 중 — 축 방향 좌표(물리 px · 창 기준). 호스트가 폭/비율로 환산·클램프한다.
    Drag(i32),
    /// 드래그 끝(호스트가 설정 저장).
    End,
}

/// 스플리터 컨트롤(경계선 하나).
#[derive(Debug)]
pub struct Splitter {
    axis: SplitAxis,
    /// 잡히는 띠(물리 px).
    rect: Rect,
    /// 드래그: (누른 축 좌표, 누를 때 띠의 축 시작).
    drag: Option<(i32, i32)>,
    fade: IntentFade,
    hover: bool,
}

impl Splitter {
    #[must_use]
    pub fn new(axis: SplitAxis) -> Self {
        Self {
            axis,
            rect: Rect::new(0, 0, 0, 0),
            drag: None,
            fade: IntentFade::with_speed(FadeSpeed::Slow),
            hover: false,
        }
    }

    #[must_use]
    pub fn axis(&self) -> SplitAxis {
        self.axis
    }

    /// 잡히는 띠(물리 px). 폭 0이면 비활성(안 그리고 · 안 잡힘).
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    #[must_use]
    pub fn rect(&self) -> Rect {
        self.rect
    }

    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// 커서가 띠 위(또는 드래그 중) — 호스트가 커서 모양을 정할 때.
    #[must_use]
    pub fn is_hover(&self) -> bool {
        self.hover || self.drag.is_some()
    }

    fn along(&self, x: i32, y: i32) -> i32 {
        match self.axis {
            SplitAxis::Vertical => x,
            SplitAxis::Horizontal => y,
        }
    }

    /// 마우스 사건 처리. `Drag(v)`의 `v` = 띠 시작의 새 축 좌표(누른 지점 기준 상대 이동).
    pub fn on_event(&mut self, ev: &InputEvent) -> SplitEvent {
        if self.rect.is_empty() {
            return SplitEvent::None;
        }
        match *ev {
            InputEvent::MouseMove { x, y } => {
                if let Some((p0, start)) = self.drag {
                    return SplitEvent::Drag(start + (self.along(x, y) - p0));
                }
                let over = self.rect.contains(Point { x, y });
                self.fade.set(over.then_some(0));
                if over != self.hover {
                    self.hover = over;
                    return SplitEvent::Hover;
                }
                SplitEvent::None
            }
            InputEvent::MouseDown { x, y, .. } if self.rect.contains(Point { x, y }) => {
                let start = match self.axis {
                    SplitAxis::Vertical => self.rect.x,
                    SplitAxis::Horizontal => self.rect.y,
                };
                self.drag = Some((self.along(x, y), start));
                self.fade.jump(Some(0));
                SplitEvent::Start
            }
            InputEvent::MouseUp { .. } if self.drag.is_some() => {
                self.drag = None;
                SplitEvent::End
            }
            _ => SplitEvent::None,
        }
    }

    /// 페이드 진행 — 다시 그려야 하면 `true`.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        self.fade.tick(now_ms)
    }

    /// 가는 경계선 + hover/드래그 손잡이(accent · 서서히).
    pub fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let r = self.rect;
        if r.is_empty() {
            return;
        }
        let line = match self.axis {
            SplitAxis::Vertical => Rect::new(r.x + r.w / 2, r.y, 1, r.h),
            SplitAxis::Horizontal => Rect::new(r.x, r.y + r.h / 2, r.w, 1),
        };
        ctx.fill_rect(line, theme.border);
        let a = hover_alpha(self.drag.is_some(), self.fade.value(0));
        if a > 0.0 {
            let grip = match self.axis {
                SplitAxis::Vertical => Rect::new(line.x - 1, r.y, 3, r.h),
                SplitAxis::Horizontal => Rect::new(r.x, line.y - 1, r.w, 3),
            };
            ctx.fill_rect_alpha(grip, theme.accent, a.max(0.15));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_reports_new_start_along_axis() {
        let mut s = Splitter::new(SplitAxis::Vertical);
        s.set_rect(Rect::new(100, 0, 6, 300));
        assert_eq!(
            s.on_event(&InputEvent::MouseMove { x: 50, y: 10 }),
            SplitEvent::None
        );
        assert_eq!(
            s.on_event(&InputEvent::MouseMove { x: 102, y: 10 }),
            SplitEvent::Hover
        );
        assert!(s.is_hover());
        assert_eq!(
            s.on_event(&InputEvent::MouseDown {
                x: 102,
                y: 10,
                shift: false,
                primary: false
            }),
            SplitEvent::Start
        );
        assert_eq!(
            s.on_event(&InputEvent::MouseMove { x: 132, y: 500 }),
            SplitEvent::Drag(130)
        );
        assert_eq!(
            s.on_event(&InputEvent::MouseUp { x: 132, y: 500 }),
            SplitEvent::End
        );
        assert!(!s.is_dragging());
    }

    #[test]
    fn horizontal_uses_y_and_empty_rect_is_inert() {
        let mut s = Splitter::new(SplitAxis::Horizontal);
        assert_eq!(
            s.on_event(&InputEvent::MouseMove { x: 1, y: 1 }),
            SplitEvent::None
        );
        s.set_rect(Rect::new(0, 200, 400, 8));
        assert_eq!(
            s.on_event(&InputEvent::MouseDown {
                x: 10,
                y: 204,
                shift: false,
                primary: false
            }),
            SplitEvent::Start
        );
        assert_eq!(
            s.on_event(&InputEvent::MouseMove { x: 999, y: 194 }),
            SplitEvent::Drag(190)
        );
    }
}

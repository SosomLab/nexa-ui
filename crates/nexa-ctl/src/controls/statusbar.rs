//! **상태 바** — 창 맨 아래 한 줄(출처: `nexa-dir2/crates/nexa-gui/src/widgets/chrome.rs` `StatusBar` · dir2 GUI-080 · nexa-dir3 104차 10-03).
//!
//! 왼쪽 글(본문색) + 오른쪽 글(흐린 색 · 오른쪽 정렬 — 왼쪽 글과 겹치면 오른쪽 글이 `x + pad`까지만 온다). 배경 `chrome_bg` ·
//! **위** 1px `border` · [`FontSlot::Status`]. [`StatusBar::set_text`]는 **바뀔 때만** 무효화한다(상태줄은 매 입력마다 갱신되므로
//! 같은 글이면 그리지 않는 것이 dir2 교훈). 입력 사건은 받지 않는다(표시 전용 — 구획·클릭은 호스트가 필요해지면 더한다).

use super::{Control, ControlBase};
use crate::draw::{DrawCtx, FontSlot};
use crate::event::InputEvent;
use crate::geom::Rect;
use crate::theme::Theme;
use crate::widget::{Invalidations, Widget};

/// 기본 높이(논리 px · dir2 상태바 22).
pub const DEFAULT_H: i32 = 22;
/// 좌우 여백(논리 px).
const PAD_X: i32 = 8;

/// 상태 바 컨트롤.
#[derive(Debug, Default)]
pub struct StatusBar {
    base: ControlBase,
    left: String,
    right: String,
}

impl StatusBar {
    /// 빈 상태 바.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 왼쪽·오른쪽 글 — **바뀔 때만** 무효화(dir2 `statusbar_set_text_invalidates_only_on_change`). 바뀌었으면 true.
    pub fn set_text(&mut self, left: &str, right: &str, inv: &mut Invalidations) -> bool {
        if self.left == left && self.right == right {
            return false;
        }
        self.left = left.to_string();
        self.right = right.to_string();
        inv.push(self.base.bounds);
        true
    }

    /// 왼쪽 글만(오른쪽 유지).
    pub fn set_left(&mut self, left: &str, inv: &mut Invalidations) -> bool {
        let right = self.right.clone();
        self.set_text(left, &right, inv)
    }

    /// 오른쪽 글만(왼쪽 유지).
    pub fn set_right(&mut self, right: &str, inv: &mut Invalidations) -> bool {
        let left = self.left.clone();
        self.set_text(&left, right, inv)
    }

    /// 왼쪽 글.
    #[must_use]
    pub fn left(&self) -> &str {
        &self.left
    }

    /// 오른쪽 글.
    #[must_use]
    pub fn right(&self) -> &str {
        &self.right
    }

    /// 권장 높이(물리 px · 배율 반영).
    #[must_use]
    pub fn preferred_height(&self) -> i32 {
        self.s(DEFAULT_H)
    }

    /// 덤프(하네스 `status.dump`): `left\tright`.
    #[must_use]
    pub fn dump(&self) -> String {
        format!("{}\t{}", self.left, self.right)
    }
}

impl Control for StatusBar {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
}

impl Widget for StatusBar {
    fn bounds(&self) -> Rect {
        self.base.bounds
    }

    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        if self.base.bounds != bounds {
            self.base.bounds = bounds;
            inv.push(bounds);
        }
    }

    fn on_event(&mut self, _ev: &InputEvent, _inv: &mut Invalidations) {}

    fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.base.bounds;
        if b.is_empty() {
            return;
        }
        ctx.fill_rect(b, theme.chrome_bg);
        ctx.fill_rect(Rect::new(b.x, b.y, b.w, 1), theme.border);
        ctx.select_font(FontSlot::Status, false);
        let pad = self.s(PAD_X);
        let ty = ctx.text_center_y(b.y + 1, b.h - 1);
        let clip = Rect::new(b.x, b.y + 1, b.w, b.h - 1);
        if !self.right.is_empty() {
            let rw = ctx.text_width(&self.right);
            let rx = (b.right() - pad - rw).max(b.x + pad);
            ctx.text(rx, ty, clip, &self.right, theme.text_dim);
        }
        if !self.left.is_empty() {
            ctx.text(b.x + pad, ty, clip, &self.left, theme.text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::RecordCtx;

    /// dir2 GUI-080 계승: 같은 글 재설정 = 무효화 없음 · 바뀌면 bounds 무효화 · 오른쪽 글은 오른쪽 정렬(`x = right − pad − 폭`) ·
    /// 위 1px 경계 · 그리기는 bounds 안.
    #[test]
    fn set_text_invalidates_only_on_change_and_right_aligns() {
        let mut sb = StatusBar::new();
        let mut inv = Invalidations::default();
        sb.set_bounds(Rect::new(0, 100, 400, 22), &mut inv);
        assert!(!inv.is_empty());
        let mut inv = Invalidations::default();
        assert!(sb.set_text("3 items", "Tab 1/2", &mut inv));
        assert!(!inv.is_empty());
        let mut inv2 = Invalidations::default();
        assert!(!sb.set_text("3 items", "Tab 1/2", &mut inv2));
        assert!(inv2.is_empty(), "같은 글 = 무효화 없음");
        assert!(sb.set_right("Tab 2/2", &mut inv2) && !inv2.is_empty());
        assert_eq!(sb.dump(), "3 items\tTab 2/2");
        let mut rec = RecordCtx::with_surface(400, 200);
        sb.paint(&mut rec, &Theme::dark());
        assert!(rec.drew_text("3 items") && rec.drew_text("Tab 2/2"));
        let lx = rec
            .texts
            .iter()
            .find(|(_, _, _, s)| s == "3 items")
            .map(|t| t.0)
            .unwrap();
        let rx = rec
            .texts
            .iter()
            .find(|(_, _, _, s)| s == "Tab 2/2")
            .map(|t| t.0)
            .unwrap();
        assert_eq!(lx, 8, "왼쪽 여백");
        assert_eq!(rx, 400 - 8 - 7 * 7, "오른쪽 정렬 = right − pad − 글자수×7");
        assert!(
            rec.fills
                .iter()
                .any(|(r, _)| r == &Rect::new(0, 100, 400, 1)),
            "위 1px 경계"
        );
        assert!(rec.all_inside(Rect::new(0, 100, 400, 22)));
        assert_eq!(sb.preferred_height(), 22);
    }

    /// 왼쪽이 길면 오른쪽 글은 `x + pad`까지만 밀린다(겹침 허용 · dir2 규약).
    #[test]
    fn right_text_never_goes_left_of_pad() {
        let mut sb = StatusBar::new();
        let mut inv = Invalidations::default();
        sb.set_bounds(Rect::new(0, 0, 60, 22), &mut inv);
        sb.set_text("", "a very long right text", &mut inv);
        let mut rec = RecordCtx::default();
        sb.paint(&mut rec, &Theme::dark());
        let rx = rec.texts[0].0;
        assert_eq!(rx, 8);
    }
}

//! **상태 바** — 창 맨 아래 한 줄(출처: `nexa-dir2/crates/nexa-gui/src/widgets/chrome.rs` `StatusBar` · dir2 GUI-080 · nexa-dir3 104차 10-03).
//!
//! 왼쪽 글(본문색) + 오른쪽 글(흐린 색 · 오른쪽 정렬 — 왼쪽 글과 겹치면 오른쪽 글이 `x + pad`까지만 온다). 배경 `chrome_bg` ·
//! **위** 1px `border` · [`FontSlot::Status`]. [`StatusBar::set_text`]는 **바뀔 때만** 무효화한다(상태줄은 매 입력마다 갱신되므로
//! 같은 글이면 그리지 않는 것이 dir2 교훈).
//!
//! **칸(세그먼트 · 133차 nexa-dir3 상태줄 구성)**: [`StatusBar::set_segments`]로 칸을 주면 오른쪽 글 대신 칸들을 그린다 —
//! 기본은 오른쪽 끝에 붙여(첫 칸이 맨 왼쪽) · [`StatusBar::set_segments_leading`]이면 왼쪽부터(패널 아래 "탭 상태바").
//! 칸은 hover 배경 · 좌클릭/우클릭 통지([`StatusBar::take_click`])를 가진다. 칸이 없으면 종전 그대로(입력 무시).

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

/// 칸 안쪽 좌우 여백(논리 px).
const SEG_PAD: i32 = 7;

/// 상태 바의 칸 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusSeg {
    /// 호스트가 붙이는 식별자(클릭 통지에 그대로 돌아온다).
    pub id: String,
    /// 표시 글.
    pub text: String,
    /// 클릭할 수 있는가(false = hover·클릭 없음 · 흐린 글).
    pub clickable: bool,
}

impl StatusSeg {
    /// 클릭 가능한 칸.
    #[must_use]
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            clickable: true,
        }
    }

    /// 표시 전용 칸.
    #[must_use]
    pub fn label(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            clickable: false,
            ..Self::new(id, text)
        }
    }
}

/// 상태 바 컨트롤.
#[derive(Debug, Default)]
pub struct StatusBar {
    base: ControlBase,
    left: String,
    right: String,
    segs: Vec<StatusSeg>,
    /// 칸을 왼쪽부터 놓는가(기본 = 오른쪽 끝에 붙임).
    leading: bool,
    /// 마지막으로 그린 칸 자리(글 폭은 그릴 때만 알 수 있다 — 입력 판정은 이 자리를 쓴다).
    rects: std::cell::RefCell<Vec<Rect>>,
    hover: Option<usize>,
    pressed: Option<usize>,
    click: Option<(String, bool)>,
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

    /// 칸 목록 — **바뀔 때만** 무효화. 칸이 하나라도 있으면 오른쪽 글 대신 칸을 그린다. 바뀌었으면 true.
    pub fn set_segments(&mut self, segs: Vec<StatusSeg>, inv: &mut Invalidations) -> bool {
        if self.segs == segs {
            return false;
        }
        if self.segs.len() != segs.len() {
            self.hover = None;
            self.pressed = None;
        }
        self.segs = segs;
        inv.push(self.base.bounds);
        true
    }

    /// 칸을 왼쪽부터 놓는다(패널 아래 상태바) — 이때 왼쪽 글은 칸들 뒤(오른쪽)에 온다.
    pub fn set_segments_leading(&mut self, on: bool, inv: &mut Invalidations) {
        if self.leading != on {
            self.leading = on;
            inv.push(self.base.bounds);
        }
    }

    /// 칸 목록.
    #[must_use]
    pub fn segments(&self) -> &[StatusSeg] {
        &self.segs
    }

    /// 마지막으로 그린 그 칸의 자리(아직 그리지 않았거나 없는 칸 = `None`).
    #[must_use]
    pub fn seg_rect(&self, id: &str) -> Option<Rect> {
        let i = self.segs.iter().position(|s| s.id == id)?;
        self.rects.borrow().get(i).copied().filter(|r| r.w > 0)
    }

    /// 칸 클릭 통지 `(id, 우클릭인가)` — 한 번만 돌려준다.
    pub fn take_click(&mut self) -> Option<(String, bool)> {
        self.click.take()
    }

    fn seg_at(&self, x: i32, y: i32) -> Option<usize> {
        let rects = self.rects.borrow();
        self.segs
            .iter()
            .zip(rects.iter())
            .position(|(s, r)| s.clickable && r.contains(crate::geom::Point { x, y }))
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

    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        if self.segs.is_empty() {
            return;
        }
        match *ev {
            InputEvent::MouseMove { x, y } => {
                let h = self.seg_at(x, y);
                if h != self.hover {
                    self.hover = h;
                    inv.push(self.base.bounds);
                }
            }
            InputEvent::MouseDown { x, y, .. } | InputEvent::DoubleClick { x, y, .. } => {
                self.pressed = self.seg_at(x, y);
                if self.pressed.is_some() {
                    inv.push(self.base.bounds);
                }
            }
            InputEvent::MouseUp { x, y } => {
                if let Some(p) = self.pressed.take() {
                    if self.seg_at(x, y) == Some(p) {
                        self.click = Some((self.segs[p].id.clone(), false));
                    }
                    inv.push(self.base.bounds);
                }
            }
            InputEvent::RightDown { x, y } => {
                if let Some(i) = self.seg_at(x, y) {
                    self.click = Some((self.segs[i].id.clone(), true));
                }
            }
            _ => {}
        }
    }

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
        if self.segs.is_empty() {
            self.rects.borrow_mut().clear();
            if !self.right.is_empty() {
                let rw = ctx.text_width(&self.right);
                let rx = (b.right() - pad - rw).max(b.x + pad);
                ctx.text(rx, ty, clip, &self.right, theme.text_dim);
            }
            if !self.left.is_empty() {
                ctx.text(b.x + pad, ty, clip, &self.left, theme.text);
            }
            return;
        }
        // 칸: 폭 = 글 + 좌우 여백 · 칸 사이 1px 세로 선. 오른쪽 정렬이면 자리가 모자랄 때 **왼쪽 칸부터** 빠진다(맨 오른쪽 우선).
        let sp = self.s(SEG_PAD);
        let widths: Vec<i32> = self
            .segs
            .iter()
            .map(|s| {
                if s.text.is_empty() {
                    0
                } else {
                    ctx.text_width(&s.text) + 2 * sp
                }
            })
            .collect();
        let mut rects = vec![Rect::new(0, 0, 0, 0); self.segs.len()];
        let left_w = if self.left.is_empty() {
            0
        } else {
            ctx.text_width(&self.left) + 2 * pad
        };
        let mut left_x = b.x + pad;
        let left_clip;
        if self.leading {
            let mut x = b.x;
            for (i, w) in widths.iter().enumerate() {
                if *w == 0 || x + w > b.right() {
                    continue;
                }
                rects[i] = Rect::new(x, b.y + 1, *w, b.h - 1);
                x += w + 1;
            }
            left_x = x + pad;
            left_clip = Rect::new(x, b.y + 1, (b.right() - x).max(0), b.h - 1);
        } else {
            let mut x = b.right();
            let min_x = b.x + left_w.min(b.w / 2);
            for (i, w) in widths.iter().enumerate().rev() {
                if *w == 0 || x - w < min_x {
                    continue;
                }
                x -= w;
                rects[i] = Rect::new(x, b.y + 1, *w, b.h - 1);
                x -= 1;
            }
            left_clip = Rect::new(b.x, b.y + 1, (x - b.x).max(0), b.h - 1);
        }
        for (i, (seg, r)) in self.segs.iter().zip(&rects).enumerate() {
            if r.w == 0 {
                continue;
            }
            let hot = seg.clickable && (self.hover == Some(i) || self.pressed == Some(i));
            if hot {
                ctx.fill_rect(*r, theme.sel_bg_inactive);
            }
            // 칸 경계(오른쪽 정렬 = 칸 왼쪽 · 왼쪽 정렬 = 칸 오른쪽).
            let line_x = if self.leading { r.right() } else { r.x - 1 };
            ctx.fill_rect(
                Rect::new(line_x, b.y + 5, 1, (b.h - 9).max(1)),
                theme.border,
            );
            let color = if hot || self.leading {
                theme.text
            } else {
                theme.text_dim
            };
            ctx.text(r.x + sp, ty, *r, &seg.text, color);
        }
        *self.rects.borrow_mut() = rects;
        if !self.left.is_empty() {
            ctx.text(left_x, ty, left_clip, &self.left, theme.text);
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

    /// 칸(133차): 오른쪽 끝에 붙고(첫 칸이 맨 왼쪽) · 좌클릭/우클릭 통지 · 표시 전용 칸은 클릭 없음 · 좁으면 왼쪽 칸부터 빠진다 ·
    /// 왼쪽부터 놓기(탭 상태바) · 칸이 없으면 입력 무시.
    #[test]
    fn segments_layout_and_clicks() {
        let mut sb = StatusBar::new();
        let mut inv = Invalidations::default();
        sb.set_bounds(Rect::new(0, 100, 400, 22), &mut inv);
        sb.set_text("left", "ignored", &mut inv);
        let segs = vec![
            StatusSeg::label("tab", "Tab 1/2"),
            StatusSeg::new("mem", "12 MB"),
            StatusSeg::new("lic", "Free"),
        ];
        assert!(sb.set_segments(segs.clone(), &mut inv));
        let mut inv2 = Invalidations::default();
        assert!(!sb.set_segments(segs, &mut inv2) && inv2.is_empty());
        assert_eq!(sb.seg_rect("lic"), None, "그리기 전");
        let mut rec = RecordCtx::with_surface(400, 200);
        sb.paint(&mut rec, &Theme::dark());
        assert!(rec.drew_text("Free") && rec.drew_text("12 MB") && rec.drew_text("left"));
        assert!(
            !rec.drew_text("ignored"),
            "칸이 있으면 오른쪽 글은 안 그린다"
        );
        let (tab, mem, lic) = (
            sb.seg_rect("tab").unwrap(),
            sb.seg_rect("mem").unwrap(),
            sb.seg_rect("lic").unwrap(),
        );
        assert_eq!(lic.right(), 400);
        assert_eq!(lic.w, 4 * 7 + 14);
        assert!(tab.right() < mem.x && mem.right() < lic.x);
        let click = |sb: &mut StatusBar, r: Rect| {
            let mut inv = Invalidations::default();
            let (x, y) = (r.x + 3, r.y + 3);
            sb.on_event(
                &InputEvent::MouseDown {
                    x,
                    y,
                    shift: false,
                    primary: false,
                },
                &mut inv,
            );
            sb.on_event(&InputEvent::MouseUp { x, y }, &mut inv);
        };
        click(&mut sb, mem);
        assert_eq!(sb.take_click(), Some(("mem".to_string(), false)));
        assert_eq!(sb.take_click(), None);
        click(&mut sb, tab);
        assert_eq!(sb.take_click(), None, "표시 전용 칸");
        sb.on_event(
            &InputEvent::RightDown {
                x: lic.x + 2,
                y: lic.y + 2,
            },
            &mut inv,
        );
        assert_eq!(sb.take_click(), Some(("lic".to_string(), true)));
        // 좁으면 왼쪽 칸부터 빠진다(맨 오른쪽 = 라이선스가 남는다).
        sb.set_bounds(Rect::new(0, 100, 90, 22), &mut inv);
        let mut rec = RecordCtx::with_surface(400, 200);
        sb.paint(&mut rec, &Theme::dark());
        assert!(sb.seg_rect("lic").is_some() && sb.seg_rect("tab").is_none());
        // 왼쪽부터.
        sb.set_bounds(Rect::new(0, 100, 400, 22), &mut inv);
        sb.set_segments_leading(true, &mut inv);
        let mut rec = RecordCtx::with_surface(400, 200);
        sb.paint(&mut rec, &Theme::dark());
        assert_eq!(sb.seg_rect("tab").unwrap().x, 0);
        assert!(sb.seg_rect("mem").unwrap().x > sb.seg_rect("tab").unwrap().right());
        // 칸 없음 = 종전(입력 무시 · 오른쪽 글).
        sb.set_segments(Vec::new(), &mut inv);
        click(&mut sb, lic);
        assert_eq!(sb.take_click(), None);
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

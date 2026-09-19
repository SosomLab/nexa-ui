//! **PositionDropdown — 위치 이미지 드롭다운**(nexa-sql 사용자 09-19 "글자가 아니라 이미지 드롭박스로 3×3 선택 · 선택된 위치도 이미지로").
//!
//! 머리(콤보 크기 상자)에 **선택된 위치의 미니 화면 타일**(4:3 화면 안 작은 박스)과 ▾ 표식을 보이고, 클릭하면 아래로
//! [`PositionPicker`] 3×3 그리드가 팝업으로 펼쳐진다. 값 코드는 `PositionPicker`와 같은 `tl…br`(행우선).
//!
//! 팝업은 [`PositionDropdown::paint_popup`]으로 맨 위 레이어에서 다시 그린다(콤보·컨텍스트 메뉴와 같은 z순서 규약).
//! 선택은 [`PositionDropdown::take_changed`] 1회성 보고 — 저장·적용은 호스트 몫.

use super::posgrid::{paint_cell, PositionPicker, CODES};
use super::{draw_chevron_down, Control, ControlBase};
use crate::draw::DrawCtx;
use crate::event::{InputEvent, Key};
use crate::geom::{Point, Rect};
use crate::theme::Theme;
use crate::widget::{Invalidations, Widget};

/// 팝업 안쪽 여백(논리 px).
const POP_PAD: i32 = 8;
/// ▾ 표식 폭(논리 px).
const DROP_W: i32 = 18;

/// 위치 이미지 드롭다운.
#[derive(Debug)]
pub struct PositionDropdown {
    base: ControlBase,
    picker: PositionPicker,
    open: bool,
    changed: Option<String>,
    /// 팝업이 넘지 못하는 아래 경계(창/목록 바닥 · 0 = 제한 없음) — 자리가 없으면 머리 **위로** 편다.
    max_bottom: i32,
}

impl Default for PositionDropdown {
    fn default() -> Self {
        Self::new("bl")
    }
}

impl PositionDropdown {
    /// 초기 값 코드(`tl…br` · 미지 코드 = 좌하)로 만든다.
    #[must_use]
    pub fn new(code: &str) -> Self {
        let mut picker = PositionPicker::new();
        picker.select_value(code);
        Self {
            base: ControlBase::default(),
            picker,
            open: false,
            changed: None,
            max_bottom: 0,
        }
    }

    /// 팝업 아래 경계(호스트 목록/창 바닥) — 넘치면 위로 펼친다(콤보와 같은 관례).
    pub fn set_max_bottom(&mut self, y: i32) {
        self.max_bottom = y;
    }

    /// 배율 지정(팝업 그리드도 같이).
    pub fn set_scale(&mut self, scale: f32) {
        self.base.scale = scale.max(0.5);
        self.picker.set_scale(scale);
    }

    /// 현재 값 코드.
    #[must_use]
    pub fn value(&self) -> String {
        self.picker.value()
    }

    /// 값 코드로 선택 지정(보고 없음 · 설정 hot-swap 동기화).
    pub fn select_value(&mut self, code: &str) {
        self.picker.select_value(code);
    }

    /// 선택 변경 1회성 보고.
    pub fn take_changed(&mut self) -> Option<String> {
        self.changed.take()
    }

    /// 팝업이 열려 있는가(호스트의 모달 캡처·z순서 판단).
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// 팝업 영역(머리 바로 아래 · 그리드 + 여백).
    fn popup_rect(&self) -> Rect {
        let b = self.base.bounds;
        let (gw, gh) = self.picker.preferred_size();
        let pad = self.s(POP_PAD);
        let (w, h) = (gw + pad * 2, gh + pad * 2);
        let below = b.bottom() + self.s(2);
        let y = if self.max_bottom > 0 && below + h > self.max_bottom {
            b.y - self.s(2) - h
        } else {
            below
        };
        Rect::new(b.x, y, w, h)
    }

    fn place_picker(&mut self, inv: &mut Invalidations) {
        let p = self.popup_rect();
        let pad = self.s(POP_PAD);
        let (gw, gh) = self.picker.preferred_size();
        self.picker
            .set_bounds(Rect::new(p.x + pad, p.y + pad, gw, gh), inv);
    }

    fn close(&mut self, inv: &mut Invalidations) {
        if self.open {
            inv.push(self.popup_rect());
        }
        self.open = false;
        self.picker.set_focused(false);
        inv.push(self.base.bounds);
    }

    /// 팝업 페인트(맨 위 레이어) — 호스트가 다른 위젯을 다 그린 뒤 부른다.
    pub fn paint_popup(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        if !self.open {
            return;
        }
        let p = self.popup_rect();
        ctx.fill_round_rect(p, self.s(6), theme.panel_bg_alt);
        ctx.stroke_round_rect(p, self.s(6), theme.border, 1.0);
        self.picker.paint(ctx, theme);
    }
}

impl Control for PositionDropdown {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
}

impl Widget for PositionDropdown {
    fn bounds(&self) -> Rect {
        self.base.bounds
    }

    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        self.base.bounds = bounds;
        if self.open {
            self.place_picker(inv);
        }
        inv.push(bounds);
    }

    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        match *ev {
            InputEvent::MouseDown { x, y, .. } => {
                let p = Point { x, y };
                if self.open {
                    // 팝업 우선(모달 캡처) — 셀 클릭 = 선택 후 닫기 · 밖(머리 포함) = 닫기.
                    if self.popup_rect().contains(p) {
                        self.picker.on_event(ev, inv);
                        if let Some(v) = self.picker.take_changed() {
                            self.changed = Some(v);
                        }
                    }
                    self.close(inv);
                    return;
                }
                if self.base.bounds.contains(p) && self.base.enabled {
                    self.open = true;
                    self.base.focused = true;
                    self.place_picker(inv);
                    self.picker.set_focused(true);
                    inv.push(self.popup_rect());
                }
            }
            InputEvent::Key { key, .. } if self.open => match key {
                Key::Escape => self.close(inv),
                Key::Enter => {
                    if let Some(v) = self.picker.take_changed() {
                        self.changed = Some(v);
                    }
                    self.close(inv);
                }
                // 방향키 = 그리드 안 이동(선택은 Enter/클릭으로 확정 · 즉시 보고는 셀 클릭과 같게 여기서 거둔다).
                _ => {
                    self.picker.on_event(ev, inv);
                    if let Some(v) = self.picker.take_changed() {
                        self.changed = Some(v);
                    }
                }
            },
            InputEvent::Key {
                key: Key::Space | Key::Enter,
                ..
            } if self.base.focused && self.base.enabled => {
                self.open = true;
                self.place_picker(inv);
                self.picker.set_focused(true);
                inv.push(self.popup_rect());
            }
            _ => {}
        }
    }

    fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.base.bounds;
        if b.is_empty() {
            return;
        }
        // 머리 = 콤보 상자 룩(필드 배경 + 테두리 · 열림/포커스 = accent).
        let edge = if self.open || self.is_focused() {
            theme.accent
        } else {
            theme.border
        };
        ctx.fill_round_rect(b, self.s(6), theme.field_bg);
        ctx.stroke_round_rect(b, self.s(6), edge, 1.0);
        self.draw_focus_ring(ctx, theme, b);
        // 선택 위치 타일(4:3 · 머리 안쪽 높이의 80% · 세로 중앙 · 왼쪽 정렬 — 사용자 09-19 "80% 수준").
        let pad = self.s(4);
        let th_ = ((b.h - pad * 2) * 4 / 5).max(6);
        let tw = th_ * 4 / 3;
        let cell = Rect::new(b.x + pad, b.y + (b.h - th_) / 2, tw, th_);
        let idx = CODES
            .iter()
            .position(|c| *c == self.picker.value())
            .unwrap_or(6);
        paint_cell(ctx, cell, idx, true, theme.accent, theme, self.base.scale);
        // ▾ 표식(오른쪽).
        let drop = self.s(DROP_W);
        let a = self.s(8);
        let area = Rect::new(b.right() - drop + (drop - a) / 2, b.y + (b.h - a) / 2, a, a);
        draw_chevron_down(
            ctx,
            area,
            if self.base.enabled {
                theme.text
            } else {
                theme.text_dim
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(x: i32, y: i32) -> InputEvent {
        InputEvent::MouseDown {
            x,
            y,
            shift: false,
            primary: false,
        }
    }

    #[test]
    fn opens_on_head_click_picks_cell_and_reports_once() {
        let mut d = PositionDropdown::new("bottom_left_unknown");
        assert_eq!(d.value(), "bl", "미지 코드 = 기본 좌하");
        let mut inv = Invalidations::default();
        d.set_bounds(Rect::new(10, 10, 64, 26), &mut inv);
        d.on_event(&click(20, 20), &mut inv);
        assert!(d.is_open());
        // 팝업 안 우상단 셀(2) 클릭 → tr 보고 + 닫힘.
        let p = d.popup_rect();
        let pad = d.s(POP_PAD);
        let (cw, _) = (28, 21);
        let cx = p.x + pad + (cw + 5) * 2 + 5;
        let cy = p.y + pad + 5;
        d.on_event(&click(cx, cy), &mut inv);
        assert!(!d.is_open());
        assert_eq!(d.take_changed().as_deref(), Some("tr"));
        assert!(d.take_changed().is_none(), "1회성");
        assert_eq!(d.value(), "tr");
        // 바깥 클릭 = 닫기만.
        d.on_event(&click(20, 20), &mut inv);
        assert!(d.is_open());
        d.on_event(&click(500, 500), &mut inv);
        assert!(!d.is_open());
        assert!(d.take_changed().is_none());
    }

    #[test]
    fn popup_flips_above_when_no_room_below() {
        let mut d = PositionDropdown::new("bl");
        let mut inv = Invalidations::default();
        d.set_bounds(Rect::new(10, 300, 64, 26), &mut inv);
        assert!(d.popup_rect().y > 326, "제한 없음 = 아래로");
        d.set_max_bottom(340);
        assert!(d.popup_rect().bottom() <= 300, "자리 없음 = 머리 위로");
        d.on_event(&click(20, 310), &mut inv);
        assert!(d.is_open());
        // 위로 펼쳐진 팝업의 좌상단 셀 클릭 → tl.
        let p = d.popup_rect();
        let pad = d.s(POP_PAD);
        d.on_event(&click(p.x + pad + 5, p.y + pad + 5), &mut inv);
        assert_eq!(d.take_changed().as_deref(), Some("tl"));
    }

    #[test]
    fn empty_bounds_paints_nothing_and_escape_closes() {
        let mut d = PositionDropdown::new("c");
        let mut inv = Invalidations::default();
        d.set_bounds(Rect::new(0, 0, 64, 26), &mut inv);
        d.on_event(&click(5, 5), &mut inv);
        assert!(d.is_open());
        d.on_event(
            &InputEvent::Key {
                key: Key::Escape,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert!(!d.is_open());
        assert_eq!(d.value(), "c");
    }
}

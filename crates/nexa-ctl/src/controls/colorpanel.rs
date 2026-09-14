//! **색 패널**(Color Picker · nexa-sql 사용자 09-14) — 투명도까지 고르는 본격 선택기.
//!
//! [채도·명도 사각형][색상 막대][투명도 막대] / [현재 스와치][`#RRGGBBAA` 입력] / [프리셋] / [최근 색].
//!
//! - 값은 **HSV + 알파**로 들고(드래그 중 정밀도 유지) `#RRGGBBAA`로 오간다(6자리 입력도 받는다 = 불투명).
//! - 드래그: 사각형(채도·명도) · 색상 막대 · 투명도 막대. 놓을 때 **최근 색**에 올린다(중복 제거 · 최대 8).
//! - 그라데이션은 코드로 만든 RGBA 이미지([`IconImage`])를 스케일해 그린다 — 정적 자원 0 · 색상이 바뀔 때만 다시 만든다.
//! - 변경은 [`ColorPanel::take_changed`] 1회성 보고(드래그 중에도 보고 = 호스트가 **실시간 미리보기**를 할 수 있다).
//!   최근 색 목록은 [`ColorPanel::recent`]/[`ColorPanel::set_recent`]로 호스트가 영속화한다.
//!
//! 사용자 규약: 색은 새로 만들지 않고 테마 기준색으로 테두리·표식을 그린다 · 잘못된 hex는 직전 값으로 원복.

use super::{Control, ControlBase, TextBox};
use crate::draw::DrawCtx;
use crate::event::InputEvent;
use crate::geom::{Point, Rect};
use crate::theme::{Color, IconImage, Theme};
use crate::widget::{Invalidations, Widget};
use std::cell::RefCell;

// 레이아웃 상수(논리 px).
const SQUARE: i32 = 160;
const BAR_W: i32 = 16;
const GAP: i32 = 8;
const SWATCH: i32 = 30;
const HEX_W: i32 = 110;
const CHIP: i32 = 18;
const CHIP_GAP: i32 = 4;
const ROW_H: i32 = 30;
/// 최근 색 최대 개수.
pub const RECENT_MAX: usize = 8;

/// 프리셋(0xRRGGBBAA) — 테마 강조·상태색 + 무채색.
const PRESETS: [u32; 12] = [
    0x3D8B_FFFF, // 파랑(accent)
    0xD8E8_FFFF, // 밝은 하늘(sel_bg light)
    0x2EA0_43FF, // 초록
    0xE553_4BFF, // 빨강
    0xF5A6_23FF, // 주황
    0xFFD6_00FF, // 노랑
    0x8B5C_F6FF, // 보라
    0x1B24_32FF, // 잉크
    0x0000_00FF, // 검정
    0xFFFF_FFFF, // 흰색
    0x8888_8880, // 반투명 회(50%)
    0x0000_0000, // 완전 투명
];

/// 드래그 중인 부위.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    Sv,
    Hue,
    Alpha,
}

/// HSV(0..360 · 0..1 · 0..1) → RGB.
#[must_use]
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = ((h % 360.0) + 360.0) % 360.0;
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let q = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (q(r), q(g), q(b))
}

/// RGB → HSV.
#[must_use]
pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let (r, g, b) = (
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= f32::EPSILON {
        0.0
    } else if (max - r).abs() <= f32::EPSILON {
        60.0 * (((g - b) / d) % 6.0)
    } else if (max - g).abs() <= f32::EPSILON {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };
    let s = if max <= f32::EPSILON { 0.0 } else { d / max };
    (h, s, max)
}

/// `#RRGGBB` 또는 `#RRGGBBAA` → 0xRRGGBBAA(6자리 = 불투명).
#[must_use]
pub fn rgba_from_hex(s: &str) -> Option<u32> {
    let h = s.trim().trim_start_matches('#');
    if !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match h.len() {
        6 => u32::from_str_radix(h, 16).ok().map(|v| (v << 8) | 0xFF),
        8 => u32::from_str_radix(h, 16).ok(),
        _ => None,
    }
}

/// 0xRRGGBBAA → `#RRGGBBAA`(대문자 · 항상 8자리).
#[must_use]
pub fn rgba_to_hex(rgba: u32) -> String {
    format!("#{rgba:08X}")
}

/// 색 패널 컨트롤.
#[derive(Debug)]
pub struct ColorPanel {
    base: ControlBase,
    h: f32,
    s: f32,
    v: f32,
    a: f32,
    hex: TextBox,
    recent: Vec<u32>,
    drag: Option<Drag>,
    changed: bool,
    /// 채도·명도 사각형 이미지 캐시(색상 · 물리 크기).
    sv_cache: RefCell<Option<(u32, i32, i32, IconImage)>>,
}

impl ColorPanel {
    /// 초기 값(`#RRGGBB[AA]` · 형식 오류면 강조 파랑).
    #[must_use]
    pub fn new(initial: &str) -> Self {
        let rgba = rgba_from_hex(initial).unwrap_or(0x3D8B_FFFF);
        let mut p = Self {
            base: ControlBase::default(),
            h: 0.0,
            s: 0.0,
            v: 0.0,
            a: 1.0,
            hex: TextBox::new("#RRGGBBAA"),
            recent: Vec::new(),
            drag: None,
            changed: false,
            sv_cache: RefCell::new(None),
        };
        p.set_rgba(rgba);
        p
    }

    /// 현재 값 0xRRGGBBAA.
    #[must_use]
    pub fn rgba(&self) -> u32 {
        let (r, g, b) = hsv_to_rgb(self.h, self.s, self.v);
        (u32::from(r) << 24)
            | (u32::from(g) << 16)
            | (u32::from(b) << 8)
            | (self.a * 255.0).round() as u32
    }

    /// 현재 값 `#RRGGBBAA`.
    #[must_use]
    pub fn value_hex(&self) -> String {
        rgba_to_hex(self.rgba())
    }

    /// 값 지정(보고 없음).
    pub fn set_rgba(&mut self, rgba: u32) {
        let (r, g, b) = ((rgba >> 24) as u8, (rgba >> 16) as u8, (rgba >> 8) as u8);
        let (h, s, v) = rgb_to_hsv(r, g, b);
        // 무채색이면 색상은 유지(막대 커서가 튀지 않게).
        if s > 0.0 && v > 0.0 {
            self.h = h;
        }
        self.s = s;
        self.v = v;
        self.a = f32::from(rgba as u8) / 255.0;
        self.hex.set_text(&self.value_hex());
    }

    /// 값 지정(`#RRGGBB[AA]` · 형식 오류 = 무시).
    pub fn set_value(&mut self, hex: &str) {
        if let Some(c) = rgba_from_hex(hex) {
            self.set_rgba(c);
        }
    }

    /// 변경 1회성 보고(`#RRGGBBAA`).
    pub fn take_changed(&mut self) -> Option<String> {
        std::mem::take(&mut self.changed).then(|| self.value_hex())
    }

    /// 최근 색(앞이 최신).
    #[must_use]
    pub fn recent(&self) -> &[u32] {
        &self.recent
    }

    /// 최근 색 주입(호스트 영속화).
    pub fn set_recent(&mut self, list: &[u32]) {
        self.recent = list.iter().copied().take(RECENT_MAX).collect();
    }

    fn push_recent(&mut self) {
        let c = self.rgba();
        self.recent.retain(|&x| x != c);
        self.recent.insert(0, c);
        self.recent.truncate(RECENT_MAX);
    }

    /// hex 입력이 포커스 상태인가(호스트 타이핑 라우팅).
    #[must_use]
    pub fn hex_focused(&self) -> bool {
        self.hex.is_focused()
    }

    /// 권장 크기(물리 px).
    #[must_use]
    pub fn preferred_size(&self) -> (i32, i32) {
        let w = (self.s(SQUARE) + self.s(GAP) + self.s(BAR_W) + self.s(GAP) + self.s(BAR_W))
            .max((self.s(CHIP) + self.s(CHIP_GAP)) * PRESETS.len() as i32);
        let h = self.s(SQUARE)
            + self.s(GAP)
            + self.s(ROW_H)
            + self.s(GAP)
            + self.s(CHIP)
            + self.s(GAP)
            + self.s(CHIP);
        (w, h)
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.base.focused = focused;
        if !focused {
            self.hex.set_focused(false);
        }
    }

    /// hex 입력란 hover 페이드 틱 — 다시 그려야 하면 true.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        self.hex.tick(now_ms)
    }

    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.hex.is_animating()
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.base.scale = scale;
        self.hex.set_scale(scale);
    }

    // ── 기하

    fn square_rect(&self) -> Rect {
        let b = self.base.bounds;
        Rect::new(b.x, b.y, self.s(SQUARE), self.s(SQUARE))
    }
    fn hue_rect(&self) -> Rect {
        let b = self.base.bounds;
        Rect::new(
            b.x + self.s(SQUARE) + self.s(GAP),
            b.y,
            self.s(BAR_W),
            self.s(SQUARE),
        )
    }
    fn alpha_rect(&self) -> Rect {
        let b = self.base.bounds;
        Rect::new(
            b.x + self.s(SQUARE) + self.s(GAP) + self.s(BAR_W) + self.s(GAP),
            b.y,
            self.s(BAR_W),
            self.s(SQUARE),
        )
    }
    fn row_y(&self) -> i32 {
        self.base.bounds.y + self.s(SQUARE) + self.s(GAP)
    }
    fn swatch_rect(&self) -> Rect {
        let b = self.base.bounds;
        Rect::new(b.x, self.row_y(), self.s(SWATCH), self.s(ROW_H))
    }
    fn presets_y(&self) -> i32 {
        self.row_y() + self.s(ROW_H) + self.s(GAP)
    }
    fn recent_y(&self) -> i32 {
        self.presets_y() + self.s(CHIP) + self.s(GAP)
    }
    fn chip_rect(&self, y: i32, i: usize) -> Rect {
        let b = self.base.bounds;
        let cs = self.s(CHIP);
        Rect::new(b.x + (cs + self.s(CHIP_GAP)) * i as i32, y, cs, cs)
    }

    fn apply_point(&mut self, d: Drag, p: Point) {
        match d {
            Drag::Sv => {
                let r = self.square_rect();
                self.s = ((p.x - r.x) as f32 / (r.w - 1).max(1) as f32).clamp(0.0, 1.0);
                self.v = 1.0 - ((p.y - r.y) as f32 / (r.h - 1).max(1) as f32).clamp(0.0, 1.0);
            }
            Drag::Hue => {
                let r = self.hue_rect();
                self.h = ((p.y - r.y) as f32 / (r.h - 1).max(1) as f32).clamp(0.0, 1.0) * 359.99;
            }
            Drag::Alpha => {
                let r = self.alpha_rect();
                self.a = 1.0 - ((p.y - r.y) as f32 / (r.h - 1).max(1) as f32).clamp(0.0, 1.0);
            }
        }
        self.hex.set_text(&self.value_hex());
        self.changed = true;
    }

    fn commit_hex(&mut self, text: &str, inv: &mut Invalidations) {
        if let Some(c) = rgba_from_hex(text) {
            if c != self.rgba() {
                self.set_rgba(c);
                self.changed = true;
                self.push_recent();
            } else {
                self.hex.set_text(&self.value_hex());
            }
        } else {
            self.hex.set_text(&self.value_hex()); // 원복
        }
        inv.push(self.base.bounds);
    }

    /// 채도·명도 사각형 이미지(현재 색상 · 물리 크기) — 캐시.
    fn sv_image(&self, w: i32, h: i32) -> IconImage {
        let key = (self.h * 10.0) as u32;
        if let Some((k, cw, ch, img)) = self.sv_cache.borrow().as_ref() {
            if *k == key && *cw == w && *ch == h {
                return img.clone();
            }
        }
        let (w, h) = (w.max(1) as u32, h.max(1) as u32);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let v = 1.0 - y as f32 / (h - 1).max(1) as f32;
            for x in 0..w {
                let s = x as f32 / (w - 1).max(1) as f32;
                let (r, g, b) = hsv_to_rgb(self.h, s, v);
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
        }
        let img = IconImage::from_rgba(w, h, rgba);
        *self.sv_cache.borrow_mut() = Some((key, w as i32, h as i32, img.clone()));
        img
    }
}

/// 체크무늬(투명 배경 표시).
fn checkerboard(ctx: &mut dyn DrawCtx, r: Rect, cell: i32) {
    let (light, dark) = (Color(0x00E6_E6E6), Color(0x00B0_B0B0));
    ctx.fill_rect(r, light);
    let cell = cell.max(2);
    let mut y = r.y;
    let mut row = 0;
    while y < r.bottom() {
        let mut x = r.x + if row % 2 == 0 { 0 } else { cell };
        while x < r.right() {
            let cr = Rect::new(x, y, cell, cell).intersection(&r);
            if cr.w > 0 && cr.h > 0 {
                ctx.fill_rect(cr, dark);
            }
            x += cell * 2;
        }
        y += cell;
        row += 1;
    }
}

/// 체크무늬 위에 RGBA 칩.
fn chip(ctx: &mut dyn DrawCtx, r: Rect, rgba: u32, radius: i32, border: Color, cell: i32) {
    checkerboard(ctx, r, cell);
    let c = Color(rgba >> 8);
    let a = f32::from(rgba as u8) / 255.0;
    if a > 0.0 {
        ctx.fill_round_rect_alpha(r, radius, c, a);
    }
    ctx.stroke_round_rect(r, radius, border, 1.0);
}

impl Control for ColorPanel {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
}

impl Widget for ColorPanel {
    fn bounds(&self) -> Rect {
        self.base.bounds
    }

    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        self.base.bounds = bounds;
        let sw = self.swatch_rect();
        let hh = self.s(26).min(sw.h);
        self.hex.set_bounds(
            Rect::new(
                sw.right() + self.s(GAP),
                sw.y + (sw.h - hh) / 2,
                self.s(HEX_W),
                hh,
            ),
            inv,
        );
        inv.push(bounds);
    }

    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        match *ev {
            InputEvent::MouseDown { x, y, .. } => {
                let p = Point { x, y };
                let hit = if self.square_rect().contains(p) {
                    Some(Drag::Sv)
                } else if self.hue_rect().contains(p) {
                    Some(Drag::Hue)
                } else if self.alpha_rect().contains(p) {
                    Some(Drag::Alpha)
                } else {
                    None
                };
                if let Some(d) = hit {
                    self.drag = Some(d);
                    self.base.focused = true;
                    self.hex.set_focused(false);
                    self.apply_point(d, p);
                    inv.push(self.base.bounds);
                    return;
                }
                // 프리셋 · 최근 색 칩 = 즉시 적용.
                for (i, &c) in PRESETS.iter().enumerate() {
                    if self.chip_rect(self.presets_y(), i).contains(p) {
                        self.set_rgba(c);
                        self.changed = true;
                        self.push_recent();
                        self.hex.set_focused(false);
                        inv.push(self.base.bounds);
                        return;
                    }
                }
                let recent = self.recent.clone();
                for (i, &c) in recent.iter().enumerate() {
                    if self.chip_rect(self.recent_y(), i).contains(p) {
                        self.set_rgba(c);
                        self.changed = true;
                        self.push_recent();
                        self.hex.set_focused(false);
                        inv.push(self.base.bounds);
                        return;
                    }
                }
                self.hex.set_focused(self.hex.bounds().contains(p));
                self.base.focused = self.base.bounds.contains(p);
            }
            InputEvent::MouseMove { x, y } => {
                if let Some(d) = self.drag {
                    self.apply_point(d, Point { x, y });
                    inv.push(self.base.bounds);
                    return;
                }
            }
            InputEvent::MouseUp { .. } if self.drag.is_some() => {
                self.drag = None;
                self.push_recent();
                inv.push(self.base.bounds);
                return;
            }
            _ => {}
        }
        self.hex.on_event(ev, inv);
        if let Some(t) = self.hex.take_committed() {
            self.commit_hex(&t, inv);
        }
    }

    fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.base.bounds;
        // 채도·명도 사각형.
        let sq = self.square_rect();
        let img = self.sv_image(sq.w, sq.h);
        ctx.image_scaled(sq, &img, b);
        ctx.stroke_round_rect(sq, 0, theme.border, 1.0);
        // 커서(흰 고리 + 검은 고리).
        let cx = sq.x + (self.s * (sq.w - 1) as f32).round() as i32;
        let cy = sq.y + ((1.0 - self.v) * (sq.h - 1) as f32).round() as i32;
        let r = self.s(6);
        ctx.stroke_ellipse(
            Rect::new(cx - r, cy - r, 2 * r, 2 * r),
            Color(0x00FF_FFFF),
            2.0,
        );
        ctx.stroke_ellipse(
            Rect::new(cx - r - 1, cy - r - 1, 2 * r + 2, 2 * r + 2),
            Color(0x0000_0000),
            1.0,
        );
        // 색상 막대(세로 그라데이션 · 1×N 이미지 스케일).
        let hr = self.hue_rect();
        {
            let n = hr.h.max(2) as u32;
            let mut rgba = Vec::with_capacity((n * 4) as usize);
            for y in 0..n {
                let (r, g, b) = hsv_to_rgb(y as f32 / (n - 1) as f32 * 359.99, 1.0, 1.0);
                rgba.extend_from_slice(&[r, g, b, 255]);
            }
            ctx.image_scaled(hr, &IconImage::from_rgba(1, n, rgba), b);
            ctx.stroke_round_rect(hr, 0, theme.border, 1.0);
            let y = hr.y + (self.h / 359.99 * (hr.h - 1) as f32).round() as i32;
            ctx.fill_rect(Rect::new(hr.x - 2, y - 1, hr.w + 4, 3), Color(0x00FF_FFFF));
            ctx.fill_rect(Rect::new(hr.x - 2, y, hr.w + 4, 1), Color(0x0000_0000));
        }
        // 투명도 막대(체크무늬 + 현재 색 알파 그라데이션).
        let ar = self.alpha_rect();
        {
            checkerboard(ctx, ar, self.s(5));
            let n = ar.h.max(2) as u32;
            let (r, g, bb) = hsv_to_rgb(self.h, self.s, self.v);
            let mut rgba = Vec::with_capacity((n * 4) as usize);
            for y in 0..n {
                let a = 255 - (y * 255 / (n - 1)) as u8;
                rgba.extend_from_slice(&[r, g, bb, a]);
            }
            ctx.image_scaled(ar, &IconImage::from_rgba(1, n, rgba), b);
            ctx.stroke_round_rect(ar, 0, theme.border, 1.0);
            let y = ar.y + ((1.0 - self.a) * (ar.h - 1) as f32).round() as i32;
            ctx.fill_rect(Rect::new(ar.x - 2, y - 1, ar.w + 4, 3), Color(0x00FF_FFFF));
            ctx.fill_rect(Rect::new(ar.x - 2, y, ar.w + 4, 1), Color(0x0000_0000));
        }
        // 현재 스와치 + hex.
        chip(
            ctx,
            self.swatch_rect(),
            self.rgba(),
            self.s(4),
            theme.border,
            self.s(5),
        );
        self.hex.paint(ctx, theme);
        // 프리셋 · 최근.
        let accent = self.accent_now(theme);
        let cur = self.rgba();
        for (i, &c) in PRESETS.iter().enumerate() {
            let r = self.chip_rect(self.presets_y(), i);
            chip(
                ctx,
                r,
                c,
                self.s(3),
                if c == cur { accent } else { theme.border },
                self.s(4),
            );
        }
        for (i, &c) in self.recent.iter().enumerate() {
            let r = self.chip_rect(self.recent_y(), i);
            chip(
                ctx,
                r,
                c,
                self.s(3),
                if c == cur { accent } else { theme.border },
                self.s(4),
            );
        }
        if self.recent.is_empty() {
            // 빈 최근 칸은 흐린 테두리로 자리만.
            for i in 0..4 {
                let r = self.chip_rect(self.recent_y(), i);
                ctx.stroke_round_rect(r, self.s(3), theme.border, 1.0);
            }
        }
        self.draw_focus_ring(ctx, theme, b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trip_and_hex_forms() {
        for &(r, g, b) in &[
            (255u8, 0u8, 0u8),
            (0, 255, 0),
            (0, 0, 255),
            (128, 64, 200),
            (255, 255, 255),
            (0, 0, 0),
        ] {
            let (h, s, v) = rgb_to_hsv(r, g, b);
            assert_eq!(hsv_to_rgb(h, s, v), (r, g, b));
        }
        assert_eq!(
            rgba_from_hex("#3D8BFF"),
            Some(0x3D8B_FFFF),
            "6자리 = 불투명"
        );
        assert_eq!(rgba_from_hex("3D8BFF80"), Some(0x3D8B_FF80));
        assert_eq!(rgba_from_hex("#12345"), None);
        assert_eq!(rgba_to_hex(0x0102_0304), "#01020304");
    }

    #[test]
    fn drag_square_and_alpha_reports_and_records_recent() {
        let mut p = ColorPanel::new("#FF0000");
        let mut inv = Invalidations::default();
        p.set_bounds(Rect::new(0, 0, 300, 260), &mut inv);
        let sq = p.square_rect();
        // 오른쪽 위 = 채도 1 · 명도 1.
        p.on_event(
            &InputEvent::MouseDown {
                x: sq.right() - 1,
                y: sq.y,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(p.take_changed().as_deref(), Some("#FF0000FF"));
        // 왼쪽 아래로 드래그 = 검정.
        p.on_event(
            &InputEvent::MouseMove {
                x: sq.x,
                y: sq.bottom() - 1,
            },
            &mut inv,
        );
        assert_eq!(p.value_hex(), "#000000FF");
        p.on_event(
            &InputEvent::MouseUp {
                x: sq.x,
                y: sq.bottom() - 1,
            },
            &mut inv,
        );
        assert_eq!(p.recent(), &[0x0000_00FF], "놓으면 최근 색에");
        // 투명도 막대 맨 아래 = 알파 0.
        let ar = p.alpha_rect();
        p.on_event(
            &InputEvent::MouseDown {
                x: ar.x + 1,
                y: ar.bottom() - 1,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert!(p.value_hex().ends_with("00"), "{}", p.value_hex());
        p.on_event(
            &InputEvent::MouseUp {
                x: ar.x + 1,
                y: ar.bottom() - 1,
            },
            &mut inv,
        );
        assert_eq!(p.recent().len(), 2);
        assert!(p.take_changed().is_some());
        assert!(p.take_changed().is_none(), "1회성");
    }

    #[test]
    fn preset_click_and_hex_commit() {
        let mut p = ColorPanel::new("#000000");
        let mut inv = Invalidations::default();
        p.set_bounds(Rect::new(0, 0, 300, 260), &mut inv);
        let r = p.chip_rect(p.presets_y(), 2); // 초록
        p.on_event(
            &InputEvent::MouseDown {
                x: r.x + 2,
                y: r.y + 2,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(p.take_changed().as_deref(), Some("#2EA043FF"));
        p.commit_hex("#3D8BFF80", &mut inv);
        assert_eq!(p.value_hex(), "#3D8BFF80");
        p.commit_hex("nope", &mut inv);
        assert_eq!(p.value_hex(), "#3D8BFF80", "형식 오류 = 원복");
    }
}

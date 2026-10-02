//! 그리기 어휘(dir2 세대 · `nexa-dir2/crates/nexa-gui/src/draw.rs`에서 그리드가 쓰는 부분만) + nexa-ctl 어댑터.
//!
//! 엔진(`rows.rs` · `edit.rs` · `fastscroll.rs`)은 이 트레이트만 본다. 호스트는 nexa-ctl `DrawCtx`를 넘기고 [`Adapt`]가 번역한다
//! (`VirtualRows`의 `Widget::paint`가 안에서 감싼다 — 호스트가 어댑터를 몰라도 된다).

use crate::geom::Rect;
use crate::theme::Color;

/// 폰트 슬롯(dir2 X-12): 위젯이 페인트 시작에 자신의 슬롯을 선택한다.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FontSlot {
    /// 특정 슬롯이 없는 전부.
    #[default]
    Base,
    /// 파일 목록 + 컬럼 헤더(굵게/이탤릭 장식은 `select_font` 인자).
    List,
    /// 상태바.
    Status,
}

impl FontSlot {
    /// nexa-ctl 슬롯 대응 — `List`는 nexa-ctl에 없어 `PeerList`(목록 글꼴 자리 · UIC-315)로.
    #[must_use]
    pub fn to_ctl(self) -> nexa_ctl::FontSlot {
        match self {
            FontSlot::Base => nexa_ctl::FontSlot::Base,
            FontSlot::List => nexa_ctl::FontSlot::PeerList,
            FontSlot::Status => nexa_ctl::FontSlot::Status,
        }
    }
}

/// 그리드가 쓰는 그리기 어휘(dir2 모델: 불투명 rect + 불투명 배경 텍스트 1회 호출). 기본 구현이 있는 메서드는 테스트 백엔드가 생략해도 된다.
pub trait DrawCtx {
    /// 폰트 슬롯/장식 선택 — 이후의 `text*`/`text_width`에 적용. 기본 = no-op.
    fn select_font(&mut self, slot: FontSlot, bold: bool, italic: bool) {
        let _ = (slot, bold, italic);
    }
    /// rect를 단색으로 불투명하게 채운다.
    fn fill_rect(&mut self, rect: Rect, color: Color);
    /// `clip`을 `bg`로 채우면서 텍스트를 `(x, y)`에 그린다(행 배경+텍스트 1회).
    fn text_opaque(&mut self, x: i32, y: i32, clip: Rect, text: &str, fg: Color, bg: Color);
    /// 텍스트 렌더 폭(px).
    fn text_width(&mut self, text: &str) -> i32;
    /// 배경 없이 텍스트만(선택 하이라이트 위). 기본 = no-op.
    fn text(&mut self, x: i32, y: i32, clip: Rect, text: &str, fg: Color) {
        let _ = (x, y, clip, text, fg);
    }
    /// 현재 글꼴의 텍스트 상자 높이(px). 기본 16.
    fn text_height(&mut self) -> i32 {
        16
    }
    /// 아이콘 — `key`/`hint`는 백엔드가 해석. 반환 = 실제로 그렸는가(아니면 호출자가 폴백). 기본 = false.
    fn draw_icon(&mut self, x: i32, y: i32, size: i32, key: &str, hint: &str) -> bool {
        let _ = (x, y, size, key, hint);
        false
    }
    /// 큰 글리프 — `clip` 안 가운데 정렬. 기본 = `text_opaque`.
    fn glyph_opaque(&mut self, clip: Rect, text: &str, fg: Color, bg: Color) {
        let ty = clip.y + (clip.h - (clip.h * 4) / 5) / 2;
        let tx = clip.x + (clip.w - self.text_width(text)).max(0) / 2;
        self.text_opaque(tx, ty, clip, text, fg, bg);
    }
    /// 원/타원 AA 채움. 기본 = no-op.
    fn fill_ellipse(&mut self, rect: Rect, color: Color) {
        let _ = (rect, color);
    }
    /// 라운드 사각형 AA 채움. 기본 = no-op.
    fn fill_round_rect(&mut self, rect: Rect, radius: i32, color: Color) {
        let _ = (rect, radius, color);
    }
    /// 알파 라운드 사각 채움(오버레이 스크롤바): `alpha` 0=투명 … 255=불투명. 기본 = 불투명 위임.
    fn fill_round_rect_alpha(&mut self, rect: Rect, radius: i32, color: Color, alpha: u8) {
        let _ = alpha;
        self.fill_round_rect(rect, radius, color);
    }
    /// 라운드 사각형 외곽선. 기본 = no-op.
    fn stroke_round_rect(&mut self, rect: Rect, radius: i32, color: Color, width: f32) {
        let _ = (rect, radius, color, width);
    }
    /// 꺾은선. 기본 = no-op.
    fn polyline(&mut self, pts: &[(i32, i32)], color: Color, width: f32) {
        let _ = (pts, color, width);
    }
    /// 클립 영역 **교차** push(가로 스크롤 콘텐츠의 왼쪽 번짐 차단 · dir2 10-02). `pop_clip`과 쌍. 기본 = no-op.
    fn push_clip(&mut self, rect: Rect) {
        let _ = rect;
    }
    /// 직전 `push_clip` 복원. 기본 = no-op.
    fn pop_clip(&mut self) {}
    /// 이미지(미리보기) — `hint`(경로)의 이미지를 `rect` 안 비율 유지 가운데. 디코드·캐시는 백엔드. 기본 = no-op.
    fn draw_image(&mut self, rect: Rect, hint: &str) {
        let _ = (rect, hint);
    }
    /// 큰 글리프 변형(패널 네비 바). 기본 = 같은 크기.
    fn glyph_opaque_lg(&mut self, clip: Rect, text: &str, fg: Color, bg: Color) {
        self.glyph_opaque(clip, text, fg, bg);
    }
}

/// nexa-ctl `DrawCtx` → 그리드 어휘 어댑터. italic은 버린다(nexa-ctl `select_font`에 없음 · U-5) · `alpha` u8 → f32 ·
/// `draw_icon`은 false(아이콘은 G-2에서 `RowSource` 쪽 `IconImage`로).
pub struct Adapt<'a>(pub &'a mut dyn nexa_ctl::DrawCtx);

impl std::fmt::Debug for Adapt<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Adapt(nexa_ctl::DrawCtx)")
    }
}

impl DrawCtx for Adapt<'_> {
    fn select_font(&mut self, slot: FontSlot, bold: bool, _italic: bool) {
        self.0.select_font(slot.to_ctl(), bold);
    }
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.0.fill_rect(rect, color);
    }
    fn text_opaque(&mut self, x: i32, y: i32, clip: Rect, text: &str, fg: Color, bg: Color) {
        self.0.text_opaque(x, y, clip, text, fg, bg);
    }
    fn text_width(&mut self, text: &str) -> i32 {
        self.0.text_width(text)
    }
    fn text(&mut self, x: i32, y: i32, clip: Rect, text: &str, fg: Color) {
        self.0.text(x, y, clip, text, fg);
    }
    fn text_height(&mut self) -> i32 {
        self.0.text_height()
    }
    fn fill_ellipse(&mut self, rect: Rect, color: Color) {
        self.0.fill_ellipse(rect, color);
    }
    fn fill_round_rect(&mut self, rect: Rect, radius: i32, color: Color) {
        self.0.fill_round_rect(rect, radius, color);
    }
    fn fill_round_rect_alpha(&mut self, rect: Rect, radius: i32, color: Color, alpha: u8) {
        self.0
            .fill_round_rect_alpha(rect, radius, color, f32::from(alpha) / 255.0);
    }
    fn stroke_round_rect(&mut self, rect: Rect, radius: i32, color: Color, width: f32) {
        self.0.stroke_round_rect(rect, radius, color, width);
    }
    fn polyline(&mut self, pts: &[(i32, i32)], color: Color, width: f32) {
        self.0.polyline(pts, color, width);
    }
    fn draw_image(&mut self, rect: Rect, hint: &str) {
        self.0.draw_image_hint(rect, hint);
    }
    fn push_clip(&mut self, rect: Rect) {
        self.0.push_clip(rect);
    }
    fn pop_clip(&mut self) {
        self.0.pop_clip();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 어댑터: 슬롯 대응 · 알파 u8 → f32 · 기록기에 그대로 닿는다.
    #[test]
    fn adapt_forwards_to_ctl_ctx() {
        let mut rec = nexa_ctl::RecordCtx::with_surface(100, 100);
        {
            let mut a = Adapt(&mut rec);
            a.select_font(FontSlot::List, true, true);
            a.fill_rect(Rect::new(0, 0, 10, 10), Color::from_rgb(1, 2, 3));
            a.text_opaque(
                1,
                1,
                Rect::new(0, 0, 50, 16),
                "ab",
                Color::from_rgb(0, 0, 0),
                Color::from_rgb(9, 9, 9),
            );
            a.fill_round_rect_alpha(Rect::new(0, 0, 4, 4), 2, Color::from_rgb(0, 0, 0), 255);
            assert_eq!(a.text_width("abc"), 21);
            a.glyph_opaque(
                Rect::new(0, 0, 20, 20),
                "▶",
                Color::from_rgb(0, 0, 0),
                Color::from_rgb(0, 0, 0),
            );
        }
        assert_eq!(rec.fills.len(), 3, "fill + text_opaque 배경 + glyph 배경");
        assert!(rec.drew_text("ab") && rec.drew_text("▶"));
        assert_eq!(rec.round_rects.len(), 1);
        assert_eq!(FontSlot::List.to_ctl(), nexa_ctl::FontSlot::PeerList);
    }
}

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
/// **끝 말줄임**(154차 · dir2 RENDER-010 `SetTrimming` 대응 · 이식 원장 N-02): `text`가 `max_w`(px)에 다 들어가면 그대로,
/// 넘치면 들어가는 만큼의 앞 글자 + `…`. 글자(char) 단위로 자른다. `…`조차 못 넣는 폭이면 빈 글.
/// 넘치지 않는 흔한 경우는 폭을 한 번만 재고 끝난다(넘칠 때만 이분 탐색 — 측정 O(log n)).
pub fn ellipsize_end<'a>(
    ctx: &mut dyn DrawCtx,
    text: &'a str,
    max_w: i32,
) -> std::borrow::Cow<'a, str> {
    use std::borrow::Cow;
    if text.is_empty() || ctx.text_width(text) <= max_w {
        return Cow::Borrowed(text);
    }
    let budget = max_w - ctx.text_width("…");
    if budget < 0 {
        return Cow::Borrowed("");
    }
    // 글자 경계(바이트 자리) — `ends[k]` = 앞 k글자의 끝.
    let ends: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    // 들어가는 가장 긴 접두사(글자 수)를 이분 탐색 — 폭은 글자 수에 대해 단조 증가로 본다.
    let (mut lo, mut hi) = (0usize, ends.len() - 1);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if ctx.text_width(&text[..ends[mid]]) <= budget {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let mut out = String::with_capacity(ends[lo] + 3);
    out.push_str(&text[..ends[lo]]);
    out.push('…');
    Cow::Owned(out)
}

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

/// nexa-ctl `DrawCtx` → 그리드 어휘 어댑터. italic은 `select_font_styled`로 전달(113차 · UIC-313 — 종전 U-5 "버림" 해소) · `alpha` u8 → f32 ·
/// `draw_icon` = 호스트가 등록한 [`set_icon_resolver`](116차 · 없으면 false).
pub struct Adapt<'a>(pub &'a mut dyn nexa_ctl::DrawCtx);

/// 행 아이콘 리졸버 — `(키, 로드 힌트, 한 변 px)` → 이미지(없으면 `None` = 안 그림 · 칸은 비워 둔다).
/// 키/힌트의 뜻은 `RowSource::icon`을 구현한 호스트가 정한다(dir2: 키 = `dir`/`file`/확장자/파일별 경로 · `L|` 접두 = 큰 아이콘).
pub type IconResolver = dyn Fn(&str, &str, i32) -> Option<std::rc::Rc<nexa_ctl::IconImage>>;

thread_local! {
    static ICON_RESOLVER: std::cell::RefCell<Option<std::rc::Rc<IconResolver>>> =
        const { std::cell::RefCell::new(None) };
}

/// 행 아이콘 리졸버 등록/해제(116차 · nexa-dir3 GAP-003 — dir2 M1-7 셸 아이콘). [`Adapt::draw_icon`]이 부른다. UI 스레드 전용.
pub fn set_icon_resolver(f: Option<std::rc::Rc<IconResolver>>) {
    ICON_RESOLVER.with(|r| *r.borrow_mut() = f);
}

thread_local! {
    /// 글리프 크기 증분(논리 px · 목록 글꼴 크기에 더한다) — 기본 −4. 호스트가 [`set_glyph_delta`]로 맞춘다
    /// (dir2 = 아이콘 글꼴 em 9 고정 → 목록 글꼴 px가 16이면 −7).
    static GLYPH_DELTA_PX: std::cell::Cell<f32> = const { std::cell::Cell::new(-4.0) };
}

/// 디스클로저/아이콘 글리프의 크기 증분(논리 px · 목록 글꼴 기준 · 118차). UI 스레드 전용.
pub fn set_glyph_delta(delta_px: f32) {
    GLYPH_DELTA_PX.with(|d| d.set(delta_px));
}

impl std::fmt::Debug for Adapt<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Adapt(nexa_ctl::DrawCtx)")
    }
}

impl DrawCtx for Adapt<'_> {
    fn select_font(&mut self, slot: FontSlot, bold: bool, italic: bool) {
        self.0.select_font_styled(slot.to_ctl(), bold, italic);
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
    /// 디스클로저/아이콘 글리프 — dir2 규약(MDL2 쉐브론 9 DIP · 본문 12 DIP보다 작게 · 셀 가운데). 그린 뒤 목록 글꼴로 복귀.
    fn glyph_opaque(&mut self, clip: Rect, text: &str, fg: Color, bg: Color) {
        self.0.fill_rect(clip, bg);
        let delta = GLYPH_DELTA_PX.with(std::cell::Cell::get);
        self.0
            .select_font_sized(nexa_ctl::FontSlot::PeerList, false, delta);
        let w = self.0.text_width(text);
        // 세로 = 글리프 잉크가 칸 정중앙(아이콘 글꼴 메트릭과 본문 글꼴 줄 높이가 달라 줄 상자 가운데는 1~2 px 어긋난다).
        let y = self.0.glyph_center_y(text, clip.y, clip.h);
        self.0
            .text(clip.x + (clip.w - w).max(0) / 2, y, clip, text, fg);
        self.0.select_font(nexa_ctl::FontSlot::PeerList, false);
    }
    /// 행 아이콘 — 등록된 리졸버가 준 이미지를 `size`×`size`로(없으면 false = 호출자 폴백).
    fn draw_icon(&mut self, x: i32, y: i32, size: i32, key: &str, hint: &str) -> bool {
        let resolver = ICON_RESOLVER.with(|r| r.borrow().clone());
        let Some(img) = resolver.and_then(|f| f(key, hint, size)) else {
            return false;
        };
        let rc = Rect::new(x, y, size, size);
        self.0.image_scaled(rc, &img, rc);
        true
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
        assert_eq!(
            rec.fonts,
            vec![
                (nexa_ctl::FontSlot::PeerList, true, true), // italic이 select_font_styled로 전달(113차)
                (nexa_ctl::FontSlot::PeerList, false, false), // 글리프 = 작은 크기(115차)
                (nexa_ctl::FontSlot::PeerList, false, false), // … 뒤 목록 글꼴 복귀
            ]
        );
        // 행 아이콘: 리졸버가 없으면 false · 있으면 그 이미지를 size×size로(116차).
        {
            let mut a = Adapt(&mut rec);
            assert!(!a.draw_icon(4, 4, 16, "dir", "C:/x"));
            let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let seen2 = seen.clone();
            set_icon_resolver(Some(std::rc::Rc::new(move |k: &str, h: &str, s: i32| {
                seen2.borrow_mut().push((k.to_string(), h.to_string(), s));
                (k == "dir")
                    .then(|| std::rc::Rc::new(nexa_ctl::IconImage::from_rgba(1, 1, vec![255; 4])))
            })));
            assert!(a.draw_icon(4, 4, 16, "dir", "C:/x"));
            assert!(!a.draw_icon(4, 4, 16, "txt", "C:/y.txt"));
            set_icon_resolver(None);
            assert!(!a.draw_icon(4, 4, 16, "dir", "C:/x"));
            assert_eq!(seen.borrow().len(), 2);
            assert_eq!(
                seen.borrow()[0],
                ("dir".to_string(), "C:/x".to_string(), 16)
            );
        }
        assert_eq!(rec.images, vec![Rect::new(4, 4, 16, 16)]);
        // 끝 말줄임(154차): 들어가면 그대로(빌림) · 넘치면 앞 글자 + … · …도 못 넣으면 빈 글 · 한글도 글자 단위.
        {
            let mut a = Adapt(&mut rec);
            // RecordCtx 글자 폭 7 → "abcdefgh" = 56.
            assert!(matches!(
                ellipsize_end(&mut a, "abcdefgh", 56),
                std::borrow::Cow::Borrowed("abcdefgh")
            ));
            let cut = ellipsize_end(&mut a, "abcdefgh", 55);
            assert!(cut.ends_with('…') && cut.len() < "abcdefgh…".len(), "{cut}");
            assert!(a.text_width(&cut) <= 55, "{cut}");
            let longer = ellipsize_end(&mut a, "abcdefgh", 55).chars().count();
            let shorter = ellipsize_end(&mut a, "abcdefgh", 30).chars().count();
            assert!(shorter < longer);
            let w1 = a.text_width("…");
            assert_eq!(ellipsize_end(&mut a, "abcdefgh", w1), "…");
            assert_eq!(ellipsize_end(&mut a, "abcdefgh", w1 - 1), "");
            assert_eq!(ellipsize_end(&mut a, "", 0), "");
            let ko = ellipsize_end(&mut a, "가나다라마바사", 30);
            assert!(ko.ends_with('…') && a.text_width(&ko) <= 30, "{ko}");
        }
        // 글리프는 셀 가운데(가로) — RecordCtx 글자 폭 7.
        let g = rec.texts.iter().find(|t| t.3 == "▶").expect("glyph text");
        assert_eq!(g.0, (20 - 7) / 2);
    }
}

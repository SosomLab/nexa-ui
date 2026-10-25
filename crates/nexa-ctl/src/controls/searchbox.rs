//! 검색 상자(10-26 · nexa-sql `filterbar.rs`/`findbar.rs::FindBtn` 이식 · 첫 공용 소비자 = nexa-dir3 빠른 필터): 텍스트박스 + 틀 **안쪽**
//! 토글 셋 **Aa · ab · .\***(Material 아이콘)이 **한 컨트롤**처럼 보이고 움직인다 —
//! - 틀 하나(둥근 6 px) · 텍스트박스의 오른쪽 테두리는 틀 배경으로 덮는다 · hover/포커스 덧칠과 포커스 테두리는 **틀 전체**(토글 자리 포함)
//! - 진행 표시 링(혜성)은 텍스트박스 속성([`TextBox::set_busy`])이되 **틀 둘레**에 그린다(`set_busy_host_painted`)
//! - 정규식 오류([`SearchBox::set_error`]) = 틀 테두리 danger
//! - 토글 = On(accent 40 % + accent 테두리) · hover 페이드 · 눌림 · 툴팁([`Tooltip`] · 호스트가 글을 준다)
//!
//! **판정(매칭)은 호스트 몫** — 이 컨트롤은 글과 토글 상태만 든다(nexa-sql = `Matcher` · dir3 = ndir-ops `namefilter`).

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;

use super::busy::{BusyRing, BusyState};
use super::ctxmenu::MenuIcon;
use super::svgicon::material;
use super::textbox::TextBox;
use super::tooltip::Tooltip;
use super::{Control, ControlBase};
use crate::draw::DrawCtx;
use crate::event::InputEvent;
use crate::geom::{Point, Rect};
use crate::theme::{Color, IconImage, Theme};
use crate::tokens::{hover_color, Fade, FadeSpeed};
use crate::widget::{Invalidations, Widget};

/// 틀 안쪽 토글 종류(순서 = 왼쪽 → 오른쪽).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SearchToggle {
    /// 대소문자 구분(Aa).
    Case,
    /// 단어 단위(ab).
    Word,
    /// 정규식(.*).
    Regex,
}

impl SearchToggle {
    fn index(self) -> usize {
        match self {
            SearchToggle::Case => 0,
            SearchToggle::Word => 1,
            SearchToggle::Regex => 2,
        }
    }
}

/// [`SearchBox::handle`]의 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchEvent {
    /// 아무 변화 없음(사건이 이 컨트롤과 무관하거나 모양만 바뀜).
    #[default]
    None,
    /// 글이 바뀌었다 — 호스트는 다시 거른다.
    Changed,
    /// 토글이 눌려 뒤집혔다(새 상태는 [`SearchBox::is_on`]) — 호스트는 다시 거르고 설정에 적는다.
    Toggled(SearchToggle),
    /// 텍스트박스가 사건을 먹었다(캐럿 · 선택 · 팝업) — 다시 그리기만.
    Consumed,
}

/// 아이콘 토글 하나(nexa-sql `FindBtn` 이식 · 마우스만 — 키보드 포커스 점선은 없다).
struct Toggle {
    kind: SearchToggle,
    rect: Rect,
    icon: fn() -> MenuIcon,
    built: OnceCell<MenuIcon>,
    tint: RefCell<Option<(Color, Rc<IconImage>)>>,
    checked: bool,
    hover: Fade,
    hover_on: bool,
    pressed: bool,
    clicked: bool,
}

impl Toggle {
    fn new(kind: SearchToggle, icon: fn() -> MenuIcon) -> Self {
        Toggle {
            kind,
            rect: Rect::default(),
            icon,
            built: OnceCell::new(),
            tint: RefCell::new(None),
            checked: false,
            // 1초에 걸쳐 밝아진다(nexa-sql 09-16) = `FadeSpeed::Slow`.
            hover: Fade::at(FadeSpeed::Slow),
            hover_on: false,
            pressed: false,
            clicked: false,
        }
    }

    fn tinted(&self, fg: Color) -> Rc<IconImage> {
        if let Some((c, img)) = self.tint.borrow().as_ref() {
            if *c == fg {
                return img.clone();
            }
        }
        let (r, g, b) = fg.rgb();
        let icon = self.built.get_or_init(self.icon);
        let img = Rc::new(IconImage::from_alpha_tinted(
            icon.w,
            icon.h,
            &icon.alpha,
            (r, g, b),
        ));
        *self.tint.borrow_mut() = Some((fg, img.clone()));
        img
    }

    fn set_hover(&mut self, on: bool) {
        if on != self.hover_on {
            self.hover_on = on;
            self.hover.set(on);
        }
    }

    fn on_event(&mut self, ev: &InputEvent) {
        match *ev {
            InputEvent::MouseMove { x, y } => {
                let inside = self.rect.contains(Point { x, y });
                self.set_hover(inside);
            }
            InputEvent::MouseDown { x, y, .. } => {
                self.pressed = self.rect.contains(Point { x, y });
            }
            InputEvent::MouseUp { x, y } => {
                if self.pressed && self.rect.contains(Point { x, y }) {
                    self.clicked = true;
                }
                self.pressed = false;
            }
            _ => {}
        }
    }

    fn paint(&self, dc: &mut dyn DrawCtx, th: &Theme, s: f32) {
        let b = self.rect;
        if b.w <= 0 || b.h <= 0 {
            return;
        }
        let r = (3.0 * s).round() as i32;
        // On = accent 40% + accent 테두리(VS Code inputOption.activeBackground/activeBorder).
        if self.checked {
            dc.fill_round_rect_alpha(b, r, th.accent, 0.4);
            dc.stroke_round_rect(b, r, th.accent, 1.0);
        }
        // hover = 회색 오버레이가 천천히 진해진다(On 위에도 얹혀 "켜진 채 hover"가 보인다).
        let hv = self.hover.value();
        if hv > 0.0 {
            let (hc, ha) = hover_color(th);
            dc.fill_round_rect_alpha(b, r, hc, (ha * 1.2).min(1.0) * hv);
        }
        if self.pressed {
            dc.fill_round_rect_alpha(b, r, th.text, 0.12);
        }
        // 아이콘 16px(스케일) · 글자색 틴트.
        let img = self.tinted(th.text);
        let icon = (16.0 * s).round().max(1.0) as i32;
        let dst = Rect::new(b.x + (b.w - icon) / 2, b.y + (b.h - icon) / 2, icon, icon);
        dc.image_scaled(dst, &img, b);
    }
}

/// 텍스트박스 + 안쪽 토글 셋 = 한 컨트롤(모듈 문서 참고).
#[allow(missing_debug_implementations)]
pub struct SearchBox {
    base: ControlBase,
    tb: TextBox,
    /// 틀 = 컨트롤 경계(텍스트박스 + 토글).
    frame: Rect,
    toggles: [Toggle; 3],
    /// 토글 툴팁 글(호스트가 i18n으로 준다 · 빈 글 = 툴팁 없음).
    tips: [String; 3],
    tooltip: Tooltip,
    clamp_x: (i32, i32),
    error: bool,
    scale: f32,
    placeholder: String,
    /// [`Widget::on_event`] 경로로 들어온 사건의 결과(호스트가 [`SearchBox::take_event`]로 거둔다).
    pending: SearchEvent,
}

impl SearchBox {
    /// 새 검색 상자(× 지우기 포함 · 포커스 링은 틀이 그린다 · 혜성은 틀 둘레).
    #[must_use]
    pub fn new(placeholder: impl Into<String>) -> Self {
        let placeholder = placeholder.into();
        let mut tb = TextBox::new(placeholder.clone()).with_clearable();
        tb.set_focus_ring(false);
        tb.set_busy_host_painted(true);
        SearchBox {
            base: ControlBase::default(),
            tb,
            frame: Rect::default(),
            toggles: [
                Toggle::new(SearchToggle::Case, material::match_case),
                Toggle::new(SearchToggle::Word, material::match_word),
                Toggle::new(SearchToggle::Regex, material::regex),
            ],
            tips: Default::default(),
            tooltip: Tooltip::new(),
            clamp_x: (0, i32::MAX / 2),
            error: false,
            scale: 1.0,
            placeholder,
            pending: SearchEvent::None,
        }
    }

    /// 자리 표시 글(언어 전환).
    pub fn set_placeholder(&mut self, placeholder: impl Into<String>) {
        let p = placeholder.into();
        self.tb.set_placeholder(p.clone());
        self.placeholder = p;
    }

    /// 토글 툴팁 글(대소문자 · 단어 · 정규식 순 · 호스트 i18n).
    pub fn set_tips(
        &mut self,
        case: impl Into<String>,
        word: impl Into<String>,
        regex: impl Into<String>,
    ) {
        self.tips = [case.into(), word.into(), regex.into()];
    }

    /// 툴팁 지연(ms · 호스트 설정).
    pub fn set_tooltip_delay_ms(&mut self, ms: u64) {
        self.tooltip.set_delay_ms(ms);
    }

    /// 툴팁 가로 클램프(창 안에 머물게 · 기본 = 제한 없음).
    pub fn set_clamp_x(&mut self, clamp: (i32, i32)) {
        self.clamp_x = clamp;
    }

    /// 안쪽 텍스트박스(세부 설정 · 되돌리기 등).
    #[must_use]
    pub fn textbox(&self) -> &TextBox {
        &self.tb
    }

    /// 안쪽 텍스트박스(가변).
    pub fn textbox_mut(&mut self) -> &mut TextBox {
        &mut self.tb
    }

    /// 글(조합 중 글자 제외 · 확정분).
    #[must_use]
    pub fn text(&self) -> String {
        self.tb.text()
    }

    /// 글을 바꾼다(캐럿 끝).
    pub fn set_text(&mut self, text: &str) {
        self.tb.set_text(text);
    }

    /// 토글 상태.
    #[must_use]
    pub fn is_on(&self, k: SearchToggle) -> bool {
        self.toggles[k.index()].checked
    }

    /// 토글 상태를 밖에서(설정 복원 · 시험) — 사건을 내지 않는다.
    pub fn set_checked(&mut self, k: SearchToggle, on: bool) {
        self.toggles[k.index()].checked = on;
    }

    /// 정규식 오류 표시(틀 테두리 danger).
    pub fn set_error(&mut self, on: bool) {
        self.error = on;
    }

    /// 정규식 오류 표시 중인가.
    #[must_use]
    pub fn error(&self) -> bool {
        self.error
    }

    /// 진행 표시 링 상태(혜성 · Idle/Running/Done).
    pub fn set_busy(&mut self, st: BusyState) {
        self.tb.set_busy(st);
    }

    /// 진행 표시 링 상태.
    #[must_use]
    pub fn busy_state(&self) -> BusyState {
        self.tb.busy_state()
    }

    /// 진행 표시 링(애니메이션 여부 등).
    #[must_use]
    pub fn busy_ring(&self) -> &BusyRing {
        self.tb.busy_ring()
    }

    /// 틀(= 경계).
    #[must_use]
    pub fn frame(&self) -> Rect {
        self.frame
    }

    /// 텍스트박스 팝업(편집 메뉴)이 열려 있는가.
    #[must_use]
    pub fn popup_open(&self) -> bool {
        self.tb.popup_open()
    }

    /// 틱 — 텍스트박스(혜성 · hover) · 토글 페이드 · 툴팁 지연. 모양이 바뀌었으면 true.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        let mut changed = self.tb.tick(now_ms);
        for t in &mut self.toggles {
            changed |= t.hover.tick(now_ms);
        }
        changed |= self.tooltip.tick();
        changed
    }

    /// 아직 움직이는 것이 있는가(호스트가 다음 틱을 예약).
    #[must_use]
    pub fn wants_tick(&self) -> bool {
        self.tb.busy_ring().animating()
            || self.tooltip.pending()
            || self.toggles.iter().any(|t| t.hover.is_animating())
    }

    /// [`Widget::on_event`]로 들어온 마지막 사건의 결과(한 번만).
    pub fn take_event(&mut self) -> SearchEvent {
        std::mem::take(&mut self.pending)
    }

    /// 사건 — 토글은 언제나 · 텍스트박스는 상자 안 클릭/우클릭 · 포커스일 때의 키/글자/휠 · 이동/놓임은 늘(드래그 선택).
    /// 토글 클릭 = 뒤집고 [`SearchEvent::Toggled`](nexa-sql처럼 상자 포커스는 풀린다).
    pub fn handle(&mut self, ev: &InputEvent, inv: &mut Invalidations) -> SearchEvent {
        for t in &mut self.toggles {
            t.on_event(ev);
        }
        let mut to_tb = false;
        match *ev {
            InputEvent::MouseDown { x, y, .. } => {
                let p = Point { x, y };
                if self.toggles.iter().any(|t| t.rect.contains(p)) {
                    if self.tb.is_focused() {
                        self.tb.set_focused(false);
                        inv.push(self.frame);
                    }
                } else if self.tb.bounds().contains(p) {
                    if !self.tb.is_focused() {
                        self.tb.set_focused(true);
                        inv.push(self.frame);
                    }
                    to_tb = true;
                } else if self.tb.popup_open() {
                    to_tb = true;
                }
            }
            InputEvent::RightDown { x, y } => {
                if self.tb.bounds().contains(Point { x, y }) {
                    self.tb.set_focused(true);
                    to_tb = true;
                } else {
                    to_tb = self.tb.popup_open();
                }
            }
            InputEvent::MouseMove { x, y } => {
                let p = Point { x, y };
                let tip = self
                    .toggles
                    .iter()
                    .find(|t| t.rect.contains(p))
                    .map(|t| (t.rect, self.tips[t.kind.index()].clone()));
                if self.tooltip.hover(tip) {
                    inv.push(self.frame);
                }
                to_tb = true;
            }
            InputEvent::MouseUp { .. } => to_tb = true,
            InputEvent::Wheel { .. } | InputEvent::HWheel { .. } => {
                to_tb = self.tb.is_focused() || self.tb.popup_open();
            }
            _ => to_tb = self.tb.is_focused(),
        }
        if let Some(t) = self.toggles.iter_mut().find(|t| t.clicked) {
            t.clicked = false;
            t.checked = !t.checked;
            let k = t.kind;
            inv.push(self.frame);
            return SearchEvent::Toggled(k);
        }
        if !to_tb {
            return SearchEvent::None;
        }
        let before = self.tb.text();
        self.tb.on_event(ev, inv);
        if self.tb.text() != before {
            SearchEvent::Changed
        } else {
            SearchEvent::Consumed
        }
    }

    /// 팝업 층 — 텍스트박스 편집 메뉴 + 토글 툴팁(지연 뒤).
    pub fn paint_popup(&self, dc: &mut dyn DrawCtx, th: &Theme) {
        self.tb.paint_popup(dc, th);
        self.tooltip.paint(dc, th, self.clamp_x, self.scale);
    }

    fn layout(&mut self, inv: &mut Invalidations) {
        let area = self.frame;
        let s = self.scale;
        let px = |v: f32| (v * s).round() as i32;
        let ih = area.h;
        // 안쪽 토글 = 정사각 20(스케일) · 간격 2 · 세로 중앙 · 오른쪽 끝에서 거꾸로(Aa · ab · .* 순이 왼쪽→오른쪽).
        let tg = px(20.0).min(ih.max(1));
        let tgap = px(2.0);
        let mut tx = area.right() - px(2.0) - tg;
        for t in self.toggles.iter_mut().rev() {
            t.rect = Rect::new(tx, area.y + (ih - tg) / 2, tg, tg);
            tx -= tg + tgap;
        }
        self.tb.set_scale(s);
        self.tb.set_bounds(
            Rect::new(area.x, area.y, (tx + tg + tgap - area.x).max(px(40.0)), ih),
            inv,
        );
        inv.push(area);
    }
}

impl Control for SearchBox {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
    /// 포커스 = 안쪽 텍스트박스의 것.
    fn set_focused(&mut self, on: bool) {
        self.base.focused = on;
        self.tb.set_focused(on);
    }
    fn is_focused(&self) -> bool {
        self.tb.is_focused()
    }
    fn set_scale(&mut self, scale: f32) {
        self.base.scale = scale;
        self.scale = scale;
        self.tb.set_scale(scale);
        if self.frame.w > 0 {
            let mut inv = Invalidations::default();
            self.layout(&mut inv);
        }
    }
}

impl Widget for SearchBox {
    fn bounds(&self) -> Rect {
        self.frame
    }

    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        self.frame = bounds;
        self.base.bounds = bounds;
        self.layout(inv);
    }

    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        let e = self.handle(ev, inv);
        if e != SearchEvent::None {
            self.pending = e;
        }
    }

    fn paint(&self, dc: &mut dyn DrawCtx, th: &Theme) {
        let s = self.scale;
        let px = |v: f32| (v * s).round() as i32;
        let fb = self.frame;
        if fb.w <= 0 || fb.h <= 0 {
            return;
        }
        let r = px(6.0);
        dc.fill_round_rect(fb, r, th.field_bg);
        // 상자의 hover 덧칠을 **틀 전체**에(토글 자리 포함) — 상자만 회색이 되고 토글 쪽은 흰색이라 갈라져 보이지 않게.
        let hov = if self.base.enabled {
            self.tb.hover_overlay_alpha()
        } else {
            0.0
        };
        if hov > 0.0 {
            dc.fill_round_rect_alpha(fb, r, th.text, hov);
        }
        let bg = th.field_bg.lerp(th.text, hov);
        dc.stroke_round_rect(fb, r, if self.error { th.danger } else { th.border }, 1.0);
        if !self.base.enabled {
            let ty = dc.text_center_y(fb.y, fb.h);
            dc.text(fb.x + px(6.0), ty, fb, &self.placeholder, th.text_dim);
            return;
        }
        self.tb.paint(dc, th);
        let tbb = self.tb.bounds();
        if tbb.w > 0 {
            // 텍스트박스 오른쪽 테두리를 틀 배경으로 덮는다 — 곧은 부분만 덮으면 반지름 안의 모서리 호가 세로 선처럼 남으므로
            // 위·아래 모서리 사각형도 덮는다(틀의 위·아래 테두리 줄은 남기고 그 안쪽만).
            dc.fill_rect(
                Rect::new(tbb.right() - 2, tbb.y + 1, 3, (tbb.h - 2).max(0)),
                bg,
            );
            let cw = r + 3;
            dc.fill_rect(Rect::new(tbb.right() - r - 1, tbb.y + 1, cw, r), bg);
            dc.fill_rect(
                Rect::new(tbb.right() - r - 1, tbb.bottom() - r - 1, cw, r),
                bg,
            );
        }
        if self.tb.is_focused() {
            dc.stroke_round_rect(fb, r, th.accent, 1.0);
        }
        // 진행 표시 링 = 상자 속성 · 그리기는 틀 둘레(상자는 `set_busy_host_painted`로 생략).
        self.tb.busy_ring().paint(dc, th, fb, r, s);
        for t in &self.toggles {
            t.paint(dc, th, s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::RecordCtx;

    fn down(x: i32, y: i32) -> InputEvent {
        InputEvent::MouseDown {
            x,
            y,
            shift: false,
            primary: false,
        }
    }

    /// 배치: 토글 셋이 틀 안 오른쪽에 20px 정사각으로 · 텍스트박스는 그 왼쪽 · 틀 = 경계.
    #[test]
    fn layout_puts_toggles_inside_frame_right() {
        let mut sb = SearchBox::new("find");
        let mut inv = Invalidations::default();
        sb.set_bounds(Rect::new(10, 5, 300, 25), &mut inv);
        assert_eq!(sb.bounds(), Rect::new(10, 5, 300, 25));
        let rects: Vec<Rect> = sb.toggles.iter().map(|t| t.rect).collect();
        assert!(
            rects.windows(2).all(|w| w[0].right() < w[1].x),
            "왼쪽→오른쪽 {rects:?}"
        );
        assert_eq!(rects[2].right(), 10 + 300 - 2, "맨 오른쪽 토글은 틀 안 2px");
        assert!(rects
            .iter()
            .all(|r| r.w == 20 && r.h == 20 && r.y >= 5 && r.bottom() <= 30));
        assert!(
            sb.textbox().bounds().right() <= rects[0].x,
            "상자는 토글 왼쪽"
        );
    }

    /// 토글 클릭(뗄 때) = 뒤집힘 + `Toggled` · 상자 포커스는 풀림 · 상자 클릭 = 포커스 · 글자 = `Changed` · 설정 복원은 사건 없음.
    #[test]
    fn toggle_click_flips_and_typing_reports_changed() {
        let mut sb = SearchBox::new("find");
        let mut inv = Invalidations::default();
        sb.set_bounds(Rect::new(0, 0, 300, 25), &mut inv);
        // 상자 안 클릭 → 포커스.
        assert_eq!(sb.handle(&down(20, 12), &mut inv), SearchEvent::Consumed);
        assert!(sb.is_focused());
        assert_eq!(
            sb.handle(&InputEvent::Char { c: 'a', now_ms: 0 }, &mut inv),
            SearchEvent::Changed
        );
        assert_eq!(sb.text(), "a");
        // 단어 토글 클릭.
        let w = sb.toggles[1].rect;
        let (cx, cy) = (w.x + w.w / 2, w.y + w.h / 2);
        assert_eq!(sb.handle(&down(cx, cy), &mut inv), SearchEvent::None);
        assert_eq!(
            sb.handle(&InputEvent::MouseUp { x: cx, y: cy }, &mut inv),
            SearchEvent::Toggled(SearchToggle::Word)
        );
        assert!(sb.is_on(SearchToggle::Word) && !sb.is_on(SearchToggle::Case));
        assert!(
            !sb.is_focused(),
            "토글을 누르면 상자 포커스는 풀린다(nexa-sql)"
        );
        // 밖에서 복원 = 사건 없음.
        sb.set_checked(SearchToggle::Regex, true);
        assert!(sb.is_on(SearchToggle::Regex));
        assert_eq!(sb.take_event(), SearchEvent::None);
        // Widget 경로 → take_event.
        Widget::on_event(&mut sb, &down(cx, cy), &mut inv);
        Widget::on_event(&mut sb, &InputEvent::MouseUp { x: cx, y: cy }, &mut inv);
        assert_eq!(sb.take_event(), SearchEvent::Toggled(SearchToggle::Word));
        assert!(!sb.is_on(SearchToggle::Word));
    }

    /// 그리기: 틀 전체에 둥근 바탕 · 오류 = danger 테두리 · 혜성 Running이면 틀 둘레에 링 · 토글 아이콘이 그려진다(기록 ctx).
    #[test]
    fn paint_frame_error_and_busy_ring_over_whole_frame() {
        let th = Theme::dark();
        let mut sb = SearchBox::new("find");
        let mut inv = Invalidations::default();
        sb.set_bounds(Rect::new(0, 0, 300, 25), &mut inv);
        let mut rec = RecordCtx::with_surface(320, 40);
        sb.paint(&mut rec, &th);
        assert_eq!(rec.images.len(), 3, "토글 아이콘 셋");
        sb.set_error(true);
        sb.set_busy(BusyState::Running);
        let mut rec = RecordCtx::with_surface(320, 40);
        sb.paint(&mut rec, &th);
        assert!(
            rec.strokes
                .iter()
                .any(|(r, _, c)| *r == Rect::new(0, 0, 300, 25) && *c == th.danger),
            "오류 = 틀 전체 danger 테두리"
        );
        assert!(sb.busy_ring().animating() && sb.wants_tick());
    }
}

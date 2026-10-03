//! 하단 도크 — **정보 뷰**(M4-1, 원본 BottomDockView의 Info 종류·docs/20 하단 도크 대원칙:
//! 듀얼=좌↔좌·우↔우 — 각 패널 하단에 1개). 미리보기(M4-2)·터미널(M4-3)은 종류 스왑으로 확장.
//! 플랫폼 중립 — 텍스트 라인 렌더 + 상단 경계선.

use crate::draw::DrawCtx;
use crate::event::{InputEvent, WheelAccum};
use crate::fastscroll::FastScroller;
use crate::geom::{Point, Rect};
use crate::overlaybar::{Axis, AxisGeom, BarHit, OverlayBars};
use crate::theme::Theme;
use crate::widget::{Invalidations, Widget};
use nexa_ctl::DrawCtx as CtlDrawCtx;

/// 하단 도크 — 호스트가 [`set_lines`](InfoDock::set_lines)로 내용을 공급한다
/// (원본 InfoText/PreviewPath 대응). 종류 스트립(정보|미리보기 — M4-2)은 클릭 전환,
/// 내용 해석은 호스트 몫(위젯은 종류 인덱스만 보유 — 원본 Kind 스왑).
pub struct InfoDock {
    bounds: Rect,
    row_h: i32,
    pad_x: i32,
    /// 종류 라벨들(예: ["정보", "미리보기"]) — 스트립에 나열, 클릭으로 전환.
    kinds: Vec<String>,
    active: usize,
    /// 스트립 라벨 x 범위 캐시(히트 테스트 — 텍스트 측정은 paint에서만).
    ranges: std::cell::RefCell<Vec<(i32, i32)>>,
    /// 종류 스트립 hover(127차 · nexa-dir3 사용자 10-03 "정보/미리보기/터미널에도 탭처럼 호버링"): 마우스가 올라간 칸 —
    /// 종류 인덱스 · `kinds.len()` = → 버튼. 활성 칸은 이미 강조돼 있어 hover 표시는 그 밖의 칸에만 한다.
    strip_hover: Option<usize>,
    lines: Vec<String>,
    /// 이미지 미리보기 경로(Some = 라인 대신 이미지 — M4-2).
    image: Option<String>,
    /// 터미널 "폴더로 이동"(→) 클릭 통지(QA 07-14 — 원본 '터미널에서 열기'). 1회성.
    pending_goto: bool,
    /// → 버튼 x 범위 캐시(paint가 채움 — 마지막 종류[터미널] 옆에 부착).
    goto_range: std::cell::Cell<(i32, i32)>,
    /// 호스트 패널 포커스 — 비활성 패널은 강조색(accent·sel_bg)을 무채색으로 낮춘다.
    focused: bool,
    /// 내용 첫 가시 라인(세로 스크롤 — 미리보기 07-26. 내용 교체 시 0).
    scroll: usize,
    /// 부분 줄 픽셀 오프셋(0..row_h — 10-02 트랙패드 픽셀 스크롤).
    scroll_frac: i32,
    /// 트랙패드 픽셀 누적(세로).
    wheel_px: WheelAccum,
    /// 우상단 "크게"(↗) 오버레이 버튼 표시(호스트가 미리보기 종류일 때 켬 — 07-26).
    popout_on: bool,
    /// ↗ 클릭 통지(1회성 — 호스트가 독립 미리보기 창을 연다).
    pending_popout: bool,
    /// ↗ 버튼 히트 존 캐시(paint가 채움).
    popout_range: std::cell::Cell<Rect>,
    /// ↗ hover(색 입힘 — X-27 sel_bg 토큰)·pressed(누름 효과 — accent 블렌드 +
    /// 아이콘 1px 오프셋, MouseUp이 버튼 안이면 발화 — 07-26 이미지 버튼 개편).
    popout_hover: bool,
    popout_pressed: bool,
    /// 내용 텍스트 선택(드래그 — QA 07-15 라인 → **문자 단위**로 보완 07-20:
    /// Info/Preview 영역 선택 복사). (앵커, 현재) = (라인, 문자 경계).
    sel: Option<((usize, usize), (usize, usize))>,
    /// 선택 드래그 중(MouseDown 시작 → MouseUp 종료, 선택은 유지).
    sel_drag: bool,
    /// paint 캐시: 그려진 라인별 문자 경계 x 오프셋(원점 = bounds.x+pad_x 접두 폭 —
    /// edit.rs 캐시 규약). 내용/지표/크기 변경 시 비움(paint가 재계산). 폭 초과
    /// 문자는 측정 중단(클릭 가능 영역 상한 = 보이는 폭).
    offsets: std::cell::RefCell<Vec<Vec<i32>>>,
    /// 가로 스크롤 오프셋(px — 10-02. 긴 경로·미리보기 행. 내용 교체 시 0).
    scroll_x: i32,
    /// paint 캐시: 가시 행 중 최대 텍스트 폭(px — 가로 스크롤 상한·가로 바 기하).
    content_w: std::cell::Cell<i32>,
    /// 오버레이 스크롤바 세로·가로(10-02 — rows.rs 09-04 규약의 공용 모듈).
    bars: OverlayBars,
    /// 휠 분수 누적(10-02 QA — 트랙패드는 노치(120) 미만 delta를 잘게 보낸다. 정수 나눗셈은
    /// 0줄로 버려 "천천히 움직이면 스크롤 안 됨"이 됐다). 세로 = 줄, 가로 = px.
    wheel: WheelAccum,
    hwheel: WheelAccum,
    /// 고속 스크롤(10-02 — 휠 노치 연타 배수 + ×N 배지).
    fast: FastScroller,
    /// 내용 식별 키(10-02 QA): 같은 키로 내용만 바뀌면(상세 도착·액세스 시각 갱신 등)
    /// 스크롤·선택을 **유지**한다 — 종전엔 모든 변경이 스크롤을 0으로 리셋해 스크롤 중
    /// "끊기거나 튀는" 증상이 났다.
    content_key: String,
}

/// 인라인 이미지 마커(07-26 다이어그램 — 이미지 수준 렌더): 라인
/// `"\u{1}img|<경로>"` 가 이미지 시작이고, 후속 `"\u{1}pad"` 라인들이 표시
/// 영역(행 수)을 예약한다. 렌더 = draw_image(비율 유지)·선택/복사 = 제외.
pub const IMG_MARKER: &str = "\u{1}img|";
/// 이미지 영역 예약 행(마커 다음 연속 — 내용 없음).
pub const IMG_PAD: &str = "\u{1}pad";

/// 클릭 x(원점 상대) → 최근접 문자 경계 인덱스(edit.rs index_at 규약).
fn nearest_boundary(offs: &[i32], rel: i32) -> usize {
    let mut best = 0usize;
    let mut bd = i32::MAX;
    for (i, o) in offs.iter().enumerate() {
        let d = (o - rel).abs();
        if d < bd {
            bd = d;
            best = i;
        }
    }
    best
}

impl std::fmt::Debug for InfoDock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InfoDock")
            .field("bounds", &self.bounds)
            .field("active_kind", &self.active_kind())
            .finish_non_exhaustive()
    }
}

impl InfoDock {
    pub fn new(title: impl Into<String>, row_h: i32, pad_x: i32) -> Self {
        InfoDock {
            bounds: Rect::default(),
            row_h: row_h.max(1),
            pad_x,
            kinds: vec![title.into()],
            active: 0,
            ranges: std::cell::RefCell::new(Vec::new()),
            strip_hover: None,
            lines: Vec::new(),
            image: None,
            pending_goto: false,
            goto_range: std::cell::Cell::new((0, 0)),
            focused: false,
            scroll: 0,
            scroll_frac: 0,
            wheel_px: WheelAccum::default(),
            popout_on: false,
            pending_popout: false,
            popout_range: std::cell::Cell::new(Rect::default()),
            popout_hover: false,
            popout_pressed: false,
            sel: None,
            sel_drag: false,
            offsets: std::cell::RefCell::new(Vec::new()),
            scroll_x: 0,
            content_w: std::cell::Cell::new(0),
            bars: OverlayBars::default(),
            wheel: WheelAccum::default(),
            hwheel: WheelAccum::default(),
            fast: FastScroller::default(),
            content_key: String::new(),
        }
    }

    /// 가시 내용 행 수(스트립 제외 — 스크롤 상한 산정).
    fn visible_rows(&self) -> usize {
        let strip_h = 1 + self.row_h.min((self.bounds.h - 1).max(0));
        ((self.bounds.h - strip_h).max(0) / self.row_h.max(1)) as usize
    }

    /// 첫 가시 줄의 y(부분 오프셋만큼 내용 상단보다 위 — 10-02 픽셀 스크롤).
    fn line_origin(&self) -> i32 {
        self.content_rect().y - self.scroll_frac
    }

    /// 픽셀 단위 세로 스크롤(트랙패드).
    fn scroll_by_px(&mut self, dy: i32, inv: &mut Invalidations) -> bool {
        let rh = self.row_h.max(1) as i64;
        let max_px = self.max_scroll() as i64 * rh;
        let pos = (self.scroll as i64 * rh + self.scroll_frac as i64 + dy as i64).clamp(0, max_px);
        let (row, frac) = ((pos / rh) as usize, (pos % rh) as i32);
        if row != self.scroll || frac != self.scroll_frac {
            self.scroll = row;
            self.scroll_frac = frac;
            self.offsets.borrow_mut().clear();
            inv.push(self.bounds);
            self.bars.flash(Axis::V, self.content_rect(), inv);
            return true;
        }
        false
    }

    /// 스크롤 상한(마지막 페이지가 화면을 채우는 지점).
    fn max_scroll(&self) -> usize {
        self.lines.len().saturating_sub(self.visible_rows().max(1))
    }

    /// 세로 스크롤 이동(휠·드래그 자동 스크롤 — 07-26). 변경 시 오프셋 캐시 무효.
    fn scroll_to(&mut self, to: usize, inv: &mut Invalidations) -> bool {
        let to = to.min(self.max_scroll());
        if to != self.scroll || self.scroll_frac != 0 {
            self.scroll = to;
            self.scroll_frac = 0; // 줄 단위 이동 = 부분 오프셋 스냅
            self.offsets.borrow_mut().clear();
            inv.push(self.bounds);
            self.bars.flash(Axis::V, self.content_rect(), inv);
            return true;
        }
        false
    }

    /// 가로 스크롤 상한(px) — paint가 측정한 가시 행 최대 폭 기준(측정 전 = 0).
    fn max_scroll_x(&self) -> i32 {
        (self.content_w.get() + self.pad_x * 2 - self.bounds.w).max(0)
    }

    /// 가로 스크롤 이동(가로 휠·바 드래그·드래그 자동 스크롤 — 10-02). 변경 시 오프셋
    /// 캐시 무효(측정 상한이 보이는 폭 + 오프셋이라 재측정).
    fn hscroll_to(&mut self, to: i32, inv: &mut Invalidations) -> bool {
        let to = to.clamp(0, self.max_scroll_x());
        if to != self.scroll_x {
            self.scroll_x = to;
            self.offsets.borrow_mut().clear();
            inv.push(self.bounds);
            self.bars.flash(Axis::H, self.content_rect(), inv);
            return true;
        }
        false
    }

    /// 두 축의 바 기하([세로, 가로]) — 뷰포트 = 내용 영역(스트립 제외).
    fn geoms(&self) -> [AxisGeom; 2] {
        let view = self.content_rect();
        let text = self.image.is_none();
        [
            AxisGeom {
                view,
                content: if text { self.lines.len() as i64 } else { 0 },
                visible: self.visible_rows() as i64,
                offset: self.scroll as i64,
            },
            AxisGeom {
                view,
                content: if text {
                    (self.content_w.get() + self.pad_x * 2) as i64
                } else {
                    0
                },
                visible: self.bounds.w as i64,
                offset: self.scroll_x as i64,
            },
        ]
    }

    /// 바 적용 — 드래그 오프셋·트랙 페이지 이동을 축별 스크롤로 변환.
    fn apply_bar(&mut self, axis: Axis, hit: BarHit, inv: &mut Invalidations) {
        match (axis, hit) {
            (Axis::V, BarHit::PageBack) => {
                let page = self.visible_rows().saturating_sub(1).max(1);
                let _ = self.scroll_to(self.scroll.saturating_sub(page), inv);
            }
            (Axis::V, BarHit::PageFwd) => {
                let page = self.visible_rows().saturating_sub(1).max(1);
                let _ = self.scroll_to(self.scroll + page, inv);
            }
            (Axis::H, BarHit::PageBack) => {
                let _ = self.hscroll_to(self.scroll_x - self.bounds.w.max(1), inv);
            }
            (Axis::H, BarHit::PageFwd) => {
                let _ = self.hscroll_to(self.scroll_x + self.bounds.w.max(1), inv);
            }
            (_, BarHit::Drag) => {}
        }
    }

    /// 주기 틱(호스트 TIMER_WIDGET_TICK) — 오버레이 바 유지/페이드.
    pub fn tick(&mut self, inv: &mut Invalidations) {
        self.bars.tick(self.content_rect(), inv);
        self.fast.tick(self.content_rect(), inv); // 속도 배지(10-02)
    }

    /// 내용 라인 y→**절대** 인덱스(스크롤 반영. 이미지·터미널 종류는 None).
    fn line_at(&self, x: i32, y: i32) -> Option<usize> {
        if self.image.is_some() || self.lines.is_empty() {
            return None;
        }
        let top = self.bounds.y + 1 + self.row_h.min((self.bounds.h - 1).max(0));
        if !self.bounds.contains(Point { x, y }) || y < top {
            return None;
        }
        let i = self.scroll + ((y - self.line_origin()) / self.row_h) as usize;
        (i < self.lines.len()).then_some(i)
    }

    /// 선택 텍스트(문자 단위 영역 — Ctrl+C 복사용, QA 07-15 → 07-20 보완).
    /// 선택 없음·빈 범위 = `None`.
    pub fn selected_text(&self) -> Option<String> {
        let (a, c) = self.sel?;
        let (lo, hi) = if a <= c { (a, c) } else { (c, a) };
        if lo == hi || lo.0 >= self.lines.len() {
            return None;
        }
        // 이미지 마커/패드 라인은 복사 대상에서 제외(07-26 — 제어 문자 유출 방지)
        let chars_of = |l: usize| {
            let s = &self.lines[l];
            if s.starts_with('\u{1}') {
                Vec::new()
            } else {
                s.chars().collect::<Vec<char>>()
            }
        };
        let (ll, lc) = lo;
        let (hl, hc) = (hi.0.min(self.lines.len() - 1), hi.1);
        if ll == hl {
            let cs = chars_of(ll);
            let (a, b) = (lc.min(cs.len()), hc.min(cs.len()));
            return (b > a).then(|| cs[a..b].iter().collect());
        }
        let mut parts = Vec::with_capacity(hl - ll + 1);
        let f = chars_of(ll);
        parts.push(f[lc.min(f.len())..].iter().collect::<String>());
        for l in ll + 1..hl {
            let s = &self.lines[l];
            parts.push(if s.starts_with('\u{1}') {
                String::new()
            } else {
                s.clone()
            });
        }
        let t = chars_of(hl);
        parts.push(t[..hc.min(t.len())].iter().collect::<String>());
        Some(parts.join("\r\n"))
    }

    /// 클릭 좌표 → (절대 라인, 최근접 문자 경계) — paint 오프셋 캐시(가시 행 단위)
    /// 역참조(paint 전 = None).
    fn char_at(&self, x: i32, y: i32) -> Option<(usize, usize)> {
        let i = self.line_at(x, y)?;
        let offs = self.offsets.borrow();
        let line = offs.get(i - self.scroll)?;
        if line.is_empty() {
            return Some((i, 0));
        }
        Some((
            i,
            nearest_boundary(line, x - (self.bounds.x + self.pad_x) + self.scroll_x),
        ))
    }

    /// 드래그 선택 앵커(10-02): 라인 위 = 그 문자 경계, 마지막 라인 아래 빈 내용 영역 =
    /// 마지막 라인 끝(메모장 규약 — 빈 곳에서 시작한 드래그가 무시되던 결함 보완).
    fn anchor_at(&self, x: i32, y: i32) -> Option<(usize, usize)> {
        if let Some(pos) = self.char_at(x, y) {
            return Some(pos);
        }
        if self.image.is_some() || !self.content_rect().contains(Point { x, y }) {
            return None;
        }
        let last = self.lines.len().checked_sub(1)?;
        let top = self.content_rect().y;
        let below = self.scroll + ((y - top).max(0) / self.row_h) as usize >= self.lines.len();
        if !below {
            return None; // 오프셋 캐시 미구축(paint 전) — 라인 위지만 경계를 모름
        }
        let end = if self.lines[last].starts_with('\u{1}') {
            0
        } else {
            self.lines[last].chars().count()
        };
        Some((last, end))
    }

    /// 우상단 "크게"(↗) 오버레이 표시 여부(호스트 — 미리보기 종류일 때만).
    pub fn set_popout(&mut self, on: bool, inv: &mut Invalidations) {
        if self.popout_on != on {
            self.popout_on = on;
            if !on {
                self.popout_range.set(Rect::default());
            }
            inv.push(self.bounds);
        }
    }

    /// ↗ 클릭 수거(1회성) — 호스트가 독립 미리보기 창(모달)을 연다.
    pub fn take_popout(&mut self) -> bool {
        std::mem::take(&mut self.pending_popout)
    }

    /// 우상단 "크게"(↗) **이미지 버튼** 그리기(내용/이미지 위 — paint 마지막 호출,
    /// 07-26 개편). 3상태 배경: pressed = accent 38% 블렌드(툴바 켜짐 규약) >
    /// hover = sel_bg(X-27 토큰) > 기본 header_bg. 누름 효과 = 아이콘 1px 우하 오프셋.
    /// 아이콘 = `emb:popout`(SVG — 잉크 = 테마 본문색·다크 신호), 미로드 폴백 = ↗ 글리프.
    fn draw_popout(&self, ctx: &mut dyn DrawCtx, theme: &Theme, content_top: i32) {
        if !self.popout_on {
            self.popout_range.set(Rect::default());
            return;
        }
        let b = self.bounds;
        let side = (self.row_h + 4).min((b.bottom() - content_top - 4).max(0));
        let cell = Rect::new(b.right() - side - self.pad_x, content_top + 2, side, side);
        if cell.w <= 0 || cell.h <= 0 {
            self.popout_range.set(Rect::default());
            return;
        }
        let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * 0.38) as u8;
        let bg = if self.popout_pressed {
            {
                let (pr, pg, pb) = theme.panel_bg.rgb();
                let (ar, ag, ab) = theme.accent.rgb();
                crate::theme::Color::from_rgb(mix(pr, ar), mix(pg, ag), mix(pb, ab))
            }
        } else if self.popout_hover {
            theme.sel_bg
        } else {
            crate::theme::header_bg(theme)
        };
        ctx.fill_rect(cell, bg);
        let off = i32::from(self.popout_pressed); // 누름 효과 1px 우하
        let isz = (side - 8).max(8);
        let ink = {
            let (r, g, b) = theme.text.rgb();
            ((r as u32) << 16) | ((g as u32) << 8) | b as u32
        };
        let dk = if theme.is_dark { "#dark" } else { "" };
        let key = format!("emb:popout{dk}#{ink:06X}");
        let drew = ctx.draw_icon(
            cell.x + (side - isz) / 2 + off,
            cell.y + (side - isz) / 2 + off,
            isz,
            &key,
            "",
        );
        if !drew {
            // 미로드/추출 실패 폴백 = ↗ 글리프(bg는 이미 채움)
            let label = "↗";
            let tw = ctx.text_width(label);
            let ty = cell.y + (cell.h - (cell.h * 4) / 5) / 2 + off;
            let fg = if self.focused {
                theme.text
            } else {
                theme.text_dim
            };
            ctx.text(cell.x + (cell.w - tw).max(0) / 2 + off, ty, cell, label, fg);
        }
        self.popout_range.set(cell);
    }

    /// 텍스트 내용 전체 선택(컨텍스트 메뉴/Edit 메뉴 — 10-01). 이미지·빈 내용은 무시.
    /// 범위 = 첫 라인 0 ~ 마지막 라인 끝(문자 수) — `selected_text` 규약과 동일 좌표.
    pub fn select_all_text(&mut self, inv: &mut Invalidations) -> bool {
        if !self.text_selectable() {
            return false;
        }
        let last = self.lines.len() - 1;
        let end = if self.lines[last].starts_with('\u{1}') {
            0
        } else {
            self.lines[last].chars().count()
        };
        self.sel = Some(((0, 0), (last, end)));
        self.sel_drag = false;
        inv.push(self.bounds);
        true
    }

    /// 선택 가능한 텍스트 내용이 있는가(이미지 미리보기·빈 내용·마커뿐이면 false).
    pub fn text_selectable(&self) -> bool {
        self.image.is_none() && self.lines.iter().any(|l| !l.starts_with('\u{1}'))
    }

    /// 종류 스트립 아래 내용 영역 안 좌표인가(우클릭 컨텍스트 메뉴 판정).
    pub fn content_hit(&self, x: i32, y: i32) -> bool {
        self.content_rect().contains(Point { x, y })
    }

    /// 선택 해제(다른 영역 클릭 시 호스트가 호출).
    pub fn clear_text_selection(&mut self, inv: &mut Invalidations) {
        self.sel_drag = false;
        if self.sel.take().is_some() {
            inv.push(self.bounds);
        }
    }

    /// 도크 키 포커스 상태 반영(호스트=터미널 포커스와 동기 — QA 07-15) —
    /// 활성 종류·→ 버튼 강조색만 바뀐다.
    pub fn set_focused(&mut self, focused: bool, inv: &mut Invalidations) {
        if self.focused != focused {
            self.focused = focused;
            inv.push(self.bounds);
        }
    }

    /// 터미널 "폴더로 이동"(→) 클릭 수거(1회성 — 호스트가 cd 전송·종류 전환).
    pub fn take_goto(&mut self) -> bool {
        std::mem::take(&mut self.pending_goto)
    }

    /// 종류 라벨 목록 교체(i18n 전환 포함) — 활성 인덱스는 범위로 클램프.
    pub fn set_kinds(&mut self, kinds: Vec<String>, inv: &mut Invalidations) {
        if self.kinds != kinds {
            self.kinds = kinds;
            self.active = self.active.min(self.kinds.len().saturating_sub(1));
            inv.push(self.bounds);
        }
    }

    /// 활성 종류 인덱스(호스트가 내용 공급 분기 — 0=정보·1=미리보기·2=터미널).
    pub fn active_kind(&self) -> usize {
        self.active
    }

    /// 활성 종류를 프로그램으로 전환(nexa-dir3 T-61 — 기동 명령 `dock.kind:<n>` · 메뉴). 범위 밖 = 무시 ·
    /// 같은 값 = 무변화. 스트립 클릭과 같은 효과(선택 해제 · 내용 교체는 호스트).
    pub fn set_active_kind(&mut self, kind: usize, inv: &mut Invalidations) -> bool {
        if kind >= self.kinds.len() || kind == self.active {
            return false;
        }
        self.active = kind;
        self.sel = None;
        self.sel_drag = false;
        inv.push(self.bounds);
        true
    }

    /// 종류 스트립 아래 내용 영역(터미널 등 호스트 직접 렌더용 — M4-3).
    pub fn content_rect(&self) -> Rect {
        let strip_h = 1 + self.row_h.min((self.bounds.h - 1).max(0));
        Rect::new(
            self.bounds.x,
            self.bounds.y + strip_h,
            self.bounds.w,
            (self.bounds.h - strip_h).max(0),
        )
    }

    pub fn set_metrics(&mut self, row_h: i32, pad_x: i32, inv: &mut Invalidations) {
        self.row_h = row_h.max(1);
        self.pad_x = pad_x;
        self.offsets.borrow_mut().clear(); // 폰트/지표 변경 = 문자 폭 캐시 무효(07-20)
        inv.push(self.bounds);
    }

    /// 표시 내용 교체(변경 시에만 무효화 — 선택 이동마다 호출돼도 무비용 유지).
    /// 내용이 바뀌면 텍스트 선택·스크롤도 초기화(범위 무효 — QA 07-15).
    pub fn set_lines(&mut self, lines: Vec<String>, inv: &mut Invalidations) {
        let key = lines.first().cloned().unwrap_or_default();
        self.set_content(&key, lines, inv);
    }

    /// 키 지정 내용 교체(10-02 QA). 키가 같으면 **같은 대상의 갱신** — 스크롤·가로 스크롤은
    /// 새 상한으로 클램프만, 선택은 범위 안이면 유지(드래그 중이면 그대로). 키가 다르면
    /// 대상 전환 = 종전처럼 전부 리셋. 내용이 같으면 무비용.
    pub fn set_content(&mut self, key: &str, lines: Vec<String>, inv: &mut Invalidations) {
        if self.lines == lines && self.content_key == key {
            return;
        }
        if self.content_key == key && !self.lines.is_empty() {
            self.lines = lines;
            self.scroll = self.scroll.min(self.max_scroll());
            if self.scroll >= self.max_scroll() {
                self.scroll_frac = 0;
            }
            self.offsets.borrow_mut().clear();
            if let Some(((al, _), (cl, _))) = self.sel {
                if al.max(cl) >= self.lines.len() {
                    self.sel = None;
                    self.sel_drag = false;
                }
            }
            inv.push(self.bounds);
            return;
        }
        self.content_key = key.to_string();
        if self.lines != lines {
            self.lines = lines;
            self.sel = None;
            self.sel_drag = false;
            self.scroll = 0;
            self.scroll_frac = 0;
            self.scroll_x = 0;
            self.content_w.set(0);
            self.offsets.borrow_mut().clear();
            inv.push(self.bounds);
            // 내용 교체 = 두 바 잠깐 표시(넘치는 축만 실제로 그려진다 — 발견성, 10-02)
            let view = self.content_rect();
            self.bars.flash(Axis::V, view, inv);
            self.bars.flash(Axis::H, view, inv);
        }
    }

    /// 이미지 미리보기 대상(M4-2 — Some이면 라인 대신 이미지 표시. 렌더는 draw_image 백엔드).
    pub fn set_image(&mut self, image: Option<String>, inv: &mut Invalidations) {
        if self.image != image {
            self.image = image;
            inv.push(self.bounds);
        }
    }
    /// 종류 스트립(정보|미리보기|터미널 + →) 그리기 — 활성 강조·클릭 범위 캐시. paint가 부르고,
    /// 부분 줄 픽셀 스크롤(10-02)로 내용이 스트립 위로 번진 뒤 다시 덮을 때도 쓴다.
    fn draw_strip(&self, ctx: &mut dyn DrawCtx, theme: &Theme, strip: Rect) {
        let ty = |cell: Rect| cell.y + (cell.h - (cell.h * 4) / 5) / 2;
        ctx.fill_rect(strip, crate::theme::header_bg(theme));
        let mut ranges = Vec::with_capacity(self.kinds.len());
        let mut x = strip.x + self.pad_x;
        let last = self.kinds.len().saturating_sub(1);
        self.goto_range.set((0, 0));
        for (i, label) in self.kinds.iter().enumerate() {
            let w = ctx.text_width(label) + self.pad_x * 2;
            let cell = Rect::new(x, strip.y, w.min((strip.right() - x).max(0)), strip.h);
            let active = i == self.active;
            let (fg, bg) = if active && self.focused {
                (theme.text, theme.sel_bg)
            } else if active {
                // 비활성 패널 — 활성 종류는 무채색으로만 표시(활성 패널과 구분)
                (theme.text, theme.sel_bg_inactive)
            } else if self.strip_hover == Some(i) {
                // hover = 탭 바와 같은 상태 레이어(글자색을 배경에 얇게 섞는다) + 글자는 본문색.
                (theme.text, strip_hover_bg(theme))
            } else {
                (theme.text_dim, crate::theme::header_bg(theme))
            };
            if cell.w > 0 {
                ctx.text_opaque(cell.x + self.pad_x, ty(cell), cell, label, fg, bg);
            }
            ranges.push((cell.x, cell.x + w));
            x += w;
            if i == last && self.kinds.len() > 1 {
                // 터미널 옆 "폴더로 이동"(→) — 한 몸 버튼(QA 07-14, 원본 '터미널에서 열기').
                // 활성=accent 배경(단, 패널 비활성이면 무채색 — 활성 영역과 구분), 비활성=무색
                let gw = ctx.text_width("→") + self.pad_x * 2;
                let gcell = Rect::new(x, strip.y, gw.min((strip.right() - x).max(0)), strip.h);
                let (gfg, gbg) = if active && self.focused {
                    (theme.text, theme.accent)
                } else if active {
                    (theme.text, theme.sel_bg_inactive)
                } else if self.strip_hover == Some(self.kinds.len()) {
                    (theme.text, strip_hover_bg(theme))
                } else {
                    (theme.text_dim, crate::theme::header_bg(theme))
                };
                if gcell.w > 0 {
                    ctx.text_opaque(gcell.x + self.pad_x, ty(gcell), gcell, "→", gfg, gbg);
                }
                self.goto_range.set((gcell.x, gcell.x + gw));
                x += gw;
            }
            x += self.pad_x;
        }
        *self.ranges.borrow_mut() = ranges;
    }

    /// 스트립의 어느 칸 위인가(종류 인덱스 · `kinds.len()` = → 버튼 · 밖 = None) — paint가 캐시한 범위로 판정.
    fn strip_hit(&self, x: i32, y: i32) -> Option<usize> {
        let b = self.bounds;
        if b.h <= 1
            || y < b.y
            || y >= b.y + 1 + self.row_h.min(b.h - 1)
            || x < b.x
            || x >= b.right()
        {
            return None;
        }
        let (glo, ghi) = self.goto_range.get();
        if ghi > glo && x >= glo && x < ghi {
            return Some(self.kinds.len());
        }
        self.ranges
            .borrow()
            .iter()
            .position(|(lo, hi)| x >= *lo && x < *hi)
    }

    /// 지금 hover 중인 스트립 칸(시험 · 호스트 덤프용).
    #[must_use]
    pub fn strip_hover(&self) -> Option<usize> {
        self.strip_hover
    }

    /// 호스트용 스트립 재도장(10-02): 터미널처럼 호스트가 내용 영역을 직접 그린 뒤 부분 행이 스트립
    /// 위로 번졌을 때 덮는다(DW 글리프는 GDI 클립을 무시).
    pub fn paint_strip(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.bounds;
        if b.h <= 1 {
            return;
        }
        ctx.select_font(crate::draw::FontSlot::Base, false, false);
        self.draw_strip(
            ctx,
            theme,
            Rect::new(b.x, b.y + 1, b.w, self.row_h.min(b.h - 1)),
        );
    }
}

impl Widget for InfoDock {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        if self.bounds != bounds {
            let old = self.bounds;
            self.bounds = bounds;
            self.scroll = self.scroll.min(self.max_scroll()); // 높이 변경 = 상한 재클램프
            self.scroll_x = self.scroll_x.min(self.max_scroll_x()); // 폭 변경 = 가로 상한
            self.offsets.borrow_mut().clear(); // 폭 변경 = 측정 상한 무효(07-20)
            inv.push(old.union(&bounds));
        }
    }

    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        match *ev {
            // 종류 스트립 클릭 = 전환(M4-2 — 원본 SyncToggles 대응). 내용 갱신은 호스트.
            InputEvent::MouseDown { x, y, .. } => {
                let strip_bottom = self.bounds.y + 1 + self.row_h;
                if y >= self.bounds.y && y < strip_bottom {
                    // → 버튼(터미널 옆 부착 — QA 07-14 '폴더로 이동')
                    let (glo, ghi) = self.goto_range.get();
                    if ghi > glo && x >= glo && x < ghi {
                        self.pending_goto = true;
                        if self.active + 1 != self.kinds.len() {
                            self.active = self.kinds.len().saturating_sub(1); // 터미널로 전환
                        }
                        inv.push(self.bounds);
                        return;
                    }
                    let hit = self
                        .ranges
                        .borrow()
                        .iter()
                        .position(|&(lo, hi)| x >= lo && x < hi);
                    if let Some(i) = hit {
                        if self.active != i {
                            self.active = i;
                            self.sel = None; // 종류 전환 = 내용 교체
                            self.sel_drag = false;
                            inv.push(self.bounds);
                        }
                    }
                } else if self.popout_on && self.popout_range.get().contains(Point { x, y }) {
                    // 우상단 ↗ "크게" 이미지 버튼(07-26) — 프레스 시각만, 발화는 MouseUp.
                    // 오버레이 바보다 **먼저** 판정(10-02 G6-10): 버튼 오른쪽 8px가 세로 바
                    // 존(right−14)과 겹쳐 scroll 0이면 썸 드래그가, 아니면 페이지 이동이
                    // 클릭을 삼켜 '색은 바뀌는데 안 눌림'이 됐다. 명시적 버튼이 바보다 우선.
                    self.popout_pressed = true;
                    inv.push(self.popout_range.get());
                } else if let Some((axis, hit)) = self.bars.mouse_down(x, y, self.geoms(), inv) {
                    // 오버레이 바(10-02): 썸 드래그 시작·트랙 페이지 이동 — 선택 불변
                    self.apply_bar(axis, hit, inv);
                } else if let Some(pos) = self.anchor_at(x, y) {
                    // 내용 드래그 선택 시작(QA 07-15 → 07-20 **문자 단위** 앵커 →
                    // 10-02 빈 영역 = 마지막 라인 끝)
                    self.sel = Some((pos, pos));
                    self.sel_drag = true;
                    inv.push(self.bounds);
                } else {
                    self.clear_text_selection(inv);
                }
            }
            InputEvent::MouseMove { x, y } => {
                // 오버레이 바 드래그(10-02) = 비례 스크롤만(선택·hover 불변)
                if let Some((axis, off)) = self.bars.mouse_move(x, y, self.geoms(), inv) {
                    match axis {
                        Axis::V => {
                            let _ = self.scroll_to(off.max(0) as usize, inv);
                        }
                        Axis::H => {
                            let _ = self.hscroll_to(off as i32, inv);
                        }
                    }
                    return;
                }
                // 종류 스트립 hover(127차): 칸이 바뀔 때만 스트립을 다시 그린다 · 스트립 밖/창 밖 = 해제.
                let over = self.strip_hit(x, y);
                if over != self.strip_hover {
                    self.strip_hover = over;
                    let b = self.bounds;
                    inv.push(Rect::new(b.x, b.y, b.w, (self.row_h + 1).min(b.h)));
                }
                // ↗ hover 색 입힘(07-26 — X-27 sel_bg 토큰. 변경 시에만 무효화)
                let hp = self.popout_on
                    && !self.sel_drag
                    && self.popout_range.get().contains(Point { x, y });
                if hp != self.popout_hover {
                    self.popout_hover = hp;
                    inv.push(self.popout_range.get());
                }
                if self.sel_drag {
                    let top = self.bounds.y + 1 + self.row_h.min((self.bounds.h - 1).max(0));
                    // 드래그 자동 스크롤(07-26): 내용 영역 위/아래 = 1행씩 이동
                    if y < top {
                        let s = self.scroll.saturating_sub(1);
                        let _ = self.scroll_to(s, inv);
                    } else if y >= self.bounds.bottom() {
                        let _ = self.scroll_to(self.scroll + 1, inv);
                    }
                    // 가로 자동 스크롤(10-02): 좌/우 가장자리 밖 = 한 행 높이만큼 px 이동
                    if x < self.bounds.x + self.pad_x {
                        let _ = self.hscroll_to(self.scroll_x - self.row_h, inv);
                    } else if x >= self.bounds.right() - self.pad_x {
                        let _ = self.hscroll_to(self.scroll_x + self.row_h, inv);
                    }
                    // 내용 영역 밖은 첫/끝 라인·문자 경계로 클램프(엣지 드래그 — 07-20)
                    let offs = self.offsets.borrow();
                    let last_line = self.lines.len().saturating_sub(1);
                    let (li, ci) = if offs.is_empty() {
                        // 스크롤 직후 = 캐시 무효 — 라인만 갱신(경계는 다음 paint 후 정밀화)
                        let li = if y < top {
                            self.scroll
                        } else {
                            (self.scroll + (((y - top) / self.row_h).max(0) as usize))
                                .min(last_line)
                        };
                        (li, 0)
                    } else {
                        let row = if y < top {
                            0
                        } else {
                            (((y - top) / self.row_h).max(0) as usize)
                                .min(offs.len().saturating_sub(1))
                        };
                        let li = (self.scroll + row).min(last_line);
                        let ci = if offs[row].is_empty() {
                            0
                        } else {
                            nearest_boundary(
                                &offs[row],
                                x - (self.bounds.x + self.pad_x) + self.scroll_x,
                            )
                        };
                        (li, ci)
                    };
                    drop(offs);
                    if let Some((a, cur)) = self.sel {
                        if cur != (li, ci) {
                            self.sel = Some((a, (li, ci)));
                            inv.push(self.bounds);
                        }
                    }
                }
            }
            InputEvent::Wheel { delta } => {
                // 내용 세로 스크롤(07-26 미리보기 → 10-02 정보 포함 — 3줄/노치·터미널 규약
                // 동일). 호스트가 내용 영역 hover일 때만 라우팅한다.
                if self.image.is_none() && !self.lines.is_empty() {
                    if delta.abs() >= crate::event::WHEEL_DELTA {
                        // 마우스 노치 = 줄 단위(시스템 줄 수) + 고속 스크롤 배수
                        let step = self.wheel.add(delta, crate::event::wheel_lines());
                        let step = self.fast.wheel(delta, step);
                        if step != 0 {
                            let to = (self.scroll as i32 - step).max(0) as usize;
                            let _ = self.scroll_to(to, inv);
                        }
                    } else {
                        // 정밀 터치패드 = 픽셀(10-02)
                        let px = self
                            .wheel_px
                            .add(delta, crate::event::wheel_lines() * self.row_h);
                        if px != 0 {
                            let _ = self.scroll_by_px(-px, inv);
                        }
                    }
                    if self.fast.hud_visible() {
                        inv.push(self.content_rect());
                        inv.request_tick();
                    }
                }
            }
            InputEvent::HWheel { delta } => {
                // 가로 스크롤(10-02 — Shift+휠·틸트 휠. 양수 = 오른쪽, 노치당 행 높이×3px)
                if self.image.is_none() && !self.lines.is_empty() {
                    let step = self
                        .hwheel
                        .add(delta, self.row_h * crate::event::wheel_lines());
                    let step = self.fast.wheel(delta, step);
                    if step != 0 {
                        let _ = self.hscroll_to(self.scroll_x + step, inv);
                    }
                    if self.fast.hud_visible() {
                        inv.push(self.content_rect());
                        inv.request_tick();
                    }
                }
            }
            InputEvent::MouseUp { x, y } => {
                if self.bars.mouse_up(self.content_rect(), inv) {
                    return; // 바 드래그 종료(10-02) — 선택 상태 불변
                }
                // ↗ 버튼 의미론(07-26): 누른 채 버튼 **안에서 뗄 때만** 발화
                if self.popout_pressed {
                    self.popout_pressed = false;
                    if self.popout_range.get().contains(Point { x, y }) {
                        self.pending_popout = true;
                    }
                    inv.push(self.popout_range.get());
                }
                self.sel_drag = false; // 선택은 유지(Ctrl+C 복사 — 터미널 규약 동일)
                if let Some((a, c)) = self.sel {
                    if a == c {
                        self.sel = None; // 이동 없는 단순 클릭 = 선택 없음(edit.rs 규약)
                    }
                }
            }
            _ => {}
        }
    }

    fn paint(&self, ctx: &mut dyn CtlDrawCtx, theme: &Theme) {
        let mut a = crate::draw::Adapt(ctx);
        self.paint_dock(&mut a, theme);
    }
}

impl InfoDock {
    /// 그리기 본체(dir2 `Widget::paint` 그대로) — 그리드 어휘 백엔드를 직접 넘길 때(시험 기록기).
    pub fn paint_dock(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        ctx.select_font(crate::draw::FontSlot::Base, false, false); // 폰트 슬롯(X-12)
        let b = self.bounds;
        if b.h <= 0 {
            return;
        }
        ctx.fill_rect(b, theme.panel_bg);
        // 상단 경계선(리스트와 구분 — docs/39 §2 경계선+명도 차)
        ctx.fill_rect(Rect::new(b.x, b.y, b.w, 1), theme.border);
        // 종류 스트립(정보|미리보기 — 활성 강조·클릭 전환. 범위 캐시)
        let strip = Rect::new(b.x, b.y + 1, b.w, self.row_h.min(b.h - 1));
        let ty = |cell: Rect| cell.y + (cell.h - (cell.h * 4) / 5) / 2;
        self.draw_strip(ctx, theme, strip);
        // 이미지 미리보기(M4-2) — 내용 영역 전체에 비율 유지 가운데 표시
        if let Some(img) = &self.image {
            let area = Rect::new(
                b.x + self.pad_x,
                strip.bottom() + 2,
                b.w - self.pad_x * 2,
                (b.bottom() - strip.bottom() - 4).max(0),
            );
            ctx.draw_image(area, img);
            self.draw_popout(ctx, theme, strip.bottom());
            return;
        }
        // 내용 라인들(스크롤 반영 — 07-26) — 드래그 선택 **문자 구간** 하이라이트
        // (QA 07-15 → 07-20 보완, Ctrl+C 복사 대상). 문자 경계 오프셋은 **가시 행**
        // 단위로 캐시(히트 테스트 역참조 — edit.rs paint_field 규약. 무효화 시 재측정)
        let sel = self.sel.map(|(a, c)| if a <= c { (a, c) } else { (c, a) });
        let rebuild = self.offsets.borrow().is_empty() && !self.lines.is_empty();
        // 가로 스크롤(10-02): 텍스트 원점을 오프셋만큼 왼쪽으로, 측정 상한은 보이는 폭 +
        // 오프셋(스크롤된 구간까지 클릭 가능). 가시 행 최대 폭 = 가로 상한·바 기하.
        let x0 = b.x + self.pad_x - self.scroll_x;
        let max_w = (b.w - self.pad_x).max(0) + self.scroll_x;
        let mut content_w = 0;
        let mut y = strip.bottom() - self.scroll_frac; // 부분 줄 오프셋(10-02 픽셀 스크롤)
        ctx.push_clip(self.content_rect()); // 가로 스크롤 텍스트의 왼쪽 번짐 차단(10-02)
        for (i, line) in self.lines.iter().enumerate().skip(self.scroll) {
            if y >= b.bottom() {
                break;
            }
            // 인라인 이미지(07-26 다이어그램): 마커 = 예약 행 전체에 draw_image,
            // 패드 = 배경만. 오프셋은 경계 0 하나(선택 대상 아님).
            if let Some(path) = line.strip_prefix(IMG_MARKER) {
                if rebuild {
                    self.offsets.borrow_mut().push(vec![0]);
                }
                let k = 1 + self.lines[i + 1..]
                    .iter()
                    .take_while(|l| l.as_str() == IMG_PAD)
                    .count() as i32;
                let area = Rect::new(
                    b.x + self.pad_x, // 인라인 이미지는 가로 스크롤 불변(폭 맞춤 렌더)
                    y,
                    (b.w - self.pad_x * 2).max(0),
                    (self.row_h * k).min(b.bottom() - y),
                );
                ctx.draw_image(area, path);
                y += self.row_h;
                continue;
            }
            if line == IMG_PAD {
                if rebuild {
                    self.offsets.borrow_mut().push(vec![0]);
                }
                y += self.row_h;
                continue;
            }
            content_w = content_w.max(ctx.text_width(line));
            if rebuild {
                let mut offs = vec![0];
                let mut prefix = String::new();
                for c in line.chars() {
                    prefix.push(c);
                    let w = ctx.text_width(&prefix);
                    offs.push(w);
                    if w > max_w {
                        break; // 보이는 폭 밖 = 클릭 불가(측정 상한)
                    }
                }
                self.offsets.borrow_mut().push(offs);
            }
            let cell = Rect::new(b.x, y, b.w, self.row_h.min(b.bottom() - y));
            if let Some(((ll, lc), (hl, hc))) = sel.filter(|&((ll, _), (hl, _))| ll <= i && i <= hl)
            {
                let offs = self.offsets.borrow();
                if let Some(o) = offs.get(i - self.scroll) {
                    let last = o.len().saturating_sub(1);
                    let cs = if i == ll { lc.min(last) } else { 0 };
                    let ce = if i == hl { hc.min(last) } else { last };
                    if ce > cs {
                        // 가로 스크롤 반영 + 셀 폭으로 클립(바깥 번짐 방지)
                        let (sx, ex) = ((x0 + o[cs]).max(cell.x), (x0 + o[ce]).min(cell.right()));
                        if ex > sx {
                            ctx.fill_rect(Rect::new(sx, cell.y, ex - sx, cell.h), theme.sel_bg);
                        }
                    }
                }
            }
            // 가로 스크롤 시 **보이는 첫 문자 경계부터** 그린다(10-02 실측: DW 비트맵 렌더
            // 타깃의 글리프 그리기는 GDI 클립을 무시 → 왼쪽 밖으로 번짐. 채움은 클립됨).
            // 숨는 부분은 최대 한 문자 — 선택 하이라이트·히트는 전체 오프셋 기준 그대로.
            let (draw_x, draw_text) = if self.scroll_x > 0 {
                let offs = self.offsets.borrow();
                match offs.get(i - self.scroll) {
                    Some(o) => {
                        let k = o
                            .iter()
                            .position(|&w| w >= self.scroll_x)
                            .unwrap_or(o.len().saturating_sub(1));
                        let byte = line
                            .char_indices()
                            .nth(k)
                            .map(|(bi, _)| bi)
                            .unwrap_or(line.len());
                        (x0 + o[k], &line[byte..])
                    }
                    None => (x0, line.as_str()),
                }
            } else {
                (x0, line.as_str())
            };
            ctx.text(draw_x, ty(cell), cell, draw_text, theme.text);
            y += self.row_h;
        }
        ctx.pop_clip();
        self.content_w.set(content_w);
        // 부분 줄이 스트립 위로 번졌을 수 있다(DW 글리프는 GDI 클립 무시) — 스트립을 **뒤에** 다시 그림
        if self.scroll_frac > 0 {
            self.draw_strip(ctx, theme, strip);
        }
        // 잔여 배경
        if y < b.bottom() {
            ctx.fill_rect(Rect::new(b.x, y, b.w, b.bottom() - y), theme.panel_bg);
        }
        self.draw_popout(ctx, theme, strip.bottom());
        self.bars.paint(ctx, theme, self.geoms()); // 오버레이 바(10-02) — 내용 위 마지막
        self.fast
            .paint(ctx, theme, self.content_rect(), self.row_h, self.pad_x); // ×N 배지
    }
}

/// 스트립 hover 배경 = 머리 배경에 글자색을 hover 상태 레이어 농도만큼 섞은 색(탭 바 hover와 같은 농도 · 불투명 —
/// 스트립 글자는 `text_opaque`로 그린다).
fn strip_hover_bg(theme: &Theme) -> crate::theme::Color {
    crate::theme::header_bg(theme).lerp(theme.text, nexa_ctl::tokens::hover_alpha(false, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Color;

    struct Probe;
    impl DrawCtx for Probe {
        fn fill_rect(&mut self, _r: Rect, _c: Color) {}
        fn text_opaque(&mut self, _x: i32, _y: i32, _c: Rect, _t: &str, _f: Color, _b: Color) {}
        fn text_width(&mut self, text: &str) -> i32 {
            text.chars().count() as i32 * 8
        }
    }

    #[test]
    fn drag_selects_char_region_and_click_alone_clears() {
        // 문자 단위 영역 선택(07-20 — 미리보기 텍스트 복사 보완). Probe = 8px/문자.
        let mut inv = Invalidations::default();
        let mut d = InfoDock::new("정보", 20, 6);
        d.set_bounds(Rect::new(0, 100, 400, 120), &mut inv);
        d.set_lines(vec!["abcdef".into(), "01234".into()], &mut inv);
        d.paint_dock(&mut Probe, &Theme::dark());
        // 내용 top = 100+1+20 = 121. 라인0 문자경계2(x=6+16) 프레스 →
        // 라인1 문자경계3(x=6+24)까지 드래그
        d.on_event(
            &InputEvent::MouseDown {
                x: 6 + 16,
                y: 121,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        d.on_event(&InputEvent::MouseMove { x: 6 + 24, y: 141 }, &mut inv);
        d.on_event(&InputEvent::MouseUp { x: 6 + 24, y: 141 }, &mut inv);
        assert_eq!(
            d.selected_text().as_deref(),
            Some("cdef\r\n012"),
            "문자 단위 다중 라인 영역"
        );
        // 같은 라인 안 부분 선택
        d.on_event(
            &InputEvent::MouseDown {
                x: 6 + 8,
                y: 121,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        d.on_event(&InputEvent::MouseMove { x: 6 + 32, y: 121 }, &mut inv);
        d.on_event(&InputEvent::MouseUp { x: 6 + 32, y: 121 }, &mut inv);
        assert_eq!(d.selected_text().as_deref(), Some("bcd"));
        // 이동 없는 단순 클릭 = 선택 없음(edit.rs 규약)
        d.on_event(
            &InputEvent::MouseDown {
                x: 6 + 16,
                y: 121,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        d.on_event(&InputEvent::MouseUp { x: 6 + 16, y: 121 }, &mut inv);
        assert_eq!(d.selected_text(), None);
    }

    #[test]
    fn wheel_scrolls_and_selection_maps_absolute_lines() {
        // 07-26 미리보기 스크롤: h=120 → 스트립 21 + 내용 4행(row_h 20)·12줄 = 상한 8.
        let mut inv = Invalidations::default();
        let mut d = InfoDock::new("정보", 20, 6);
        d.set_bounds(Rect::new(0, 100, 400, 120), &mut inv);
        d.set_lines((0..12).map(|i| format!("l{i}")).collect(), &mut inv);
        d.paint_dock(&mut Probe, &Theme::dark());
        d.on_event(&InputEvent::Wheel { delta: -120 }, &mut inv); // 1노치 아래 = 3줄
        d.paint_dock(&mut Probe, &Theme::dark()); // 오프셋 재측정(가시 행 기준)
                                                  // 첫 가시 행 클릭 = 절대 라인 3 — "l3"의 문자 1~2 선택 = "3"
        d.on_event(
            &InputEvent::MouseDown {
                x: 6 + 8,
                y: 121,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        d.on_event(&InputEvent::MouseMove { x: 6 + 16, y: 121 }, &mut inv);
        d.on_event(&InputEvent::MouseUp { x: 6 + 16, y: 121 }, &mut inv);
        assert_eq!(
            d.selected_text().as_deref(),
            Some("3"),
            "스크롤 반영 절대 라인"
        );
        // 상한 클램프(대량 휠)
        d.on_event(&InputEvent::Wheel { delta: -120 * 50 }, &mut inv);
        assert_eq!(d.scroll, 8, "max_scroll 클램프");
        d.on_event(&InputEvent::Wheel { delta: 120 * 50 }, &mut inv);
        assert_eq!(d.scroll, 0, "0 클램프");
    }

    /// 스트립 hover(127차): 비활성 종류 칸과 → 버튼 위에서만 표시가 바뀐다 · 칸이 바뀔 때만 무효화 · 벗어나면 해제.
    #[test]
    fn strip_cells_hover_like_tabs() {
        let mut d = InfoDock::new("Info", 20, 6);
        let mut inv = Invalidations::default();
        d.set_kinds(
            vec!["Info".into(), "Preview".into(), "Terminal".into()],
            &mut inv,
        );
        d.set_bounds(Rect::new(0, 100, 400, 200), &mut inv);
        let theme = Theme::dark();
        let mut rec = nexa_ctl::RecordCtx::with_surface(400, 300);
        d.paint(&mut rec, &theme); // 칸 범위 캐시
        let ranges = d.ranges.borrow().clone();
        assert_eq!(ranges.len(), 3);
        let mid = |i: usize| (ranges[i].0 + ranges[i].1) / 2;
        let _ = inv.drain().count();
        d.on_event(&InputEvent::MouseMove { x: mid(1), y: 110 }, &mut inv);
        assert_eq!(d.strip_hover(), Some(1));
        assert_eq!(inv.drain().count(), 1, "칸 진입 = 무효화 1회");
        d.on_event(
            &InputEvent::MouseMove {
                x: mid(1) + 1,
                y: 111,
            },
            &mut inv,
        );
        assert_eq!(inv.drain().count(), 0, "같은 칸 = 무비용");
        // hover 칸은 hover 배경 · 다른 비활성 칸은 머리 배경.
        rec.clear();
        d.paint(&mut rec, &theme);
        let hover_bg = strip_hover_bg(&theme);
        assert_ne!(hover_bg, crate::theme::header_bg(&theme));
        assert!(
            rec.fills.iter().any(|(_, c)| *c == hover_bg),
            "hover 배경이 그려진다"
        );
        // → 버튼 · 활성 칸(표시는 활성 강조 그대로) · 내용 영역 = 해제.
        let (glo, ghi) = d.goto_range.get();
        d.on_event(
            &InputEvent::MouseMove {
                x: (glo + ghi) / 2,
                y: 110,
            },
            &mut inv,
        );
        assert_eq!(d.strip_hover(), Some(3));
        d.on_event(&InputEvent::MouseMove { x: mid(0), y: 110 }, &mut inv);
        assert_eq!(d.strip_hover(), Some(0));
        d.on_event(&InputEvent::MouseMove { x: mid(1), y: 200 }, &mut inv);
        assert_eq!(d.strip_hover(), None);
        d.on_event(&InputEvent::MouseMove { x: mid(1), y: 110 }, &mut inv);
        d.on_event(&InputEvent::MouseMove { x: -1, y: -1 }, &mut inv);
        assert_eq!(d.strip_hover(), None, "창 밖 = 해제");
    }

    #[test]
    fn popout_button_hover_press_release_semantics() {
        // 07-26 이미지 버튼 개편: hover 색·press 시각·**안에서 릴리스할 때만** 발화.
        let mut inv = Invalidations::default();
        let mut d = InfoDock::new("정보", 20, 6);
        d.set_bounds(Rect::new(0, 100, 400, 120), &mut inv);
        d.set_lines(vec!["abc".into()], &mut inv);
        d.set_popout(true, &mut inv);
        d.paint_dock(&mut Probe, &Theme::dark());
        // 버튼 정사각 side = row_h+4 = 24 → cell x=400-24-6=370, y=123
        let _ = inv.drain().count();
        d.on_event(&InputEvent::MouseMove { x: 380, y: 130 }, &mut inv);
        assert!(d.popout_hover, "hover 색 입힘");
        assert_eq!(inv.drain().count(), 1, "hover 진입 = 무효화 1회");
        d.on_event(&InputEvent::MouseMove { x: 380, y: 130 }, &mut inv);
        assert_eq!(inv.drain().count(), 0, "동일 hover = 무비용");
        // 프레스 = 시각만(발화 없음)
        d.on_event(
            &InputEvent::MouseDown {
                x: 380,
                y: 130,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert!(d.popout_pressed && !d.take_popout(), "press = 발화 전");
        // 밖에서 릴리스 = 취소
        d.on_event(&InputEvent::MouseUp { x: 10, y: 130 }, &mut inv);
        assert!(!d.popout_pressed && !d.take_popout(), "밖 릴리스 = 취소");
        // 안에서 릴리스 = 1회성 발화
        d.on_event(
            &InputEvent::MouseDown {
                x: 380,
                y: 130,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        d.on_event(&InputEvent::MouseUp { x: 380, y: 130 }, &mut inv);
        assert!(d.take_popout(), "안 릴리스 = 발화");
        assert!(!d.take_popout(), "수거 후 소진");
        assert_eq!(d.selected_text(), None, "버튼 조작은 선택 시작 아님");
        // 꺼진 상태에서는 히트 없음
        d.set_popout(false, &mut inv);
        d.paint_dock(&mut Probe, &Theme::dark());
        d.on_event(
            &InputEvent::MouseDown {
                x: 380,
                y: 130,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        d.on_event(&InputEvent::MouseUp { x: 380, y: 130 }, &mut inv);
        assert!(!d.take_popout());
    }

    #[test]
    fn drag_below_content_autoscrolls() {
        let mut inv = Invalidations::default();
        let mut d = InfoDock::new("정보", 20, 6);
        d.set_bounds(Rect::new(0, 100, 400, 120), &mut inv);
        d.set_lines((0..12).map(|i| format!("l{i}")).collect(), &mut inv);
        d.paint_dock(&mut Probe, &Theme::dark());
        d.on_event(
            &InputEvent::MouseDown {
                x: 6,
                y: 121,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        // 하단 밖 드래그 = 1행씩 자동 스크롤(이동할 때마다 — 07-26)
        d.on_event(&InputEvent::MouseMove { x: 6, y: 400 }, &mut inv);
        assert_eq!(d.scroll, 1);
        d.paint_dock(&mut Probe, &Theme::dark());
        d.on_event(&InputEvent::MouseMove { x: 6, y: 400 }, &mut inv);
        assert_eq!(d.scroll, 2);
        // 선택 끝은 문서 끝 방향으로 확장 중(앵커 = 라인 0)
        d.on_event(&InputEvent::MouseUp { x: 6, y: 400 }, &mut inv);
        assert!(d.selected_text().is_some());
    }

    #[test]
    fn select_all_text_covers_whole_content() {
        let mut d = InfoDock::new("Info", 20, 6);
        let mut inv = Invalidations::default();
        assert!(!d.select_all_text(&mut inv), "빈 내용 = 선택 불가");
        d.set_lines(vec!["abc".into(), "de".into()], &mut inv);
        assert!(d.text_selectable());
        assert!(d.select_all_text(&mut inv));
        assert_eq!(d.selected_text().as_deref(), Some("abc\r\nde"));
        d.set_image(Some("x.png".into()), &mut inv);
        assert!(!d.text_selectable(), "이미지 미리보기 = 텍스트 선택 없음");
    }

    #[test]
    fn hwheel_scrolls_horizontally_and_hit_test_follows() {
        // 10-02: 폭 100px·pad 6 → 보이는 내용 폭 94. 라인 "0123456789abcdef"(16자×8=128px)
        let mut inv = Invalidations::default();
        let mut d = InfoDock::new("정보", 20, 6);
        d.set_bounds(Rect::new(0, 100, 100, 120), &mut inv);
        d.set_lines(vec!["0123456789abcdef".into(), "x".into()], &mut inv);
        d.paint_dock(&mut Probe, &Theme::dark());
        assert_eq!(d.content_w.get(), 128, "가시 행 최대 폭 측정");
        assert_eq!(d.max_scroll_x(), 128 + 12 - 100);
        // 측정 전 클램프(내용 없는 상태) = 0 — 측정 후 1노치 오른쪽 = row_h×3 = 60 → 40 클램프
        d.on_event(&InputEvent::HWheel { delta: 120 }, &mut inv);
        assert_eq!(d.scroll_x, 40, "가로 상한 클램프");
        assert!(inv.tick_requested(), "스크롤 = 바 표시(틱 요청)");
        d.paint_dock(&mut Probe, &Theme::dark()); // 오프셋 재측정(스크롤 반영)
                                                  // 화면 x=6(원점)은 문서 x=40 → 문자 경계 5 — 2문자 드래그 = "56"
        d.on_event(
            &InputEvent::MouseDown {
                x: 6,
                y: 121,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        d.on_event(&InputEvent::MouseMove { x: 6 + 16, y: 121 }, &mut inv);
        d.on_event(&InputEvent::MouseUp { x: 6 + 16, y: 121 }, &mut inv);
        assert_eq!(
            d.selected_text().as_deref(),
            Some("56"),
            "가로 스크롤 반영 히트"
        );
        d.on_event(&InputEvent::HWheel { delta: -120 * 9 }, &mut inv);
        assert_eq!(d.scroll_x, 0, "0 클램프");
        // 내용 교체 = 가로 오프셋 리셋
        d.on_event(&InputEvent::HWheel { delta: 120 }, &mut inv);
        d.set_lines(vec!["short".into()], &mut inv);
        assert_eq!(d.scroll_x, 0);
    }

    #[test]
    fn drag_from_empty_area_anchors_at_end_of_last_line() {
        // 10-02: 마지막 라인 아래 빈 영역에서 시작한 드래그 = 끝 앵커(메모장 규약)
        let mut inv = Invalidations::default();
        let mut d = InfoDock::new("정보", 20, 6);
        d.set_bounds(Rect::new(0, 100, 400, 120), &mut inv);
        d.set_lines(vec!["abc".into(), "de".into()], &mut inv);
        d.paint_dock(&mut Probe, &Theme::dark());
        // 내용 top=121 · 라인1 = 141~160 · 빈 영역 y=200
        d.on_event(
            &InputEvent::MouseDown {
                x: 50,
                y: 200,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(d.sel, Some(((1, 2), (1, 2))), "앵커 = 마지막 라인 끝");
        d.on_event(&InputEvent::MouseMove { x: 6 + 8, y: 121 }, &mut inv);
        d.on_event(&InputEvent::MouseUp { x: 6 + 8, y: 121 }, &mut inv);
        assert_eq!(
            d.selected_text().as_deref(),
            Some("bc\r\nde"),
            "위로 드래그 = 역방향 영역"
        );
        // 이미지 종류·빈 내용은 앵커 없음
        d.set_image(Some("x.png".into()), &mut inv);
        assert_eq!(d.anchor_at(50, 200), None);
    }

    #[test]
    fn vertical_bar_drag_scrolls_without_selecting() {
        // 10-02 오버레이 바: 내용 12줄·가시 4행 → 세로 바. 썸 드래그 = 스크롤·선택 없음
        let mut inv = Invalidations::default();
        let mut d = InfoDock::new("정보", 20, 6);
        d.set_bounds(Rect::new(0, 100, 400, 120), &mut inv);
        d.set_lines((0..12).map(|i| format!("l{i}")).collect(), &mut inv);
        d.paint_dock(&mut Probe, &Theme::dark());
        assert!(d.bars.visible(Axis::V), "내용 교체 = 바 표시");
        let g = d.geoms();
        assert_eq!((g[0].content, g[0].visible), (12, 4));
        let t = d.bars.thumb(Axis::V, &g[0], true).expect("세로 썸");
        d.on_event(
            &InputEvent::MouseDown {
                x: t.x + 1,
                y: t.y + 1,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert!(
            d.bars.dragging() && d.sel.is_none(),
            "썸 프레스 = 선택 시작 아님"
        );
        d.on_event(
            &InputEvent::MouseMove {
                x: t.x + 1,
                y: t.y + 1 + 500,
            },
            &mut inv,
        );
        assert_eq!(d.scroll, 8, "트랙 끝까지 = 최대 스크롤");
        d.on_event(
            &InputEvent::MouseUp {
                x: t.x + 1,
                y: t.y + 501,
            },
            &mut inv,
        );
        assert!(!d.bars.dragging() && d.sel.is_none());
        // 바 틱 = 유지 후 페이드(호스트 tick 경유)
        for _ in 0..40 {
            d.tick(&mut inv);
        }
        assert!(!d.bars.visible(Axis::V), "페이드 완료");
    }

    #[test]
    fn popout_click_wins_over_flashed_bar() {
        // 10-02 G6-10: 12줄 미리보기 = set_lines flash로 세로 바 가시. 팝아웃 셀
        // x∈[370,394)와 세로 바 존 x≥386(썸 x=388−2)이 8px 겹침 — scroll 0이면 썸이
        // 버튼 위(y 121..)라 종전엔 썸 드래그가 클릭을 삼켰다. 버튼 판정 선행 = 발화.
        let mut inv = Invalidations::default();
        let mut d = InfoDock::new("정보", 20, 6);
        d.set_bounds(Rect::new(0, 100, 400, 120), &mut inv);
        d.set_lines((0..12).map(|i| format!("l{i}")).collect(), &mut inv);
        d.set_popout(true, &mut inv);
        d.paint_dock(&mut Probe, &Theme::dark());
        assert!(d.bars.visible(Axis::V), "flash 상태");
        let cell = d.popout_range.get();
        assert_eq!((cell.x, cell.right()), (370, 394));
        let g = d.geoms();
        let t = d.bars.thumb(Axis::V, &g[0], true).expect("세로 썸");
        let (x, y) = (cell.right() - 8, cell.y + 7);
        assert!(
            x >= t.x - 2 && y >= t.y && y < t.bottom(),
            "클릭점이 썸 히트 존 안(겹침 재현 전제)"
        );
        d.on_event(
            &InputEvent::MouseDown {
                x,
                y,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert!(
            d.popout_pressed && !d.bars.dragging(),
            "버튼 프레스가 바 드래그보다 우선"
        );
        d.on_event(&InputEvent::MouseUp { x, y }, &mut inv);
        assert!(d.take_popout(), "안 릴리스 = 발화");
        assert_eq!(d.scroll, 0, "스크롤 불변");
        // 바 드래그 동작 불변: 버튼 아래 썸 구간 프레스는 종전대로 드래그
        let (bx, by) = (t.x + 1, cell.bottom() + 1);
        assert!(by < t.bottom(), "썸 안·버튼 밖");
        d.on_event(
            &InputEvent::MouseDown {
                x: bx,
                y: by,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert!(
            d.bars.dragging() && !d.popout_pressed,
            "버튼 밖 = 바 드래그"
        );
        d.on_event(&InputEvent::MouseUp { x: bx, y: by }, &mut inv);
        assert!(!d.take_popout());
    }

    #[test]
    fn trackpad_small_deltas_accumulate_into_scroll() {
        // 10-02 QA: 트랙패드 delta 8씩 = 노치 1/15 — 종전 정수 나눗셈은 0줄(무반응)
        let mut inv = Invalidations::default();
        let mut d = InfoDock::new("정보", 20, 6);
        d.set_bounds(Rect::new(0, 100, 400, 120), &mut inv);
        d.set_lines((0..30).map(|i| format!("l{i}")).collect(), &mut inv);
        for _ in 0..15 {
            d.on_event(&InputEvent::Wheel { delta: -8 }, &mut inv);
        }
        assert_eq!(d.scroll, 3, "누적 120 = 3줄");
        for _ in 0..5 {
            d.on_event(&InputEvent::Wheel { delta: 8 }, &mut inv);
        }
        assert_eq!(d.scroll, 2, "역방향 40 = 1줄");
    }

    #[test]
    fn trackpad_half_line_scrolls_by_pixels_and_notch_snaps() {
        // 10-02: 노치 미만 delta = 픽셀(60 = 1.5줄 = 30px → 줄 1 + 10px), 노치 = 줄 단위 스냅
        let mut inv = Invalidations::default();
        let mut d = InfoDock::new("정보", 20, 6);
        d.set_bounds(Rect::new(0, 100, 400, 120), &mut inv);
        d.set_lines((0..30).map(|i| format!("l{i}")).collect(), &mut inv);
        d.on_event(&InputEvent::Wheel { delta: -60 }, &mut inv);
        assert_eq!((d.scroll, d.scroll_frac), (1, 10));
        assert_eq!(d.line_origin(), 121 - 10);
        d.paint_dock(&mut Probe, &Theme::dark());
        assert_eq!(d.line_at(6, 121), Some(1), "10px 가려진 줄 1이 첫 줄");
        assert_eq!(d.line_at(6, 131), Some(2));
        d.on_event(&InputEvent::Wheel { delta: -120 }, &mut inv);
        assert_eq!((d.scroll, d.scroll_frac), (4, 0), "노치 = 3줄 + 스냅");
    }

    #[test]
    fn same_key_update_keeps_scroll_and_selection() {
        // 10-02 QA: 상세 도착·액세스 시각 갱신이 스크롤을 0으로 튕기던 결함
        let mut inv = Invalidations::default();
        let mut d = InfoDock::new("정보", 20, 6);
        d.set_bounds(Rect::new(0, 100, 400, 120), &mut inv);
        let mk = |n: usize, tag: &str| (0..n).map(|i| format!("{tag}{i}")).collect::<Vec<_>>();
        d.set_content("a.pptx", mk(10, "x"), &mut inv);
        d.on_event(&InputEvent::Wheel { delta: -120 }, &mut inv);
        assert_eq!(d.scroll, 3);
        d.sel = Some(((3, 0), (6, 1)));
        d.set_content("a.pptx", mk(20, "y"), &mut inv);
        assert_eq!(d.scroll, 3, "같은 대상 갱신 = 스크롤 유지");
        assert!(d.sel.is_some(), "범위 안 선택 유지");
        d.set_content("a.pptx", mk(5, "z"), &mut inv);
        assert_eq!(d.scroll, 1, "줄 수 감소 = 상한 클램프(5-4)");
        assert!(d.sel.is_none(), "범위 밖 선택 해제");
        d.set_content("b.pptx", mk(20, "w"), &mut inv);
        assert_eq!(d.scroll, 0, "대상 전환 = 리셋");
    }

    #[test]
    fn set_lines_invalidates_only_on_change() {
        let mut inv = Invalidations::default();
        let mut d = InfoDock::new("정보", 20, 6);
        d.set_bounds(Rect::new(0, 100, 400, 120), &mut inv);
        let _ = inv.drain().count();
        d.set_lines(vec!["a.txt".into(), "크기: 10 B".into()], &mut inv);
        assert!(
            inv.drain().count() >= 1,
            "내용 변경 = 무효화(+바 스트립 10-02)"
        );
        d.set_lines(vec!["a.txt".into(), "크기: 10 B".into()], &mut inv);
        assert_eq!(inv.drain().count(), 0, "동일 내용 = 무비용");
        d.paint_dock(&mut Probe, &Theme::dark()); // 렌더 스모크
    }
}

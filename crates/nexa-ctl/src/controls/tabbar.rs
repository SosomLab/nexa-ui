//! **탭 바** — `nexa-dir2/crates/nexa-gui/src/widgets/tabbar.rs` 이식(사용자 요청 09-14 · U-2).
//!
//! 도메인 비종속: 제목·잠금·핀 목록만 알고, 동작은 [`TabAction`]으로 호스트에 통지
//! ([`TabBar::take_action`] 1회성). 두 배치 모드:
//!
//! - **단일행**(기본 · 편집기 탭): 폭을 넘치면 오른쪽에 ◀ ▶ 스크롤 버튼이 나타나고 띠가
//!   스크롤된다(휠·가로 휠 포함). 활성 탭이 바뀌면 다음 페인트에서 **보이도록 스크롤**한다.
//! - **다중행**([`TabBar::set_multiline`] · Golden식): 폭을 넘치면 다음 줄로 접는다.
//!   줄 수는 paint가 측정·캐시([`TabBar::lines`])하고 호스트가
//!   [`TabBar::take_lines_changed`]를 보고 재레이아웃한다(텍스트 측정은 paint에서만 가능).
//!
//! 공통: 활성 탭 = 패널 배경 + 상단 accent 줄(dir2 규약) · hover/pressed =
//! [`State`] 상태 레이어 · 닫기(×) = hover·활성 탭에만 표시(잠긴 탭은 자물쇠) ·
//! 드래그 재정렬(대상 탭 중간점 통과 시 [`TabAction::Move`]) · 우클릭 = [`TabAction::Context`].
//! 가운데 클릭 닫기는 [`InputEvent`]에 버튼 구분이 없어 호스트가 [`TabBar::middle_down`]을 부른다.

use std::cell::{Cell, RefCell};

use super::{draw_chevron_right, Control, ControlBase};
use crate::draw::{DrawCtx, FontSlot};
use crate::event::{InputEvent, WHEEL_DELTA};
use crate::geom::{Point, Rect};
use crate::theme::{Color, Theme};
use crate::tokens::{hover_alpha, space, State};
use crate::widget::{Invalidations, Widget};

/// 호스트가 수행할 탭 동작(dir2 변형 그대로).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TabAction {
    /// 탭 전환(본체 클릭).
    Switch(usize),
    /// 탭 닫기(× · 가운데 클릭). 마지막 탭·잠긴 탭은 나오지 않는다.
    Close(usize),
    /// 새 탭([+]).
    New,
    /// 드래그 재정렬 — `from` 탭을 `to` 위치로. 호스트가 반영(`set_tabs`)하면 연속 드래그.
    Move {
        /// 잡은 탭.
        from: usize,
        /// 목적지.
        to: usize,
    },
    /// 탭 우클릭(컨텍스트 메뉴는 호스트가 표시 — 잠금/핀/복제/닫기).
    Context(usize),
}

/// 히트 존.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Zone {
    /// 탭 `i` — `true`면 닫기(×) 상자 위.
    Tab(usize, bool),
    /// [+] 새 탭.
    Plus,
    /// ◀ 스크롤(단일행 넘침).
    Left,
    /// ▶ 스크롤(단일행 넘침).
    Right,
}

/// 페인트가 채우고 히트 테스트가 읽는 배치 캐시(창 클라이언트 좌표 · 스크롤 반영).
#[derive(Debug, Default, Clone)]
struct Layout {
    tabs: Vec<Rect>,
    plus: Rect,
    /// 탭이 보이는 띠(단일행 넘침 = 버튼 제외 · 그 외 = bounds).
    strip: Rect,
    /// 단일행 콘텐츠 총 폭(탭 + [+]).
    content_w: i32,
    left_btn: Rect,
    right_btn: Rect,
}

/// 기본 한 줄 높이(논리 px).
const DEFAULT_ROW_H: i32 = 28;
/// 드래그 시작 임계(px — 가로/세로 공통).
const DRAG_THRESHOLD: i32 = 8;
/// 탭 최대 폭(논리 px) — 긴 제목은 잘린다.
const MAX_TAB_W: i32 = 240;
/// 닫기(×)/자물쇠 상자 한 변(논리 px).
const CLOSE_BOX: i32 = 16;
/// 핀 표식 지름(논리 px).
const PIN_MARK: i32 = 6;
/// 휠 1노치당 스크롤(논리 px).
const WHEEL_STEP: i32 = 48;

/// 탭 바 컨트롤.
#[derive(Debug)]
pub struct TabBar {
    base: ControlBase,
    titles: Vec<String>,
    /// 탭별 잠금(닫기 제외). titles와 인덱스 정렬(부족분 = false).
    locked: Vec<bool>,
    /// 탭별 고정(핀 그룹 앞 정렬은 호스트 몫).
    pinned: Vec<bool>,
    active: usize,
    multiline: bool,
    show_new: bool,
    /// 마지막 탭도 닫을 수 있는가(기본 false — dir2 규약).
    close_last: bool,
    row_h: i32,
    pad_x: i32,
    hover: Option<Zone>,
    /// 프레스 중인 버튼 존(×·[+]·◀▶ — 해제 시 같은 존이면 동작).
    pressed: Option<Zone>,
    /// 드래그 재정렬: (잡은 탭, 프레스 좌표, 임계 통과 여부).
    drag: Option<(usize, Point, bool)>,
    pending: Option<TabAction>,
    /// 단일행 스크롤 오프셋(물리 px) — paint가 클램프한다.
    scroll_x: Cell<i32>,
    /// 다음 페인트에서 활성 탭이 보이도록 스크롤(1회성).
    ensure_active: Cell<bool>,
    layout: RefCell<Layout>,
    lines: Cell<usize>,
    lines_changed: Cell<bool>,
}

impl Default for TabBar {
    fn default() -> Self {
        Self::new()
    }
}

impl TabBar {
    /// 빈 탭 바(단일행 · [+] 표시 · 줄 높이 28 · 좌우 여백 [`space::S`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            base: ControlBase::default(),
            titles: Vec::new(),
            locked: Vec::new(),
            pinned: Vec::new(),
            active: 0,
            multiline: false,
            show_new: true,
            close_last: false,
            row_h: DEFAULT_ROW_H,
            pad_x: space::S,
            hover: None,
            pressed: None,
            drag: None,
            pending: None,
            scroll_x: Cell::new(0),
            ensure_active: Cell::new(true),
            layout: RefCell::new(Layout::default()),
            lines: Cell::new(1),
            lines_changed: Cell::new(false),
        }
    }

    /// 탭 목록·활성 인덱스 교체. 활성 탭은 다음 페인트에서 보이도록 스크롤된다.
    pub fn set_tabs(&mut self, titles: Vec<String>, active: usize, inv: &mut Invalidations) {
        self.titles = titles;
        self.active = active.min(self.titles.len().saturating_sub(1));
        self.hover = None;
        self.pressed = None;
        self.ensure_active.set(true);
        inv.push(self.base.bounds);
    }

    /// 활성 탭 변경(제목 유지) — 다음 페인트에서 보이도록 스크롤.
    pub fn set_active(&mut self, active: usize, inv: &mut Invalidations) {
        let a = active.min(self.titles.len().saturating_sub(1));
        if a != self.active || !self.titles.is_empty() {
            self.active = a;
            self.ensure_active.set(true);
            inv.push(self.base.bounds);
        }
    }

    /// 활성 탭 인덱스.
    #[must_use]
    pub fn active(&self) -> usize {
        self.active
    }

    /// 탭 수.
    #[must_use]
    pub fn len(&self) -> usize {
        self.titles.len()
    }

    /// 탭이 없는가.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.titles.is_empty()
    }

    /// 탭별 잠금 표시 갱신(잠긴 탭 = 닫기 불가 · 자물쇠 표식).
    pub fn set_locked(&mut self, locked: Vec<bool>, inv: &mut Invalidations) {
        if self.locked != locked {
            self.locked = locked;
            inv.push(self.base.bounds);
        }
    }

    /// 탭별 고정 표시 갱신(핀 점 표식).
    pub fn set_pinned(&mut self, pinned: Vec<bool>, inv: &mut Invalidations) {
        if self.pinned != pinned {
            self.pinned = pinned;
            inv.push(self.base.bounds);
        }
    }

    /// 다중행 모드(true = 폭 초과 시 줄바꿈 · false = 단일행 + ◀▶ 스크롤).
    /// 호스트가 [`TabBar::preferred_height`]로 재레이아웃(`set_bounds`)한다.
    pub fn set_multiline(&mut self, on: bool) {
        if self.multiline != on {
            self.multiline = on;
            self.scroll_x.set(0);
            self.ensure_active.set(true);
            self.lines_changed.set(true);
        }
    }

    /// 다중행 모드 여부.
    #[must_use]
    pub fn is_multiline(&self) -> bool {
        self.multiline
    }

    /// [+] 새 탭 버튼 표시 여부(기본 true).
    pub fn set_show_new(&mut self, on: bool) {
        self.show_new = on;
    }

    /// 마지막 탭도 닫을 수 있게(기본 false — 마지막 탭 × 클릭은 전환).
    pub fn set_close_last(&mut self, on: bool) {
        self.close_last = on;
    }

    /// 한 줄 높이·좌우 여백(논리 px) 지정.
    pub fn set_metrics(&mut self, row_h: i32, pad_x: i32, inv: &mut Invalidations) {
        self.row_h = row_h.max(1);
        self.pad_x = pad_x.max(0);
        inv.push(self.base.bounds);
    }

    /// 한 줄 높이(논리 px).
    #[must_use]
    pub fn row_height(&self) -> i32 {
        self.row_h
    }

    /// 권장 높이(논리 px) = 줄 수 × 줄 높이. 단일행은 항상 1줄 · 다중행은 마지막
    /// 페인트가 측정한 [`TabBar::lines`](첫 페인트 전 = 1).
    #[must_use]
    pub fn preferred_height(&self) -> i32 {
        let rows = if self.multiline { self.lines() } else { 1 };
        rows as i32 * self.row_h
    }

    /// 마지막 페인트가 계산한 필요 줄 수(≥1).
    #[must_use]
    pub fn lines(&self) -> usize {
        self.lines.get().max(1)
    }

    /// 줄 수 변경 표지 수거(1회성) — `true`면 호스트가 재레이아웃해야 한다.
    pub fn take_lines_changed(&self) -> bool {
        self.lines_changed.replace(false)
    }

    /// 호스트가 수거할 탭 동작(1회성).
    pub fn take_action(&mut self) -> Option<TabAction> {
        self.pending.take()
    }

    /// 탭 본체 히트(× 상자 포함) — 호스트의 더블클릭 라우팅용.
    #[must_use]
    pub fn tab_index_at(&self, x: i32, y: i32) -> Option<usize> {
        match self.zone_at(x, y) {
            Some(Zone::Tab(i, _)) => Some(i),
            _ => None,
        }
    }

    /// 마지막 페인트 기준 탭 `i`의 rect(스크롤 반영 · 잘림 전).
    #[must_use]
    pub fn tab_rect(&self, i: usize) -> Option<Rect> {
        self.layout.borrow().tabs.get(i).copied()
    }

    /// 탭 바 안 빈 공간(탭·[+]·◀▶ 밖) 판정 — 더블클릭 = 새 탭 등.
    #[must_use]
    pub fn empty_area_at(&self, x: i32, y: i32) -> bool {
        self.base.bounds.contains(Point { x, y }) && self.zone_at(x, y).is_none()
    }

    /// **가운데 클릭** = 탭 닫기. [`InputEvent`]에 버튼 구분이 없어 호스트가 부른다.
    pub fn middle_down(&mut self, x: i32, y: i32, inv: &mut Invalidations) {
        if let Some(Zone::Tab(i, _)) = self.zone_at(x, y) {
            if self.can_close(i) {
                self.pending = Some(TabAction::Close(i));
                inv.push(self.base.bounds);
            }
        }
    }

    /// 진행 중 탭 드래그(임계 통과분만) — 호스트의 패널 간 이동 판정용.
    #[must_use]
    pub fn dragging(&self) -> Option<usize> {
        self.drag.and_then(|(i, _, started)| started.then_some(i))
    }

    /// 드래그 상태 강제 해제(호스트가 이동을 수행한 뒤 — 인덱스 무효화 방어).
    pub fn cancel_drag(&mut self) {
        self.drag = None;
    }

    /// 프레스된 탭(드래그 시작 전 후보 포함) — 호스트의 ESC 취소 스냅샷용.
    #[must_use]
    pub fn pressed_tab(&self) -> Option<usize> {
        self.drag.map(|(i, _, _)| i)
    }

    /// 호스트 주도 드래그 시작(패널 간 이양) — 탭 `index`를 잡은 상태(임계 통과 취급).
    pub fn begin_drag(&mut self, index: usize, x: i32, y: i32) {
        self.drag = Some((index, Point { x, y }, true));
    }

    /// 단일행 스크롤 오프셋(물리 px · 마지막 페인트 기준).
    #[must_use]
    pub fn scroll_x(&self) -> i32 {
        self.scroll_x.get()
    }

    /// 단일행 띠를 `dx`(물리 px · 양수 = 오른쪽 내용 보기)만큼 스크롤 — 마지막 페인트의
    /// 범위로 클램프. 다중행에서는 무시.
    pub fn scroll_by(&mut self, dx: i32, inv: &mut Invalidations) {
        if self.multiline || dx == 0 {
            return;
        }
        let max = self.max_scroll();
        let v = (self.scroll_x.get() + dx).clamp(0, max);
        if v != self.scroll_x.get() {
            self.scroll_x.set(v);
            inv.push(self.base.bounds);
        }
    }

    fn max_scroll(&self) -> i32 {
        let lay = self.layout.borrow();
        (lay.content_w - lay.strip.w).max(0)
    }

    fn is_locked(&self, i: usize) -> bool {
        self.locked.get(i).copied().unwrap_or(false)
    }

    fn is_pinned(&self, i: usize) -> bool {
        self.pinned.get(i).copied().unwrap_or(false)
    }

    fn can_close(&self, i: usize) -> bool {
        i < self.titles.len() && !self.is_locked(i) && (self.titles.len() > 1 || self.close_last)
    }

    /// 닫기(×)/자물쇠 상자 — 탭 오른쪽 여백 안쪽.
    fn close_rect(&self, cell: Rect) -> Rect {
        let d = self.s(CLOSE_BOX);
        Rect::new(
            cell.right() - self.s(self.pad_x) - d,
            cell.y + (cell.h - d) / 2,
            d,
            d,
        )
    }

    fn zone_at(&self, x: i32, y: i32) -> Option<Zone> {
        let p = Point { x, y };
        if !self.base.bounds.contains(p) {
            return None;
        }
        let lay = self.layout.borrow();
        if lay.left_btn.contains(p) {
            return Some(Zone::Left);
        }
        if lay.right_btn.contains(p) {
            return Some(Zone::Right);
        }
        if !lay.strip.contains(p) {
            return None;
        }
        for (i, r) in lay.tabs.iter().enumerate() {
            if r.w > 0 && r.contains(p) {
                let close = !self.is_locked(i) && self.close_rect(*r).contains(p);
                return Some(Zone::Tab(i, close));
            }
        }
        if lay.plus.w > 0 && lay.plus.contains(p) {
            return Some(Zone::Plus);
        }
        None
    }

    /// ◀/▶ 한 단계 — 부분적으로 가려진 다음/이전 탭을 온전히 드러낸다.
    fn scroll_step(&mut self, dir: Zone, inv: &mut Invalidations) {
        let (target, max) = {
            let lay = self.layout.borrow();
            let strip = lay.strip;
            let cur = self.scroll_x.get();
            let max = (lay.content_w - strip.w).max(0);
            let t = match dir {
                Zone::Right => lay
                    .tabs
                    .iter()
                    .find(|r| r.right() > strip.right())
                    .map_or(max, |r| cur + (r.right() - strip.right())),
                _ => lay
                    .tabs
                    .iter()
                    .rev()
                    .find(|r| r.x < strip.x)
                    .map_or(0, |r| cur - (strip.x - r.x)),
            };
            (t, max)
        };
        self.scroll_x.set(target.clamp(0, max));
        inv.push(self.base.bounds);
    }

    /// 드래그 중 재정렬 판정 — 대상 탭의 x 중간점을 통과한 순간에만 스냅(dir2 QA 07-14).
    /// 다중행에서 아래 줄 = 항상 뒤 인덱스라 중간점 규칙이 그대로 성립.
    fn drag_move(&mut self, x: i32, y: i32, inv: &mut Invalidations) {
        let Some((from, press, started)) = self.drag else {
            return;
        };
        let begun =
            started || (x - press.x).abs() > DRAG_THRESHOLD || (y - press.y).abs() > DRAG_THRESHOLD;
        if !begun {
            return;
        }
        if let Some(Zone::Tab(to, _)) = self.zone_at(x, y) {
            let crossed = to != from && {
                let r = self.layout.borrow().tabs[to];
                let mid = r.x + r.w / 2;
                (to > from && x >= mid) || (to < from && x <= mid)
            };
            if crossed {
                self.pending = Some(TabAction::Move { from, to });
                self.drag = Some((to, Point { x, y }, true));
                inv.push(self.base.bounds);
                return;
            }
        }
        if !started {
            self.drag = Some((from, press, true));
            inv.push(self.base.bounds);
        }
    }

    fn draw_close_glyph(&self, ctx: &mut dyn DrawCtx, r: Rect, color: Color) {
        let m = r.w * 3 / 10;
        let w = (r.w as f32 / 11.0).max(1.2);
        ctx.polyline(
            &[(r.x + m, r.y + m), (r.right() - m, r.bottom() - m)],
            color,
            w,
        );
        ctx.polyline(
            &[(r.right() - m, r.y + m), (r.x + m, r.bottom() - m)],
            color,
            w,
        );
    }

    fn draw_lock_glyph(&self, ctx: &mut dyn DrawCtx, r: Rect, color: Color) {
        // 몸통(아래 절반 채움) + 걸쇠(위 ∩ 꺾은선).
        let bw = (r.w / 2).max(4);
        let bh = (r.h * 3 / 8).max(3);
        let body = Rect::new(r.x + (r.w - bw) / 2, r.y + r.h / 2, bw, bh);
        ctx.fill_round_rect(body, (bw / 5).max(1), color);
        let inset = (bw / 4).max(1);
        let top = body.y - (r.h / 4).max(2);
        ctx.polyline(
            &[
                (body.x + inset, body.y),
                (body.x + inset, top),
                (body.right() - inset, top),
                (body.right() - inset, body.y),
            ],
            color,
            (r.w as f32 / 11.0).max(1.2),
        );
    }

    fn draw_plus_glyph(&self, ctx: &mut dyn DrawCtx, r: Rect, color: Color) {
        let cx = r.x + r.w / 2;
        let cy = r.y + r.h / 2;
        let half = (r.h / 5).max(3);
        let w = (r.h as f32 / 14.0).max(1.2);
        ctx.polyline(&[(cx - half, cy), (cx + half, cy)], color, w);
        ctx.polyline(&[(cx, cy - half), (cx, cy + half)], color, w);
    }

    fn draw_chevron_left(&self, ctx: &mut dyn DrawCtx, area: Rect, color: Color) {
        let cx = area.x + area.w / 2;
        let cy = area.y + area.h / 2;
        let half = (area.h / 5).max(2);
        let w = (area.w as f32 / 10.0).max(1.5);
        ctx.polyline(
            &[
                (cx + half / 2, cy - half),
                (cx - half / 2, cy),
                (cx + half / 2, cy + half),
            ],
            color,
            w,
        );
    }

    /// 배치 계산(paint 전반부) — 폭 측정·줄바꿈/스크롤 클램프. 반환 = (배치, 줄 수).
    fn compute_layout(&self, ctx: &mut dyn DrawCtx) -> (Layout, usize) {
        let b = self.base.bounds;
        let pad = self.s(self.pad_x);
        let gap = self.s(space::XS);
        let close = self.s(CLOSE_BOX);
        let mark = self.s(PIN_MARK);
        let lh = if self.multiline {
            self.s(self.row_h).clamp(1, b.h.max(1))
        } else {
            b.h.max(1)
        };
        let max_w = self.s(MAX_TAB_W).min(b.w).max(1);
        let widths: Vec<i32> = self
            .titles
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let m = if self.is_pinned(i) { mark + gap } else { 0 };
                (pad + m + ctx.text_width(t) + gap + close + pad).min(max_w)
            })
            .collect();
        let plus_w = if self.show_new { lh } else { 0 };

        let mut lay = Layout::default();
        let mut rows = 1usize;
        if self.multiline {
            lay.strip = b;
            lay.content_w = b.w;
            let mut x = b.x;
            let mut row = 0i32;
            for &w in &widths {
                if x + w > b.right() && x > b.x {
                    row += 1;
                    x = b.x;
                }
                lay.tabs.push(Rect::new(x, b.y + row * lh, w, lh));
                x += w;
            }
            if plus_w > 0 {
                if x + plus_w > b.right() && x > b.x {
                    row += 1;
                    x = b.x;
                }
                lay.plus = Rect::new(x, b.y + row * lh, plus_w, lh);
            }
            rows = row as usize + 1;
            self.scroll_x.set(0);
        } else {
            let content = widths.iter().sum::<i32>() + plus_w;
            lay.content_w = content;
            let mut strip = Rect::new(b.x, b.y, b.w, lh);
            if content > b.w {
                let bw = lh.min(b.w / 2);
                strip.w = (b.w - bw * 2).max(0);
                lay.left_btn = Rect::new(strip.right(), b.y, bw, lh);
                lay.right_btn = Rect::new(strip.right() + bw, b.y, bw, lh);
            }
            lay.strip = strip;
            let max_scroll = (content - strip.w).max(0);
            let mut scroll = self.scroll_x.get();
            if self.ensure_active.take() {
                if let Some(&aw) = widths.get(self.active) {
                    let ax: i32 = widths.iter().take(self.active).sum();
                    if ax < scroll {
                        scroll = ax;
                    } else if ax + aw > scroll + strip.w {
                        scroll = ax + aw - strip.w;
                    }
                }
            }
            scroll = scroll.clamp(0, max_scroll);
            self.scroll_x.set(scroll);
            let mut x = strip.x - scroll;
            for &w in &widths {
                lay.tabs.push(Rect::new(x, b.y, w, lh));
                x += w;
            }
            if plus_w > 0 {
                lay.plus = Rect::new(x, b.y, plus_w, lh);
            }
        }
        (lay, rows)
    }
}

/// `inner`가 `clip` 안에 완전히 들어가는가(클립 없는 폴리라인 글리프 보호).
fn fully_inside(inner: Rect, clip: Rect) -> bool {
    !inner.is_empty()
        && inner.x >= clip.x
        && inner.y >= clip.y
        && inner.right() <= clip.right()
        && inner.bottom() <= clip.bottom()
}

impl Control for TabBar {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
}

impl Widget for TabBar {
    fn bounds(&self) -> Rect {
        self.base.bounds
    }

    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        if self.base.bounds != bounds {
            let old = self.base.bounds;
            self.base.bounds = bounds;
            self.ensure_active.set(true);
            inv.push(old.union(&bounds));
        }
    }

    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        match *ev {
            InputEvent::MouseDown { x, y, .. } => match self.zone_at(x, y) {
                Some(Zone::Tab(i, close)) => {
                    if close && self.can_close(i) {
                        // × 프레스 — 해제 시 같은 상자면 닫기.
                        self.pressed = Some(Zone::Tab(i, true));
                    } else {
                        self.pending = Some(TabAction::Switch(i));
                        // 본체 프레스 = 드래그 재정렬 후보.
                        self.drag = Some((i, Point { x, y }, false));
                    }
                    inv.push(self.base.bounds);
                }
                Some(z @ (Zone::Plus | Zone::Left | Zone::Right)) => {
                    self.pressed = Some(z);
                    if matches!(z, Zone::Left | Zone::Right) {
                        self.scroll_step(z, inv);
                    }
                    inv.push(self.base.bounds);
                }
                None => {}
            },
            InputEvent::MouseUp { x, y } => {
                self.drag = None;
                if let Some(p) = self.pressed.take() {
                    let over = self.zone_at(x, y);
                    match (p, over) {
                        (Zone::Tab(i, true), Some(Zone::Tab(j, true))) if i == j => {
                            if self.can_close(i) {
                                self.pending = Some(TabAction::Close(i));
                            }
                        }
                        (Zone::Plus, Some(Zone::Plus)) => self.pending = Some(TabAction::New),
                        _ => {}
                    }
                    inv.push(self.base.bounds);
                }
            }
            InputEvent::RightDown { x, y } => {
                if let Some(Zone::Tab(i, _)) = self.zone_at(x, y) {
                    self.pending = Some(TabAction::Context(i));
                }
            }
            InputEvent::MouseMove { x, y } => {
                if self.drag.is_some() {
                    self.drag_move(x, y, inv);
                    return;
                }
                let hover = self.zone_at(x, y);
                if hover != self.hover {
                    self.hover = hover;
                    inv.push(self.base.bounds);
                }
            }
            InputEvent::Wheel { delta } => {
                // 세로 휠: 위 = 왼쪽 내용 보기(브라우저 관례).
                let step = self.s(WHEEL_STEP);
                self.scroll_by(-delta * step / WHEEL_DELTA, inv);
            }
            InputEvent::HWheel { delta } => {
                let step = self.s(WHEEL_STEP);
                self.scroll_by(delta * step / WHEEL_DELTA, inv);
            }
            _ => {}
        }
    }

    fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.base.bounds;
        if b.is_empty() {
            *self.layout.borrow_mut() = Layout::default();
            return;
        }
        ctx.select_font(FontSlot::Base, false);
        ctx.fill_rect(b, theme.chrome_bg);
        let (lay, rows) = self.compute_layout(ctx);
        let strip = lay.strip;
        let pad = self.s(self.pad_x);
        let gap = self.s(space::XS);
        let mark = self.s(PIN_MARK);
        let th = ctx.text_height();
        let drag_idx = self.dragging();
        let hover_tab = match self.hover {
            Some(Zone::Tab(i, c)) => Some((i, c)),
            _ => None,
        };

        for (i, cell) in lay.tabs.iter().enumerate() {
            let clip = cell.intersection(&strip);
            if clip.is_empty() {
                continue;
            }
            let active = i == self.active;
            let hover = hover_tab.is_some_and(|(h, _)| h == i);
            let hover_close = hover_tab == Some((i, true));
            let locked = self.is_locked(i);
            if active {
                ctx.fill_rect(clip, theme.panel_bg);
                if hover {
                    ctx.fill_rect_alpha(clip, theme.text, hover_alpha(true, 1.0));
                }
                let line = Rect::new(cell.x, cell.y, cell.w, self.s(2).max(1)).intersection(&clip);
                if !line.is_empty() {
                    ctx.fill_rect(line, self.accent_now(theme));
                }
            } else {
                ctx.state_layer(
                    clip,
                    theme.text,
                    State::of(false, hover, drag_idx == Some(i), true),
                );
                let inset = self.s(space::S);
                let sep = Rect::new(cell.right() - 1, cell.y + inset, 1, cell.h - inset * 2)
                    .intersection(&clip);
                if !sep.is_empty() {
                    ctx.fill_rect(sep, theme.border);
                }
            }
            // 핀 점 표식 + 제목.
            let mut tx = cell.x + pad;
            if self.is_pinned(i) {
                let dot = Rect::new(tx, cell.y + (cell.h - mark) / 2, mark, mark);
                if fully_inside(dot, clip) {
                    ctx.fill_ellipse(dot, self.accent_now(theme));
                }
                tx += mark + gap;
            }
            let cr = self.close_rect(*cell);
            let text_clip = Rect::new(tx, cell.y, cr.x - gap - tx, cell.h).intersection(&clip);
            if !text_clip.is_empty() {
                let fg = if active { theme.text } else { theme.text_dim };
                ctx.text(
                    tx,
                    cell.y + (cell.h - th) / 2,
                    text_clip,
                    &self.titles[i],
                    fg,
                );
            }
            // 닫기 상자: 잠김 = 자물쇠 · hover/활성 = ×(상자 hover 시 상태 레이어).
            if fully_inside(cr, clip) {
                if locked {
                    self.draw_lock_glyph(ctx, cr, theme.text_dim);
                } else if hover || active {
                    let pressed = self.pressed == Some(Zone::Tab(i, true));
                    let st = State::of(false, hover_close, pressed, true);
                    if st.overlay_alpha() > 0.0 {
                        ctx.fill_round_rect_alpha(cr, cr.w / 2, theme.text, st.overlay_alpha());
                    }
                    let c = if hover_close {
                        theme.text
                    } else {
                        theme.text_dim
                    };
                    self.draw_close_glyph(ctx, cr, c);
                }
            }
        }

        // [+] 새 탭.
        let plus_clip = lay.plus.intersection(&strip);
        if fully_inside(lay.plus, plus_clip) {
            let st = State::of(
                false,
                self.hover == Some(Zone::Plus),
                self.pressed == Some(Zone::Plus),
                true,
            );
            ctx.state_layer(lay.plus, theme.text, st);
            self.draw_plus_glyph(ctx, lay.plus, theme.text_dim);
        }

        // ◀ ▶ (단일행 넘침) — 띠 밖으로 나간 탭을 덮고 그린다.
        if !lay.left_btn.is_empty() {
            let btns = lay.left_btn.union(&lay.right_btn);
            ctx.fill_rect(btns, theme.chrome_bg);
            ctx.fill_rect(Rect::new(btns.x, btns.y, 1, btns.h), theme.border);
            let scroll = self.scroll_x.get();
            let max = (lay.content_w - strip.w).max(0);
            for (z, r, enabled) in [
                (Zone::Left, lay.left_btn, scroll > 0),
                (Zone::Right, lay.right_btn, scroll < max),
            ] {
                let st = State::of(
                    false,
                    self.hover == Some(z),
                    self.pressed == Some(z),
                    enabled,
                );
                ctx.state_layer(r, theme.text, st);
                let c = if enabled { theme.text } else { theme.text_dim };
                if z == Zone::Left {
                    self.draw_chevron_left(ctx, r, c);
                } else {
                    draw_chevron_right(ctx, r, c);
                }
            }
        }

        ctx.fill_rect(Rect::new(b.x, b.bottom() - 1, b.w, 1), theme.border);
        *self.layout.borrow_mut() = lay;
        if self.lines.replace(rows) != rows {
            self.lines_changed.set(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::ProbeCtx;

    /// ProbeCtx: 글자 폭 7 · 글자 높이 16. pad 8 · gap 4 · 닫기 상자 16.
    fn bar_sized(
        titles: &[&str],
        active: usize,
        w: i32,
        h: i32,
        multiline: bool,
    ) -> (TabBar, Invalidations) {
        let mut inv = Invalidations::default();
        let mut t = TabBar::new();
        t.set_multiline(multiline);
        t.set_bounds(Rect::new(0, 0, w, h), &mut inv);
        t.set_tabs(
            titles.iter().map(|s| (*s).to_string()).collect(),
            active,
            &mut inv,
        );
        t.paint(&mut ProbeCtx, &Theme::dark());
        (t, inv)
    }

    fn bar(titles: &[&str], active: usize) -> (TabBar, Invalidations) {
        bar_sized(titles, active, 600, 28, false)
    }

    fn down(t: &mut TabBar, inv: &mut Invalidations, x: i32, y: i32) {
        t.on_event(
            &InputEvent::MouseDown {
                x,
                y,
                shift: false,
                primary: false,
            },
            inv,
        );
    }

    fn up(t: &mut TabBar, inv: &mut Invalidations, x: i32, y: i32) {
        t.on_event(&InputEvent::MouseUp { x, y }, inv);
    }

    fn mv(t: &mut TabBar, inv: &mut Invalidations, x: i32, y: i32) {
        t.on_event(&InputEvent::MouseMove { x, y }, inv);
    }

    fn click(t: &mut TabBar, inv: &mut Invalidations, x: i32, y: i32) {
        down(t, inv, x, y);
        up(t, inv, x, y);
    }

    fn center(r: Rect) -> (i32, i32) {
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    fn close_center(t: &TabBar, i: usize) -> (i32, i32) {
        center(t.close_rect(t.tab_rect(i).unwrap()))
    }

    #[test]
    fn click_switches_close_box_closes_and_plus_creates() {
        let (mut t, mut inv) = bar(&["alpha", "beta"], 0);
        // 탭0 폭 = 8+35+4+16+8 = 71 · 탭1 = 64 · [+] = 28.
        assert_eq!(t.tab_rect(0), Some(Rect::new(0, 0, 71, 28)));
        assert_eq!(t.tab_rect(1), Some(Rect::new(71, 0, 64, 28)));
        click(&mut t, &mut inv, 10, 14);
        assert_eq!(t.take_action(), Some(TabAction::Switch(0)));
        let (cx, cy) = close_center(&t, 1);
        click(&mut t, &mut inv, cx, cy);
        assert_eq!(t.take_action(), Some(TabAction::Close(1)));
        let (px, py) = center(t.layout.borrow().plus);
        click(&mut t, &mut inv, px, py);
        assert_eq!(t.take_action(), Some(TabAction::New));
        click(&mut t, &mut inv, 500, 14);
        assert_eq!(t.take_action(), None);
        assert!(t.empty_area_at(500, 14));
    }

    #[test]
    fn close_release_outside_cancels() {
        let (mut t, mut inv) = bar(&["alpha", "beta"], 0);
        let (cx, cy) = close_center(&t, 1);
        down(&mut t, &mut inv, cx, cy);
        assert_eq!(t.take_action(), None, "프레스만으로 닫지 않는다");
        up(&mut t, &mut inv, 10, 14);
        assert_eq!(t.take_action(), None);
    }

    #[test]
    fn last_tab_close_box_switches_instead() {
        let (mut t, mut inv) = bar(&["only"], 0);
        let (cx, cy) = close_center(&t, 0);
        click(&mut t, &mut inv, cx, cy);
        assert_eq!(
            t.take_action(),
            Some(TabAction::Switch(0)),
            "마지막 탭은 닫기 불가"
        );
        t.set_close_last(true);
        click(&mut t, &mut inv, cx, cy);
        assert_eq!(t.take_action(), Some(TabAction::Close(0)));
    }

    #[test]
    fn locked_tab_is_not_closable_but_switches() {
        let (mut t, mut inv) = bar(&["alpha", "beta"], 0);
        t.set_locked(vec![false, true], &mut inv);
        let (cx, cy) = close_center(&t, 1);
        click(&mut t, &mut inv, cx, cy);
        assert_eq!(t.take_action(), Some(TabAction::Switch(1)));
        t.middle_down(cx, cy, &mut inv);
        assert_eq!(t.take_action(), None, "가운데 클릭도 잠긴 탭은 무시");
    }

    #[test]
    fn middle_click_closes_and_right_click_reports_context() {
        let (mut t, mut inv) = bar(&["alpha", "beta"], 0);
        t.middle_down(10, 14, &mut inv);
        assert_eq!(t.take_action(), Some(TabAction::Close(0)));
        t.on_event(&InputEvent::RightDown { x: 80, y: 14 }, &mut inv);
        assert_eq!(t.take_action(), Some(TabAction::Context(1)));
    }

    #[test]
    fn hover_tracks_tabs_and_close_box() {
        let (mut t, mut inv) = bar(&["alpha", "beta"], 0);
        mv(&mut t, &mut inv, 80, 14);
        assert_eq!(t.hover, Some(Zone::Tab(1, false)));
        assert_eq!(t.tab_index_at(80, 14), Some(1));
        let (cx, cy) = close_center(&t, 1);
        mv(&mut t, &mut inv, cx, cy);
        assert_eq!(t.hover, Some(Zone::Tab(1, true)));
        mv(&mut t, &mut inv, 80, 50);
        assert_eq!(t.hover, None);
    }

    #[test]
    fn pinned_tab_reserves_mark_width() {
        let (mut t, mut inv) = bar(&["alpha", "beta"], 0);
        t.set_pinned(vec![true, false], &mut inv);
        t.paint(&mut ProbeCtx, &Theme::dark());
        assert_eq!(t.tab_rect(0).map(|r| r.w), Some(71 + 6 + 4));
    }

    #[test]
    fn single_line_fits_without_scroll_buttons() {
        let (t, _) = bar(&["alpha", "beta"], 0);
        let lay = t.layout.borrow();
        assert!(lay.left_btn.is_empty() && lay.right_btn.is_empty());
        assert_eq!(lay.strip, Rect::new(0, 0, 600, 28));
        assert_eq!(t.preferred_height(), DEFAULT_ROW_H);
        assert_eq!(t.lines(), 1);
    }

    #[test]
    fn single_line_overflow_scrolls_to_keep_active_visible() {
        let titles: Vec<String> = (0..8).map(|i| format!("tab-{i}")).collect();
        let refs: Vec<&str> = titles.iter().map(String::as_str).collect();
        // 탭 폭 = 8+35+4+16+8 = 71 × 8 + [+] 28 = 596 > 200 → 넘침.
        let (mut t, mut inv) = bar_sized(&refs, 7, 200, 28, false);
        let (strip, lb, rb) = {
            let lay = t.layout.borrow();
            (lay.strip, lay.left_btn, lay.right_btn)
        };
        assert_eq!(
            strip,
            Rect::new(0, 0, 200 - 56, 28),
            "◀▶ 만큼 띠가 줄어든다"
        );
        assert_eq!(lb, Rect::new(144, 0, 28, 28));
        assert_eq!(rb, Rect::new(172, 0, 28, 28));
        let r7 = t.tab_rect(7).unwrap();
        assert!(
            r7.x >= strip.x && r7.right() <= strip.right(),
            "활성 탭 7이 띠 안에 온전히: {r7:?}"
        );
        assert!(t.scroll_x() > 0);
        assert_eq!(t.tab_index_at(r7.x + 5, 14), Some(7));
        let r0 = t.tab_rect(0).unwrap();
        assert!(r0.right() <= strip.x, "탭0은 띠 왼쪽으로 밀려난다: {r0:?}");
        assert_eq!(
            t.tab_index_at(r0.x + 5, 14),
            None,
            "왼쪽으로 밀려난 탭은 히트 불가"
        );

        // 활성 → 0: 다음 페인트에서 맨 앞으로.
        t.set_active(0, &mut inv);
        t.paint(&mut ProbeCtx, &Theme::dark());
        assert_eq!(t.scroll_x(), 0);
        assert_eq!(t.tab_rect(0).map(|r| r.x), Some(strip.x));

        // ▶ 클릭 = 부분 가려진 다음 탭(탭2 [142,213))을 온전히 드러낸다 → 213-144 = 69.
        let (bx, by) = center(rb);
        click(&mut t, &mut inv, bx, by);
        assert_eq!(t.scroll_x(), 69);
        t.paint(&mut ProbeCtx, &Theme::dark());
        assert_eq!(t.tab_rect(2).map(|r| r.right()), Some(strip.right()));
        // ◀ 클릭 = 왼쪽으로 가려진 마지막 탭(탭0)을 드러낸다 → 0.
        let (bx, by) = center(lb);
        click(&mut t, &mut inv, bx, by);
        assert_eq!(t.scroll_x(), 0);
        assert!(t.take_action().is_none(), "버튼은 동작을 만들지 않는다");

        // 휠: 위 노치 1 = 왼쪽(이미 0 → 그대로) · 아래 노치 1 = 48px 오른쪽.
        t.on_event(
            &InputEvent::Wheel {
                delta: -WHEEL_DELTA,
            },
            &mut inv,
        );
        assert_eq!(t.scroll_x(), 48);
        t.on_event(
            &InputEvent::HWheel {
                delta: -WHEEL_DELTA,
            },
            &mut inv,
        );
        assert_eq!(t.scroll_x(), 0);
        // 과대 스크롤은 페인트 범위로 클램프.
        t.scroll_by(10_000, &mut inv);
        assert_eq!(t.scroll_x(), 596 - 144);
    }

    #[test]
    fn multiline_wraps_into_rows_and_preferred_height_grows() {
        // 탭 폭 = 71 · [+] = 28. 폭 150 = 줄당 2탭 → 4탭 = 2줄 + [+]는 3번째 줄.
        let (t, _) = bar_sized(&["alpha", "betaa", "gamma", "delta"], 0, 150, 90, true);
        assert_eq!(t.lines(), 3, "2탭×2줄 + [+] 줄");
        assert!(t.take_lines_changed(), "1→3줄 변경 표지");
        assert!(!t.take_lines_changed(), "표지는 1회성");
        assert_eq!(t.preferred_height(), 3 * DEFAULT_ROW_H);
        assert_eq!(t.tab_rect(2), Some(Rect::new(0, 28, 71, 28)));
        assert_eq!(t.tab_index_at(10, 30), Some(2));
        assert_eq!(t.tab_index_at(10, 5), Some(0));
        assert_eq!(t.scroll_x(), 0, "다중행은 스크롤 없음");
        assert!(t.layout.borrow().left_btn.is_empty());
    }

    #[test]
    fn multiline_toggle_marks_lines_changed() {
        let (mut t, _) = bar(&["alpha", "beta"], 0);
        assert!(!t.take_lines_changed());
        t.set_multiline(true);
        assert!(t.is_multiline());
        assert!(t.take_lines_changed(), "모드 전환 = 재레이아웃 신호");
    }

    #[test]
    fn host_drag_handoff_api() {
        let (mut t, mut inv) = bar(&["alpha", "beta"], 0);
        down(&mut t, &mut inv, 10, 14);
        assert_eq!(t.pressed_tab(), Some(0), "프레스 = 후보");
        assert_eq!(t.dragging(), None, "임계 전 = 미시작");
        mv(&mut t, &mut inv, 30, 14);
        assert_eq!(t.dragging(), Some(0), "임계 통과 = 시작");
        t.cancel_drag();
        assert_eq!(t.pressed_tab(), None, "취소 = 소거");
        t.begin_drag(1, 100, 14);
        assert_eq!(t.dragging(), Some(1), "호스트 주도 시작(이양)");
    }

    #[test]
    fn drag_reorder_single_line_emits_move_on_midpoint() {
        let (mut t, mut inv) = bar(&["alpha", "beta", "gamma"], 0);
        down(&mut t, &mut inv, 10, 14);
        assert_eq!(t.take_action(), Some(TabAction::Switch(0)));
        // 탭1 = [71,135) 중간 103. 중간 전은 무동작.
        mv(&mut t, &mut inv, 90, 14);
        assert_eq!(t.take_action(), None);
        mv(&mut t, &mut inv, 110, 14);
        assert_eq!(t.take_action(), Some(TabAction::Move { from: 0, to: 1 }));
        // 호스트가 반영하면 잡은 인덱스 = 목적지 → 연속 드래그.
        assert_eq!(t.dragging(), Some(1));
        up(&mut t, &mut inv, 110, 14);
        assert_eq!(t.dragging(), None);
    }

    #[test]
    fn drag_moves_across_lines() {
        // 폭 150 = 줄당 2탭: 0줄=[탭0,탭1] · 1줄=[탭2,탭3].
        let (mut t, mut inv) = bar_sized(&["alpha", "betaa", "gamma", "delta"], 0, 150, 90, true);
        down(&mut t, &mut inv, 10, 5);
        assert_eq!(t.take_action(), Some(TabAction::Switch(0)));
        mv(&mut t, &mut inv, 40, 30);
        assert_eq!(
            t.take_action(),
            Some(TabAction::Move { from: 0, to: 2 }),
            "아래 줄로 드래그 이동"
        );
        mv(&mut t, &mut inv, 20, 5);
        assert_eq!(t.take_action(), Some(TabAction::Move { from: 2, to: 0 }));
        up(&mut t, &mut inv, 20, 5);
    }

    #[test]
    fn set_tabs_clamps_active_and_empty_bar_is_inert() {
        let (mut t, mut inv) = bar(&["alpha"], 5);
        assert_eq!(t.active(), 0);
        assert_eq!(t.len(), 1);
        t.set_tabs(Vec::new(), 0, &mut inv);
        t.paint(&mut ProbeCtx, &Theme::dark());
        assert!(t.is_empty());
        // 빈 바에도 [+]는 남는다(맨 앞) · 그 밖은 무동작.
        click(&mut t, &mut inv, 10, 14);
        assert_eq!(t.take_action(), Some(TabAction::New));
        click(&mut t, &mut inv, 100, 14);
        assert_eq!(t.take_action(), None);
        t.middle_down(10, 14, &mut inv);
        assert_eq!(t.take_action(), None);
    }
}

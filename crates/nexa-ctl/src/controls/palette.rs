//! 명령 팔레트 `Palette`(nexa-sql `palette.rs` 승격 · nexa-dir3 T-138 · UIK-104 · Sublime `Ctrl+⇧P`) — 입력 한 줄 + 퍼지 필터 목록.
//!
//! - 호스트가 [`Palette::set_items`]로 항목(`id` · 라벨 · 오른쪽 보조 글(단축키) · 사용 가능 여부)을 준다 · 글은 전부 호스트가
//!   [`PaletteStrings`]로 넣는다(i18n은 호스트 몫 · nexa-ctl 경계 규칙).
//! - 키: ↑↓ · PgUp/PgDn 이동 · Enter 실행 · Esc 닫기 · 바깥 클릭(좌 · 우) 닫기 · 행 클릭 실행 · hover = 선택 · 휠 = 굴리기.
//! - **프롬프트 모드**([`Palette::open_prompt_at`]): 목록 없이 글자 입력만(탭 이름 바꾸기 · 값 입력) · 대상 rect 옆에 붙는다.
//! - **줄 이동 모드**(선택 · [`Palette::set_goto_prefix`]): 질의가 접두어(`:`)로 시작하면 숫자를 [`PaletteAction::Goto`]로.
//! - 최근 실행([`Palette::set_recent`])은 같은 점수 안에서 앞에 온다(빈 질의 = 최근 순).
//! - 클립보드는 호스트 몫: 입력란의 편집 메뉴 요청은 [`Palette::take_edit_ctx`]로 꺼내 `copy_selection`/`cut_selection`/`paste`를 부른다.

use crate::controls::textbox::EditCtxAction;
use crate::draw::DrawCtx;
use crate::geom::{nudge_into, Point, Rect};
use crate::theme::Theme;
use crate::{Control, InputEvent, Invalidations, Key, TextBox, Widget};

/// 팔레트가 호스트에 요청하는 것.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaletteAction {
    None,
    /// 항목 실행(id).
    Pick(String),
    /// 프롬프트 모드의 입력 확정(`open_prompt*`의 id · 입력 글자).
    Prompt {
        id: String,
        text: String,
    },
    /// 줄 이동 모드의 확정(1 이상).
    Goto(usize),
    Close,
}

/// 항목 하나.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteItem {
    pub id: String,
    /// 검색 · 표시 라벨(예 `"파일: 새 탭"`).
    pub label: String,
    /// 오른쪽에 흐리게(단축키 등 · 검색 대상 아님).
    pub detail: String,
    /// 거짓이면 흐리게 보이고 고를 수 없다.
    pub enabled: bool,
}

impl PaletteItem {
    #[must_use]
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        PaletteItem {
            id: id.into(),
            label: label.into(),
            detail: String::new(),
            enabled: true,
        }
    }
    #[must_use]
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }
    #[must_use]
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }
}

/// 호스트가 넣는 글(i18n).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaletteStrings {
    /// 입력란 자리표시자(예 "명령 검색…").
    pub placeholder: String,
    /// 결과 없음.
    pub no_match: String,
    /// 줄 이동 안내(숫자 있음 · `{0}` = 줄 번호).
    pub goto_line: String,
    /// 줄 이동 안내(숫자 없음).
    pub goto_hint: String,
}

/// 보이는 최대 행 수.
pub const PALETTE_MAX_ROWS: usize = 12;
/// 최근 실행 가산점이 붙는 항목 수.
const RECENT_MAX: usize = 10;

#[derive(Debug)]
pub struct Palette {
    open: bool,
    input: TextBox,
    strings: PaletteStrings,
    items: Vec<PaletteItem>,
    /// 최근 실행 id(앞 = 가장 최근).
    recent: Vec<String>,
    /// 필터 결과 — items index(점수순).
    matches: Vec<usize>,
    sel: usize,
    /// 보이는 첫 행(전체 결과 기준).
    top: usize,
    /// 마지막으로 본 마우스 위치 — 같은 자리의 MouseMove(키보드 이동 직후 재발행)로 선택이 되돌아가지 않게.
    last_mouse: (i32, i32),
    bounds: Rect,
    row_h: i32,
    scale: f32,
    last_query: String,
    goto_prefix: Option<char>,
    /// 줄 이동 모드(`Some(None)` = 접두어만 · `Some(Some(n))` = 숫자 있음).
    goto: Option<Option<usize>>,
    /// 프롬프트 모드의 id.
    prompt: Option<String>,
    /// 프롬프트 모드의 안내 글(입력란 아래 한 줄).
    hint: String,
    /// 프롬프트가 붙을 대상 rect.
    anchor: Option<Rect>,
    /// 창 크기(물리 px) — 붙는 상자를 창 안에 두는 데 쓴다.
    win: (i32, i32),
}

impl Default for Palette {
    fn default() -> Self {
        Self::new(PaletteStrings::default())
    }
}

impl Palette {
    #[must_use]
    pub fn new(strings: PaletteStrings) -> Self {
        Palette {
            open: false,
            input: TextBox::new(&strings.placeholder).with_clearable(),
            strings,
            items: Vec::new(),
            recent: Vec::new(),
            matches: Vec::new(),
            sel: 0,
            top: 0,
            last_mouse: (i32::MIN, i32::MIN),
            bounds: Rect::new(0, 0, 0, 0),
            row_h: 26,
            scale: 1.0,
            last_query: String::new(),
            goto_prefix: None,
            goto: None,
            prompt: None,
            hint: String::new(),
            anchor: None,
            win: (0, 0),
        }
    }

    /// 글 바꾸기(언어 전환).
    pub fn set_strings(&mut self, strings: PaletteStrings) {
        self.strings = strings;
    }

    /// 줄 이동 접두어(기본 없음 · nexa-sql = `:`).
    pub fn set_goto_prefix(&mut self, prefix: Option<char>) {
        self.goto_prefix = prefix;
    }

    pub fn set_items(&mut self, items: Vec<PaletteItem>) {
        self.items = items;
        self.refilter(true);
    }

    /// 최근 실행 id(앞 = 가장 최근) — 같은 점수 안에서 앞에 온다.
    pub fn set_recent(&mut self, recent: Vec<String>) {
        self.recent = recent;
        self.refilter(true);
    }

    /// IME 조합 중 글자(preedit)를 입력 상자에 보인다 — 호스트가 IME 사건을 여기로 넘긴다.
    pub fn set_preedit(&mut self, text: &str, inv: &mut Invalidations) {
        self.input.set_preedit(text, inv);
    }

    /// 지금 검색어.
    #[must_use]
    pub fn query(&self) -> String {
        self.input.text()
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open
    }

    #[must_use]
    pub fn is_prompt(&self) -> bool {
        self.prompt.is_some()
    }

    /// 명령 모드로 열기(`prefill` = 초기 질의).
    pub fn open(&mut self, prefill: &str) {
        self.open = true;
        self.prompt = None;
        self.anchor = None;
        self.input = TextBox::new(&self.strings.placeholder)
            .with_clearable()
            .with_text(prefill);
        self.input.set_scale(self.scale);
        self.input.set_focused(true);
        let mut inv = Invalidations::default();
        self.layout(&mut inv);
        self.last_query.clear();
        self.refilter(true);
    }

    /// 프롬프트 모드로 열기 — `id`는 확정 때 그대로 돌려준다 · `placeholder` 안내 · `initial` 초기 글자(전체 선택) ·
    /// `anchor` = 붙을 대상(없으면 창 위 가운데).
    pub fn open_prompt_at(
        &mut self,
        id: &str,
        placeholder: &str,
        initial: &str,
        anchor: Option<Rect>,
    ) {
        self.open = true;
        self.anchor = anchor;
        self.prompt = Some(id.to_string());
        self.hint = placeholder.to_string();
        self.input = TextBox::new(placeholder).with_text(initial);
        self.input.set_scale(self.scale);
        self.input.set_focused(true);
        let mut inv = Invalidations::default();
        self.input.on_event(&InputEvent::SelectAll, &mut inv);
        self.layout(&mut inv);
        self.matches.clear();
        self.goto = None;
        self.sel = 0;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.prompt = None;
        self.anchor = None;
        self.goto = None;
        self.input.set_focused(false);
    }

    /// 창 폭 · 상단 y(메뉴/툴바 아래) · 스케일.
    pub fn set_bounds(&mut self, win_w: i32, top: i32, scale: f32) {
        self.scale = scale;
        self.row_h = (26.0 * scale) as i32;
        let w = ((560.0 * scale) as i32)
            .min(win_w - (32.0 * scale) as i32)
            .max(200);
        let h = self.row_h * (PALETTE_MAX_ROWS as i32 + 1) + (16.0 * scale) as i32;
        self.bounds = Rect::new((win_w - w) / 2, top + (6.0 * scale) as i32, w, h);
        self.input.set_scale(scale);
        let mut inv = Invalidations::default();
        self.layout(&mut inv);
    }

    /// 창 크기(붙는 상자의 경계).
    pub fn set_window(&mut self, w: i32, h: i32) {
        self.win = (w, h);
    }

    /// 지금 모드의 실제 상자: 프롬프트는 입력란 + 안내 한 줄 · 폭 380 · 대상 아래(없으면 위 · 그래도 안 되면 창 안으로).
    #[must_use]
    pub fn frame(&self) -> Rect {
        let b = self.bounds;
        if self.prompt.is_none() {
            return b;
        }
        let pad = (6.0 * self.scale) as i32;
        let w = ((380.0 * self.scale) as i32).min(b.w);
        let h = pad + self.row_h + pad / 2 + self.row_h * 3 / 4 + pad;
        match self.anchor {
            Some(a) if self.win.0 > 0 && self.win.1 > 0 => {
                let gap = (3.0 * self.scale) as i32;
                let below = a.bottom() + gap;
                let y = if below + h <= self.win.1 {
                    below
                } else if a.y - gap - h >= 0 {
                    a.y - gap - h
                } else {
                    (self.win.1 - h).max(0)
                };
                let host = Rect::new(0, 0, self.win.0, self.win.1);
                nudge_into(Rect::new(a.x, y, w, h), host)
            }
            _ => Rect::new(b.x + (b.w - w) / 2, b.y, w, h),
        }
    }

    fn layout(&mut self, inv: &mut Invalidations) {
        let b = self.frame();
        let p = (6.0 * self.scale) as i32;
        self.input
            .set_bounds(Rect::new(b.x + p, b.y + p, b.w - p * 2, self.row_h), inv);
    }

    fn rows_rect(&self) -> Rect {
        let b = self.bounds;
        let p = (6.0 * self.scale) as i32;
        let top = b.y + p + self.row_h + p / 2;
        Rect::new(
            b.x + p,
            top,
            b.w - p * 2,
            self.row_h * PALETTE_MAX_ROWS as i32,
        )
    }

    /// 점 아래 결과 행(전체 결과 기준 인덱스 · 목록 밖/빈 행 = None).
    fn row_at(&self, p: Point) -> Option<usize> {
        let rr = self.rows_rect();
        if self.goto.is_some() || self.prompt.is_some() || !rr.contains(p) {
            return None;
        }
        let row = ((p.y - rr.y) / self.row_h.max(1)) as usize;
        let i = self.top + row;
        (row < PALETTE_MAX_ROWS && i < self.matches.len()).then_some(i)
    }

    fn reveal_sel(&mut self) {
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + PALETTE_MAX_ROWS {
            self.top = self.sel + 1 - PALETTE_MAX_ROWS;
        }
    }

    fn refilter(&mut self, force: bool) {
        if self.prompt.is_some() {
            self.matches.clear();
            return;
        }
        let q = self.input.text();
        if !force && q == self.last_query {
            return;
        }
        self.last_query = q.clone();
        if let Some(rest) = self.goto_prefix.and_then(|p| q.trim().strip_prefix(p)) {
            self.goto = Some(rest.trim().parse::<usize>().ok().filter(|n| *n > 0));
            self.matches.clear();
            self.sel = 0;
            self.top = 0;
            return;
        }
        self.goto = None;
        self.matches = rank(&self.items, &q, &self.recent);
        self.sel = 0;
        self.top = 0;
    }

    /// 지금 결과(items index 순서 · 시험 · 호스트 미리 보기).
    #[must_use]
    pub fn matches(&self) -> &[usize] {
        &self.matches
    }

    /// 선택 행(결과 기준).
    #[must_use]
    pub fn selected(&self) -> usize {
        self.sel
    }

    fn pick(&self, i: usize) -> PaletteAction {
        match self.matches.get(i).map(|&k| &self.items[k]) {
            Some(it) if it.enabled => PaletteAction::Pick(it.id.clone()),
            Some(_) => PaletteAction::None,
            None => PaletteAction::Close,
        }
    }

    pub fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) -> PaletteAction {
        if !self.open {
            return PaletteAction::None;
        }
        // 입력란의 우클릭 편집 메뉴가 떠 있으면 모든 사건은 입력란(메뉴)에게 — 메뉴가 상자 밖으로 펼쳐져도 팔레트가 닫히지 않는다.
        if self.input.popup_open() {
            let outside = match *ev {
                InputEvent::MouseDown { x, y, .. } | InputEvent::RightDown { x, y } => {
                    let p = Point { x, y };
                    !self.frame().contains(p) && !self.input.popup_bounds().contains(p)
                }
                _ => false,
            };
            if outside {
                self.input.close_menu();
            } else {
                self.input.on_event(ev, inv);
                return PaletteAction::None;
            }
        }
        match *ev {
            InputEvent::Key {
                key: Key::Escape, ..
            } => return PaletteAction::Close,
            InputEvent::Key {
                key: Key::Enter, ..
            } => {
                if let Some(id) = self.prompt.clone() {
                    let text = self.input.text().trim().to_string();
                    return if text.is_empty() {
                        PaletteAction::Close
                    } else {
                        PaletteAction::Prompt { id, text }
                    };
                }
                if let Some(g) = self.goto {
                    return match g {
                        Some(n) => PaletteAction::Goto(n),
                        None => PaletteAction::None,
                    };
                }
                return self.pick(self.sel);
            }
            InputEvent::Key { key: Key::Up, .. } => {
                self.sel = self.sel.saturating_sub(1);
                self.reveal_sel();
                return PaletteAction::None;
            }
            InputEvent::Key { key: Key::Down, .. } => {
                if self.sel + 1 < self.matches.len() {
                    self.sel += 1;
                }
                self.reveal_sel();
                return PaletteAction::None;
            }
            InputEvent::Key {
                key: Key::PageUp, ..
            } => {
                self.sel = self.sel.saturating_sub(PALETTE_MAX_ROWS);
                self.reveal_sel();
                return PaletteAction::None;
            }
            InputEvent::Key {
                key: Key::PageDown, ..
            } => {
                self.sel = (self.sel + PALETTE_MAX_ROWS).min(self.matches.len().saturating_sub(1));
                self.reveal_sel();
                return PaletteAction::None;
            }
            // 마우스: 목록 위에서 움직이면 그 행이 선택 · 휠 = 굴리기 · 클릭 = 실행. 같은 자리의 MouseMove는 무시.
            InputEvent::MouseMove { x, y } => {
                self.input.on_event(ev, inv);
                if (x, y) != self.last_mouse {
                    self.last_mouse = (x, y);
                    if let Some(i) = self.row_at(Point { x, y }) {
                        self.sel = i;
                    }
                }
                return PaletteAction::None;
            }
            InputEvent::Wheel { .. } if self.prompt.is_some() => {
                self.input.on_event(ev, inv);
                return PaletteAction::None;
            }
            InputEvent::Wheel { delta } => {
                let rows = if delta > 0 { -3isize } else { 3 };
                let max_top = self.matches.len().saturating_sub(PALETTE_MAX_ROWS);
                self.top = (self.top as isize + rows).clamp(0, max_top as isize) as usize;
                let under = self.row_at(Point {
                    x: self.last_mouse.0,
                    y: self.last_mouse.1,
                });
                self.sel = under.unwrap_or_else(|| {
                    self.sel
                        .clamp(self.top, (self.top + PALETTE_MAX_ROWS).saturating_sub(1))
                        .min(self.matches.len().saturating_sub(1))
                });
                return PaletteAction::None;
            }
            // 바깥 우클릭도 취소 · 상자 안의 우클릭은 입력란의 편집 메뉴로.
            InputEvent::RightDown { x, y } if !self.frame().contains(Point { x, y }) => {
                return PaletteAction::Close;
            }
            InputEvent::MouseDown { x, y, .. } => {
                let p = Point { x, y };
                if !self.frame().contains(p) {
                    return PaletteAction::Close;
                }
                if self.rows_rect().contains(p) {
                    if let Some(i) = self.row_at(p) {
                        return self.pick(i);
                    }
                    return PaletteAction::None;
                }
            }
            _ => {}
        }
        self.input.on_event(ev, inv);
        self.refilter(false);
        PaletteAction::None
    }

    /// 입력란 편집 메뉴의 요청(복사 · 잘라내기 · 붙여넣기) — 호스트가 OS 클립보드를 잇는다.
    pub fn take_edit_ctx(&mut self) -> Option<EditCtxAction> {
        self.input.take_edit_ctx()
    }

    /// 선택 글(복사용).
    #[must_use]
    pub fn copy_selection(&self) -> Option<String> {
        self.input.copy_selection()
    }

    pub fn cut_selection(&mut self, inv: &mut Invalidations) -> Option<String> {
        let t = self.input.cut_selection(inv);
        self.refilter(false);
        t
    }

    pub fn paste(&mut self, text: &str, inv: &mut Invalidations) {
        self.input.paste(text, inv);
        self.refilter(false);
    }

    pub fn select_all(&mut self, inv: &mut Invalidations) {
        self.input.on_event(&InputEvent::SelectAll, inv);
    }

    /// 캐럿 깜빡임 등 — 열려 있을 때만.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        self.open && self.input.tick(now_ms)
    }

    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.open && self.input.is_animating()
    }

    pub fn paint(&self, dc: &mut dyn DrawCtx, th: &Theme) {
        if !self.open {
            return;
        }
        self.paint_body(dc, th);
        // 입력란 편집 메뉴 = 맨 위 층.
        self.input.paint_popup(dc, th);
    }

    fn paint_body(&self, dc: &mut dyn DrawCtx, th: &Theme) {
        let b = self.frame();
        dc.fill_rect(Rect::new(b.x - 1, b.y - 1, b.w + 2, b.h + 2), th.border);
        dc.fill_rect(b, th.chrome_bg);
        self.input.paint(dc, th);
        if let (Some(a), true) = (self.anchor, self.prompt.is_some()) {
            // 수정 중인 대상 강조: 대상에 강조색 테두리(2px) + 상자의 그쪽 변도 같은 색.
            let w2 = (2.0 * self.scale).round().max(2.0) as i32;
            for r in [
                Rect::new(a.x, a.y, a.w, w2),
                Rect::new(a.x, a.bottom() - w2, a.w, w2),
                Rect::new(a.x, a.y, w2, a.h),
                Rect::new(a.right() - w2, a.y, w2, a.h),
            ] {
                dc.fill_rect(r, th.accent);
            }
            let edge_y = if b.y >= a.bottom() {
                b.y - 1
            } else {
                b.bottom() - w2 + 1
            };
            dc.fill_rect(Rect::new(b.x - 1, edge_y, b.w + 2, w2), th.accent);
        }
        let th_px = dc.text_height();
        if self.prompt.is_some() {
            let pad = (6.0 * self.scale) as i32;
            let y = b.y + pad + self.row_h + pad / 2;
            dc.text(
                b.x + pad + (4.0 * self.scale) as i32,
                y + (self.row_h * 3 / 4 - th_px) / 2,
                b,
                &self.hint,
                th.text_dim,
            );
            return;
        }
        let rr = self.rows_rect();
        let pad = (8.0 * self.scale) as i32;
        if let Some(g) = self.goto {
            let (text, color) = match g {
                Some(n) => (
                    self.strings.goto_line.replace("{0}", &n.to_string()),
                    th.text,
                ),
                None => (self.strings.goto_hint.clone(), th.text_dim),
            };
            let r = Rect::new(rr.x, rr.y, rr.w, self.row_h);
            if g.is_some() {
                dc.fill_rect(r, th.sel_bg);
            }
            let shown = crate::draw::ellipsize_middle(dc, &text, r.w - pad * 2);
            dc.text(r.x + pad, rr.y + (self.row_h - th_px) / 2, r, &shown, color);
            return;
        }
        if self.matches.is_empty() {
            dc.text(
                rr.x + pad,
                rr.y + (self.row_h - th_px) / 2,
                rr,
                &self.strings.no_match,
                th.text_dim,
            );
            return;
        }
        for (row, &i) in self
            .matches
            .iter()
            .skip(self.top)
            .take(PALETTE_MAX_ROWS)
            .enumerate()
        {
            let it = &self.items[i];
            let y = rr.y + row as i32 * self.row_h;
            let r = Rect::new(rr.x, y, rr.w, self.row_h);
            if self.top + row == self.sel {
                dc.fill_rect(r, th.sel_bg);
            }
            let ty = y + (self.row_h - th_px) / 2;
            // 오른쪽 보조 글(단축키) 먼저 자리를 잡고 라벨은 그 앞까지.
            let mut label_w = r.w - pad * 2;
            if !it.detail.is_empty() {
                let dw = dc.text_width(&it.detail);
                dc.text(r.right() - pad - dw, ty, r, &it.detail, th.text_dim);
                label_w -= dw + pad;
            }
            let shown = crate::draw::ellipsize_middle(dc, &it.label, label_w);
            dc.text(
                r.x + pad,
                ty,
                r,
                &shown,
                if it.enabled { th.text } else { th.text_dim },
            );
        }
    }
}

/// 질의로 항목을 걸러 점수순(같으면 최근 실행 · 라벨 순)으로 — items index 목록(순수).
#[must_use]
pub fn rank(items: &[PaletteItem], query: &str, recent: &[String]) -> Vec<usize> {
    let bonus = |id: &str| {
        recent
            .iter()
            .take(RECENT_MAX)
            .position(|r| r == id)
            .map_or(0, |p| (RECENT_MAX - p) as i32)
    };
    let mut scored: Vec<(i32, i32, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, it)| fuzzy_score(query, &it.label).map(|s| (s, bonus(&it.id), i)))
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| items[a.2].label.cmp(&items[b.2].label))
    });
    scored.into_iter().map(|(_, _, i)| i).collect()
}

/// 퍼지 점수 — 대소문자 무관 부분열 매치. 연속 매치 +3 · 단어 첫 글자 +2 · 글자 +1 · 없으면 None. 빈 질의 = 0.
#[must_use]
pub fn fuzzy_score(query: &str, label: &str) -> Option<i32> {
    let q: Vec<char> = query.trim().to_lowercase().chars().collect();
    if q.is_empty() {
        return Some(0);
    }
    let l: Vec<char> = label.to_lowercase().chars().collect();
    let mut score = 0i32;
    let mut qi = 0usize;
    let mut prev_hit = false;
    for (i, &c) in l.iter().enumerate() {
        if qi < q.len() && c == q[qi] {
            score += 1;
            if prev_hit {
                score += 3;
            }
            if i == 0 || !l[i - 1].is_alphanumeric() {
                score += 2;
            }
            prev_hit = true;
            qi += 1;
        } else {
            prev_hit = false;
        }
    }
    (qi == q.len()).then_some(score)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings() -> PaletteStrings {
        PaletteStrings {
            placeholder: "명령 검색".into(),
            no_match: "없음".into(),
            goto_line: "줄 {0}".into(),
            goto_hint: "줄 번호".into(),
        }
    }

    fn key(k: Key) -> InputEvent {
        InputEvent::Key {
            key: k,
            shift: false,
            primary: false,
        }
    }

    /// 한글 글자가 검색어로 들어가 거른다 · 조합 중 글자는 본문이 아니다(nexa-sql 09-27).
    #[test]
    fn hangul_chars_reach_the_query_and_filter() {
        let mut p = Palette::new(strings());
        p.set_items(vec![
            PaletteItem::new("view.log", "보기: 로그 창"),
            PaletteItem::new("file.new", "새 편집기"),
        ]);
        p.open("");
        let mut inv = Invalidations::default();
        for c in "로그".chars() {
            p.on_event(&InputEvent::Char { c, now_ms: 0 }, &mut inv);
        }
        assert_eq!(p.query(), "로그");
        assert_eq!(p.matches().len(), 1);
        assert_eq!(p.items[p.matches()[0]].id, "view.log");
        p.set_preedit("ㅊ", &mut inv);
        assert_eq!(p.query(), "로그", "조합 중 글자는 본문이 아니다");
        assert!(
            matches!(p.on_event(&key(Key::Enter), &mut inv), PaletteAction::Pick(id) if id == "view.log")
        );
    }

    #[test]
    fn subsequence_and_ranking() {
        assert!(fuzzy_score("syntax", "Set Syntax: SQL").is_some());
        assert!(fuzzy_score("xyz", "Set Syntax: SQL").is_none());
        let a = fuzzy_score("sql", "Set Syntax: SQL").unwrap_or(0);
        let b = fuzzy_score("sql", "Set Syntax: Plain Text").unwrap_or(-1);
        assert!(a > b);
        assert_eq!(fuzzy_score("", "anything"), Some(0));
    }

    /// 최근 실행은 같은 점수 안에서 앞에 · 빈 질의 = 최근 순 → 라벨 순 · 사용 불가 항목은 Enter/클릭으로 고를 수 없다.
    #[test]
    fn recent_first_and_disabled_not_pickable() {
        let items = vec![
            PaletteItem::new("a", "Alpha"),
            PaletteItem::new("b", "Beta"),
            PaletteItem::new("c", "Gamma").enabled(false),
        ];
        assert_eq!(rank(&items, "", &[]), vec![0, 1, 2], "빈 질의 = 라벨 순");
        assert_eq!(
            rank(&items, "", &["b".into()]),
            vec![1, 0, 2],
            "최근 실행이 앞"
        );
        let mut p = Palette::new(strings());
        p.set_items(items);
        p.set_bounds(1000, 40, 1.0);
        p.open("gam");
        let mut inv = Invalidations::default();
        assert_eq!(p.matches().len(), 1);
        assert_eq!(
            p.on_event(&key(Key::Enter), &mut inv),
            PaletteAction::None,
            "사용 불가 = 실행 없음"
        );
    }

    /// 줄 이동 모드(접두어 지정 때만): `:12` → Goto(12) · 접두어만 = None · 접두어 없이는 보통 검색.
    #[test]
    fn goto_prefix_is_optional() {
        let mut p = Palette::new(strings());
        p.set_items(vec![PaletteItem::new("x", ": 12 things")]);
        p.open(":12");
        let mut inv = Invalidations::default();
        assert_eq!(p.matches().len(), 1, "접두어 없으면 ':12'도 보통 질의");
        p.set_goto_prefix(Some(':'));
        p.open(":12");
        assert_eq!(
            p.on_event(&key(Key::Enter), &mut inv),
            PaletteAction::Goto(12)
        );
        p.open(":");
        assert_eq!(p.on_event(&key(Key::Enter), &mut inv), PaletteAction::None);
    }

    /// 프롬프트: 대상 바로 아래(자리 없으면 위) · 입력란 + 안내만 · 바깥 좌/우클릭 = 취소 · 안 클릭은 유지 · Enter = Prompt · 빈 글 = Close.
    #[test]
    fn prompt_is_anchored_compact_and_outside_clicks_cancel() {
        let mut p = Palette::new(strings());
        p.set_bounds(1000, 40, 1.0);
        p.set_window(1000, 700);
        let mut inv = Invalidations::default();
        let tab = Rect::new(120, 90, 110, 28);
        p.open_prompt_at("tab.rename:1", "hint", "Script_1", Some(tab));
        assert!(p.is_prompt());
        let f = p.frame();
        assert_eq!((f.x, f.y), (tab.x, tab.bottom() + 3));
        assert!(f.h < 26 * 4, "목록 영역이 없다: {f:?}");
        let inside = InputEvent::RightDown {
            x: f.x + 10,
            y: f.y + 10,
        };
        assert_eq!(
            p.on_event(&key(Key::Enter), &mut inv),
            PaletteAction::Prompt {
                id: "tab.rename:1".into(),
                text: "Script_1".into()
            }
        );
        // 상자 안 우클릭 = 편집 메뉴(닫히지 않는다).
        assert!(!matches!(
            p.on_event(&inside, &mut inv),
            PaletteAction::Close
        ));
        p.input.close_menu();
        let outside = InputEvent::RightDown { x: 900, y: 600 };
        assert_eq!(p.on_event(&outside, &mut inv), PaletteAction::Close);
        let left = InputEvent::MouseDown {
            x: 900,
            y: 600,
            shift: false,
            primary: false,
        };
        assert_eq!(p.on_event(&left, &mut inv), PaletteAction::Close);
        p.close();
        let low = Rect::new(80, 680, 90, 18);
        p.open_prompt_at("r:7", "hint", "Result 1", Some(low));
        assert!(p.frame().bottom() <= low.y, "아래에 자리가 없으면 위로");
        p.close();
        p.open_prompt_at("x", "hint", "", None);
        assert_eq!(p.frame().y, p.bounds.y);
        assert_eq!(
            p.on_event(&key(Key::Enter), &mut inv),
            PaletteAction::Close,
            "빈 글 = 닫기"
        );
    }

    /// 마우스: hover = 선택 · 같은 자리 MouseMove 무시 · 휠 = 3행 굴림 · ↓는 끝까지 · 클릭 = 실행(굴린 위치 기준).
    #[test]
    fn mouse_hover_wheel_click_and_scrolling() {
        let mut p = Palette::new(strings());
        p.set_items(
            (0..40)
                .map(|i| PaletteItem::new(format!("cmd.{i}"), format!("Command {i:02}")))
                .collect(),
        );
        p.set_bounds(1000, 40, 1.0);
        p.open("");
        let mut inv = Invalidations::default();
        let rr = p.rows_rect();
        let at = |row: i32| InputEvent::MouseMove {
            x: rr.x + 10,
            y: rr.y + row * p.row_h + 3,
        };
        let ev = at(3);
        p.on_event(&ev, &mut inv);
        assert_eq!(p.selected(), 3, "hover = 선택");
        p.on_event(&key(Key::Down), &mut inv);
        assert_eq!(p.selected(), 4);
        p.on_event(&ev, &mut inv);
        assert_eq!(p.selected(), 4, "같은 자리 = 무시");
        p.on_event(&InputEvent::Wheel { delta: -120 }, &mut inv);
        assert_eq!(p.top, 3);
        assert_eq!(p.selected(), 6);
        for _ in 0..100 {
            p.on_event(&key(Key::Down), &mut inv);
        }
        assert_eq!(p.selected(), 39);
        assert_eq!(p.top, 40 - PALETTE_MAX_ROWS);
        let click = InputEvent::MouseDown {
            x: rr.x + 10,
            y: rr.y + 3,
            shift: false,
            primary: false,
        };
        assert!(matches!(
            p.on_event(&click, &mut inv),
            PaletteAction::Pick(id) if id == format!("cmd.{}", 40 - PALETTE_MAX_ROWS)
        ));
        assert_eq!(
            p.on_event(&key(Key::Escape), &mut inv),
            PaletteAction::Close
        );
    }
}

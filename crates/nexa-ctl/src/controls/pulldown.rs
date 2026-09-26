//! **메뉴바(Pull-down 메뉴)** — 일반적인 메뉴바를 자체 렌더로 그린다(사용자 요청 08-09).
//!
//! **Windows 스타일 + 크로스플랫폼(DR-6)**: OS 네이티브 메뉴를 쓰지 않고 창 안에 직접
//! 그린다 — 상단 바에 평평한 라벨을 나열하고, 클릭하면 아래로 드롭다운이 열린다.
//! 열려 있는 동안 다른 라벨에 hover만 해도 그 메뉴로 전환(표준 메뉴바 동작).
//! 항목은 값이 아니라 **액션** — 고르는 즉시 [`MenuBar::take_picked`] 1회성 보고 후 닫힌다.
//!
//! 라벨/팝업 폭은 페인트 시점에 실제 글꼴로 측정해 캐시한다(`RefCell` — 첫 페인트 전엔
//! 문자폭 추정치 사용). 공통 기능(활성·배율)은 [`Control`] 상속.
//!
//! **하위 메뉴**([`MenuEntry::Sub`] · nexa-sql 사용자 09-22 "Edit 메뉴가 1레벨로 너무 길다 — Sublime/VS Code/IntelliJ처럼
//! 그룹으로"): 한 단계 · 오른쪽 `›` · hover/→/Enter/클릭으로 펼침 · ←/Esc/다른 행 hover로 접힘 · 부모 팝업 오른쪽에
//! 붙여 놓되 표면(창) 밖이면 [`crate::geom::nudge_into`]로 밀어 넣는다(표면 크기는 페인트 때 배운다).

use super::{image_fit_contain, ComboItem, Control, ControlBase, LEADING_ICON};
use crate::draw::{DrawCtx, FontSlot};
use crate::event::{InputEvent, Key};
use crate::geom::{Point, Rect};
use crate::theme::Theme;
use crate::widget::{Invalidations, Widget};
use std::cell::RefCell;

/// 드롭다운 한 줄 — 액션 항목 또는 구분선.
#[derive(Clone, Debug)]
pub enum MenuEntry {
    /// 액션 항목(값 = 보고 id · 라벨 · 선택적 앞 이미지).
    Item(ComboItem),
    /// **강조** 항목 — `Item`과 같되 라벨을 강조색(`theme.warn`)으로(nexa-sql 미저장 탭 · 사용자 09-26).
    Emph(ComboItem),
    /// **비활성** 항목(흐리게 · hover/선택 없음 · 09-17 nexa-sql "이미 있으면 메뉴 Disable").
    Disabled(ComboItem),
    /// 구분선.
    Separator,
    /// **하위 메뉴**(라벨 · 그 안의 항목들 — 한 단계만 · 안의 `Sub`는 일반 항목처럼 그려지고 열리지 않는다).
    Sub(ComboItem, Vec<MenuEntry>),
}

impl MenuEntry {
    /// 하위 메뉴 항목 생성(값 = 보고 id가 아니라 식별용 · 라벨).
    #[must_use]
    pub fn sub(label: impl Into<String>, entries: Vec<MenuEntry>) -> Self {
        let label = label.into();
        Self::Sub(ComboItem::new(label.clone(), label), entries)
    }

    fn label_of(&self) -> Option<&str> {
        match self {
            Self::Item(it) | Self::Emph(it) | Self::Disabled(it) | Self::Sub(it, _) => {
                Some(&it.label)
            }
            Self::Separator => None,
        }
    }
}

/// 최상위 메뉴 하나 — 바 라벨 + 드롭다운 항목들.
#[derive(Clone, Debug)]
pub struct MenuDef {
    /// 바에 보이는 라벨.
    pub label: String,
    /// 드롭다운 내용.
    pub entries: Vec<MenuEntry>,
}

impl MenuDef {
    /// 라벨과 항목으로 만든다.
    #[must_use]
    pub fn new(label: impl Into<String>, entries: Vec<MenuEntry>) -> Self {
        Self {
            label: label.into(),
            entries,
        }
    }
}

const ITEM_H: i32 = 26;
const SEP_H: i32 = 7;
const LABEL_PAD: i32 = 12;
const POPUP_PAD: i32 = 4;
const POPUP_MIN_W: i32 = 160;

/// 메뉴바 컨트롤.
#[derive(Debug)]
pub struct MenuBar {
    base: ControlBase,
    menus: Vec<MenuDef>,
    /// 열린 최상위 메뉴 index.
    open: Option<usize>,
    hover_top: Option<usize>,
    /// 열린 드롭다운 안의 hover 항목(entries index).
    hover_item: Option<usize>,
    /// 펼친 하위 메뉴(부모 entries index) · 그 안의 hover(하위 entries index).
    sub_open: Option<usize>,
    hover_sub: Option<usize>,
    /// 하위 메뉴 내용 폭 실측 캐시 `((메뉴, 부모 항목), 폭)` · 표면 크기(페인트 때 배움 — 하위 메뉴가 창 밖으로 안 나가게).
    sub_measured: RefCell<Option<((usize, usize), i32)>>,
    surface: std::cell::Cell<Option<(i32, i32)>>,
    picked: Option<String>,
    /// 페인트 시 측정한 (라벨 폭들, 팝업 내용 폭들) 캐시 — 측정 전엔 추정치.
    measured: RefCell<(Vec<i32>, Vec<i32>)>,
    /// 드롭다운 항목 라벨 폭 상한(논리 px · nexa-sql 사용자 09-22 "최근 파일 경로가 길면 가운데 …") — 넘는 라벨은
    /// [`crate::draw::ellipsize_middle`]로 줄인다 · 전체 보기 스위치(Alt)면 상한 없이 그대로.
    max_label_w: i32,
}

impl MenuBar {
    /// 메뉴 목록으로 만든다.
    #[must_use]
    pub fn new(menus: Vec<MenuDef>) -> Self {
        Self {
            base: ControlBase::default(),
            menus,
            open: None,
            hover_top: None,
            hover_item: None,
            sub_open: None,
            hover_sub: None,
            sub_measured: RefCell::new(None),
            surface: std::cell::Cell::new(None),
            picked: None,
            measured: RefCell::new((Vec::new(), Vec::new())),
            max_label_w: 480,
        }
    }

    /// 메뉴 전체 교체(i18n 언어 전환 등) — 열림 상태·측정 캐시 초기화.
    /// 드롭다운 항목 라벨 폭 상한(논리 px · 기본 480).
    pub fn set_max_label_width(&mut self, logical_px: i32) {
        self.max_label_w = logical_px.max(80);
    }

    pub fn set_menus(&mut self, menus: Vec<MenuDef>) {
        self.menus = menus;
        self.open = None;
        self.hover_top = None;
        self.hover_item = None;
        self.sub_open = None;
        self.hover_sub = None;
        *self.sub_measured.borrow_mut() = None;
        let mut m = self.measured.borrow_mut();
        m.0.clear();
        m.1.clear();
    }

    /// 드롭다운이 열려 있는가(모달 캡처·최상위 재도색 근거).
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// 골라진 액션 값(1회성) — 호스트가 실행.
    pub fn take_picked(&mut self) -> Option<String> {
        self.picked.take()
    }

    /// 문자폭 추정(측정 전 폴백) — ASCII 7 · 그 외(CJK 등) 14.
    fn estimate_w(&self, text: &str) -> i32 {
        let units: i32 = text
            .chars()
            .map(|c| if c.is_ascii() { 7 } else { 14 })
            .sum();
        self.s(units)
    }

    fn label_w(&self, i: usize) -> i32 {
        let cached = self.measured.borrow().0.get(i).copied();
        let text_w =
            cached.unwrap_or_else(|| self.menus.get(i).map_or(0, |m| self.estimate_w(&m.label)));
        text_w + self.s(LABEL_PAD) * 2
    }

    fn label_rect(&self, i: usize) -> Rect {
        let b = self.base.bounds;
        let mut x = b.x + self.s(4);
        for j in 0..i {
            x += self.label_w(j);
        }
        Rect::new(x, b.y, self.label_w(i), b.h)
    }

    fn label_at(&self, x: i32, y: i32) -> Option<usize> {
        (0..self.menus.len()).find(|&i| self.label_rect(i).contains(Point { x, y }))
    }

    fn entry_h(&self, e: &MenuEntry) -> i32 {
        match e {
            MenuEntry::Item(_)
            | MenuEntry::Emph(_)
            | MenuEntry::Disabled(_)
            | MenuEntry::Sub(..) => self.s(ITEM_H),
            MenuEntry::Separator => self.s(SEP_H),
        }
    }

    fn popup_rect(&self) -> Rect {
        let Some(i) = self.open else {
            return Rect::new(0, 0, 0, 0);
        };
        let lr = self.label_rect(i);
        let m = &self.menus[i];
        let h: i32 = m.entries.iter().map(|e| self.entry_h(e)).sum::<i32>() + self.s(POPUP_PAD) * 2;
        let cached = self.measured.borrow().1.get(i).copied();
        let content_w = cached.unwrap_or_else(|| {
            m.entries
                .iter()
                .map(|e| e.label_of().map_or(0, |l| self.estimate_w(l)))
                .max()
                .unwrap_or(0)
        });
        let w = (content_w + self.s(LEADING_ICON) + self.s(34)).max(self.s(POPUP_MIN_W));
        Rect::new(lr.x, self.base.bounds.bottom(), w, h)
    }

    /// 부모 팝업 안 행 `k`의 사각형.
    fn entry_rect(&self, k: usize) -> Option<Rect> {
        let i = self.open?;
        let pop = self.popup_rect();
        let mut cy = pop.y + self.s(POPUP_PAD);
        for (j, e) in self.menus[i].entries.iter().enumerate() {
            let h = self.entry_h(e);
            if j == k {
                return Some(Rect::new(pop.x, cy, pop.w, h));
            }
            cy += h;
        }
        None
    }

    /// 펼친 하위 메뉴의 항목들.
    fn sub_entries(&self) -> Option<&[MenuEntry]> {
        let i = self.open?;
        let k = self.sub_open?;
        match self.menus[i].entries.get(k) {
            Some(MenuEntry::Sub(_, v)) => Some(v),
            _ => None,
        }
    }

    /// 하위 메뉴 사각형 — 부모 행 오른쪽(부모 팝업과 2px 겹침 · 첫 항목이 부모 행과 같은 높이) · 표면 안으로 밀어 넣는다.
    fn sub_rect(&self) -> Rect {
        let (Some(i), Some(k), Some(v)) = (self.open, self.sub_open, self.sub_entries()) else {
            return Rect::new(0, 0, 0, 0);
        };
        let Some(row) = self.entry_rect(k) else {
            return Rect::new(0, 0, 0, 0);
        };
        let cached = self
            .sub_measured
            .borrow()
            .filter(|(key, _)| *key == (i, k))
            .map(|(_, w)| w);
        let content_w = cached.unwrap_or_else(|| {
            v.iter()
                .map(|e| e.label_of().map_or(0, |l| self.estimate_w(l)))
                .max()
                .unwrap_or(0)
        });
        let w = (content_w + self.s(LEADING_ICON) + self.s(34)).max(self.s(POPUP_MIN_W));
        let h: i32 = v.iter().map(|e| self.entry_h(e)).sum::<i32>() + self.s(POPUP_PAD) * 2;
        let r = Rect::new(row.right() - self.s(2), row.y - self.s(POPUP_PAD), w, h);
        match self.surface.get() {
            Some((sw, sh)) if sw > 0 && sh > 0 => {
                let host = Rect::new(0, 0, sw, sh);
                // 오른쪽에 못 들어가면 부모 왼쪽으로 · 그래도 안 되면 밀어 넣기.
                let r = if r.right() > host.right() && row.x - w >= host.x {
                    Rect::new(row.x - w + self.s(2), r.y, w, h)
                } else {
                    r
                };
                crate::geom::nudge_into(r, host)
            }
            _ => r,
        }
    }

    /// 하위 메뉴 좌표 → 하위 entries index.
    fn sub_entry_at(&self, x: i32, y: i32) -> Option<usize> {
        let v = self.sub_entries()?;
        let r = self.sub_rect();
        if !r.contains(Point { x, y }) {
            return None;
        }
        let mut cy = r.y + self.s(POPUP_PAD);
        for (k, e) in v.iter().enumerate() {
            let h = self.entry_h(e);
            if y >= cy && y < cy + h {
                return Some(k);
            }
            cy += h;
        }
        None
    }

    fn open_sub(&mut self, k: usize, inv: &mut Invalidations) {
        if self.sub_open != Some(k) {
            self.sub_open = Some(k);
            self.hover_sub = None;
            inv.push(self.popup_rect().union(&self.sub_rect()));
        }
    }

    fn close_sub(&mut self, inv: &mut Invalidations) {
        if self.sub_open.is_some() {
            inv.push(self.popup_rect().union(&self.sub_rect()));
            self.sub_open = None;
            self.hover_sub = None;
        }
    }

    /// 하위 메뉴 안의 다음/이전 항목.
    fn step_sub(&self, from: Option<usize>, down: bool) -> Option<usize> {
        let v = self.sub_entries()?;
        let idxs: Vec<usize> = (0..v.len())
            .filter(|&k| matches!(v[k], MenuEntry::Item(_) | MenuEntry::Emph(_)))
            .collect();
        if idxs.is_empty() {
            return None;
        }
        let pos = from.and_then(|f| idxs.iter().position(|&k| k == f));
        Some(match (pos, down) {
            (None, true) => idxs[0],
            (None, false) => *idxs.last()?,
            (Some(p), true) => idxs[(p + 1) % idxs.len()],
            (Some(p), false) => idxs[(p + idxs.len() - 1) % idxs.len()],
        })
    }

    fn pick_sub(&mut self, k: usize, inv: &mut Invalidations) {
        if let Some(MenuEntry::Item(it) | MenuEntry::Emph(it)) =
            self.sub_entries().and_then(|v| v.get(k))
        {
            self.picked = Some(it.value.clone());
            self.close(inv);
        }
    }

    /// 팝업 좌표 → entries index(구분선 포함 — pick에서 항목만 허용).
    fn entry_at(&self, x: i32, y: i32) -> Option<usize> {
        let i = self.open?;
        let pop = self.popup_rect();
        if !pop.contains(Point { x, y }) {
            return None;
        }
        let mut cy = pop.y + self.s(POPUP_PAD);
        for (k, e) in self.menus[i].entries.iter().enumerate() {
            let h = self.entry_h(e);
            if y >= cy && y < cy + h {
                return Some(k);
            }
            cy += h;
        }
        None
    }

    fn pick(&mut self, entry: usize, inv: &mut Invalidations) {
        if let Some(i) = self.open {
            match self.menus[i].entries.get(entry) {
                Some(MenuEntry::Item(it) | MenuEntry::Emph(it)) => {
                    self.picked = Some(it.value.clone());
                    self.close(inv);
                }
                Some(MenuEntry::Sub(..)) => {
                    self.hover_item = Some(entry);
                    self.open_sub(entry, inv);
                }
                _ => {}
            }
        }
    }

    fn close(&mut self, inv: &mut Invalidations) {
        inv.push(self.popup_rect().union(&self.sub_rect()));
        self.open = None;
        self.hover_item = None;
        self.sub_open = None;
        self.hover_sub = None;
        inv.push(self.base.bounds);
    }

    /// 열린 드롭다운 닫기(호스트: 우클릭 메뉴와 배타 · nexa-sql 09-22).
    pub fn dismiss(&mut self) {
        self.open = None;
        self.hover_item = None;
        self.sub_open = None;
        self.hover_sub = None;
    }

    fn open_menu(&mut self, i: usize, inv: &mut Invalidations) {
        inv.push(self.popup_rect().union(&self.sub_rect()));
        self.open = Some(i);
        self.hover_item = None;
        self.sub_open = None;
        self.hover_sub = None;
        inv.push(self.base.bounds);
    }

    /// 다음/이전 **항목**(구분선 건너뜀) entries index.
    fn step_item(&self, from: Option<usize>, down: bool) -> Option<usize> {
        let i = self.open?;
        let entries = &self.menus[i].entries;
        let idxs: Vec<usize> = (0..entries.len())
            .filter(|&k| {
                matches!(
                    entries[k],
                    MenuEntry::Item(_) | MenuEntry::Emph(_) | MenuEntry::Sub(..)
                )
            })
            .collect();
        if idxs.is_empty() {
            return None;
        }
        let pos = from.and_then(|f| idxs.iter().position(|&k| k == f));
        Some(match (pos, down) {
            (None, true) => idxs[0],
            (None, false) => *idxs.last()?,
            (Some(p), true) => idxs[(p + 1) % idxs.len()],
            (Some(p), false) => idxs[(p + idxs.len() - 1) % idxs.len()],
        })
    }

    /// 하위 메뉴 영역(열려 있을 때 · 호스트 히트 판정용 — 부모 팝업과 합집합은 `popup_bounds`).
    #[must_use]
    pub fn popup_bounds(&self) -> Rect {
        if !self.is_open() {
            return Rect::new(0, 0, 0, 0);
        }
        let p = self.popup_rect();
        if self.sub_open.is_some() {
            p.union(&self.sub_rect())
        } else {
            p
        }
    }
}

impl Control for MenuBar {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
}

impl Widget for MenuBar {
    fn bounds(&self) -> Rect {
        self.base.bounds
    }

    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        self.base.bounds = bounds;
        inv.push(bounds);
    }

    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        match *ev {
            InputEvent::MouseDown { x, y, .. } => {
                if let Some(i) = self.label_at(x, y) {
                    // 라벨 클릭 = 토글(같은 메뉴 재클릭 = 닫기 — 표준 동작).
                    if self.open == Some(i) {
                        self.close(inv);
                    } else {
                        self.open_menu(i, inv);
                    }
                    return;
                }
                if self.open.is_some() {
                    if let Some(k) = self.sub_entry_at(x, y) {
                        self.pick_sub(k, inv);
                    } else if let Some(k) = self.entry_at(x, y) {
                        self.pick(k, inv);
                    } else {
                        self.close(inv); // 바깥 클릭 = 닫기
                    }
                }
            }
            InputEvent::MouseMove { x, y } => {
                if self.open.is_some() {
                    // 열림 중 다른 라벨 hover = 그 메뉴로 전환(표준 메뉴바 동작).
                    if let Some(i) = self.label_at(x, y) {
                        if self.open != Some(i) {
                            self.open_menu(i, inv);
                        }
                        return;
                    }
                    // 하위 메뉴 안 = 하위 hover(부모 hover는 펼친 행에 고정).
                    if self.sub_open.is_some() && self.sub_rect().contains(Point { x, y }) {
                        let over = self.sub_entry_at(x, y);
                        if over != self.hover_sub {
                            self.hover_sub = over;
                            inv.push(self.sub_rect());
                        }
                        return;
                    }
                    let over = self.entry_at(x, y);
                    if over != self.hover_item {
                        self.hover_item = over;
                        inv.push(self.popup_rect());
                    }
                    // 부모의 다른 행 위 = 하위 접기 · `Sub` 행 위 = 펼치기(비활성·구분선·여백 위도 접는다 — Windows 관례).
                    if let Some(k) = over {
                        if matches!(
                            self.menus[self.open.unwrap_or(0)].entries.get(k),
                            Some(MenuEntry::Sub(..))
                        ) {
                            self.open_sub(k, inv);
                        } else {
                            self.close_sub(inv);
                        }
                    } else if self.popup_rect().contains(Point { x, y }) {
                        self.close_sub(inv);
                    }
                } else {
                    let over = self.label_at(x, y);
                    if over != self.hover_top {
                        self.hover_top = over;
                        inv.push(self.base.bounds);
                    }
                }
            }
            // 하위 메뉴가 펼쳐져 있으면 키는 하위가 먼저: ↑/↓ 안에서 이동 · ←/Esc 접기 · Enter/Space 선택 · ← → 는 접은 뒤 메뉴 전환 안 함.
            InputEvent::Key { key, .. } if self.open.is_some() && self.sub_open.is_some() => {
                match key {
                    Key::Escape | Key::Left => self.close_sub(inv),
                    Key::Down => {
                        self.hover_sub = self.step_sub(self.hover_sub, true);
                        inv.push(self.sub_rect());
                    }
                    Key::Up => {
                        self.hover_sub = self.step_sub(self.hover_sub, false);
                        inv.push(self.sub_rect());
                    }
                    Key::Enter | Key::Space => {
                        if let Some(k) = self.hover_sub {
                            self.pick_sub(k, inv);
                        }
                    }
                    _ => {}
                }
            }
            InputEvent::Key { key, .. } if self.open.is_some() => match key {
                Key::Escape => self.close(inv),
                Key::Down => {
                    self.hover_item = self.step_item(self.hover_item, true);
                    inv.push(self.popup_rect());
                }
                Key::Up => {
                    self.hover_item = self.step_item(self.hover_item, false);
                    inv.push(self.popup_rect());
                }
                Key::Right
                    if self
                        .hover_item
                        .and_then(|k| self.menus[self.open.unwrap_or(0)].entries.get(k))
                        .is_some_and(|e| matches!(e, MenuEntry::Sub(..))) =>
                {
                    // `Sub` 행에서 → = 펼치고 첫 항목으로.
                    let k = self.hover_item.unwrap_or(0);
                    self.open_sub(k, inv);
                    self.hover_sub = self.step_sub(None, true);
                }
                Key::Left | Key::Right => {
                    if let Some(i) = self.open {
                        let n = self.menus.len();
                        let next = if matches!(key, Key::Right) {
                            (i + 1) % n
                        } else {
                            (i + n - 1) % n
                        };
                        self.open_menu(next, inv);
                    }
                }
                Key::Enter | Key::Space => {
                    if let Some(k) = self.hover_item {
                        self.pick(k, inv);
                        if self.sub_open == Some(k) {
                            self.hover_sub = self.step_sub(None, true);
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.base.bounds;
        // 바 배경 + 아래 경계선.
        ctx.fill_rect(b, theme.chrome_bg);
        ctx.fill_rect(Rect::new(b.x, b.bottom() - 1, b.w, 1), theme.border);

        // 라벨/팝업 폭 실측 캐시 갱신(첫 페인트 이후 hit-test가 정확해진다).
        ctx.select_font(FontSlot::Base, false);
        {
            let mut m = self.measured.borrow_mut();
            m.0 = self
                .menus
                .iter()
                .map(|d| ctx.text_width(&d.label))
                .collect();
            m.1 = self
                .menus
                .iter()
                .map(|d| {
                    d.entries
                        .iter()
                        .map(|e| e.label_of().map_or(0, |l| ctx.text_width(l)))
                        .max()
                        .unwrap_or(0)
                        .min(if crate::draw::show_full() {
                            i32::MAX
                        } else {
                            self.s(self.max_label_w)
                        })
                })
                .collect();
        }

        self.surface.set(ctx.surface_size());
        // 하위 메뉴 폭 실측(펼친 것만).
        if let (Some(i), Some(k), Some(v)) = (self.open, self.sub_open, self.sub_entries()) {
            let w = v
                .iter()
                .map(|e| e.label_of().map_or(0, |l| ctx.text_width(l)))
                .max()
                .unwrap_or(0)
                .min(if crate::draw::show_full() {
                    i32::MAX
                } else {
                    self.s(self.max_label_w)
                });
            *self.sub_measured.borrow_mut() = Some(((i, k), w));
        }
        // 텍스트 세로 중앙 = 실측 높이(고정 16 근사 폐기 — 08-09).
        let th = ctx.text_height();
        // 최상위 라벨들 — 평평한 텍스트, 열림 = 선택색 / hover = 옅은 배경(Windows 스타일).
        for (i, d) in self.menus.iter().enumerate() {
            let lr = self.label_rect(i);
            let slot = Rect::new(lr.x, lr.y + 2, lr.w, lr.h - 4);
            if self.open == Some(i) {
                ctx.fill_rect(slot, theme.sel_bg);
            } else if self.hover_top == Some(i) && self.open.is_none() {
                ctx.fill_rect(slot, theme.panel_bg_alt);
            }
            let tw = ctx.text_width(&d.label);
            let ty = ctx.text_center_y(lr.y, lr.h);
            ctx.text(lr.x + (lr.w - tw) / 2, ty, lr, &d.label, theme.text);
        }

        // 드롭다운 — 직각에 가까운 패널(1px 테두리), 항목 전체폭 하이라이트 · 하위 메뉴는 그 뒤에(위에 덮이게).
        if let Some(i) = self.open {
            let pop = self.popup_rect();
            self.paint_panel(ctx, theme, pop, &self.menus[i].entries, self.hover_item, th);
            if let Some(v) = self.sub_entries() {
                let sr = self.sub_rect();
                self.paint_panel(ctx, theme, sr, v, self.hover_sub, th);
            }
        }
    }
}

impl MenuBar {
    /// 팝업 패널 하나(부모 드롭다운·하위 메뉴 공용) — `Sub` 행은 오른쪽에 `›`.
    fn paint_panel(
        &self,
        ctx: &mut dyn DrawCtx,
        theme: &Theme,
        pop: Rect,
        entries: &[MenuEntry],
        hover: Option<usize>,
        th: i32,
    ) {
        ctx.fill_rect(pop, theme.chrome_bg);
        ctx.stroke_round_rect(pop, self.s(2), theme.border, 1.0);
        let mut y = pop.y + self.s(POPUP_PAD);
        for (k, e) in entries.iter().enumerate() {
            let h = self.entry_h(e);
            match e {
                MenuEntry::Separator => {
                    ctx.fill_rect(
                        Rect::new(pop.x + self.s(6), y + h / 2, pop.w - self.s(12), 1),
                        theme.border,
                    );
                }
                MenuEntry::Item(it)
                | MenuEntry::Emph(it)
                | MenuEntry::Disabled(it)
                | MenuEntry::Sub(it, _) => {
                    let disabled = matches!(e, MenuEntry::Disabled(_));
                    let emph = matches!(e, MenuEntry::Emph(_));
                    let is_sub = matches!(e, MenuEntry::Sub(..));
                    let row = Rect::new(pop.x + 1, y, pop.w - 2, h);
                    if hover == Some(k) && !disabled {
                        ctx.fill_rect(row, theme.sel_bg);
                    }
                    let cy = row.y + h / 2;
                    let tx = row.x + self.s(10);
                    if let Some(img) = it.image.as_deref() {
                        let isz = self.s(LEADING_ICON);
                        let boxr = Rect::new(tx, cy - isz / 2, isz, isz);
                        let fit = image_fit_contain(boxr, img.w as i32, img.h as i32);
                        ctx.image_scaled(fit, img, row);
                    }
                    let tx = tx + self.s(LEADING_ICON) + self.s(6);
                    let right_pad = if is_sub { self.s(22) } else { self.s(10) };
                    let shown =
                        crate::draw::ellipsize_middle(ctx, &it.label, row.right() - right_pad - tx);
                    let fg = if disabled {
                        theme.text_dim
                    } else if emph {
                        theme.warn
                    } else {
                        theme.text
                    };
                    ctx.text(tx, cy - th / 2, row, &shown, fg);
                    if is_sub {
                        let a = self.s(10);
                        super::draw_chevron_right(
                            ctx,
                            Rect::new(row.right() - self.s(8) - a, cy - a / 2, a, a),
                            fg,
                        );
                    }
                }
            }
            y += h;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> (MenuBar, Invalidations) {
        let mut m = MenuBar::new(vec![
            MenuDef::new(
                "메뉴",
                vec![
                    MenuEntry::Item(ComboItem::new("settings", "설정")),
                    MenuEntry::Item(ComboItem::new("gallery", "갤러리")),
                    MenuEntry::Separator,
                    MenuEntry::Item(ComboItem::new("about", "About")),
                ],
            ),
            MenuDef::new(
                "도움말",
                vec![MenuEntry::Item(ComboItem::new("help", "도움말 보기"))],
            ),
            MenuDef::new(
                "편집",
                vec![
                    MenuEntry::Item(ComboItem::new("undo", "Undo")),
                    MenuEntry::sub(
                        "Line",
                        vec![
                            MenuEntry::Item(ComboItem::new("dup", "Duplicate")),
                            MenuEntry::Separator,
                            MenuEntry::Item(ComboItem::new("join", "Join")),
                        ],
                    ),
                    MenuEntry::Item(ComboItem::new("prefs", "Preferences")),
                ],
            ),
        ]);
        let mut inv = Invalidations::default();
        m.set_bounds(Rect::new(0, 0, 400, 28), &mut inv);
        (m, inv)
    }
    fn click(x: i32, y: i32) -> InputEvent {
        InputEvent::MouseDown {
            x,
            y,
            shift: false,
            primary: false,
        }
    }
    fn key(key: Key) -> InputEvent {
        InputEvent::Key {
            key,
            shift: false,
            primary: false,
        }
    }

    #[test]
    fn click_opens_and_picks_action_once() {
        let (mut m, mut inv) = bar();
        let l0 = m.label_rect(0);
        m.on_event(&click(l0.x + 5, l0.y + 5), &mut inv);
        assert!(m.is_open());
        let pop = m.popup_rect();
        // 두 번째 항목(갤러리) 중앙.
        let y = pop.y + m.s(POPUP_PAD) + m.s(ITEM_H) + m.s(ITEM_H) / 2;
        m.on_event(&click(pop.x + 20, y), &mut inv);
        assert!(!m.is_open(), "선택 = 닫힘");
        assert_eq!(m.take_picked().as_deref(), Some("gallery"));
        assert!(m.take_picked().is_none(), "1회성");
    }

    #[test]
    fn separator_is_not_pickable_and_keyboard_skips_it() {
        let (mut m, mut inv) = bar();
        let l0 = m.label_rect(0);
        m.on_event(&click(l0.x + 5, l0.y + 5), &mut inv);
        // ↓×3 = 설정→갤러리→(구분선 건너뜀)About.
        for _ in 0..3 {
            m.on_event(&key(Key::Down), &mut inv);
        }
        m.on_event(&key(Key::Enter), &mut inv);
        assert_eq!(m.take_picked().as_deref(), Some("about"));
    }

    #[test]
    fn hover_switches_open_menu_and_outside_click_closes() {
        let (mut m, mut inv) = bar();
        let l0 = m.label_rect(0);
        let l1 = m.label_rect(1);
        m.on_event(&click(l0.x + 5, l0.y + 5), &mut inv);
        // 열림 중 두 번째 라벨 hover = 전환.
        m.on_event(
            &InputEvent::MouseMove {
                x: l1.x + 5,
                y: l1.y + 5,
            },
            &mut inv,
        );
        assert!(m.is_open());
        let pop = m.popup_rect();
        assert_eq!(pop.x, l1.x, "팝업이 두 번째 라벨 아래로 이동");
        // 바깥 클릭 = 닫기(선택 없음).
        m.on_event(&click(800, 600), &mut inv);
        assert!(!m.is_open());
        assert!(m.take_picked().is_none());
        // 같은 라벨 재클릭 = 토글.
        m.on_event(&click(l0.x + 5, l0.y + 5), &mut inv);
        m.on_event(&click(l0.x + 5, l0.y + 5), &mut inv);
        assert!(!m.is_open());
    }
}

#[cfg(test)]
mod sub_tests {
    use super::*;

    fn bar() -> (MenuBar, Invalidations) {
        let mut m = MenuBar::new(vec![MenuDef::new(
            "편집",
            vec![
                MenuEntry::Item(ComboItem::new("undo", "Undo")),
                MenuEntry::sub(
                    "Line",
                    vec![
                        MenuEntry::Item(ComboItem::new("dup", "Duplicate")),
                        MenuEntry::Separator,
                        MenuEntry::Item(ComboItem::new("join", "Join")),
                    ],
                ),
                MenuEntry::Item(ComboItem::new("prefs", "Preferences")),
            ],
        )]);
        let mut inv = Invalidations::default();
        m.set_bounds(Rect::new(0, 0, 400, 28), &mut inv);
        let l0 = m.label_rect(0);
        m.on_event(
            &InputEvent::MouseDown {
                x: l0.x + 5,
                y: l0.y + 5,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        (m, inv)
    }
    fn key(key: Key) -> InputEvent {
        InputEvent::Key {
            key,
            shift: false,
            primary: false,
        }
    }
    fn mv(p: Point) -> InputEvent {
        InputEvent::MouseMove { x: p.x, y: p.y }
    }
    fn center(r: Rect) -> Point {
        Point {
            x: r.x + r.w / 2,
            y: r.y + r.h / 2,
        }
    }

    /// hover로 펼치고 · 하위 항목 클릭 = 그 id 보고 + 전부 닫힘.
    #[test]
    fn hover_opens_sub_and_click_picks_child() {
        let (mut m, mut inv) = bar();
        let row = m.entry_rect(1).unwrap();
        m.on_event(&mv(center(row)), &mut inv);
        assert_eq!(m.sub_open, Some(1));
        let sr = m.sub_rect();
        assert!(sr.x >= row.right() - m.s(2), "부모 오른쪽에");
        assert_eq!(
            sr.y,
            row.y - m.s(POPUP_PAD),
            "첫 항목이 부모 행과 같은 높이"
        );
        // 하위 두 번째 항목(구분선 건너 Join) 중앙.
        let y = sr.y + m.s(POPUP_PAD) + m.s(ITEM_H) + m.s(SEP_H) + m.s(ITEM_H) / 2;
        m.on_event(&mv(Point { x: sr.x + 20, y }), &mut inv);
        assert_eq!(m.hover_sub, Some(2));
        m.on_event(
            &InputEvent::MouseDown {
                x: sr.x + 20,
                y,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert!(!m.is_open());
        assert_eq!(m.take_picked().as_deref(), Some("join"));
    }

    /// 다른 부모 행으로 hover하면 접힌다 · 키보드 → 펼치고 ↓ Enter 선택 · ← 접기.
    #[test]
    fn other_row_closes_sub_and_keyboard_navigates() {
        let (mut m, mut inv) = bar();
        let row = m.entry_rect(1).unwrap();
        m.on_event(&mv(center(row)), &mut inv);
        assert!(m.sub_open.is_some());
        let other = m.entry_rect(2).unwrap();
        m.on_event(&mv(center(other)), &mut inv);
        assert!(m.sub_open.is_none(), "다른 행 = 접힘");
        // 키보드: ↓×2 = Line · → = 펼침(첫 항목 hover) · ↓ = Join(구분선 건너뜀) · Enter = 선택.
        m.hover_item = None;
        m.on_event(&key(Key::Down), &mut inv);
        m.on_event(&key(Key::Down), &mut inv);
        assert_eq!(m.hover_item, Some(1));
        m.on_event(&key(Key::Right), &mut inv);
        assert_eq!(m.sub_open, Some(1));
        assert_eq!(m.hover_sub, Some(0));
        m.on_event(&key(Key::Left), &mut inv);
        assert!(m.sub_open.is_none(), "← = 접기(메뉴 전환 아님)");
        assert_eq!(m.open, Some(0));
        m.on_event(&key(Key::Enter), &mut inv);
        assert_eq!(m.sub_open, Some(1), "Sub 행에서 Enter = 펼침");
        m.on_event(&key(Key::Down), &mut inv);
        m.on_event(&key(Key::Enter), &mut inv);
        assert_eq!(m.take_picked().as_deref(), Some("join"));
        assert!(!m.is_open());
    }

    /// 표면 오른쪽 끝에 닿으면 하위 메뉴는 부모 왼쪽으로(잘리지 않는다).
    #[test]
    fn sub_flips_left_when_surface_is_narrow() {
        let (mut m, mut inv) = bar();
        m.set_bounds(Rect::new(300, 0, 400, 28), &mut inv); // 바가 오른쪽에 있어 왼쪽에 자리가 있다
        let row = m.entry_rect(1).unwrap();
        m.on_event(&mv(center(row)), &mut inv);
        let pop = m.popup_rect();
        m.surface.set(Some((pop.right() + 20, 600)));
        let sr = m.sub_rect();
        assert!(sr.right() <= pop.right() + 20, "표면 안");
        assert!(sr.x < pop.x + 4, "부모 왼쪽으로 뒤집힘");
    }
}

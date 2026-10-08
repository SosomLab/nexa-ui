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
use std::collections::{HashMap, HashSet};

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
    /// 항목 위에서 눌렀다(`(하위 메뉴인가, 항목 index)`) — 같은 항목 위에서 놓을 때 확정(사용자 10-09 "풀다운도 Down에서 동작하지 않게" ·
    ///   Windows 메뉴 표준 = 누른 채 끌어 다른 항목에서 놓으면 그 항목).
    pressed_entry: Option<(bool, usize)>,
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
    /// ★ 항목 id → 단축키 표기(오른쪽 열 · `text_dim` · dir2 GUI-063 · nexa-dir3 103차 10-03). 메뉴 정의와 별개로 두어
    /// 키맵이 바뀌면 `set_shortcut`만 다시 부른다(메뉴 재구성 없음).
    shortcuts: HashMap<String, String>,
    /// 항목 id → 체크 상태(dir2 GUI-066 `set_checked`) — 표에 **있는** 항목만 체크 가능 항목으로 그린다.
    checks: HashMap<String, bool>,
    /// 체크 대신 **라디오 점**으로 그릴 id(보기 모드·테마·언어 그룹 — 호스트가 그룹의 id마다 `set_checked`로 하나만 켠다).
    radios: HashSet<String>,
    /// 메뉴별 단축키 열 폭 실측 캐시(측정 전엔 추정치).
    measured_sc: RefCell<Vec<i32>>,
    /// ↑/↓ 순환 이동 끔(156차 · 기본 false = 순환).
    no_wrap: bool,
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
            pressed_entry: None,
            sub_measured: RefCell::new(None),
            surface: std::cell::Cell::new(None),
            picked: None,
            measured: RefCell::new((Vec::new(), Vec::new())),
            max_label_w: 480,
            shortcuts: HashMap::new(),
            checks: HashMap::new(),
            radios: HashSet::new(),
            measured_sc: RefCell::new(Vec::new()),
            no_wrap: false,
        }
    }

    /// 드롭다운 · 하위 메뉴의 ↑/↓ **순환 이동(wrap-around)** 켬/끔(156차 · 기본 켜짐 = 종전: 끝에서 ↓ = 처음 · 처음에서 ↑ = 끝).
    /// 끄면 양 끝에서 멈춘다([`crate::controls::ContextMenu::set_wrap_around`]와 같은 뜻 — 호스트가 한 설정으로 둘을 맞춘다).
    /// ←/→ 의 메뉴 전환(파일 ↔ 편집 …)은 늘 순환한다.
    pub fn set_wrap_around(&mut self, on: bool) {
        self.no_wrap = !on;
    }

    /// 순환 이동이 켜져 있는가.
    #[must_use]
    pub fn wrap_around(&self) -> bool {
        !self.no_wrap
    }

    /// 항목의 단축키 표기(`""` = 지움). 키맵 변경 때 호스트가 다시 부른다 — 메뉴를 다시 만들지 않는다.
    pub fn set_shortcut(&mut self, id: &str, text: &str) {
        if text.is_empty() {
            self.shortcuts.remove(id);
        } else {
            self.shortcuts.insert(id.to_string(), text.to_string());
        }
        self.measured_sc.borrow_mut().clear();
    }

    /// 항목의 단축키 표기.
    #[must_use]
    pub fn shortcut_of(&self, id: &str) -> Option<&str> {
        self.shortcuts.get(id).map(String::as_str)
    }

    /// ★ 체크 상태(dir2 GUI-066) — 최상위·하위 항목 공통(id로 찾는다). 라디오 그룹은 호스트가 id마다 부른다. 열려 있으면 다시 그린다.
    pub fn set_checked(&mut self, id: &str, on: bool, inv: &mut Invalidations) {
        if self.checks.insert(id.to_string(), on) != Some(on) && self.is_open() {
            inv.push(self.popup_bounds());
        }
    }

    /// 체크 가능 항목의 상태(표에 없으면 None).
    #[must_use]
    pub fn is_checked(&self, id: &str) -> Option<bool> {
        self.checks.get(id).copied()
    }

    /// 이 id는 체크(✓) 대신 라디오 점(●)으로 그린다.
    pub fn set_radio(&mut self, id: &str) {
        self.radios.insert(id.to_string());
    }

    /// ★ 활성/비활성(dir2에는 없던 것 — nexa-ctl `Disabled` 변형을 id로 토글 · 최상위·하위 공통). 바뀌었으면 true.
    pub fn set_enabled(&mut self, id: &str, on: bool, inv: &mut Invalidations) -> bool {
        fn swap(entries: &mut [MenuEntry], id: &str, on: bool) -> bool {
            let mut changed = false;
            for e in entries.iter_mut() {
                let next = match e {
                    MenuEntry::Item(it) if !on && it.value == id => {
                        Some(MenuEntry::Disabled(it.clone()))
                    }
                    MenuEntry::Disabled(it) if on && it.value == id => {
                        Some(MenuEntry::Item(it.clone()))
                    }
                    MenuEntry::Sub(_, v) => {
                        changed |= swap(v, id, on);
                        None
                    }
                    _ => None,
                };
                if let Some(n) = next {
                    *e = n;
                    changed = true;
                }
            }
            changed
        }
        let mut changed = false;
        for m in &mut self.menus {
            changed |= swap(&mut m.entries, id, on);
        }
        if changed && self.is_open() {
            inv.push(self.popup_bounds());
        }
        changed
    }

    /// ★ 항목 라벨 바꾸기(176차 · nexa-dir3 "테마/언어 '시스템' 항목에 OS 현재 값 표시") — 최상위·하위 항목 공통(id로 찾는다 ·
    /// `Item`/`Emph`/`Disabled`/`Sub` 머리 모두) · 메뉴를 다시 만들지 않는다 · 폭 캐시를 비우고 열려 있으면 다시 그린다. 바뀌었으면 true.
    pub fn set_label(&mut self, id: &str, text: &str, inv: &mut Invalidations) -> bool {
        fn walk(entries: &mut [MenuEntry], id: &str, text: &str) -> bool {
            let mut changed = false;
            for e in entries.iter_mut() {
                match e {
                    MenuEntry::Item(it) | MenuEntry::Emph(it) | MenuEntry::Disabled(it)
                        if it.value == id =>
                    {
                        if it.label != text {
                            it.label = text.to_string();
                            changed = true;
                        }
                    }
                    MenuEntry::Sub(it, v) => {
                        if it.value == id && it.label != text {
                            it.label = text.to_string();
                            changed = true;
                        }
                        changed |= walk(v, id, text);
                    }
                    _ => {}
                }
            }
            changed
        }
        let mut changed = false;
        for m in &mut self.menus {
            changed |= walk(&mut m.entries, id, text);
        }
        if changed {
            *self.measured.borrow_mut() = (Vec::new(), Vec::new());
            *self.sub_measured.borrow_mut() = None;
            if self.is_open() {
                inv.push(self.popup_bounds());
            }
        }
        changed
    }

    /// 항목 라벨(최상위·하위 공통 · 없으면 None).
    #[must_use]
    pub fn label_of(&self, id: &str) -> Option<&str> {
        fn find<'a>(entries: &'a [MenuEntry], id: &str) -> Option<&'a str> {
            for e in entries {
                match e {
                    MenuEntry::Item(it) | MenuEntry::Emph(it) | MenuEntry::Disabled(it)
                        if it.value == id =>
                    {
                        return Some(it.label.as_str());
                    }
                    MenuEntry::Sub(it, v) => {
                        if it.value == id {
                            return Some(it.label.as_str());
                        }
                        if let Some(l) = find(v, id) {
                            return Some(l);
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        self.menus.iter().find_map(|m| find(&m.entries, id))
    }

    /// 프로그램으로 `i`번째 최상위 메뉴를 연다(호스트 Alt/F10 진입 · dir2 GUI-062).
    pub fn open_menu_index(&mut self, i: usize, inv: &mut Invalidations) {
        if i < self.menus.len() {
            self.open_menu(i, inv);
            self.hover_item = self.step_item(None, true);
        }
    }

    /// 열린 최상위 메뉴 index.
    #[must_use]
    pub fn open_index(&self) -> Option<usize> {
        self.open
    }

    /// 메뉴 i의 단축키 열 폭(실측 캐시 → 추정).
    fn shortcut_w(&self, i: usize) -> i32 {
        if let Some(w) = self.measured_sc.borrow().get(i) {
            return *w;
        }
        self.menus.get(i).map_or(0, |m| {
            m.entries
                .iter()
                .filter_map(|e| match e {
                    MenuEntry::Item(it) | MenuEntry::Emph(it) | MenuEntry::Disabled(it) => {
                        self.shortcuts.get(&it.value)
                    }
                    _ => None,
                })
                .map(|s| self.estimate_w(s))
                .max()
                .unwrap_or(0)
        })
    }

    /// 단축키 열이 있으면 라벨과의 사이 간격을 더한 폭.
    fn shortcut_col(&self, i: usize) -> i32 {
        let w = self.shortcut_w(i);
        if w > 0 {
            w + self.s(24)
        } else {
            0
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
        self.measured_sc.borrow_mut().clear();
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
        let w = (content_w + self.shortcut_col(i) + self.s(LEADING_ICON) + self.s(34))
            .max(self.s(POPUP_MIN_W));
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
            (Some(p), down) => idxs[step_index(p, idxs.len(), down, !self.no_wrap)],
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
            (Some(p), down) => idxs[step_index(p, idxs.len(), down, !self.no_wrap)],
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

/// 고를 수 있는 항목 `n`개 중 `p`번째에서 한 칸 이동한 자리(순수 · 156차): 순환이면 끝 ↔ 처음으로 넘고, 아니면 양 끝에서 멈춘다.
fn step_index(p: usize, n: usize, down: bool, wrap: bool) -> usize {
    match (down, wrap) {
        (true, true) => (p + 1) % n,
        (false, true) => (p + n - 1) % n,
        (true, false) => (p + 1).min(n - 1),
        (false, false) => p.saturating_sub(1),
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
                        self.pressed_entry = Some((true, k));
                    } else if let Some(k) = self.entry_at(x, y) {
                        // 하위 메뉴 항목은 누를 때 펼친다(hover와 같음) · 일반 항목은 놓을 때 확정.
                        let is_sub = matches!(
                            self.open.and_then(|i| self.menus[i].entries.get(k)),
                            Some(MenuEntry::Sub(..))
                        );
                        if is_sub {
                            self.pick(k, inv);
                        } else {
                            self.pressed_entry = Some((false, k));
                        }
                    } else {
                        self.close(inv); // 바깥 클릭 = 닫기
                    }
                }
            }
            InputEvent::MouseUp { x, y } => {
                // 놓은 자리의 항목을 확정(누른 항목과 달라도 — 누른 채 끌어 고르는 표준) · 항목 밖에서 놓으면 아무것도 없음.
                if self.pressed_entry.take().is_some() && self.open.is_some() {
                    if let Some(k) = self.sub_entry_at(x, y) {
                        self.pick_sub(k, inv);
                    } else if let Some(k) = self.entry_at(x, y) {
                        if !matches!(
                            self.open.and_then(|i| self.menus[i].entries.get(k)),
                            Some(MenuEntry::Sub(..))
                        ) {
                            self.pick(k, inv);
                        }
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
            // 단축키 열 폭(메뉴별 최댓값).
            *self.measured_sc.borrow_mut() = self
                .menus
                .iter()
                .map(|d| {
                    d.entries
                        .iter()
                        .filter_map(|e| match e {
                            MenuEntry::Item(it) | MenuEntry::Emph(it) | MenuEntry::Disabled(it) => {
                                self.shortcuts.get(&it.value)
                            }
                            _ => None,
                        })
                        .map(|s| ctx.text_width(s))
                        .max()
                        .unwrap_or(0)
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
                    let fg = if disabled {
                        theme.text_dim
                    } else if emph {
                        theme.warn
                    } else {
                        theme.text
                    };
                    if let Some(img) = it.image.as_deref() {
                        let isz = self.s(LEADING_ICON);
                        let boxr = Rect::new(tx, cy - isz / 2, isz, isz);
                        let fit = image_fit_contain(boxr, img.w as i32, img.h as i32);
                        ctx.image_scaled(fit, img, row);
                    } else if self.checks.get(&it.value).copied() == Some(true) {
                        // ✓ 또는 라디오 ●(앞 아이콘 자리 · 강조색 · dir2 GUI-063 체크 열).
                        let isz = self.s(LEADING_ICON);
                        let mark = if disabled {
                            theme.text_dim
                        } else {
                            theme.accent
                        };
                        if self.radios.contains(&it.value) {
                            let d = (isz / 2).max(4);
                            ctx.fill_ellipse(Rect::new(tx + (isz - d) / 2, cy - d / 2, d, d), mark);
                        } else {
                            super::draw_check_mark(
                                ctx,
                                Rect::new(tx, cy - isz / 2, isz, isz),
                                mark,
                            );
                        }
                    }
                    let tx = tx + self.s(LEADING_ICON) + self.s(6);
                    // 단축키 열(오른쪽 정렬 · 흐린 글자) — 라벨 폭 상한에서 뺀다.
                    let sc = if is_sub {
                        None
                    } else {
                        self.shortcuts.get(&it.value)
                    };
                    let right_pad = if is_sub { self.s(22) } else { self.s(10) };
                    let mut label_right = row.right() - right_pad;
                    if let Some(sc) = sc {
                        let sw = ctx.text_width(sc);
                        let sx = row.right() - right_pad - sw;
                        ctx.text(sx, cy - th / 2, row, sc, theme.text_dim);
                        label_right = sx - self.s(16);
                    }
                    let shown = crate::draw::ellipsize_middle(ctx, &it.label, label_right - tx);
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

    /// 클릭 = 누름 + 놓음(놓을 때 동작 · 10-09).
    fn tap<C: Control>(c: &mut C, x: i32, y: i32, inv: &mut Invalidations) {
        c.on_event(&click(x, y), inv);
        c.on_event(&InputEvent::MouseUp { x, y }, inv);
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
        tap(&mut m, l0.x + 5, l0.y + 5, &mut inv);
        assert!(m.is_open());
        let pop = m.popup_rect();
        // 두 번째 항목(갤러리) 중앙.
        let y = pop.y + m.s(POPUP_PAD) + m.s(ITEM_H) + m.s(ITEM_H) / 2;
        tap(&mut m, pop.x + 20, y, &mut inv);
        assert!(!m.is_open(), "선택 = 닫힘");
        assert_eq!(m.take_picked().as_deref(), Some("gallery"));
        assert!(m.take_picked().is_none(), "1회성");
    }

    /// 순환 이동 스위치(156차): 기본 = 끝에서 ↓ = 처음 · 끄면 양 끝에서 멈춘다.
    #[test]
    fn keyboard_wrap_around_can_be_switched_off() {
        assert_eq!(step_index(2, 3, true, true), 0);
        assert_eq!(step_index(0, 3, false, true), 2);
        assert_eq!(step_index(2, 3, true, false), 2);
        assert_eq!(step_index(0, 3, false, false), 0);
        assert_eq!(step_index(1, 3, true, false), 2);
        assert_eq!(step_index(1, 3, false, false), 0);
        let (mut m, mut inv) = bar();
        assert!(m.wrap_around(), "기본 = 순환");
        let l0 = m.label_rect(0);
        tap(&mut m, l0.x + 5, l0.y + 5, &mut inv);
        // 항목 3개(설정 · 갤러리 · About): ↓×4 = 한 바퀴 돌아 첫 항목.
        for _ in 0..4 {
            m.on_event(&key(Key::Down), &mut inv);
        }
        m.on_event(&key(Key::Enter), &mut inv);
        let first = m.take_picked();
        assert!(first.is_some());
        // 끔: ↓를 많이 눌러도 마지막(About)에서 멈춘다 · ↑를 많이 눌러도 첫 항목에서 멈춘다.
        m.set_wrap_around(false);
        assert!(!m.wrap_around());
        tap(&mut m, l0.x + 5, l0.y + 5, &mut inv);
        for _ in 0..9 {
            m.on_event(&key(Key::Down), &mut inv);
        }
        m.on_event(&key(Key::Enter), &mut inv);
        assert_eq!(m.take_picked().as_deref(), Some("about"), "끝에서 멈춤");
        tap(&mut m, l0.x + 5, l0.y + 5, &mut inv);
        m.on_event(&key(Key::Down), &mut inv);
        for _ in 0..9 {
            m.on_event(&key(Key::Up), &mut inv);
        }
        m.on_event(&key(Key::Enter), &mut inv);
        assert_eq!(m.take_picked(), first, "처음에서 멈춤");
    }

    #[test]
    fn separator_is_not_pickable_and_keyboard_skips_it() {
        let (mut m, mut inv) = bar();
        let l0 = m.label_rect(0);
        tap(&mut m, l0.x + 5, l0.y + 5, &mut inv);
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
        tap(&mut m, l0.x + 5, l0.y + 5, &mut inv);
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
        tap(&mut m, 800, 600, &mut inv);
        assert!(!m.is_open());
        assert!(m.take_picked().is_none());
        // 같은 라벨 재클릭 = 토글.
        tap(&mut m, l0.x + 5, l0.y + 5, &mut inv);
        tap(&mut m, l0.x + 5, l0.y + 5, &mut inv);
        assert!(!m.is_open());
    }

    /// ★ dir2 메뉴 기능(GUI-063·066 · nexa-dir3 103차): 단축키 열이 오른쪽에 그려지고 팝업이 그만큼 넓어진다 · 체크 ✓/라디오 ●이
    /// 켜진 항목에만 그려진다 · `set_checked`는 열린 팝업을 무효화한다 · 라벨은 단축키 열을 침범하지 않는다.
    #[test]
    fn shortcut_column_and_check_marks() {
        use crate::controls::RecordCtx;
        let (mut m, mut inv) = bar();
        let l0 = m.label_rect(0);
        tap(&mut m, l0.x + 5, l0.y + 5, &mut inv);
        let w0 = m.popup_rect().w;
        m.set_shortcut("settings", "Ctrl+,");
        m.set_shortcut("about", "");
        assert_eq!(m.shortcut_of("settings"), Some("Ctrl+,"));
        assert_eq!(m.shortcut_of("about"), None);
        assert!(
            m.popup_rect().w >= w0 && m.shortcut_col(0) > 0,
            "단축키 열이 폭에 더해진다(추정치 · 최소 폭에 가릴 수 있다)"
        );
        let mut rec = RecordCtx::with_surface(800, 600);
        m.paint(&mut rec, &Theme::dark());
        assert!(
            m.popup_rect().w >= w0 && m.shortcut_w(0) == 7 * 6,
            "실측 뒤 단축키 열 폭 = 글자수 × 7"
        );
        assert!(rec.drew_text("Ctrl+,"), "단축키 표기");
        let pop = m.popup_rect();
        let sc = rec
            .texts
            .iter()
            .find(|(_, _, _, s)| s == "Ctrl+,")
            .map(|(x, ..)| *x)
            .unwrap_or(0);
        let label = rec
            .texts
            .iter()
            .find(|(_, _, _, s)| s == "설정")
            .map(|(x, ..)| *x + 7 * 2)
            .unwrap_or(0);
        assert!(
            sc + 7 * 6 <= pop.right() && label < sc,
            "오른쪽 정렬 · 라벨 왼쪽"
        );
        // 체크: 켜진 항목만 ✓(폴리라인) · 라디오는 ●(타원).
        let marks_before = rec.polylines;
        m.set_checked("gallery", true, &mut inv);
        assert_eq!(m.is_checked("gallery"), Some(true));
        assert!(!inv.is_empty(), "열린 팝업 무효화");
        rec.clear();
        m.paint(&mut rec, &Theme::dark());
        assert!(rec.polylines > marks_before, "✓ 그려짐");
        m.set_checked("gallery", false, &mut inv);
        rec.clear();
        m.paint(&mut rec, &Theme::dark());
        assert_eq!(rec.polylines, marks_before, "꺼지면 안 그림");
        m.set_radio("settings");
        m.set_checked("settings", true, &mut inv);
        rec.clear();
        m.paint(&mut rec, &Theme::dark());
        assert_eq!(rec.polylines, marks_before, "라디오는 폴리라인이 아니다");
        assert!(rec.all_inside(Rect::new(0, 0, 800, 600)));
    }

    /// `set_enabled(false)`면 클릭해도 발화하지 않고 흐리게 · 다시 켜면 발화 · 하위 메뉴 항목도 id로 닿는다 · `open_menu_index`.
    /// 176차: 라벨 바꾸기 = 최상위 · 하위 항목 모두 id로 · 같은 글이면 false · 없는 id = None.
    #[test]
    fn set_label_updates_top_and_sub_items() {
        let (mut m, mut inv) = bar();
        assert_eq!(m.label_of("dup"), Some("Duplicate"));
        assert!(m.set_label("dup", "Dup!", &mut inv));
        assert_eq!(m.label_of("dup"), Some("Dup!"));
        assert!(!m.set_label("dup", "Dup!", &mut inv), "같은 글 = 변화 없음");
        assert!(m.set_label("settings", "Settings (ko)", &mut inv));
        assert_eq!(m.label_of("settings"), Some("Settings (ko)"));
        assert_eq!(m.label_of("nope"), None);
        assert!(!m.set_label("nope", "x", &mut inv));
    }

    #[test]
    fn set_enabled_toggles_and_open_menu_index() {
        let (mut m, mut inv) = bar();
        assert!(m.set_enabled("gallery", false, &mut inv));
        assert!(
            !m.set_enabled("gallery", false, &mut inv),
            "같은 값 = 안 바뀜"
        );
        assert!(m.set_enabled("dup", false, &mut inv), "하위 메뉴 항목");
        m.open_menu_index(0, &mut inv);
        assert_eq!(m.open_index(), Some(0));
        let pop = m.popup_rect();
        let y = pop.y + m.s(POPUP_PAD) + m.s(ITEM_H) + m.s(ITEM_H) / 2;
        tap(&mut m, pop.x + 20, y, &mut inv);
        assert!(
            m.is_open() && m.take_picked().is_none(),
            "비활성 = 발화 없음 · 열린 채"
        );
        assert!(m.set_enabled("gallery", true, &mut inv));
        tap(&mut m, pop.x + 20, y, &mut inv);
        assert_eq!(m.take_picked().as_deref(), Some("gallery"));
        m.open_menu_index(99, &mut inv);
        assert!(!m.is_open(), "범위 밖 = 무시");
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
        assert!(m.is_open(), "누름만으로는 확정 안 됨(10-09)");
        m.on_event(&InputEvent::MouseUp { x: sr.x + 20, y }, &mut inv);
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

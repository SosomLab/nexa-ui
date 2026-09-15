//! **컨텍스트 메뉴**(우클릭 팝업) — 커서 자리에 뜨는 작은 목록.
//!
//! 우클릭이 **곧바로 동작을 실행하면 되돌릴 방법이 없다**. 예전 대화 창은 풍선을
//! 우클릭하는 순간 클립보드를 덮어써서, 사용자가 "복사가 됐는지, 뭐가 됐는지" 알 수 없었다
//! (08-10 지적). 메뉴는 **무엇이 일어날지 먼저 보여주고 고르게** 한다.
//!
//! 이 컨트롤은 **무엇을 할지 모른다** — 항목 id를 돌려줄 뿐이고, 실행은 호스트 몫이다
//! (클립보드 접근은 `<app>-plat`에 있고 UI 크레이트는 그걸 모른다 — 이음새 유지).
//!
//! 화면 밖으로 나가지 않도록 **경계 안으로 접어 넣는다**(오른쪽·아래에서 열면 위/왼쪽으로).
//!
//! ★ 09-15(nexa-sql 사용자 "DBeaver처럼"): 항목에 **아이콘**(알파 마스크 · 상태색으로 틴트) · **단축키 문구**(오른쪽 정렬 · 흐리게) ·
//! **하위 메뉴**(`children` · 오른쪽 `›` · hover/→/클릭으로 펼침 · Esc/←로 접힘). 아이콘이 하나라도 있으면 **아이콘 칸을 전 행에
//! 예약**해 아이콘 없는 항목의 글자도 세로로 정렬된다.

use crate::draw::DrawCtx;
use crate::event::{InputEvent, Key};
use crate::geom::{Point, Rect};
use crate::theme::{IconImage, Theme};
use crate::FontSlot;
use std::rc::Rc;

// 레이아웃 상수(논리 px).
const PAD_H: i32 = 12;
const PAD_V: i32 = 5;
const ROW_EXTRA: i32 = 10;
const SEP_H: i32 = 7;
const MIN_W: i32 = 120;
const RADIUS: i32 = 6;
/// 아이콘 칸(아이콘 한 변 + 오른쪽 여백).
const ICON_PX: i32 = 16;
const ICON_GAP: i32 = 8;
/// 라벨과 단축키 사이 최소 간격.
const SC_GAP: i32 = 28;
/// 하위 메뉴 화살표 칸.
const ARROW_W: i32 = 14;

/// 메뉴 아이콘 — **알파 마스크만**(색은 그릴 때 행 상태색으로 틴트 · 테마 전환에 다시 만들 필요 없음).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuIcon {
    /// 폭(px).
    pub w: u32,
    /// 높이(px).
    pub h: u32,
    /// `w*h` 커버리지.
    pub alpha: Rc<[u8]>,
}

impl MenuIcon {
    /// 마스크로 만든다.
    ///
    /// # Panics
    /// 길이가 `w*h`가 아니면 패닉(구성 오류).
    #[must_use]
    pub fn from_alpha(w: u32, h: u32, alpha: &[u8]) -> Self {
        assert_eq!(alpha.len(), (w * h) as usize, "알파 마스크 길이 불일치");
        Self {
            w,
            h,
            alpha: Rc::from(alpha),
        }
    }
}

/// 메뉴 항목.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CtxItem {
    /// 고를 수 있는 항목 — `(id, 라벨, 활성)`. 비활성은 흐리게 표시되고 골라지지 않는다.
    Item {
        /// 호스트가 받을 식별자.
        id: String,
        /// 표시 문구(i18n 적용 후).
        label: String,
        /// `false`면 흐리게 · 선택 불가(선택할 게 없는데 "복사"가 멀쩡히 보이면 거짓말이다).
        enabled: bool,
        /// 아이콘(옵션).
        icon: Option<MenuIcon>,
        /// 단축키 문구(옵션 · 오른쪽 정렬 · 표시 전용 — 키 처리는 호스트 키맵 몫).
        shortcut: Option<String>,
        /// 하위 메뉴(비면 없음).
        children: Vec<CtxItem>,
    },
    /// 구분선.
    Separator,
}

impl CtxItem {
    /// 활성 항목.
    #[must_use]
    pub fn item(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::Item {
            id: id.into(),
            label: label.into(),
            enabled: true,
            icon: None,
            shortcut: None,
            children: Vec::new(),
        }
    }
    /// 활성 여부를 지정한 항목.
    #[must_use]
    pub fn maybe(id: impl Into<String>, label: impl Into<String>, enabled: bool) -> Self {
        Self::Item {
            id: id.into(),
            label: label.into(),
            enabled,
            icon: None,
            shortcut: None,
            children: Vec::new(),
        }
    }
    /// 하위 메뉴 항목(라벨 + 자식 목록 · 활성 자식이 없으면 비활성).
    #[must_use]
    pub fn submenu(
        id: impl Into<String>,
        label: impl Into<String>,
        children: Vec<CtxItem>,
    ) -> Self {
        let enabled = children
            .iter()
            .any(|c| matches!(c, CtxItem::Item { enabled: true, .. }));
        Self::Item {
            id: id.into(),
            label: label.into(),
            enabled,
            icon: None,
            shortcut: None,
            children,
        }
    }
    /// 아이콘 붙이기(빌더).
    #[must_use]
    pub fn with_icon(mut self, ic: Option<MenuIcon>) -> Self {
        if let Self::Item { icon, .. } = &mut self {
            *icon = ic;
        }
        self
    }
    /// 단축키 문구 붙이기(빌더 · 빈 문자열 = 없음).
    #[must_use]
    pub fn with_shortcut(mut self, sc: impl Into<String>) -> Self {
        if let Self::Item { shortcut, .. } = &mut self {
            let s: String = sc.into();
            *shortcut = (!s.is_empty()).then_some(s);
        }
        self
    }
    fn has_children(&self) -> bool {
        matches!(self, Self::Item { children, .. } if !children.is_empty())
    }
}

/// 커서 자리에 뜨는 팝업 메뉴.
#[derive(Clone, Debug, Default)]
pub struct ContextMenu {
    items: Vec<CtxItem>,
    /// 열려 있으면 좌상단 좌표(경계 보정 완료 · 셀 — paint의 실측 재접기).
    at: std::cell::Cell<Option<Point>>,
    hover: Option<usize>,
    picked: Option<String>,
    scale: f32,
    /// 마지막으로 계산한 팝업 rect(히트 판정용). 열 때는 호스트가 준 근사 폭으로
    /// 잡고, **첫 paint가 실측 폭으로 보정**한다(셀 — paint는 `&self`).
    rect: std::cell::Cell<Rect>,
    /// 팝업이 넘어가면 안 되는 영역(열 때 저장 — 실측 보정 후 경계 재접기용).
    host: Rect,
    /// 라벨 최대 폭(px) — 열 때는 호스트 근사, paint가 실측으로 올려친다(08-14
    /// 실기: 자당 근사가 글꼴 크기에 뒤처져 "소유자만 초대로 전환"이 잘렸다).
    fit_w: std::cell::Cell<i32>,
    /// 단축키 문구 최대 폭(px · paint 실측).
    sc_w: std::cell::Cell<i32>,
    /// 열린 하위 메뉴(+ 어느 항목의 것인가).
    child: Option<Box<ContextMenu>>,
    child_of: Option<usize>,
}

impl ContextMenu {
    /// 새 메뉴(닫힌 상태).
    #[must_use]
    pub fn new() -> Self {
        Self {
            scale: 1.0,
            ..Self::default()
        }
    }

    /// 배율(고DPI).
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
    }

    fn s(&self, v: i32) -> i32 {
        (v as f32 * self.scale).round() as i32
    }

    /// 열려 있는가.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.at.get().is_some()
    }

    /// 현재 팝업 영역(닫혀 있으면 빈 rect · 하위 메뉴까지 합집합) — 호스트의 무효화 범위 계산용.
    #[must_use]
    pub fn bounds(&self) -> Rect {
        if !self.is_open() {
            return Rect::new(0, 0, 0, 0);
        }
        let r = self.rect.get();
        match &self.child {
            Some(c) if c.is_open() => r.union(&c.bounds()),
            _ => r,
        }
    }

    /// 닫는다(하위 메뉴 포함).
    pub fn close(&mut self) {
        self.at.set(None);
        self.hover = None;
        self.child = None;
        self.child_of = None;
    }

    /// **`(x, y)`에 연다** — `host`는 팝업이 넘어가면 안 되는 영역(보통 창 전체).
    /// `text_w`는 라벨 최대 폭(호스트가 [`DrawCtx::text_width`]로 잰 값): 이 컨트롤은
    /// paint 밖에서 글자를 잴 수 없어, 폭 측정만 호출 측에서 받는다.
    pub fn open_at(&mut self, x: i32, y: i32, items: Vec<CtxItem>, host: Rect, text_w: i32) {
        if items.is_empty() {
            self.close();
            return;
        }
        self.items = items;
        self.fit_w.set(text_w);
        self.sc_w.set(0);
        self.host = host;
        self.hover = None;
        self.picked = None;
        self.child = None;
        self.child_of = None;
        let (w, h) = self.size_px();
        // 경계 접기 — 오른쪽/아래로 넘치면 커서 반대쪽으로 편다.
        let px = if x + w > host.right() {
            (x - w).max(host.x)
        } else {
            x
        };
        let py = if y + h > host.bottom() {
            (y - h).max(host.y)
        } else {
            y
        };
        self.at.set(Some(Point { x: px, y: py }));
        self.rect.set(Rect::new(px, py, w, h));
    }

    fn row_h(&self) -> i32 {
        // 글꼴 높이를 모르는 자리라 논리 px 기준으로 잡는다(paint에서 실제 글자를 세로 중앙에 둔다).
        self.s(16 + ROW_EXTRA)
    }

    fn has_icons(&self) -> bool {
        self.items
            .iter()
            .any(|it| matches!(it, CtxItem::Item { icon: Some(_), .. }))
    }

    fn has_arrows(&self) -> bool {
        self.items.iter().any(CtxItem::has_children)
    }

    /// 아이콘 칸 폭(아이콘이 하나라도 있으면 전 행 예약 — 글자 세로 정렬).
    fn icon_col(&self) -> i32 {
        if self.has_icons() {
            self.s(ICON_PX + ICON_GAP)
        } else {
            0
        }
    }

    fn size_px(&self) -> (i32, i32) {
        let mut h = self.s(PAD_V) * 2;
        for it in &self.items {
            h += match it {
                CtxItem::Item { .. } => self.row_h(),
                CtxItem::Separator => self.s(SEP_H),
            };
        }
        let sc = self.sc_w.get();
        let extra = if sc > 0 { self.s(SC_GAP) + sc } else { 0 }
            + if self.has_arrows() {
                self.s(ARROW_W)
            } else {
                0
            };
        let w = (self.icon_col() + self.fit_w.get() + extra + self.s(PAD_H) * 2).max(self.s(MIN_W));
        (w, h)
    }

    /// 인덱스 → 그 행의 rect.
    fn row_rect(&self, idx: usize) -> Option<Rect> {
        let at = self.at.get()?;
        let mut y = at.y + self.s(PAD_V);
        for (i, it) in self.items.iter().enumerate() {
            let h = match it {
                CtxItem::Item { .. } => self.row_h(),
                CtxItem::Separator => self.s(SEP_H),
            };
            if i == idx {
                return Some(Rect::new(at.x, y, self.rect.get().w, h));
            }
            y += h;
        }
        None
    }

    fn hit(&self, p: Point) -> Option<usize> {
        (0..self.items.len()).find(|&i| {
            matches!(self.items[i], CtxItem::Item { enabled: true, .. })
                && self.row_rect(i).is_some_and(|r| r.contains(p))
        })
    }

    /// 키보드 탐색(08-13 실기: "메뉴를 키보드로 이동 못 한다") — 활성 항목 사이를
    /// 순환한다(비활성·구분선 건너뜀 · 처음 ↓ = 첫 항목, 처음 ↑ = 마지막).
    fn move_hover(&mut self, down: bool) {
        let sel: Vec<usize> = (0..self.items.len())
            .filter(|&i| matches!(self.items[i], CtxItem::Item { enabled: true, .. }))
            .collect();
        if sel.is_empty() {
            return;
        }
        let cur = self.hover.and_then(|h| sel.iter().position(|&i| i == h));
        let next = match cur {
            None => {
                if down {
                    0
                } else {
                    sel.len() - 1
                }
            }
            Some(p) => {
                if down {
                    (p + 1) % sel.len()
                } else {
                    (p + sel.len() - 1) % sel.len()
                }
            }
        };
        self.hover = Some(sel[next]);
    }

    /// 항목 `i`의 하위 메뉴를 연다(오른쪽 · 넘치면 왼쪽). 이미 그 항목의 것이 열려 있으면 그대로.
    fn open_child(&mut self, i: usize, hover_first: bool) {
        if self.child_of == Some(i) && self.child.as_ref().is_some_and(|c| c.is_open()) {
            if hover_first {
                if let Some(c) = &mut self.child {
                    if c.hover.is_none() {
                        c.move_hover(true);
                    }
                }
            }
            return;
        }
        let Some(CtxItem::Item { children, .. }) = self.items.get(i) else {
            return;
        };
        if children.is_empty() {
            return;
        }
        let Some(row) = self.row_rect(i) else { return };
        let mut c = ContextMenu::new();
        c.set_scale(self.scale);
        // 라벨 폭 근사(부모와 같은 근사 · paint가 실측으로 보정).
        let approx = children
            .iter()
            .map(|it| match it {
                CtxItem::Item { label, .. } => label
                    .chars()
                    .map(|ch| if ch.is_ascii() { 8 } else { 15 })
                    .sum::<i32>(),
                CtxItem::Separator => 0,
            })
            .max()
            .unwrap_or(0);
        let x = row.right() - self.s(4);
        let y = row.y - self.s(PAD_V);
        c.open_at(x, y, children.clone(), self.host, self.s(approx));
        // 오른쪽에 자리가 없으면(open_at이 왼쪽으로 접었으면) 부모 왼쪽에 붙인다.
        let cw = c.rect.get().w;
        if x + cw > self.host.right() {
            let nx = (row.x - cw + self.s(4)).max(self.host.x);
            let cy = c.rect.get().y;
            c.at.set(Some(Point { x: nx, y: cy }));
            c.rect.set(Rect::new(nx, cy, cw, c.rect.get().h));
        }
        if hover_first {
            c.move_hover(true);
        }
        self.child = Some(Box::new(c));
        self.child_of = Some(i);
        self.hover = Some(i);
    }

    fn close_child(&mut self) {
        self.child = None;
        self.child_of = None;
    }

    /// 이벤트 처리 — `true`면 **소비**(호스트는 그 이벤트를 아래 콘텐츠에 쓰지 않는다).
    ///
    /// 열려 있는 동안은 바깥 클릭·Esc로 닫히며, 그 클릭도 소비한다
    /// (메뉴를 닫으려던 클릭이 뒤 컨텐츠의 선택을 바꾸면 놀란다).
    pub fn on_event(&mut self, ev: &InputEvent) -> bool {
        if !self.is_open() {
            return false;
        }
        // ── 하위 메뉴가 열려 있으면: 그 안의 사건은 자식이 · 부모 행 위 이동은 부모가(다른 항목 = 자식 교체).
        let child_open = self.child.as_ref().is_some_and(|c| c.is_open());
        if child_open {
            let child_rect = self
                .child
                .as_ref()
                .map_or(Rect::default(), |c| c.rect.get());
            match *ev {
                InputEvent::MouseMove { x, y } => {
                    let p = Point { x, y };
                    if child_rect.contains(p) {
                        return self.forward_child(ev);
                    }
                    if let Some(i) = self.hit(p) {
                        if Some(i) != self.child_of {
                            self.close_child();
                            self.hover = Some(i);
                            if self.items[i].has_children() {
                                self.open_child(i, false);
                            }
                        }
                        return true;
                    }
                    // 부모·자식 어디도 아님 — 자식 hover만 지운다.
                    return self.forward_child(ev);
                }
                InputEvent::MouseDown { x, y, .. } | InputEvent::RightDown { x, y } => {
                    let p = Point { x, y };
                    if child_rect.contains(p) {
                        return self.forward_child(ev);
                    }
                    // 부모 항목(하위 메뉴 있는 것) 클릭 = 유지 · 다른 부모 항목 = 일반 처리 · 바깥 = 전부 닫기.
                    if self.hit(p) == self.child_of {
                        return true;
                    }
                    self.close_child();
                    // 아래 일반 처리로 이어진다.
                }
                InputEvent::Key {
                    key: Key::Left | Key::Escape,
                    ..
                } => {
                    self.close_child();
                    return true;
                }
                _ => return self.forward_child(ev),
            }
        }
        match *ev {
            InputEvent::MouseMove { x, y } => {
                let h = self.hit(Point { x, y });
                let changed = h != self.hover;
                self.hover = h;
                if let Some(i) = h {
                    if self.items[i].has_children() {
                        self.open_child(i, false);
                    }
                }
                changed
            }
            InputEvent::MouseDown { x, y, .. } => {
                let p = Point { x, y };
                if let Some(i) = self.hit(p) {
                    if self.items[i].has_children() {
                        self.open_child(i, false);
                        return true;
                    }
                    if let CtxItem::Item { id, .. } = &self.items[i] {
                        self.picked = Some(id.clone());
                    }
                    self.close();
                } else {
                    // 팝업 안의 비활성 행/여백이면 그냥 무시, 바깥이면 닫는다.
                    if !self.rect.get().contains(p) {
                        self.close();
                    }
                }
                true
            }
            // 팝업이 열린 동안의 우클릭·휠·키는 모두 팝업이 먹고 닫는다.
            InputEvent::RightDown { x, y } => {
                if !self.rect.get().contains(Point { x, y }) {
                    self.close();
                }
                true
            }
            InputEvent::MouseUp { .. } | InputEvent::Wheel { .. } | InputEvent::HWheel { .. } => {
                true
            }
            // 키보드 — ↑/↓ 이동 · → 하위 열기 · Enter 선택(하위 있으면 열기) · 그 외(Esc 포함)는 메뉴만 닫는다.
            InputEvent::Key { key, .. } => {
                match key {
                    Key::Down => self.move_hover(true),
                    Key::Up => self.move_hover(false),
                    Key::Right => {
                        if let Some(i) = self.hover {
                            if self.items[i].has_children() {
                                self.open_child(i, true);
                            }
                        }
                    }
                    Key::Enter => {
                        if let Some(i) = self.hover {
                            if self.items[i].has_children() {
                                self.open_child(i, true);
                                return true;
                            }
                        }
                        if let Some(CtxItem::Item {
                            id, enabled: true, ..
                        }) = self.hover.map(|i| &self.items[i])
                        {
                            self.picked = Some(id.clone());
                        }
                        self.close();
                    }
                    _ => self.close(),
                }
                true
            }
            InputEvent::Char { .. } => {
                self.close();
                true
            }
            _ => false,
        }
    }

    /// 자식에게 넘기고 결과를 거둔다(선택 = 전부 닫기 · 자식이 바깥 클릭으로 닫혔으면 부모도 닫는다).
    fn forward_child(&mut self, ev: &InputEvent) -> bool {
        let Some(c) = &mut self.child else {
            return false;
        };
        let consumed = c.on_event(ev);
        if let Some(id) = c.take_picked() {
            self.picked = Some(id);
            self.close();
            return true;
        }
        if !c.is_open() {
            match ev {
                InputEvent::MouseDown { .. } | InputEvent::RightDown { .. } => self.close(),
                _ => self.close_child(),
            }
        }
        consumed
    }

    /// 현재 항목 목록(테스트·검증용) — 활성 여부까지 그대로 본다.
    #[must_use]
    pub fn items_for_test(&self) -> &[CtxItem] {
        &self.items
    }

    /// 인덱스 행의 rect(테스트·호스트 히트 검증용).
    #[must_use]
    pub fn row_rect_of(&self, idx: usize) -> Option<Rect> {
        self.row_rect(idx)
    }

    /// 열린 하위 메뉴(테스트).
    #[must_use]
    pub fn child_for_test(&self) -> Option<&ContextMenu> {
        self.child.as_deref().filter(|c| c.is_open())
    }

    /// 고른 항목 id를 **가져간다**(한 번만).
    pub fn take_picked(&mut self) -> Option<String> {
        self.picked.take()
    }

    /// 팝업 렌더 — 다른 것들을 다 그린 **뒤에** 불러야 위에 뜬다.
    pub fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let Some(mut at) = self.at.get() else { return };
        // ★ 폭 실측 보정(08-14 실기 — 자당 근사가 글꼴 크기에 뒤처져 라벨이 잘렸다):
        // 여기서만 글자를 잴 수 있으므로 첫 paint가 진짜 폭으로 올려치고, 넓어져서
        // 호스트 오른쪽을 넘으면 경계 접기를 다시 한다(히트 판정 rect·at 동기 갱신).
        ctx.select_font(FontSlot::Base, false);
        let mut real = 0;
        let mut sc_real = 0;
        for it in &self.items {
            if let CtxItem::Item {
                label, shortcut, ..
            } = it
            {
                real = real.max(ctx.text_width(label));
                if let Some(sc) = shortcut {
                    sc_real = sc_real.max(ctx.text_width(sc));
                }
            }
        }
        if real > self.fit_w.get() || sc_real > self.sc_w.get() {
            self.fit_w.set(real.max(self.fit_w.get()));
            self.sc_w.set(sc_real.max(self.sc_w.get()));
            let (w, h) = self.size_px();
            let mut x = at.x;
            if x + w > self.host.right() {
                x = (self.host.right() - w).max(self.host.x);
            }
            at = Point { x, y: at.y };
            self.at.set(Some(at));
            self.rect.set(Rect::new(x, at.y, w, h));
        }
        let r = self.rect.get();
        // 바탕 + 테두리(그림자 대신 테두리로 층을 만든다 — 렌더러에 블러가 없다).
        ctx.fill_round_rect(r, self.s(RADIUS), theme.panel_bg);
        ctx.stroke_round_rect(r, self.s(RADIUS), theme.border, 1.0);
        ctx.select_font(FontSlot::Base, false);
        let th = ctx.text_height();
        let icon_col = self.icon_col();
        let arrows = self.has_arrows();
        let mut y = at.y + self.s(PAD_V);
        for (i, it) in self.items.iter().enumerate() {
            match it {
                CtxItem::Item {
                    label,
                    enabled,
                    icon,
                    shortcut,
                    children,
                    ..
                } => {
                    let h = self.row_h();
                    let row = Rect::new(r.x, y, r.w, h);
                    let hot = *enabled && (self.hover == Some(i) || self.child_of == Some(i));
                    if hot {
                        ctx.fill_rect(
                            Rect::new(r.x + self.s(2), y, r.w - self.s(4), h),
                            theme.accent,
                        );
                    }
                    let fg = if !*enabled {
                        theme.text_dim
                    } else if hot {
                        theme.window_bg
                    } else {
                        theme.text
                    };
                    let mut x = r.x + self.s(PAD_H);
                    // 아이콘(상태색 틴트 · 세로 중앙) — 없는 행도 칸은 비워 둔다(글자 세로 정렬).
                    if let Some(ic) = icon {
                        let sz = self.s(ICON_PX);
                        let (cr, cg, cb) = fg.rgb();
                        let img = IconImage::from_alpha_tinted(ic.w, ic.h, &ic.alpha, (cr, cg, cb));
                        ctx.image_scaled(Rect::new(x, y + (h - sz) / 2, sz, sz), &img, row);
                    }
                    x += icon_col;
                    // 세로 정확히 가운데 — 글자 높이를 재서 놓는다(눈대중 상수 금지 · 08-09).
                    ctx.text(x, y + (h - th) / 2, row, label, fg);
                    // 단축키 — 오른쪽 정렬 · 흐리게(hover면 본문색).
                    let right =
                        r.right() - self.s(PAD_H) - if arrows { self.s(ARROW_W) } else { 0 };
                    if let Some(sc) = shortcut {
                        let w = ctx.text_width(sc);
                        let scfg = if hot { fg } else { theme.text_dim };
                        ctx.text(right - w, y + (h - th) / 2, row, sc, scfg);
                    }
                    // 하위 메뉴 화살표.
                    if !children.is_empty() {
                        let a = Rect::new(
                            r.right() - self.s(PAD_H) - self.s(ARROW_W) + self.s(4),
                            y + (h - self.s(10)) / 2,
                            self.s(10),
                            self.s(10),
                        );
                        super::draw_chevron_right(ctx, a, fg);
                    }
                    y += h;
                }
                CtxItem::Separator => {
                    let h = self.s(SEP_H);
                    ctx.fill_rect(
                        Rect::new(r.x + self.s(6), y + h / 2, r.w - self.s(12), 1),
                        theme.border,
                    );
                    y += h;
                }
            }
        }
        if let Some(c) = &self.child {
            c.paint(ctx, theme);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> Rect {
        Rect::new(0, 0, 400, 300)
    }
    fn items() -> Vec<CtxItem> {
        vec![
            CtxItem::item("copy", "복사"),
            CtxItem::maybe("cut", "잘라내기", false),
            CtxItem::Separator,
            CtxItem::item("paste", "붙여넣기"),
        ]
    }
    fn down(x: i32, y: i32) -> InputEvent {
        InputEvent::MouseDown {
            x,
            y,
            shift: false,
            primary: true,
        }
    }

    #[test]
    fn opens_at_cursor_and_picks_an_item() {
        let mut m = ContextMenu::new();
        assert!(!m.is_open());
        m.open_at(10, 10, items(), host(), 60);
        assert!(m.is_open());
        let first = m.row_rect(0).unwrap();
        assert!(m.on_event(&down(first.x + 5, first.y + 2)), "클릭 소비");
        assert_eq!(m.take_picked().as_deref(), Some("copy"));
        assert!(!m.is_open(), "고르면 닫힌다");
        assert_eq!(m.take_picked(), None, "결과는 한 번만 가져간다");
    }

    #[test]
    fn disabled_item_cannot_be_picked() {
        let mut m = ContextMenu::new();
        m.open_at(10, 10, items(), host(), 60);
        let cut = m.row_rect(1).unwrap();
        m.on_event(&down(cut.x + 5, cut.y + 2));
        assert_eq!(m.take_picked(), None, "비활성은 골라지지 않는다");
        assert!(m.is_open(), "비활성 클릭으로 닫히지도 않는다");
    }

    #[test]
    fn separator_is_not_selectable() {
        let mut m = ContextMenu::new();
        m.open_at(10, 10, items(), host(), 60);
        let sep = m.row_rect(2).unwrap();
        m.on_event(&down(sep.x + 5, sep.y));
        assert_eq!(m.take_picked(), None);
    }

    #[test]
    fn outside_click_closes_and_is_consumed() {
        // 닫으려는 클릭이 뒤 콘텐츠까지 가면 선택이 엉뚱하게 바뀐다.
        let mut m = ContextMenu::new();
        m.open_at(10, 10, items(), host(), 60);
        assert!(m.on_event(&down(390, 290)), "바깥 클릭도 소비");
        assert!(!m.is_open());
        assert_eq!(m.take_picked(), None);
    }

    #[test]
    fn folds_inside_host_bounds_near_edges() {
        let mut m = ContextMenu::new();
        // 우하단 모서리에서 열면 위/왼쪽으로 펴야 화면 밖으로 안 나간다.
        m.open_at(398, 298, items(), host(), 60);
        let r = m.bounds();
        assert!(r.right() <= host().right(), "오른쪽 경계 안: {r:?}");
        assert!(r.bottom() <= host().bottom(), "아래 경계 안: {r:?}");
        assert!(r.x >= 0 && r.y >= 0);
    }

    fn key(k: Key) -> InputEvent {
        InputEvent::Key {
            key: k,
            shift: false,
            primary: false,
        }
    }

    #[test]
    fn keyboard_navigates_skipping_disabled_and_picks_with_enter() {
        // 08-13 실기 — 키보드로 메뉴를 이동·선택할 수 없었다(모든 키가 닫기였다).
        let mut m = ContextMenu::new();
        m.open_at(10, 10, items(), host(), 60);
        assert!(m.on_event(&key(Key::Down)), "키 소비");
        assert!(m.is_open(), "↓는 메뉴를 닫지 않는다");
        m.on_event(&key(Key::Down)); // copy → paste(비활성 cut·구분선 건너뜀)
        m.on_event(&key(Key::Enter));
        assert_eq!(m.take_picked().as_deref(), Some("paste"));
        assert!(!m.is_open(), "Enter 선택 = 닫힘");
    }

    #[test]
    fn up_from_nothing_starts_at_last_enabled() {
        let mut m = ContextMenu::new();
        m.open_at(10, 10, items(), host(), 60);
        m.on_event(&key(Key::Up));
        m.on_event(&key(Key::Enter));
        assert_eq!(
            m.take_picked().as_deref(),
            Some("paste"),
            "↑ 시작 = 마지막 활성"
        );
    }

    #[test]
    fn escape_closes_without_pick_and_enter_without_hover_closes() {
        let mut m = ContextMenu::new();
        m.open_at(10, 10, items(), host(), 60);
        assert!(
            m.on_event(&key(Key::Escape)),
            "Esc 소비(창 닫기로 새면 안 된다)"
        );
        assert!(!m.is_open());
        assert_eq!(m.take_picked(), None);
        m.open_at(10, 10, items(), host(), 60);
        m.on_event(&key(Key::Enter)); // 아무것도 고르지 않은 Enter = 그냥 닫기
        assert!(!m.is_open());
        assert_eq!(m.take_picked(), None);
    }

    #[test]
    fn closed_menu_ignores_events() {
        let mut m = ContextMenu::new();
        assert!(!m.on_event(&down(5, 5)), "닫혀 있으면 소비하지 않는다");
    }

    #[test]
    fn empty_items_do_not_open() {
        // 줄 게 없으면 빈 상자를 띄우지 않는다.
        let mut m = ContextMenu::new();
        m.open_at(10, 10, vec![], host(), 60);
        assert!(!m.is_open());
    }

    fn nested() -> Vec<CtxItem> {
        vec![
            CtxItem::item("copy", "Copy").with_shortcut("Ctrl+C"),
            CtxItem::submenu(
                "adv",
                "Advanced Copy",
                vec![
                    CtxItem::item("csv", "CSV"),
                    CtxItem::submenu("sql", "SQL", vec![CtxItem::item("ins", "INSERT")]),
                ],
            ),
            CtxItem::Separator,
            CtxItem::item("all", "Select All"),
        ]
    }

    #[test]
    fn submenu_opens_on_click_and_pick_bubbles_up() {
        // 09-15 — DBeaver식 하위 메뉴: 부모 항목 클릭 = 펼침(선택 아님) · 자식 선택 = 전체 닫힘 + id 전달.
        let mut m = ContextMenu::new();
        m.open_at(10, 10, nested(), host(), 100);
        let adv = m.row_rect(1).unwrap();
        assert!(m.on_event(&down(adv.x + 5, adv.y + 2)));
        assert!(m.is_open(), "하위 메뉴 항목 클릭은 닫지 않는다");
        assert_eq!(m.take_picked(), None);
        let child = m.child_for_test().expect("하위 메뉴가 열린다");
        let csv = child.row_rect(0).unwrap();
        assert!(
            csv.x >= adv.right() - 10,
            "오른쪽에 붙는다: {csv:?} vs {adv:?}"
        );
        m.on_event(&down(csv.x + 5, csv.y + 2));
        assert_eq!(m.take_picked().as_deref(), Some("csv"));
        assert!(!m.is_open(), "자식 선택 = 전부 닫힘");
    }

    #[test]
    fn submenu_keyboard_right_enter_and_left() {
        let mut m = ContextMenu::new();
        m.open_at(10, 10, nested(), host(), 100);
        m.on_event(&key(Key::Down)); // copy
        m.on_event(&key(Key::Down)); // adv
        m.on_event(&key(Key::Right)); // 열고 첫 항목 hover
        assert!(m.child_for_test().is_some());
        m.on_event(&key(Key::Left));
        assert!(m.child_for_test().is_none(), "←는 하위 메뉴만 닫는다");
        assert!(m.is_open());
        m.on_event(&key(Key::Enter)); // 하위 있는 항목의 Enter = 열기
        assert!(m.child_for_test().is_some());
        m.on_event(&key(Key::Down)); // csv → sql
        m.on_event(&key(Key::Right)); // sql 하위 열기(2단)
        m.on_event(&key(Key::Enter)); // INSERT
        assert_eq!(m.take_picked().as_deref(), Some("ins"));
        assert!(!m.is_open());
    }

    #[test]
    fn hover_moves_submenu_to_other_parent_row() {
        let mut m = ContextMenu::new();
        m.open_at(10, 10, nested(), host(), 100);
        let adv = m.row_rect(1).unwrap();
        m.on_event(&InputEvent::MouseMove {
            x: adv.x + 5,
            y: adv.y + 2,
        });
        assert!(m.child_for_test().is_some(), "hover로 열린다");
        let copy = m.row_rect(0).unwrap();
        m.on_event(&InputEvent::MouseMove {
            x: copy.x + 5,
            y: copy.y + 2,
        });
        assert!(
            m.child_for_test().is_none(),
            "다른 부모 행 hover = 하위 메뉴 닫힘"
        );
        // 바깥 클릭은 전부 닫는다.
        m.on_event(&InputEvent::MouseMove {
            x: adv.x + 5,
            y: adv.y + 2,
        });
        assert!(m.on_event(&down(390, 290)));
        assert!(!m.is_open());
    }

    #[test]
    fn icon_column_is_reserved_for_all_rows_when_any_has_icon() {
        let mut plain = ContextMenu::new();
        plain.open_at(10, 10, items(), host(), 160);
        let w0 = plain.bounds().w;
        let mut with_icon = ContextMenu::new();
        let mut it = items();
        it[0] =
            CtxItem::item("copy", "복사").with_icon(Some(MenuIcon::from_alpha(2, 2, &[255; 4])));
        with_icon.open_at(10, 10, it, host(), 160);
        assert!(
            with_icon.bounds().w > w0,
            "아이콘 칸만큼 넓어진다(전 행 공통)"
        );
        assert!(with_icon.icon_col() > 0);
    }
}

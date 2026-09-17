//! **툴바 그룹 도크**(09-17 nexa-sql 사용자 요청) — 목적별 그룹(아이콘·구분자 계층) · 그룹은 **도크 안에서 드래그로 순서 이동** ·
//! **떼어 내면(세로로 끌면) 플로팅** · 배치는 [`DockLayout`] 문자열 하나로 저장/복원 · 초기화.
//!
//! 구조(재사용 규약):
//! - 모델 = [`ToolGroup`]{id · 제목 · [`ToolItem`] 목록(구분자 포함) · 오른쪽 정렬}.
//! - 그룹마다 [`Toolbar`] 하나를 **도크가 소유**한다. 플로팅이어도 도크가 갖고 있고, 호스트는 [`ToolDock::bar_mut`]로
//!   빌려 자기 창(플로팅 창)에 `set_bounds` → `on_event`/`paint` 한다 — 상태(활성·색조·표시)는 한 곳(도크)에만.
//! - 도크는 창을 만들지 않는다(창은 호스트 몫). 떼어 내기는 [`DockAction::Float`]로 **위치만 보고**한다.
//! - 배치 = [`DockLayout`](도크 순서 + 플로팅 좌표) ↔ 문자열(`file,run,conn@120:340,view`) — 설정 키 하나에 저장.
//!
//! 편집(아이콘·구분자의 표시/순서 편집)은 후속(nexa-sql T-105) — 모델은 이미 계층이라 표만 바꾸면 된다.

use super::toolbar::{ToolIcon, ToolItem, ToolTone, Toolbar};
use super::{Control, ControlBase};
use crate::draw::DrawCtx;
use crate::event::InputEvent;
use crate::geom::{Point, Rect};
use crate::theme::Theme;
use crate::widget::{Invalidations, Widget};

/// 그룹 손잡이(그립) 폭(논리 px).
const GRIP_W: i32 = 10;
/// 그룹 사이 간격(논리 px).
const GROUP_GAP: i32 = 4;
/// 세로로 이만큼 끌면 떼어 낸다(논리 px).
const TEAR_PX: i32 = 22;

/// 목적별 그룹 — 아이콘·구분자의 순서 있는 목록.
#[derive(Clone, Debug)]
pub struct ToolGroup {
    /// 그룹 id(배치 저장 키 · 안정적이어야 한다).
    pub id: String,
    /// 제목(플로팅 창 제목 · 메뉴).
    pub title: String,
    /// 항목(아이콘 · [`ToolItem::separator`]).
    pub items: Vec<ToolItem>,
    /// 도크 오른쪽 끝부터 배치.
    pub right: bool,
}

impl ToolGroup {
    pub fn new(id: impl Into<String>, title: impl Into<String>, items: Vec<ToolItem>) -> Self {
        ToolGroup {
            id: id.into(),
            title: title.into(),
            items,
            right: false,
        }
    }

    /// 오른쪽 정렬(체이닝).
    #[must_use]
    pub fn align_right(mut self) -> Self {
        self.right = true;
        self
    }
}

/// 배치 — 도크에 붙은 그룹의 순서 + 플로팅 그룹의 창 좌표(물리 px · 화면 기준).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DockLayout {
    /// 도크 순서(그룹 id · 오른쪽 정렬 그룹도 함께 · 없는 id는 무시 · 빠진 그룹은 기본 자리).
    pub order: Vec<String>,
    /// 플로팅(그룹 id, x, y).
    pub floating: Vec<(String, i32, i32)>,
}

impl DockLayout {
    /// `a,b,c@120:340,d` — `@x:y`가 붙은 그룹은 플로팅(순서는 돌아올 자리).
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut l = DockLayout::default();
        for tok in text.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            let (id, pos) = match tok.split_once('@') {
                Some((id, pos)) => (id.trim(), Some(pos)),
                None => (tok, None),
            };
            if id.is_empty() {
                continue;
            }
            l.order.push(id.to_string());
            if let Some(pos) = pos {
                if let Some((x, y)) = pos.split_once(':') {
                    if let (Ok(x), Ok(y)) = (x.trim().parse(), y.trim().parse()) {
                        l.floating.push((id.to_string(), x, y));
                    }
                }
            }
        }
        l
    }

    /// [`Self::parse`]의 역.
    #[must_use]
    pub fn serialize(&self) -> String {
        self.order
            .iter()
            .map(|id| match self.floating.iter().find(|(f, _, _)| f == id) {
                Some((_, x, y)) => format!("{id}@{x}:{y}"),
                None => id.clone(),
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// 도크가 호스트에 알리는 일.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DockAction {
    /// 그룹을 떼어 냈다 — 호스트가 (x, y)(도크가 속한 창의 클라이언트 좌표 · 커서 위치)에 플로팅 창을 만든다.
    Float { id: String, x: i32, y: i32 },
    /// 배치가 바뀌었다(순서 이동 · 붙이기 · 떼기 · 플로팅 이동) — 저장할 때.
    LayoutChanged,
}

struct Drag {
    gi: usize,
    x0: i32,
    y0: i32,
    moved: bool,
}

/// 툴바 그룹 도크 컨트롤.
#[derive(Debug)]
pub struct ToolDock {
    base: ControlBase,
    groups: Vec<ToolGroup>,
    bars: Vec<Toolbar>,
    /// 도크 표시 순서(그룹 index) — 플로팅 그룹도 자리(돌아올 곳)를 유지한다.
    order: Vec<usize>,
    /// 플로팅 그룹(index, x, y).
    floating: Vec<(usize, i32, i32)>,
    /// 그룹별 그립 사각형(마지막 배치).
    grips: Vec<Rect>,
    hover_grip: Option<usize>,
    drag: Option<Drag>,
    actions: Vec<DockAction>,
    icon_px: i32,
}

impl std::fmt::Debug for Drag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Drag(g{} moved={})", self.gi, self.moved)
    }
}

impl ToolDock {
    /// 그룹 정의로 만든다(아이콘 크기는 [`Self::set_icon_size`]).
    #[must_use]
    pub fn new(groups: Vec<ToolGroup>) -> Self {
        let bars: Vec<Toolbar> = groups
            .iter()
            .map(|g| {
                let mut tb = Toolbar::new(g.items.clone());
                tb.set_icon_size(super::toolbar::DEFAULT_ICON);
                tb
            })
            .collect();
        let n = groups.len();
        ToolDock {
            base: ControlBase::default(),
            groups,
            bars,
            order: (0..n).collect(),
            floating: Vec::new(),
            grips: vec![Rect::new(0, 0, 0, 0); n],
            hover_grip: None,
            drag: None,
            actions: Vec::new(),
            icon_px: super::toolbar::DEFAULT_ICON,
        }
    }

    /// 아이콘 크기(논리 px) — 모든 그룹에.
    pub fn set_icon_size(&mut self, px: i32) {
        self.icon_px = px;
        for b in &mut self.bars {
            b.set_icon_size(px);
        }
    }

    /// 권장 높이(논리 px) — 그룹 툴바와 같다.
    #[must_use]
    pub fn preferred_height(&self) -> i32 {
        self.bars
            .first()
            .map_or(self.icon_px + 8, Toolbar::preferred_height)
    }

    fn index(&self, id: &str) -> Option<usize> {
        self.groups.iter().position(|g| g.id == id)
    }

    /// 그룹 툴바(도크·플로팅 무관 · 항목 상태의 단일 원천).
    pub fn bar_mut(&mut self, id: &str) -> Option<&mut Toolbar> {
        let i = self.index(id)?;
        self.bars.get_mut(i)
    }

    /// 그룹 툴바(읽기).
    #[must_use]
    pub fn bar(&self, id: &str) -> Option<&Toolbar> {
        let i = self.index(id)?;
        self.bars.get(i)
    }

    /// 그룹 (id, 제목, 플로팅 여부) — 메뉴 구성용(도크 순서).
    #[must_use]
    pub fn groups(&self) -> Vec<(String, String, bool)> {
        self.order
            .iter()
            .map(|&i| {
                (
                    self.groups[i].id.clone(),
                    self.groups[i].title.clone(),
                    self.is_floating_idx(i),
                )
            })
            .collect()
    }

    /// 그룹 제목.
    #[must_use]
    pub fn title(&self, id: &str) -> Option<&str> {
        self.index(id).map(|i| self.groups[i].title.as_str())
    }

    fn is_floating_idx(&self, i: usize) -> bool {
        self.floating.iter().any(|(f, _, _)| *f == i)
    }

    #[must_use]
    pub fn is_floating(&self, id: &str) -> bool {
        self.index(id).is_some_and(|i| self.is_floating_idx(i))
    }

    /// 플로팅 그룹 좌표.
    #[must_use]
    pub fn floating_pos(&self, id: &str) -> Option<(i32, i32)> {
        let i = self.index(id)?;
        self.floating
            .iter()
            .find(|(f, _, _)| *f == i)
            .map(|(_, x, y)| (*x, *y))
    }

    /// 플로팅으로(좌표 = 화면 물리 px). 이미 플로팅이면 좌표만 갱신.
    pub fn float(&mut self, id: &str, x: i32, y: i32) {
        let Some(i) = self.index(id) else { return };
        if let Some(f) = self.floating.iter_mut().find(|(f, _, _)| *f == i) {
            f.1 = x;
            f.2 = y;
        } else {
            self.floating.push((i, x, y));
            let mut inv = Invalidations::default();
            self.bars[i].clear_hover(&mut inv);
        }
        self.actions.push(DockAction::LayoutChanged);
    }

    /// 플로팅 좌표만 갱신(창 이동) — 배치 변경 보고.
    pub fn set_floating_pos(&mut self, id: &str, x: i32, y: i32) {
        if let Some(i) = self.index(id) {
            if let Some(f) = self.floating.iter_mut().find(|(f, _, _)| *f == i) {
                if f.1 != x || f.2 != y {
                    f.1 = x;
                    f.2 = y;
                    self.actions.push(DockAction::LayoutChanged);
                }
            }
        }
    }

    /// 도크로 되돌리기(자리는 `order`에 남아 있던 곳).
    pub fn dock(&mut self, id: &str) {
        let Some(i) = self.index(id) else { return };
        let before = self.floating.len();
        self.floating.retain(|(f, _, _)| *f != i);
        if self.floating.len() != before {
            let mut inv = Invalidations::default();
            self.bars[i].clear_hover(&mut inv);
            self.actions.push(DockAction::LayoutChanged);
        }
    }

    /// 현재 배치.
    #[must_use]
    pub fn layout(&self) -> DockLayout {
        DockLayout {
            order: self
                .order
                .iter()
                .map(|&i| self.groups[i].id.clone())
                .collect(),
            floating: self
                .floating
                .iter()
                .map(|&(i, x, y)| (self.groups[i].id.clone(), x, y))
                .collect(),
        }
    }

    /// 배치 적용 — 모르는 id는 무시 · 빠진 그룹은 기본 순서로 뒤에 붙는다(그룹이 늘어도 저장값이 깨지지 않게).
    pub fn apply_layout(&mut self, l: &DockLayout) {
        let mut order: Vec<usize> = Vec::new();
        for id in &l.order {
            if let Some(i) = self.index(id) {
                if !order.contains(&i) {
                    order.push(i);
                }
            }
        }
        for i in 0..self.groups.len() {
            if !order.contains(&i) {
                order.push(i);
            }
        }
        self.order = order;
        self.floating = l
            .floating
            .iter()
            .filter_map(|(id, x, y)| self.index(id).map(|i| (i, *x, *y)))
            .collect();
    }

    /// 기본 배치로(전부 도크 · 정의 순서).
    pub fn reset(&mut self) {
        self.order = (0..self.groups.len()).collect();
        self.floating.clear();
        self.drag = None;
        self.actions.push(DockAction::LayoutChanged);
    }

    /// 호스트가 처리할 일(1회성).
    pub fn take_actions(&mut self) -> Vec<DockAction> {
        std::mem::take(&mut self.actions)
    }

    /// 도크에 붙은 그룹에서 클릭된 액션 id(1회성 · 플로팅 창의 클릭은 호스트가 `bar_mut`로 직접 가져간다).
    pub fn take_clicked(&mut self) -> Option<String> {
        for &i in &self.order {
            if self.is_floating_idx(i) {
                continue;
            }
            if let Some(id) = self.bars[i].take_clicked() {
                return Some(id);
            }
        }
        None
    }

    // ── 항목 상태(모든 그룹에서 id로 찾는다 · 도크/플로팅 무관)

    pub fn set_item_enabled(&mut self, id: &str, enabled: bool, inv: &mut Invalidations) {
        for b in &mut self.bars {
            b.set_item_enabled(id, enabled, inv);
        }
    }

    pub fn set_item_tone(&mut self, id: &str, tone: ToolTone, inv: &mut Invalidations) {
        for b in &mut self.bars {
            b.set_item_tone(id, tone, inv);
        }
    }

    pub fn set_item_visible(&mut self, id: &str, visible: bool, inv: &mut Invalidations) {
        for b in &mut self.bars {
            b.set_item_visible(id, visible, inv);
        }
    }

    pub fn set_item_badge(&mut self, id: &str, badge: Option<&str>, inv: &mut Invalidations) {
        for b in &mut self.bars {
            b.set_item_badge(id, badge, inv);
        }
    }

    pub fn set_item_tip(&mut self, id: &str, tip: &str) {
        for b in &mut self.bars {
            b.set_item_tip(id, tip);
        }
    }

    pub fn set_item_icon(&mut self, id: &str, icon: ToolIcon, inv: &mut Invalidations) {
        for b in &mut self.bars {
            b.set_item_icon(id, icon.clone(), inv);
        }
    }

    #[must_use]
    pub fn item_enabled(&self, id: &str) -> bool {
        self.bars.iter().any(|b| b.item_enabled(id))
    }

    /// 어느 그룹에 속한 항목인가.
    #[must_use]
    pub fn group_of(&self, item_id: &str) -> Option<&str> {
        self.bars
            .iter()
            .zip(self.groups.iter())
            .find(|(b, _)| b.items().iter().any(|it| it.id == item_id))
            .map(|(_, g)| g.id.as_str())
    }

    /// 도크 안 그룹 배치 — 왼쪽 정렬은 왼쪽부터, 오른쪽 정렬은 오른쪽 끝부터. 플로팅 그룹은 자리를 차지하지 않는다.
    fn relayout(&mut self) {
        let b = self.base.bounds;
        let grip = self.s(GRIP_W);
        let gap = self.s(GROUP_GAP);
        let mut inv = Invalidations::default();
        let mut x = b.x + self.s(2);
        let mut rx = b.right() - self.s(2);
        for &i in &self.order.clone() {
            if self.is_floating_idx(i) {
                self.grips[i] = Rect::new(0, 0, 0, 0);
                continue;
            }
            self.bars[i].set_scale(self.base.scale);
            let w = self.bars[i].preferred_width();
            if self.groups[i].right {
                let bx = rx - w;
                self.bars[i].set_bounds(Rect::new(bx, b.y, w, b.h), &mut inv);
                self.grips[i] = Rect::new(bx - grip, b.y, grip, b.h);
                rx = bx - grip - gap;
            } else {
                self.grips[i] = Rect::new(x, b.y, grip, b.h);
                self.bars[i].set_bounds(Rect::new(x + grip, b.y, w, b.h), &mut inv);
                x += grip + w + gap;
            }
        }
    }

    fn grip_at(&self, p: Point) -> Option<usize> {
        self.order
            .iter()
            .copied()
            .find(|&i| !self.is_floating_idx(i) && self.grips[i].contains(p))
    }

    /// 드래그 중 순서 이동 — 커서가 이웃 그룹의 중앙을 넘으면 자리를 바꾼다(같은 정렬끼리).
    fn reorder_to(&mut self, gi: usize, x: i32) -> bool {
        let right = self.groups[gi].right;
        let pos = self.order.iter().position(|&i| i == gi);
        let Some(pos) = pos else { return false };
        let docked: Vec<usize> = self
            .order
            .iter()
            .copied()
            .filter(|&i| !self.is_floating_idx(i) && self.groups[i].right == right)
            .collect();
        let k = docked.iter().position(|&i| i == gi);
        let Some(k) = k else { return false };
        let mut target: Option<usize> = None;
        if k > 0 {
            let prev = docked[k - 1];
            let r = self.bars[prev].bounds();
            if x < r.x + r.w / 2 {
                target = Some(prev);
            }
        }
        if target.is_none() && k + 1 < docked.len() {
            let next = docked[k + 1];
            let r = self.bars[next].bounds();
            if x > r.x + r.w / 2 {
                target = Some(next);
            }
        }
        let Some(t) = target else { return false };
        let tpos = self.order.iter().position(|&i| i == t);
        let Some(tpos) = tpos else { return false };
        self.order.swap(pos, tpos);
        self.relayout();
        true
    }

    /// 툴팁(팝업 층).
    pub fn paint_tooltip(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        for &i in &self.order {
            if !self.is_floating_idx(i) {
                self.bars[i].paint_tooltip(ctx, theme);
            }
        }
    }
}

impl Control for ToolDock {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
}

impl Widget for ToolDock {
    fn bounds(&self) -> Rect {
        self.base.bounds
    }

    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        self.base.bounds = bounds;
        self.relayout();
        inv.push(bounds);
    }

    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        let b = self.base.bounds;
        match *ev {
            InputEvent::MouseDown { x, y, .. } => {
                let p = Point { x, y };
                if let Some(gi) = self.grip_at(p) {
                    self.drag = Some(Drag {
                        gi,
                        x0: x,
                        y0: y,
                        moved: false,
                    });
                    inv.push(b);
                    return;
                }
                // 마우스 라우팅 규칙: 커서 아래 그룹에만.
                for &i in &self.order.clone() {
                    if !self.is_floating_idx(i) && self.bars[i].bounds().contains(p) {
                        self.bars[i].on_event(ev, inv);
                    }
                }
            }
            InputEvent::MouseUp { x, y } => {
                if let Some(d) = self.drag.take() {
                    if d.moved {
                        self.actions.push(DockAction::LayoutChanged);
                    }
                    inv.push(b);
                    return;
                }
                let p = Point { x, y };
                for &i in &self.order.clone() {
                    if !self.is_floating_idx(i) {
                        // Up은 눌린 툴바가 스스로 판정(눌림 해제 포함)하도록 전부에.
                        let _ = p;
                        self.bars[i].on_event(ev, inv);
                    }
                }
            }
            InputEvent::MouseMove { x, y } => {
                if let Some((gi, x0, y0)) = self.drag.as_ref().map(|d| (d.gi, d.x0, d.y0)) {
                    let (tear, wobble) = (self.s(TEAR_PX), self.s(3));
                    if (y - y0).abs() >= tear {
                        // 세로로 끌어 냈다 — 떼어 내기(호스트가 창을 만든다).
                        self.drag = None;
                        let id = self.groups[gi].id.clone();
                        self.actions.push(DockAction::Float { id, x, y });
                        inv.push(b);
                        return;
                    }
                    if (x - x0).abs() >= wobble {
                        if let Some(d) = self.drag.as_mut() {
                            d.moved = true;
                        }
                    }
                    if self.reorder_to(gi, x) {
                        inv.push(b);
                    }
                    return;
                }
                let over = self.grip_at(Point { x, y });
                if over != self.hover_grip {
                    self.hover_grip = over;
                    inv.push(b);
                }
                for &i in &self.order.clone() {
                    if !self.is_floating_idx(i) {
                        self.bars[i].on_event(ev, inv);
                    }
                }
            }
            _ => {}
        }
    }

    fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.base.bounds;
        ctx.fill_rect(b, theme.chrome_bg);
        for &i in &self.order {
            if self.is_floating_idx(i) {
                continue;
            }
            self.bars[i].paint(ctx, theme);
            // 그립 = 점 2열(DBeaver 손잡이) · hover/드래그 = accent.
            let g = self.grips[i];
            let active =
                self.hover_grip == Some(i) || self.drag.as_ref().is_some_and(|d| d.gi == i);
            let c = if active { theme.accent } else { theme.text_dim };
            let dot = self.s(2).max(1);
            let rows = 4;
            let span = rows * dot * 2 - dot;
            let y0 = g.y + (g.h - span) / 2;
            let x0 = g.x + (g.w - (dot * 3)) / 2;
            for r in 0..rows {
                for col in 0..2 {
                    ctx.fill_rect(Rect::new(x0 + col * dot * 2, y0 + r * dot * 2, dot, dot), c);
                }
            }
        }
        ctx.fill_rect(Rect::new(b.x, b.bottom() - 1, b.w, 1), theme.border);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> ToolItem {
        ToolItem::new(id, ToolIcon::Glyph("x".into()))
    }

    fn dock() -> ToolDock {
        let mut d = ToolDock::new(vec![
            ToolGroup::new("file", "File", vec![item("f.new"), item("f.open")]),
            ToolGroup::new(
                "run",
                "Run",
                vec![item("r.stmt"), ToolItem::separator(), item("r.commit")],
            ),
            ToolGroup::new("view", "View", vec![item("v.log")]).align_right(),
        ]);
        d.set_icon_size(16);
        let mut inv = Invalidations::default();
        d.set_bounds(Rect::new(0, 0, 800, 28), &mut inv);
        d
    }

    #[test]
    fn layout_roundtrip_and_unknown_ids() {
        let l = DockLayout::parse("run@120:340, file ,zzz,view");
        assert_eq!(l.order, vec!["run", "file", "zzz", "view"]);
        assert_eq!(l.floating, vec![("run".to_string(), 120, 340)]);
        assert_eq!(l.serialize(), "run@120:340,file,zzz,view");
        let mut d = dock();
        d.apply_layout(&l);
        let got = d.layout();
        assert_eq!(got.order, vec!["run", "file", "view"], "모르는 id는 버린다");
        assert!(d.is_floating("run"));
        assert_eq!(d.floating_pos("run"), Some((120, 340)));
        // 플로팅 그룹은 도크 자리를 차지하지 않고, 되돌리면 자기 자리(맨 앞)로.
        d.set_bounds(Rect::new(0, 0, 800, 28), &mut Invalidations::default());
        assert_eq!(d.grips[1].w, 0);
        d.dock("run");
        d.set_bounds(Rect::new(0, 0, 800, 28), &mut Invalidations::default());
        assert!(d.grips[1].x < d.grips[0].x, "run이 file 앞");
        assert!(d.take_actions().contains(&DockAction::LayoutChanged));
    }

    #[test]
    fn grip_drag_reorders_and_vertical_drag_tears_off() {
        let mut d = dock();
        let mut inv = Invalidations::default();
        let g_file = d.grips[0];
        let file_x = d.bars[0].bounds().x;
        let run_b = d.bars[1].bounds();
        // file 그립을 잡고 run 중앙 너머로 → 순서 바뀜.
        d.on_event(
            &InputEvent::MouseDown {
                x: g_file.x + 2,
                y: g_file.y + 5,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        d.on_event(
            &InputEvent::MouseMove {
                x: run_b.x + run_b.w / 2 + 4,
                y: g_file.y + 5,
            },
            &mut inv,
        );
        d.on_event(
            &InputEvent::MouseUp {
                x: run_b.x + run_b.w / 2 + 4,
                y: g_file.y + 5,
            },
            &mut inv,
        );
        assert_eq!(d.layout().order, vec!["run", "file", "view"]);
        assert!(d.bars[0].bounds().x > file_x);
        assert_eq!(d.take_actions(), vec![DockAction::LayoutChanged]);
        // 세로로 끌면 떼어 내기 보고 · 그룹은 아직 도크(호스트가 float를 부른다).
        let g_run = d.grips[1];
        d.on_event(
            &InputEvent::MouseDown {
                x: g_run.x + 2,
                y: g_run.y + 5,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        d.on_event(
            &InputEvent::MouseMove {
                x: g_run.x + 2,
                y: g_run.y + 60,
            },
            &mut inv,
        );
        assert_eq!(
            d.take_actions(),
            vec![DockAction::Float {
                id: "run".into(),
                x: g_run.x + 2,
                y: g_run.y + 60
            }]
        );
        assert!(!d.is_floating("run"));
        d.float("run", 10, 20);
        assert!(d.is_floating("run"));
        d.reset();
        assert!(!d.is_floating("run"));
        assert_eq!(d.layout().order, vec!["file", "run", "view"]);
    }

    #[test]
    fn clicks_route_to_group_under_cursor_and_state_is_shared() {
        let mut d = dock();
        let mut inv = Invalidations::default();
        d.set_item_enabled("r.commit", false, &mut inv);
        assert!(!d.item_enabled("r.commit"));
        assert_eq!(d.group_of("r.commit"), Some("run"));
        let r = d.bars[1].bounds();
        // run 그룹 첫 슬롯 클릭.
        let (x, y) = (r.x + 12, r.y + r.h / 2);
        d.on_event(
            &InputEvent::MouseDown {
                x,
                y,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        d.on_event(&InputEvent::MouseUp { x, y }, &mut inv);
        assert_eq!(d.take_clicked(), Some("r.stmt".into()));
        // 구분자는 클릭되지 않는다(폭만 차지).
        assert!(d.bars[1].preferred_width() > d.bars[0].preferred_width());
    }
}

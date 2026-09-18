//! **툴바 그룹 도크**(09-17 nexa-sql 사용자 요청) — 목적별 그룹(아이콘·구분자 계층) · 그룹은 **도크 안에서 드래그로 순서 이동**
//! (09-19: 이동 대상이 **고스트로 커서를 따라 보이고** · 도크는 **여러 행** · 어느 방향으로든 · 자리 표시 = 강조 틀 · **Esc = 취소**) ·
//! **떼어 내면(도크 밖으로 멀리 끌면) 플로팅** · 배치는 [`DockLayout`] 문자열 하나로 저장/복원 · 초기화.
//!
//! 구조(재사용 규약):
//! - 모델 = [`ToolGroup`]{id · 제목 · [`ToolItem`] 목록(구분자 포함) · 오른쪽 정렬}.
//! - 그룹마다 [`Toolbar`] 하나를 **도크가 소유**한다. 플로팅이어도 도크가 갖고 있고, 호스트는 [`ToolDock::bar_mut`]로
//!   빌려 자기 창(플로팅 창)에 `set_bounds` → `on_event`/`paint` 한다 — 상태(활성·색조·표시)는 한 곳(도크)에만.
//! - 도크는 창을 만들지 않는다(창은 호스트 몫). 떼어 내기는 [`DockAction::Float`]로 **위치만 보고**한다.
//! - 배치 = [`DockLayout`](도크 순서 + 행 + 플로팅 좌표) ↔ 문자열(`file,run;conn@120:340,view` · `;` = 다음 행) — 설정 키 하나에 저장.
//! - 드래그 중 고스트는 도크 밖(편집기 위)까지 나가므로 호스트가 **팝업 층**에서 [`ToolDock::paint_drag_overlay`]를 부른다 ·
//!   행 수가 바뀌면 [`DockAction::Resized`] → 호스트가 창을 다시 배치한다([`ToolDock::preferred_height`]).
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
/// 도크(+ 새 행 영역) 밖으로 이만큼 더 끌면 떼어 낸다(논리 px).
const TEAR_PX: i32 = 22;
/// 이만큼 움직여야 드래그가 시작된다(논리 px) — 그 전 MouseUp = 클릭.
const DRAG_START_PX: i32 = 4;

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

/// 배치 — 도크에 붙은 그룹의 순서(+ 행) + 플로팅 그룹의 창 좌표(물리 px · 화면 기준).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DockLayout {
    /// 도크 순서(그룹 id · 오른쪽 정렬 그룹도 함께 · 없는 id는 무시 · 빠진 그룹은 기본 자리).
    pub order: Vec<String>,
    /// `order`와 나란한 행 번호(0부터 · 09-19 다중 행). 비어 있으면 전부 0행.
    pub rows: Vec<usize>,
    /// 플로팅(그룹 id, x, y).
    pub floating: Vec<(String, i32, i32)>,
}

impl DockLayout {
    /// `a,b;c@120:340,d` — `,` = 같은 행의 다음 · `;` = 다음 행 · `@x:y`가 붙은 그룹은 플로팅(순서는 돌아올 자리).
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut l = DockLayout::default();
        for (row, line) in text.split(';').enumerate() {
            for tok in line.split(',').map(str::trim).filter(|t| !t.is_empty()) {
                let (id, pos) = match tok.split_once('@') {
                    Some((id, pos)) => (id.trim(), Some(pos)),
                    None => (tok, None),
                };
                if id.is_empty() {
                    continue;
                }
                l.order.push(id.to_string());
                l.rows.push(row);
                if let Some(pos) = pos {
                    if let Some((x, y)) = pos.split_once(':') {
                        if let (Ok(x), Ok(y)) = (x.trim().parse(), y.trim().parse()) {
                            l.floating.push((id.to_string(), x, y));
                        }
                    }
                }
            }
        }
        l.compact_rows();
        l
    }

    /// 빈 행을 없애 0부터 이어지게.
    fn compact_rows(&mut self) {
        let mut used: Vec<usize> = self.rows.clone();
        used.sort_unstable();
        used.dedup();
        for r in &mut self.rows {
            *r = used.iter().position(|u| u == r).unwrap_or(0);
        }
    }

    fn row_at(&self, i: usize) -> usize {
        self.rows.get(i).copied().unwrap_or(0)
    }

    /// [`Self::parse`]의 역.
    #[must_use]
    pub fn serialize(&self) -> String {
        let nrows = self.rows.iter().copied().max().map_or(1, |m| m + 1);
        (0..nrows)
            .map(|r| {
                self.order
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| self.row_at(*i) == r)
                    .map(
                        |(_, id)| match self.floating.iter().find(|(f, _, _)| f == id) {
                            Some((_, x, y)) => format!("{id}@{x}:{y}"),
                            None => id.clone(),
                        },
                    )
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect::<Vec<_>>()
            .join(";")
    }
}

/// 도크가 호스트에 알리는 일.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DockAction {
    /// 그룹을 떼어 냈다 — 호스트가 (x, y)(도크가 속한 창의 클라이언트 좌표 · 커서 위치)에 플로팅 창을 만든다.
    Float { id: String, x: i32, y: i32 },
    /// 배치가 바뀌었다(순서 이동 · 붙이기 · 떼기 · 플로팅 이동) — 저장할 때.
    LayoutChanged,
    /// 행 수가 바뀌어 권장 높이가 달라졌다 — 호스트가 창을 다시 배치한다(드래그 중에도).
    Resized,
}

struct Drag {
    gi: usize,
    x0: i32,
    y0: i32,
    /// 잡은 점의 그룹 툴바 왼쪽 위 기준 오프셋(고스트가 손 아래 그대로 붙어 있게).
    off_x: i32,
    off_y: i32,
    /// 지금 커서.
    cur: Point,
    /// 임계 이상 움직여 드래그가 시작됐다.
    moved: bool,
    /// 시작 때 순서·행(Esc 복원).
    orig_order: Vec<usize>,
    orig_rows: Vec<usize>,
}

/// 툴바 그룹 도크 컨트롤.
#[derive(Debug)]
pub struct ToolDock {
    base: ControlBase,
    groups: Vec<ToolGroup>,
    bars: Vec<Toolbar>,
    /// 도크 표시 순서(그룹 index) — 플로팅 그룹도 자리(돌아올 곳)를 유지한다.
    order: Vec<usize>,
    /// 그룹별 행(0부터 · 빈 행 없음).
    row_of: Vec<usize>,
    /// 마지막 배치의 행 높이(물리 px).
    row_h: i32,
    /// 플로팅 그룹(index, x, y).
    floating: Vec<(usize, i32, i32)>,
    /// 그룹별 그립 사각형(마지막 배치).
    grips: Vec<Rect>,
    hover_grip: Option<usize>,
    drag: Option<Drag>,
    actions: Vec<DockAction>,
    icon_px: i32,
}

impl Drag {
    fn order_changed(&self, order: &[usize], rows: &[usize]) -> bool {
        self.orig_order != order || self.orig_rows != rows
    }
}

impl std::fmt::Debug for Drag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Drag(g{} moved={} at {:?})",
            self.gi, self.moved, self.cur
        )
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
            row_of: vec![0; n],
            row_h: 0,
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

    /// 권장 높이(논리 px) — 그룹 툴바 높이 × 행 수.
    #[must_use]
    pub fn preferred_height(&self) -> i32 {
        self.bar_height() * self.rows() as i32
    }

    fn bar_height(&self) -> i32 {
        self.bars
            .first()
            .map_or(self.icon_px + 8, Toolbar::preferred_height)
    }

    /// 도크에 붙은 그룹이 차지하는 행 수(최소 1).
    #[must_use]
    pub fn rows(&self) -> usize {
        self.order
            .iter()
            .filter(|&&i| !self.is_floating_idx(i))
            .map(|&i| self.row_of[i] + 1)
            .max()
            .unwrap_or(1)
            .max(1)
    }

    /// 드래그 중인가(호스트가 Esc를 넘길지 판단).
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.drag.as_ref().is_some_and(|d| d.moved)
    }

    /// Esc — 드래그를 취소하고 시작 때의 순서·행으로 되돌린다. 드래그 중이었으면 true(호스트가 키를 소비).
    pub fn cancel_drag(&mut self, inv: &mut Invalidations) -> bool {
        let Some(d) = self.drag.take() else {
            return false;
        };
        let rows_before = self.rows();
        self.order = d.orig_order;
        self.row_of = d.orig_rows;
        self.relayout();
        if self.rows() != rows_before {
            self.actions.push(DockAction::Resized);
        }
        inv.push(self.base.bounds);
        inv.push(self.ghost_rect_of(&d.cur, d.off_x, d.off_y, d.gi));
        d.moved
    }

    /// 빈 행을 없앤다(도크에 붙은 그룹 기준 · 플로팅은 자기 행을 따라간다).
    fn compact_rows(&mut self) {
        let mut used: Vec<usize> = self
            .order
            .iter()
            .filter(|&&i| !self.is_floating_idx(i))
            .map(|&i| self.row_of[i])
            .collect();
        used.sort_unstable();
        used.dedup();
        for r in &mut self.row_of {
            *r = used.iter().position(|u| u == r).unwrap_or(0);
        }
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
            rows: self.order.iter().map(|&i| self.row_of[i]).collect(),
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
        let mut row_of = vec![0usize; self.groups.len()];
        for (k, id) in l.order.iter().enumerate() {
            if let Some(i) = self.index(id) {
                if !order.contains(&i) {
                    order.push(i);
                    row_of[i] = l.row_at(k);
                }
            }
        }
        for i in 0..self.groups.len() {
            if !order.contains(&i) {
                order.push(i);
            }
        }
        self.order = order;
        self.row_of = row_of;
        self.floating = l
            .floating
            .iter()
            .filter_map(|(id, x, y)| self.index(id).map(|i| (i, *x, *y)))
            .collect();
    }

    /// 기본 배치로(전부 도크 · 정의 순서).
    pub fn reset(&mut self) {
        self.order = (0..self.groups.len()).collect();
        self.row_of = vec![0; self.groups.len()];
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

    /// 항목의 자리(도크에 붙어 있는 그룹만 · 플로팅 그룹은 다른 창이라 None).
    #[must_use]
    pub fn item_rect(&self, id: &str) -> Option<Rect> {
        let gid = self.group_of(id)?.to_string();
        if self.is_floating(&gid) {
            return None;
        }
        self.bar(&gid)?.item_rect(id)
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

    /// 도크 안 그룹 배치 — 행마다 왼쪽 정렬은 왼쪽부터, 오른쪽 정렬은 오른쪽 끝부터. 플로팅 그룹은 자리를 차지하지 않는다.
    /// 행 높이 = 도크 높이 ÷ 행 수(호스트가 아직 높이를 못 맞췄어도 겹치지 않게).
    fn relayout(&mut self) {
        let b = self.base.bounds;
        let grip = self.s(GRIP_W);
        let gap = self.s(GROUP_GAP);
        let mut inv = Invalidations::default();
        let nrows = self.rows();
        let row_h = (b.h / nrows as i32).max(1);
        self.row_h = row_h;
        for r in 0..nrows {
            let ry = b.y + r as i32 * row_h;
            let mut x = b.x + self.s(2);
            let mut rx = b.right() - self.s(2);
            for &i in &self.order.clone() {
                if self.is_floating_idx(i) {
                    self.grips[i] = Rect::new(0, 0, 0, 0);
                    continue;
                }
                if self.row_of[i] != r {
                    continue;
                }
                self.bars[i].set_scale(self.base.scale);
                let w = self.bars[i].preferred_width();
                if self.groups[i].right {
                    let bx = rx - w;
                    self.bars[i].set_bounds(Rect::new(bx, ry, w, row_h), &mut inv);
                    self.grips[i] = Rect::new(bx - grip, ry, grip, row_h);
                    rx = bx - grip - gap;
                } else {
                    self.grips[i] = Rect::new(x, ry, grip, row_h);
                    self.bars[i].set_bounds(Rect::new(x + grip, ry, w, row_h), &mut inv);
                    x += grip + w + gap;
                }
            }
        }
    }

    /// 고스트 자리(커서 − 잡은 오프셋 · 그룹 툴바 크기).
    fn ghost_rect_of(&self, cur: &Point, off_x: i32, off_y: i32, gi: usize) -> Rect {
        let r = self.bars[gi].bounds();
        Rect::new(cur.x - off_x, cur.y - off_y, r.w, r.h)
    }

    /// 지금 드래그의 고스트 자리.
    fn ghost_rect(&self) -> Option<Rect> {
        let d = self.drag.as_ref().filter(|d| d.moved)?;
        Some(self.ghost_rect_of(&d.cur, d.off_x, d.off_y, d.gi))
    }

    /// 커서 위치의 목표 행 — 도크 안 = 그 행 · 도크 바로 아래 한 행 폭 = **새 행** · 그 밖은 None(떼어 내기 후보).
    fn target_row(&self, y: i32) -> Option<usize> {
        let b = self.base.bounds;
        let row_h = self.row_h.max(1);
        let tear = self.s(TEAR_PX);
        if y < b.y - tear || y >= b.bottom() + row_h + tear {
            return None;
        }
        if y < b.y {
            return Some(0);
        }
        let r = ((y - b.y) / row_h) as usize;
        Some(r.min(self.rows()))
    }

    fn grip_at(&self, p: Point) -> Option<usize> {
        self.order
            .iter()
            .copied()
            .find(|&i| !self.is_floating_idx(i) && self.grips[i].contains(p))
    }

    /// 드래그 중 자리 이동(라이브 미리보기) — 커서의 행(`target_row`) 안에서, 같은 정렬 띠의 다른 그룹들 중앙을 기준으로
    /// 삽입 위치를 정해 `order`·`row_of`를 바꾼다. 바뀌었으면 true(행 수가 바뀌면 `Resized` 보고).
    fn reorder_to(&mut self, gi: usize, p: Point) -> bool {
        let Some(row) = self.target_row(p.y) else {
            return false;
        };
        let right = self.groups[gi].right;
        // 그 행·같은 띠의 다른 그룹(순서대로)과 중앙 x.
        let peers: Vec<(usize, i32)> = self
            .order
            .iter()
            .copied()
            .filter(|&i| {
                i != gi
                    && !self.is_floating_idx(i)
                    && self.row_of[i] == row
                    && self.groups[i].right == right
            })
            .map(|i| {
                let r = self.bars[i].bounds();
                (i, r.x + r.w / 2)
            })
            .collect();
        // 왼쪽 띠는 x 오름차순 · 오른쪽 띠는 order 앞이 더 오른쪽이라 내림차순으로 센다.
        let k = peers
            .iter()
            .filter(|(_, mid)| if right { p.x < *mid } else { p.x > *mid })
            .count();
        let before = (self.order.clone(), self.row_of.clone());
        let rows_before = self.rows();
        let pos = self.order.iter().position(|&i| i == gi);
        let Some(pos) = pos else { return false };
        self.order.remove(pos);
        // 행 순서를 지키기 위해 order를 (행, 띠, 순서)로 다시 세운다: 같은 행 안에서 왼쪽 띠 → 오른쪽 띠.
        let at = if k < peers.len() {
            // k번째 동료 바로 앞.
            self.order
                .iter()
                .position(|&i| i == peers[k].0)
                .unwrap_or(self.order.len())
        } else {
            // 같은 행·같은 띠의 마지막 뒤 → 없으면 (왼쪽 띠면) 그 행 첫 그룹 앞 · (오른쪽 띠면) 그 행 마지막 뒤 →
            // 행이 비었으면 앞 행들의 끝(새 행이면 맨 끝).
            let same_band_last = self.order.iter().rposition(|&i| {
                !self.is_floating_idx(i) && self.row_of[i] == row && self.groups[i].right == right
            });
            match same_band_last {
                Some(q) => q + 1,
                None => {
                    let row_first = self
                        .order
                        .iter()
                        .position(|&i| !self.is_floating_idx(i) && self.row_of[i] == row);
                    let row_last = self
                        .order
                        .iter()
                        .rposition(|&i| !self.is_floating_idx(i) && self.row_of[i] == row);
                    match (row_first, row_last) {
                        (Some(f), Some(l)) => {
                            if right {
                                l + 1
                            } else {
                                f
                            }
                        }
                        _ => self
                            .order
                            .iter()
                            .rposition(|&i| !self.is_floating_idx(i) && self.row_of[i] < row)
                            .map_or(0, |q| q + 1),
                    }
                }
            }
        };
        self.order.insert(at, gi);
        self.row_of[gi] = row;
        self.compact_rows();
        if (self.order.clone(), self.row_of.clone()) == before {
            return false;
        }
        self.relayout();
        if self.rows() != rows_before {
            self.actions.push(DockAction::Resized);
        }
        true
    }

    /// 드래그 고스트(팝업 층 · 호스트가 툴팁과 함께 부른다) — 이동 대상 툴바를 커서 아래 그대로 그리고 강조 틀을 두른다.
    pub fn paint_drag_overlay(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let Some(d) = self.drag.as_ref().filter(|d| d.moved) else {
            return;
        };
        let Some(g) = self.ghost_rect() else { return };
        let r = self.bars[d.gi].bounds();
        self.bars[d.gi].paint_offset(ctx, theme, g.x - r.x, g.y - r.y);
        let c = theme.accent;
        ctx.fill_rect(Rect::new(g.x, g.y, g.w, 1), c);
        ctx.fill_rect(Rect::new(g.x, g.bottom() - 1, g.w, 1), c);
        ctx.fill_rect(Rect::new(g.x, g.y, 1, g.h), c);
        ctx.fill_rect(Rect::new(g.right() - 1, g.y, 1, g.h), c);
    }

    /// 툴팁(팝업 층).
    pub fn paint_tooltip(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        // 도크의 폭(= 창 폭) 안에 — 오른쪽 정렬 그룹의 툴팁도 창 안에서 왼쪽으로 밀린다.
        let clamp = (self.base.bounds.x, self.base.bounds.right());
        for &i in &self.order {
            if !self.is_floating_idx(i) {
                self.bars[i].paint_tooltip_in(ctx, theme, clamp);
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
                    let r = self.bars[gi].bounds();
                    self.drag = Some(Drag {
                        gi,
                        x0: x,
                        y0: y,
                        off_x: x - r.x,
                        off_y: y - r.y,
                        cur: p,
                        moved: false,
                        orig_order: self.order.clone(),
                        orig_rows: self.row_of.clone(),
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
                        if d.order_changed(&self.order, &self.row_of) {
                            self.actions.push(DockAction::LayoutChanged);
                        }
                        inv.push(self.ghost_rect_of(&d.cur, d.off_x, d.off_y, d.gi));
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
                    let p = Point { x, y };
                    let start = self.s(DRAG_START_PX);
                    if let Some(old) = self.ghost_rect() {
                        inv.push(old);
                    }
                    let started = if let Some(d) = self.drag.as_mut() {
                        d.cur = p;
                        let over = (x - x0).abs() >= start || (y - y0).abs() >= start;
                        let first = over && !d.moved;
                        d.moved |= over;
                        first
                    } else {
                        false
                    };
                    if started {
                        self.bars[gi].clear_hover(inv);
                    }
                    if !self.is_dragging() {
                        return;
                    }
                    if self.target_row(y).is_none() {
                        // 도크(+ 새 행 영역) 밖으로 멀리 끌어 냈다 — 떼어 내기(호스트가 창을 만든다). 순서는 시작 때로.
                        if let Some(d) = self.drag.take() {
                            let rows_before = self.rows();
                            self.order = d.orig_order;
                            self.row_of = d.orig_rows;
                            self.relayout();
                            if self.rows() != rows_before {
                                self.actions.push(DockAction::Resized);
                            }
                        }
                        let id = self.groups[gi].id.clone();
                        self.actions.push(DockAction::Float { id, x, y });
                        inv.push(b);
                        return;
                    }
                    if self.reorder_to(gi, p) {
                        inv.push(b);
                    }
                    if let Some(g) = self.ghost_rect() {
                        inv.push(g);
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
        let ghosted = self.drag.as_ref().filter(|d| d.moved).map(|d| d.gi);
        for &i in &self.order {
            if self.is_floating_idx(i) {
                continue;
            }
            if ghosted == Some(i) {
                // 이동 대상은 고스트로 커서를 따라간다 — 제자리엔 **자리 표시**(강조 틀)만.
                let r = self.bars[i].bounds();
                ctx.fill_rect_alpha(r, theme.accent, 0.12);
                let c = theme.accent;
                ctx.fill_rect(Rect::new(r.x, r.y, r.w, 1), c);
                ctx.fill_rect(Rect::new(r.x, r.bottom() - 1, r.w, 1), c);
                ctx.fill_rect(Rect::new(r.x, r.y, 1, r.h), c);
                ctx.fill_rect(Rect::new(r.right() - 1, r.y, 1, r.h), c);
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
        // 다중 행: `;` · 빈 행은 접힌다.
        let m = DockLayout::parse("file;;run,view");
        assert_eq!(m.rows, vec![0, 1, 1]);
        assert_eq!(m.serialize(), "file;run,view");
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
        // file 그립을 잡고 run 중앙 너머로 → 순서 바뀜(라이브 미리보기 · 고스트는 커서 아래).
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
        assert!(d.is_dragging());
        assert_eq!(d.layout().order, vec!["run", "file", "view"], "미리보기");
        let g = d.ghost_rect().expect("ghost");
        assert_eq!(g.w, d.bars[0].bounds().w);
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
        // 도크 바로 아래 한 행 = 새 행(2행이 된다 · Resized 보고) · 더 멀리 = 떼어 내기(순서는 시작 때로).
        let g_run = d.grips[1];
        let row_h = d.row_h;
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
                y: g_run.y + row_h + 5,
            },
            &mut inv,
        );
        assert_eq!(d.rows(), 2);
        assert_eq!(d.layout().rows, vec![0, 0, 1], "run만 1행");
        assert_eq!(d.take_actions(), vec![DockAction::Resized]);
        d.on_event(
            &InputEvent::MouseMove {
                x: g_run.x + 2,
                y: g_run.y + row_h * 2 + 60,
            },
            &mut inv,
        );
        assert_eq!(
            d.take_actions(),
            vec![
                DockAction::Resized,
                DockAction::Float {
                    id: "run".into(),
                    x: g_run.x + 2,
                    y: g_run.y + row_h * 2 + 60
                }
            ]
        );
        assert_eq!(d.rows(), 1);
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

#[cfg(test)]
mod drag_cancel_tests {
    use super::*;

    fn item(id: &str) -> ToolItem {
        ToolItem::new(id, ToolIcon::Glyph("x".into()))
    }

    /// Esc = 드래그 취소 — 순서·행이 시작 때로 · 행 수가 바뀌었으면 Resized · 드래그 아니었으면 false.
    #[test]
    fn escape_restores_order_and_rows() {
        let mut d = ToolDock::new(vec![
            ToolGroup::new("file", "File", vec![item("f.new")]),
            ToolGroup::new("run", "Run", vec![item("r.stmt")]),
        ]);
        d.set_icon_size(16);
        let mut inv = Invalidations::default();
        d.set_bounds(Rect::new(0, 0, 800, 28), &mut inv);
        assert!(!d.cancel_drag(&mut inv), "드래그 없음");
        let g = d.grips[0];
        d.on_event(
            &InputEvent::MouseDown {
                x: g.x + 2,
                y: g.y + 5,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        d.on_event(
            &InputEvent::MouseMove {
                x: g.x + 2,
                y: g.y + d.row_h + 5,
            },
            &mut inv,
        );
        // order는 행 순으로 다시 선다: run(0행) · file(1행).
        assert_eq!(d.layout().order, vec!["run", "file"]);
        assert_eq!(d.layout().rows, vec![0, 1]);
        assert_eq!(d.rows(), 2);
        let _ = d.take_actions();
        assert!(d.cancel_drag(&mut inv));
        assert_eq!(d.layout().order, vec!["file", "run"]);
        assert_eq!(d.layout().rows, vec![0, 0]);
        assert_eq!(d.rows(), 1);
        assert_eq!(d.take_actions(), vec![DockAction::Resized]);
        // 취소 뒤 MouseUp은 아무 일도 없다.
        d.on_event(&InputEvent::MouseUp { x: 0, y: 0 }, &mut inv);
        assert!(d.take_actions().is_empty());
    }
}

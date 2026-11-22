//! **툴바 그룹 도크**(09-17 nexa-sql 사용자 요청) — 목적별 그룹(아이콘·구분자 계층) · 그룹은 **도크 안에서 드래그로 순서 이동**
//! (09-19: 이동 대상이 **고스트로 커서를 따라 보이고** · 도크는 **여러 행** · 어느 방향으로든 · 자리 표시 = 강조 틀 · **Esc = 취소**) ·
//! **도크 밖으로 끌면 플로팅 미리보기 · 놓으면 떼어 냄 · 놓지 않고 다시 들어오면 순서 이동으로 복귀**(226차 · dir3 사용자 10-24 —
//! 종전 = 밖으로 나가는 순간 떼어 냈다) · 배치는 [`DockLayout`] 문자열 하나로 저장/복원 · 초기화.
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
/// 플로팅 미리보기 틀의 두께(논리 px) — 고스트 바깥에 창 테두리처럼 그린다(226차).
const PREVIEW_FRAME_PX: i32 = 3;
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
    /// 옆 띠에 붙은 그룹(그룹 id, 어느 쪽) — 띠 안 순서 = `order`(nexa-sql T-105 세로 도크).
    pub sides: Vec<(String, DockSide)>,
}

/// 도크 옆 띠(창 왼쪽/오른쪽 가장자리의 세로 띠 · nexa-sql T-105).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockSide {
    Left,
    Right,
}

impl DockSide {
    fn code(self) -> &'static str {
        match self {
            DockSide::Left => "L",
            DockSide::Right => "R",
        }
    }
}

impl DockLayout {
    /// `a,b;c@120:340,d` — `,` = 같은 행의 다음 · `;` = 다음 행 · `@x:y`가 붙은 그룹은 플로팅(순서는 돌아올 자리) ·
    /// `@L`/`@R` = 왼쪽/오른쪽 띠에 붙은 그룹(행·순서는 위 도크로 돌아올 자리이자 띠 안 순서).
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
                    match pos.trim() {
                        "L" | "l" => l.sides.push((id.to_string(), DockSide::Left)),
                        "R" | "r" => l.sides.push((id.to_string(), DockSide::Right)),
                        _ => {}
                    }
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
                    .map(|(_, id)| {
                        if let Some((_, side)) = self.sides.iter().find(|(f, _)| f == id) {
                            return format!("{id}@{}", side.code());
                        }
                        match self.floating.iter().find(|(f, _, _)| f == id) {
                            Some((_, x, y)) => format!("{id}@{x}:{y}"),
                            None => id.clone(),
                        }
                    })
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
    /// 도크(+ 새 행 영역) 밖에 있다 = **플로팅 미리보기**(놓으면 떼어 낸다 · 다시 들어오면 순서 이동으로 복귀 · 226차).
    outside: bool,
    /// 밖에 있으면서 옆 띠 자리 위에 있다 = 놓으면 그 띠에 붙는다(T-105).
    over_side: Option<DockSide>,
    /// 끌기 시작 때 그룹이 붙어 있던 옆 띠(위 도크에서 시작 = None).
    from_side: Option<DockSide>,
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
    /// ★ 옆 띠에 붙은 그룹(index별 · T-105): `Some` = 위 도크 배치에서 빠지고 그 띠에 세로 툴바로 그려진다 · 띠 안 순서 = `order`.
    side_of: Vec<Option<DockSide>>,
    /// 옆 띠 자리(호스트가 준다 · [`ToolDock::set_side_bounds`]) — 왼쪽 · 오른쪽. 폭 0 = 그 띠에 그룹 없음.
    side_rects: [Rect; 2],
    /// 끌어 붙일 수 있는 가장자리 영역(호스트가 준다 · 띠가 비어 있어도 잡히게 띠보다 넓다) — 왼쪽 · 오른쪽.
    side_zones: [Rect; 2],
    /// 바깥(플로팅 창)에서 끌어 온 그룹이 이 띠 자리 위에 있다 — 붙을 수 있음 표시([`ToolDock::set_side_hint`]).
    side_hint: Option<DockSide>,
    /// ★ 숨긴 그룹(index별 · nexa-sql T-278 10-30 "그룹 숨김"): 배치·그리기·사건에서 빠진다 · 순서(`order`)·행은 그대로 둬 다시 보이면 제자리.
    hidden: Vec<bool>,
    /// 그룹별 그립 사각형(마지막 배치).
    grips: Vec<Rect>,
    hover_grip: Option<usize>,
    drag: Option<Drag>,
    /// 바깥(플로팅 창)에서 끌어 온 그룹이 지금 도크 위에 있다 — 붙을 수 있음을 테두리로 알린다([`ToolDock::set_drop_hint`]).
    drop_hint: bool,
    actions: Vec<DockAction>,
    /// ★ 그룹 사각형이 낡았다(10-01 nexa-sql ⑲): 글자 항목의 폭은 그릴 때 실측되는데(`Toolbar::label_w`) 글이 바뀌면 그룹 폭이
    /// 달라져 항목이 사각형 밖으로 나간다 — 마우스 라우팅이 "그룹 사각형 안"이라 클릭이 안 닿았다. `paint`가 표시하고
    /// 다음 사건·[`ToolDock::relayout_if_stale`]에서 다시 배치한다.
    stale: std::cell::Cell<bool>,
    icon_px: i32,
    /// 그룹 사이 간격(논리 px · 기본 [`GROUP_GAP`] · 117차 `set_gaps`).
    group_gap: i32,
    /// 행 사이 간격(논리 px · 기본 0).
    row_gap: i32,
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
            side_of: vec![None; n],
            side_rects: [Rect::new(0, 0, 0, 0); 2],
            side_zones: [Rect::new(0, 0, 0, 0); 2],
            side_hint: None,
            hidden: vec![false; n],
            grips: vec![Rect::new(0, 0, 0, 0); n],
            hover_grip: None,
            drag: None,
            drop_hint: false,
            actions: Vec::new(),
            stale: std::cell::Cell::new(false),
            icon_px: super::toolbar::DEFAULT_ICON,
            group_gap: GROUP_GAP,
            row_gap: 0,
        }
    }

    /// 그룹 사이 · 행 사이 간격(논리 px · 음수는 0) — 기본 = 4 · 0(117차 · nexa-dir3 "툴바 간격을 설정으로"). 바뀌면 다시 배치하고
    /// 행 간격이 바뀌면 권장 높이가 달라지므로 [`DockAction::Resized`]를 보고한다.
    pub fn set_gaps(&mut self, group_gap: i32, row_gap: i32) {
        let (g, r) = (group_gap.max(0), row_gap.max(0));
        if (g, r) == (self.group_gap, self.row_gap) {
            return;
        }
        let row_changed = r != self.row_gap;
        self.group_gap = g;
        self.row_gap = r;
        self.relayout();
        if row_changed {
            self.actions.push(DockAction::Resized);
        }
    }

    /// 지금 간격 (그룹, 행) — 논리 px.
    #[must_use]
    pub fn gaps(&self) -> (i32, i32) {
        (self.group_gap, self.row_gap)
    }

    /// 모든 그룹 툴바의 여백 — `slot_pad` = 아이콘 칸 좌우(아이콘 사이 간격의 절반) · `bar_pad` = 툴바 양끝([`Toolbar::set_padding`]).
    pub fn set_padding(&mut self, slot_pad: i32, bar_pad: i32) {
        for b in &mut self.bars {
            b.set_padding(slot_pad, bar_pad);
        }
        self.relayout();
    }

    /// 모든 그룹 툴바의 상태 표시 방식([`Toolbar::set_soft_states`]).
    pub fn set_soft_states(&mut self, style: Option<super::toolbar::SoftStates>) {
        for b in &mut self.bars {
            b.set_soft_states(style);
        }
    }

    /// 모든 그룹 툴바의 항목 사이 간격([`Toolbar::set_item_gap`]).
    pub fn set_item_gap(&mut self, gap: i32) {
        for b in &mut self.bars {
            b.set_item_gap(gap);
        }
        self.relayout();
    }

    /// 토글 항목의 켜짐 표시(모든 그룹에서 id로) — 바뀐 것이 있으면 true.
    pub fn set_item_checked(&mut self, id: &str, on: bool, inv: &mut Invalidations) -> bool {
        let mut changed = false;
        for b in &mut self.bars {
            changed |= b.set_item_checked(id, on, inv);
        }
        changed
    }

    /// 아이콘 그림 배율 — 모든 그룹에([`Toolbar::set_icon_scale`] · 136차).
    pub fn set_icon_scale(&mut self, scale: f32) {
        for b in &mut self.bars {
            b.set_icon_scale(scale);
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
        let rows = self.rows() as i32;
        self.bar_height() * rows + self.row_gap * (rows - 1).max(0)
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
            .filter(|&&i| !self.skip_idx(i))
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

    /// 드래그 중 **자리 표시**(강조 틀) 사각형 — 끌리는 그룹의 도크 안 자리(227차 · dir3 10-24: 호스트가 툴바 아래 구분선을 이 폭만큼
    /// 비워 틀의 아래 선이 덮이지 않게). 드래그 중이 아니면 `None`.
    #[must_use]
    pub fn placeholder_rect(&self) -> Option<Rect> {
        let d = self.drag.as_ref().filter(|d| d.moved)?;
        Some(self.bars[d.gi].bounds())
    }

    /// Esc — 드래그를 취소하고 시작 때의 순서·행으로 되돌린다. 드래그 중이었으면 true(호스트가 키를 소비).
    pub fn cancel_drag(&mut self, inv: &mut Invalidations) -> bool {
        let Some(d) = self.drag.take() else {
            return false;
        };
        let rows_before = self.rows();
        self.order = d.orig_order;
        self.row_of = d.orig_rows;
        // 옆 띠에서 끌어 위 도크로 들어와 있던 그룹은 원래 띠로 되돌린다(T-105 · 끄는 동안 띠 소속을 잠시 풀어 둔다).
        if self.side_of[d.gi] != d.from_side {
            self.side_of[d.gi] = d.from_side;
            self.bars[d.gi].set_vertical(d.from_side.is_some());
            self.actions.push(DockAction::Resized);
        }
        self.relayout();
        if self.rows() != rows_before {
            self.actions.push(DockAction::Resized);
        }
        inv.push(self.base.bounds);
        inv.push(self.ghost_inv_rect_of(&d.cur, d.off_x, d.off_y, d.gi));
        d.moved
    }

    /// 빈 행을 없앤다(도크에 붙은 그룹 기준 · 플로팅은 자기 행을 따라간다).
    fn compact_rows(&mut self) {
        let mut used: Vec<usize> = self
            .order
            .iter()
            .filter(|&&i| !self.skip_idx(i))
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

    /// 도크 배치·그리기·사건에서 빠지는 그룹(플로팅 또는 숨김).
    fn skip_idx(&self, i: usize) -> bool {
        self.is_floating_idx(i) || self.hidden.get(i).copied().unwrap_or(false) || self.on_side(i)
    }

    /// 옆 띠에 붙어 있고 플로팅이 아닌가(띠에 실제로 그려지는 그룹).
    fn on_side(&self, i: usize) -> bool {
        self.side_of.get(i).copied().flatten().is_some() && !self.is_floating_idx(i)
    }

    /// 띠 `side`에 그려지는 그룹(도크 순서 · 숨김·플로팅 제외).
    fn side_members(&self, side: DockSide) -> Vec<usize> {
        self.order
            .iter()
            .copied()
            .filter(|&i| {
                self.side_of[i] == Some(side)
                    && !self.is_floating_idx(i)
                    && !self.hidden.get(i).copied().unwrap_or(false)
            })
            .collect()
    }

    fn side_slot(side: DockSide) -> usize {
        match side {
            DockSide::Left => 0,
            DockSide::Right => 1,
        }
    }

    // ── 옆 띠(T-105 세로 도크) ─────────────────────────────────────────────

    /// 그룹이 붙어 있는 옆 띠(위 도크·플로팅 = None).
    #[must_use]
    pub fn side_of(&self, id: &str) -> Option<DockSide> {
        let i = self.index(id)?;
        self.side_of[i].filter(|_| !self.is_floating_idx(i))
    }

    /// 그룹을 옆 띠에 붙이거나(`Some`) 위 도크로 되돌린다(`None`) — 플로팅이었으면 플로팅도 푼다. 배치 변경 + 크기 변경 보고
    /// (띠 폭·위 도크 행 수가 달라질 수 있다 → 호스트가 창을 다시 배치).
    pub fn set_side(&mut self, id: &str, side: Option<DockSide>) {
        let Some(i) = self.index(id) else { return };
        self.floating.retain(|(f, _, _)| *f != i);
        self.side_of[i] = side;
        let mut inv = Invalidations::default();
        self.bars[i].clear_hover(&mut inv);
        self.bars[i].set_vertical(side.is_some());
        self.compact_rows();
        self.relayout();
        self.side_hint = None;
        self.actions.push(DockAction::LayoutChanged);
        self.actions.push(DockAction::Resized);
    }

    /// 띠 `side`의 **권장 폭**(논리 px) — 붙은 그룹이 없으면 0. 호스트가 배율을 곱해 본문을 그만큼 들인다.
    #[must_use]
    pub fn side_width(&self, side: DockSide) -> i32 {
        let scale = self.base.scale.max(0.01);
        self.side_members(side)
            .into_iter()
            .map(|i| (self.bars[i].preferred_width_v() as f32 / scale).round() as i32)
            .max()
            .unwrap_or(0)
    }

    /// 옆 띠 자리(물리 px · 왼쪽, 오른쪽)를 정한다 — 호스트의 창 배치가 부른다(붙은 그룹이 없으면 폭 0 사각형).
    pub fn set_side_bounds(&mut self, left: Rect, right: Rect, inv: &mut Invalidations) {
        if self.side_rects != [left, right] {
            inv.push(self.side_rects[0]);
            inv.push(self.side_rects[1]);
            self.side_rects = [left, right];
            self.relayout();
            inv.push(left);
            inv.push(right);
        }
    }

    /// 끌어 붙일 수 있는 가장자리 영역(물리 px · 왼쪽, 오른쪽) — 띠가 비어 있어도 잡히도록 호스트가 본문 가장자리의 얇은 띠를 준다.
    pub fn set_side_zones(&mut self, left: Rect, right: Rect) {
        self.side_zones = [left, right];
    }

    /// 띠 자리(읽기) — 호스트의 사건 라우팅·그리기 영역 판단용.
    #[must_use]
    pub fn side_bounds(&self, side: DockSide) -> Rect {
        self.side_rects[Self::side_slot(side)]
    }

    /// 이 점이 옆 띠(그룹이 붙어 있는 자리) 안인가 — 호스트가 마우스 사건을 도크로 보낼지 판단.
    #[must_use]
    pub fn side_contains(&self, p: Point) -> bool {
        self.side_rects.iter().any(|r| r.w > 0 && r.contains(p))
    }

    /// 이 점에 놓으면 붙는 옆 띠(가장자리 영역 또는 띠 자리) — 위 도크 자리(`accepts_drop`)가 먼저다.
    #[must_use]
    pub fn side_zone_at(&self, p: Point) -> Option<DockSide> {
        [DockSide::Left, DockSide::Right].into_iter().find(|&s| {
            let k = Self::side_slot(s);
            self.side_zones[k].contains(p)
                || (self.side_rects[k].w > 0 && self.side_rects[k].contains(p))
        })
    }

    /// 점이 위 도크 띠 아래이면서 옆 띠 자리(가장자리 영역) 안인가 — 위 도크의 "새 행" 여유보다 옆 띠가 먼저다.
    fn below_on_side(&self, p: Point) -> bool {
        p.y >= self.base.bounds.bottom() && self.side_zone_at(p).is_some()
    }

    /// 붙을 수 있음 표시(옆 띠 가장자리)를 켜고 끈다 — 바뀌었으면 true(호스트가 다시 그린다).
    pub fn set_side_hint(&mut self, side: Option<DockSide>) -> bool {
        let changed = self.side_hint != side;
        self.side_hint = side;
        changed
    }

    /// 옆 띠 배치 — 띠마다 위에서 아래로: 손잡이(가로 띠) + 세로 툴바.
    fn relayout_sides(&mut self) {
        let grip = self.s(GRIP_W);
        let gap = self.s(self.group_gap);
        let mut inv = Invalidations::default();
        for side in [DockSide::Left, DockSide::Right] {
            let r = self.side_rects[Self::side_slot(side)];
            let mut y = r.y + self.s(2);
            for i in self.side_members(side) {
                self.bars[i].set_scale(self.base.scale);
                self.bars[i].set_vertical(true);
                let h = self.bars[i].preferred_length_v();
                self.grips[i] = Rect::new(r.x, y, r.w, grip);
                self.bars[i].set_bounds(Rect::new(r.x, y + grip, r.w, h), &mut inv);
                y += grip + h + gap;
            }
        }
    }

    /// 옆 띠 그리기(바탕 · 안쪽 테두리 · 그룹 손잡이 · 세로 툴바 · 붙을 수 있음 표시).
    fn paint_sides(&self, ctx: &mut dyn DrawCtx, theme: &Theme, ghosted: Option<usize>) {
        for side in [DockSide::Left, DockSide::Right] {
            let k = Self::side_slot(side);
            let r = self.side_rects[k];
            if r.w > 0 {
                ctx.fill_rect(r, theme.chrome_bg);
                // 본문 쪽 가장자리에 테두리 선.
                let ex = match side {
                    DockSide::Left => r.right() - 1,
                    DockSide::Right => r.x,
                };
                ctx.fill_rect(Rect::new(ex, r.y, 1, r.h), theme.border);
                for i in self.side_members(side) {
                    let br = self.bars[i].bounds();
                    if ghosted == Some(i) {
                        ctx.fill_rect_alpha(br, theme.accent, 0.12);
                        continue;
                    }
                    self.bars[i].paint(ctx, theme);
                    // 손잡이 = 가로 점 2행.
                    let g = self.grips[i];
                    let active =
                        self.hover_grip == Some(i) || self.drag.as_ref().is_some_and(|d| d.gi == i);
                    let c = if active { theme.accent } else { theme.text_dim };
                    let dot = self.s(2).max(1);
                    let cols = 4;
                    let span = cols * dot * 2 - dot;
                    let x0 = g.x + (g.w - span) / 2;
                    let y0 = g.y + (g.h - (dot * 3)) / 2;
                    for col in 0..cols {
                        for row in 0..2 {
                            ctx.fill_rect(
                                Rect::new(x0 + col * dot * 2, y0 + row * dot * 2, dot, dot),
                                c,
                            );
                        }
                    }
                }
            }
            // 붙을 수 있음 표시 — 끌고 있는 그룹(도크 안 끌기 · 플로팅 창)이 이 가장자리 위에 있다.
            let hint = self.side_hint == Some(side)
                || self
                    .drag
                    .as_ref()
                    .is_some_and(|d| d.moved && d.over_side == Some(side));
            if hint {
                let z = if r.w > 0 { r } else { self.side_zones[k] };
                let t = self.s(3).max(2);
                let x = match side {
                    DockSide::Left => z.x,
                    DockSide::Right => z.right() - t,
                };
                ctx.fill_rect(Rect::new(x, z.y, t, z.h), theme.accent);
                ctx.fill_rect_alpha(z, theme.accent, 0.10);
            }
        }
    }

    /// ★ 그룹 표시/숨김(T-278): 숨기면 자리를 차지하지 않는다(행이 비면 접힌다) · 사각형은 0 · 플로팅 상태·순서는 그대로. 모르는 id = false.
    pub fn set_group_visible(&mut self, id: &str, visible: bool) -> bool {
        let Some(i) = self.index(id) else {
            return false;
        };
        if self.hidden[i] == !visible {
            return false;
        }
        self.hidden[i] = !visible;
        if !visible {
            let mut inv = Invalidations::default();
            self.bars[i].set_bounds(Rect::new(0, 0, 0, 0), &mut inv);
            self.grips[i] = Rect::new(0, 0, 0, 0);
        }
        self.relayout();
        // 옆 띠의 그룹을 숨기거나 보이면 띠 폭이 달라진다 → 호스트가 창을 다시 배치(T-105).
        if self.side_of[i].is_some() {
            self.actions.push(DockAction::Resized);
        }
        true
    }

    /// 그룹이 보이는가(숨김이 아님 · 플로팅은 보임으로 센다). 모르는 id = false.
    #[must_use]
    pub fn is_group_visible(&self, id: &str) -> bool {
        self.index(id).is_some_and(|i| !self.hidden[i])
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
            // 옆 띠에서 떼어 낸 그룹 = 띠 소속을 푼다(플로팅 창은 가로 툴바 · 다시 붙일 자리는 놓는 곳이 정한다 · T-105).
            if self.side_of[i].take().is_some() {
                self.bars[i].set_vertical(false);
                self.relayout();
                self.actions.push(DockAction::Resized);
            }
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

    /// 바깥에서 끌어 온 그룹을 이 점(도크 좌표)에 놓으면 붙는가 — 도크 띠와 그 위아래 여유(떼어 내기와 같은 문턱) 안.
    #[must_use]
    pub fn accepts_drop(&self, p: Point) -> bool {
        let b = self.base.bounds;
        p.x >= b.x && p.x < b.right() && self.target_row(p.y).is_some() && !self.below_on_side(p)
    }

    /// 붙을 수 있음 표시(도크 테두리)를 켜고 끈다 — 바뀌었으면 `true`(호스트가 다시 그린다).
    pub fn set_drop_hint(&mut self, on: bool) -> bool {
        let changed = self.drop_hint != on;
        self.drop_hint = on;
        changed
    }

    /// 플로팅 그룹을 **이 점의 자리에** 붙인다(nexa-sql 사용자 11-21 "떼어 낸 그룹을 끌어서 다시 붙이기") — 그립으로 끌어
    /// 순서를 바꿀 때와 같은 자리 계산(행 + 같은 띠 그룹들의 가운데 비교). 점이 도크 밖이면 원래 자리로([`ToolDock::dock`]).
    pub fn dock_at(&mut self, id: &str, p: Point) {
        self.drop_hint = false;
        let Some(i) = self.index(id) else { return };
        // 위 도크의 이 자리에 붙인다 = 옆 띠 소속도 푼다(T-105).
        self.side_of[i] = None;
        self.bars[i].set_vertical(false);
        self.dock(id);
        self.relayout();
        self.reorder_to(i, p);
        self.actions.push(DockAction::LayoutChanged);
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
            sides: self
                .order
                .iter()
                .filter_map(|&i| Some((self.groups[i].id.clone(), self.side_of[i]?)))
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
        self.side_of = vec![None; self.groups.len()];
        for (id, side) in &l.sides {
            if let Some(i) = self.index(id) {
                self.side_of[i] = Some(*side);
            }
        }
        for i in 0..self.bars.len() {
            let v = self.side_of[i].is_some();
            self.bars[i].set_vertical(v);
        }
        self.compact_rows();
        self.relayout();
    }

    /// 기본 배치로(전부 도크 · 정의 순서).
    pub fn reset(&mut self) {
        self.order = (0..self.groups.len()).collect();
        self.row_of = vec![0; self.groups.len()];
        self.floating.clear();
        self.side_of = vec![None; self.groups.len()];
        self.drag = None;
        self.relayout();
        self.actions.push(DockAction::LayoutChanged);
        self.actions.push(DockAction::Resized);
    }

    /// 호스트가 처리할 일(1회성).
    pub fn take_actions(&mut self) -> Vec<DockAction> {
        std::mem::take(&mut self.actions)
    }

    /// 도크에 붙은 그룹에서 클릭된 액션 id(1회성 · 플로팅 창의 클릭은 호스트가 `bar_mut`로 직접 가져간다).
    pub fn take_clicked(&mut self) -> Option<String> {
        for i in self.live_groups() {
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

    /// ★ 2단 확인 무장(222차 · [`Toolbar::set_item_armed`]) — 모든 그룹에서 같은 id · 하나라도 바뀌면 true.
    pub fn set_item_armed(
        &mut self,
        id: &str,
        ratio: Option<f32>,
        inv: &mut Invalidations,
    ) -> bool {
        let mut hit = false;
        for b in &mut self.bars {
            hit |= b.set_item_armed(id, ratio, inv);
        }
        hit
    }

    /// 무장 진척률(첫 일치 · 덤프용).
    #[must_use]
    pub fn item_armed(&self, id: &str) -> Option<f32> {
        self.bars.iter().find_map(|b| b.item_armed(id))
    }

    pub fn set_item_tip(&mut self, id: &str, tip: &str) {
        for b in &mut self.bars {
            b.set_item_tip(id, tip);
        }
    }

    /// 글자 항목의 글(09-28 · nexa-sql 탭 연결정보).
    pub fn set_item_label(&mut self, id: &str, label: &str, inv: &mut Invalidations) {
        for b in &mut self.bars {
            b.set_item_label(id, label, inv);
        }
    }

    pub fn set_item_icon(&mut self, id: &str, icon: ToolIcon, inv: &mut Invalidations) {
        for b in &mut self.bars {
            b.set_item_icon(id, icon.clone(), inv);
        }
    }

    /// 토글 항목이 켜져 있는가(어느 그룹이든).
    #[must_use]
    pub fn item_checked(&self, id: &str) -> bool {
        self.bars.iter().any(|b| b.item_checked(id))
    }

    /// 모든 그룹의 항목(도크 순서 · 구분자 포함) — 호스트 점검·덤프용.
    #[must_use]
    pub fn all_items(&self) -> Vec<ToolItem> {
        self.order
            .iter()
            .flat_map(|&i| self.bars[i].items().iter().cloned())
            .collect()
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
    /// ★ 그려진 뒤 글자 폭이 바뀌어 그룹 사각형이 낡았으면 다시 배치(10-01) — 호스트가 paint 뒤에 불러 화면도 맞춘다. 반환 = 배치했는가.
    pub fn relayout_if_stale(&mut self, inv: &mut Invalidations) -> bool {
        if !self.stale.replace(false) {
            return false;
        }
        self.relayout();
        inv.push(self.base.bounds);
        true
    }

    fn relayout(&mut self) {
        let b = self.base.bounds;
        let grip = self.s(GRIP_W);
        let gap = self.s(self.group_gap);
        let rgap = self.s(self.row_gap);
        let mut inv = Invalidations::default();
        let nrows = self.rows();
        let row_h = ((b.h - rgap * (nrows as i32 - 1)) / nrows as i32).max(1);
        self.row_h = row_h;
        for r in 0..nrows {
            let ry = b.y + r as i32 * (row_h + rgap);
            let mut x = b.x + self.s(2);
            let mut rx = b.right() - self.s(2);
            for &i in &self.order.clone() {
                if self.skip_idx(i) {
                    self.grips[i] = Rect::new(0, 0, 0, 0);
                    continue;
                }
                if self.row_of[i] != r {
                    continue;
                }
                self.bars[i].set_scale(self.base.scale);
                self.bars[i].set_vertical(false);
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
        self.relayout_sides();
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

    /// 고스트 무효화 사각형 — 플로팅 미리보기 틀(바깥 3px)까지.
    fn ghost_inv_rect_of(&self, cur: &Point, off_x: i32, off_y: i32, gi: usize) -> Rect {
        let g = self.ghost_rect_of(cur, off_x, off_y, gi);
        let m = self.s(PREVIEW_FRAME_PX);
        Rect::new(g.x - m, g.y - m, g.w + m * 2, g.h + m * 2)
    }

    fn ghost_inv_rect(&self) -> Option<Rect> {
        let d = self.drag.as_ref().filter(|d| d.moved)?;
        Some(self.ghost_inv_rect_of(&d.cur, d.off_x, d.off_y, d.gi))
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
        let r = ((y - b.y) / (row_h + self.s(self.row_gap)).max(1)) as usize;
        Some(r.min(self.rows()))
    }

    fn grip_at(&self, p: Point) -> Option<usize> {
        self.order
            .iter()
            .copied()
            .find(|&i| (!self.skip_idx(i) || self.shown_on_side(i)) && self.grips[i].contains(p))
    }

    /// 옆 띠에 실제로 그려지는 그룹인가(붙어 있고 · 플로팅·숨김 아님).
    fn shown_on_side(&self, i: usize) -> bool {
        self.on_side(i) && !self.hidden.get(i).copied().unwrap_or(false)
    }

    /// 사건을 받는 그룹(위 도크에 그려지는 것 + 옆 띠에 그려지는 것 · 도크 순서).
    fn live_groups(&self) -> Vec<usize> {
        self.order
            .iter()
            .copied()
            .filter(|&i| !self.skip_idx(i) || self.shown_on_side(i))
            .collect()
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
                    && !self.skip_idx(i)
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
                !self.skip_idx(i) && self.row_of[i] == row && self.groups[i].right == right
            });
            match same_band_last {
                Some(q) => q + 1,
                None => {
                    let row_first = self
                        .order
                        .iter()
                        .position(|&i| !self.skip_idx(i) && self.row_of[i] == row);
                    let row_last = self
                        .order
                        .iter()
                        .rposition(|&i| !self.skip_idx(i) && self.row_of[i] == row);
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
                            .rposition(|&i| !self.skip_idx(i) && self.row_of[i] < row)
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
        let c = theme.accent;
        if d.outside {
            // 플로팅 미리보기 = 창처럼 보이는 틀(바탕 + 바깥 테두리) 위에 고스트.
            let m = self.s(PREVIEW_FRAME_PX);
            let f = Rect::new(g.x - m, g.y - m, g.w + m * 2, g.h + m * 2);
            ctx.fill_rect(f, theme.chrome_bg);
            ctx.fill_rect(Rect::new(f.x, f.y, f.w, 1), theme.border);
            ctx.fill_rect(Rect::new(f.x, f.bottom() - 1, f.w, 1), theme.border);
            ctx.fill_rect(Rect::new(f.x, f.y, 1, f.h), theme.border);
            ctx.fill_rect(Rect::new(f.right() - 1, f.y, 1, f.h), theme.border);
        }
        self.bars[d.gi].paint_offset(ctx, theme, g.x - r.x, g.y - r.y);
        ctx.fill_rect(Rect::new(g.x, g.y, g.w, 1), c);
        ctx.fill_rect(Rect::new(g.x, g.bottom() - 1, g.w, 1), c);
        ctx.fill_rect(Rect::new(g.x, g.y, 1, g.h), c);
        ctx.fill_rect(Rect::new(g.right() - 1, g.y, 1, g.h), c);
    }

    /// 툴팁(팝업 층).
    /// 어느 바든 툴팁이 보이거나 그룹 손잡이 이름 툴팁이 보이는가(`paint_tooltip`이 그리는 조건과 같다 · 217차) — 호스트의
    /// 부분 다시 그리기가 이때는 전체로 바꾼다.
    #[must_use]
    pub fn tooltip_visible(&self) -> bool {
        let bar = self
            .live_groups()
            .into_iter()
            .any(|i| self.bars[i].tooltip_visible());
        let grip = self
            .hover_grip
            .filter(|_| self.drag.is_none())
            .is_some_and(|i| {
                self.groups.get(i).is_some_and(|g| !g.title.is_empty())
                    && self.grips.get(i).is_some_and(|r| r.w > 0)
            });
        bar || grip
    }

    pub fn paint_tooltip(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        // 도크의 폭(= 창 폭) 안에 — 오른쪽 정렬 그룹의 툴팁도 창 안에서 왼쪽으로 밀린다.
        let clamp = (self.base.bounds.x, self.base.bounds.right());
        for i in self.live_groups() {
            self.bars[i].paint_tooltip_in(ctx, theme, clamp);
        }
        // ★ 그룹 손잡이 위 = 그룹 이름 툴팁(nexa-sql 09-28 "그룹을 이동하는 영역에 마우스를 올리면 그룹 이름") — 끄는 중에는 없음.
        if let Some(i) = self.hover_grip.filter(|_| self.drag.is_none()) {
            if let (Some(g), Some(r)) = (self.groups.get(i), self.grips.get(i)) {
                if !g.title.is_empty() && r.w > 0 {
                    crate::draw::draw_tooltip_in(ctx, theme, *r, clamp, &g.title, self.base.scale);
                }
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
        // 글자 폭이 바뀐 뒤 첫 사건이면 먼저 재배치(그룹 사각형 = 지금 폭) — 라우팅이 맞게.
        self.relayout_if_stale(inv);
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
                        outside: false,
                        over_side: None,
                        from_side: self.side_of[gi],
                        orig_order: self.order.clone(),
                        orig_rows: self.row_of.clone(),
                    });
                    inv.push(b);
                    return;
                }
                // 마우스 라우팅 규칙: 커서 아래 그룹에만.
                for i in self.live_groups() {
                    if self.bars[i].bounds().contains(p) {
                        self.bars[i].on_event(ev, inv);
                    }
                }
            }
            InputEvent::MouseUp { x, y } => {
                if let Some(d) = self.drag.take() {
                    if d.moved {
                        inv.push(self.ghost_inv_rect_of(&d.cur, d.off_x, d.off_y, d.gi));
                        inv.push(self.side_rects[0]);
                        inv.push(self.side_rects[1]);
                        if d.outside {
                            let id = self.groups[d.gi].id.clone();
                            // ★ 옆 띠 자리에 놓음 = 그 띠에 붙인다(T-105) · 같은 띠에서 끌다 제자리에 놓으면 변화 없음.
                            if let Some(side) = d.over_side {
                                if d.from_side != Some(side) {
                                    self.set_side(&id, Some(side));
                                }
                                inv.push(b);
                                return;
                            }
                            // 도크 밖에서 **놓음** = 떼어 내기(호스트가 창을 만든다 · 순서는 이미 시작 때로 돌아가 있다 · 226차).
                            self.actions.push(DockAction::Float { id, x, y });
                            inv.push(b);
                            return;
                        }
                        // 옆 띠에서 끌어 위 도크 자리에 놓음 = 위 도크로(자리는 끄는 동안 정해졌다).
                        if d.from_side.is_some() {
                            self.actions.push(DockAction::LayoutChanged);
                            self.actions.push(DockAction::Resized);
                            inv.push(b);
                            return;
                        }
                        if d.order_changed(&self.order, &self.row_of) {
                            self.actions.push(DockAction::LayoutChanged);
                        }
                    }
                    inv.push(b);
                    return;
                }
                // Up은 눌린 툴바가 스스로 판정(눌림 해제 포함)하도록 전부에.
                for i in self.live_groups() {
                    self.bars[i].on_event(ev, inv);
                }
            }
            InputEvent::MouseMove { x, y } => {
                if let Some((gi, x0, y0)) = self.drag.as_ref().map(|d| (d.gi, d.x0, d.y0)) {
                    let p = Point { x, y };
                    let start = self.s(DRAG_START_PX);
                    if let Some(old) = self.ghost_inv_rect() {
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
                    // 도크(+ 새 행 영역) 밖 = **플로팅 미리보기**: 떼어 내기는 놓을 때(MouseUp) · 놓지 않고 다시 들어오면 순서 이동으로
                    // 복귀(dir3 사용자 10-24 · 종전 = 나가는 순간 떼어 냈다). 밖에 있는 동안 도크는 시작 때 순서로 돌려 둔다.
                    // 도크 띠 아래의 옆 띠 자리는 "새 행" 여유와 겹친다 → 옆 띠 자리가 먼저다(밖으로 본다).
                    let outside = self.target_row(y).is_none() || self.below_on_side(p);
                    let was_outside = self.drag.as_ref().is_some_and(|d| d.outside);
                    // 밖에 있을 때 옆 띠 자리 위인가(T-105) — 표시가 바뀌면 그 띠를 다시 그린다.
                    let over_side = if outside { self.side_zone_at(p) } else { None };
                    if self.drag.as_ref().is_some_and(|d| d.over_side != over_side) {
                        if let Some(d) = self.drag.as_mut() {
                            d.over_side = over_side;
                        }
                        for k in 0..2 {
                            inv.push(self.side_rects[k]);
                            inv.push(self.side_zones[k]);
                        }
                    }
                    // 옆 띠에서 끌어 온 그룹이 위 도크 자리로 들어왔다 = 위 도크 그룹으로 바꿔 자리 잡기를 잇는다(나가면 다시 띠로).
                    let from_side = self.drag.as_ref().and_then(|d| d.from_side);
                    if from_side.is_some() && outside == self.side_of[gi].is_none() {
                        self.side_of[gi] = if outside { from_side } else { None };
                        self.bars[gi].set_vertical(outside);
                        let rows_before = self.rows();
                        self.compact_rows();
                        self.relayout();
                        if self.rows() != rows_before {
                            self.actions.push(DockAction::Resized);
                        }
                        inv.push(b);
                        inv.push(self.side_rects[0]);
                        inv.push(self.side_rects[1]);
                    }
                    if outside != was_outside {
                        if let Some(d) = self.drag.as_mut() {
                            d.outside = outside;
                        }
                        if outside && from_side.is_none() {
                            let (orig_order, orig_rows) = self
                                .drag
                                .as_ref()
                                .map(|d| (d.orig_order.clone(), d.orig_rows.clone()))
                                .unwrap_or_default();
                            let rows_before = self.rows();
                            self.order = orig_order;
                            self.row_of = orig_rows;
                            self.relayout();
                            if self.rows() != rows_before {
                                self.actions.push(DockAction::Resized);
                            }
                        }
                        inv.push(b);
                    }
                    if !outside && self.reorder_to(gi, p) {
                        inv.push(b);
                    }
                    if let Some(g) = self.ghost_inv_rect() {
                        inv.push(g);
                    }
                    return;
                }
                let over = self.grip_at(Point { x, y });
                if over != self.hover_grip {
                    self.hover_grip = over;
                    inv.push(b);
                    inv.push(self.side_rects[0]);
                    inv.push(self.side_rects[1]);
                }
                for i in self.live_groups() {
                    self.bars[i].on_event(ev, inv);
                }
            }
            _ => {}
        }
    }

    fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.base.bounds;
        ctx.fill_rect(b, theme.chrome_bg);
        // 도크 바닥 선은 **먼저**(228차 · dir3 10-24 캡처 검수): 맨 끝에 그리면 드래그 중 자리 표시 틀의 아래 선(같은 줄)을 덮는다.
        ctx.fill_rect(Rect::new(b.x, b.bottom() - 1, b.w, 1), theme.border);
        let ghosted = self.drag.as_ref().filter(|d| d.moved).map(|d| d.gi);
        // ★ 글자 항목 폭 실측은 툴바 paint에서 — 그려진 뒤 권장 폭이 배치 폭과 다르면 낡음 표시(다음 사건·호스트가 재배치).
        let check_stale = |dock: &Self| {
            for &i in &dock.order {
                if dock.skip_idx(i) {
                    continue;
                }
                if dock.bars[i].preferred_width() != dock.bars[i].bounds().w {
                    dock.stale.set(true);
                    return;
                }
            }
        };
        let _ = &check_stale;
        for &i in &self.order {
            if self.skip_idx(i) {
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
        self.paint_sides(ctx, theme, ghosted);
        if self.drop_hint {
            let t = self.s(2).max(1);
            let c = theme.accent;
            ctx.fill_rect(Rect::new(b.x, b.y, b.w, t), c);
            ctx.fill_rect(Rect::new(b.x, b.bottom() - t, b.w, t), c);
            ctx.fill_rect(Rect::new(b.x, b.y, t, b.h), c);
            ctx.fill_rect(Rect::new(b.right() - t, b.y, t, b.h), c);
        }
        check_stale(self);
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

    /// T-105 옆 띠: 배치 글 왕복(`@L`/`@R`) · 붙이면 위 도크에서 빠지고 띠에 세로로 놓임 · 클릭이 닿음 · 위 도크로 되돌리면 제자리.
    #[test]
    fn side_dock_layout_round_trip_and_placement() {
        let l = DockLayout::parse("file,run@L;view@R,x@10:20");
        assert_eq!(
            l.sides,
            vec![
                ("run".to_string(), DockSide::Left),
                ("view".to_string(), DockSide::Right)
            ]
        );
        assert_eq!(l.serialize(), "file,run@L;view@R,x@10:20");
        assert_eq!(DockLayout::parse("a,b;c@120:340").sides, vec![]);

        let mut d = dock();
        let mut inv = Invalidations::default();
        assert_eq!(d.side_width(DockSide::Left), 0);
        let run_top = d.bar("run").unwrap().bounds();
        d.set_side("run", Some(DockSide::Left));
        assert!(d.take_actions().contains(&DockAction::Resized));
        assert_eq!(d.side_of("run"), Some(DockSide::Left));
        let w = d.side_width(DockSide::Left);
        assert!(w > 0);
        d.set_side_bounds(Rect::new(0, 28, w, 400), Rect::new(0, 0, 0, 0), &mut inv);
        let rb = d.bar("run").unwrap();
        assert!(rb.is_vertical());
        let r = rb.bounds();
        assert!(r.x == 0 && r.y > 28 && r.w == w && r.h > r.w, "{r:?}");
        // 위 도크에서는 빠졌다 = 뒤 그룹이 그 자리로 당겨지지 않는다(오른쪽 정렬 view는 그대로) · file은 그대로.
        assert!(d.bar("file").unwrap().bounds().y == 0 && !d.bar("file").unwrap().is_vertical());
        assert_eq!(d.layout().serialize(), "file,run@L,view");
        assert!(
            d.side_contains(Point { x: 2, y: 100 }) && !d.side_contains(Point { x: 300, y: 100 })
        );
        // 띠 안 항목 클릭.
        let it = d.item_rect("r.stmt").unwrap();
        let (cx, cy) = (it.x + it.w / 2, it.y + it.h / 2);
        d.on_event(
            &InputEvent::MouseDown {
                x: cx,
                y: cy,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        d.on_event(&InputEvent::MouseUp { x: cx, y: cy }, &mut inv);
        assert_eq!(d.take_clicked().as_deref(), Some("r.stmt"));
        // 배치 적용으로도 같은 상태 · 되돌리기.
        let saved = d.layout();
        let mut d2 = dock();
        d2.apply_layout(&saved);
        assert_eq!(d2.side_of("run"), Some(DockSide::Left));
        d.set_side("run", None);
        d.set_side_bounds(Rect::new(0, 0, 0, 0), Rect::new(0, 0, 0, 0), &mut inv);
        assert_eq!(d.side_of("run"), None);
        assert_eq!(d.bar("run").unwrap().bounds(), run_top);
        d.reset();
        assert_eq!(d.layout().sides, vec![]);
    }

    /// T-105 끌어 붙이기: 위 도크의 손잡이를 끌어 왼쪽 가장자리 영역에 놓으면 그 띠에 붙고(Float 보고 없음) · 띠의 손잡이를 끌어
    /// 위 도크 자리에 놓으면 위 도크로 · 아무 데도 아닌 곳에 놓으면 종전대로 떼어 내기.
    #[test]
    fn drag_to_edge_docks_to_side_and_back() {
        let mut d = dock();
        let mut inv = Invalidations::default();
        d.set_side_zones(Rect::new(0, 28, 24, 400), Rect::new(776, 28, 24, 400));
        let dn = |x, y| InputEvent::MouseDown {
            x,
            y,
            shift: false,
            primary: false,
        };
        let mv = |x, y| InputEvent::MouseMove { x, y };
        let up = |x, y| InputEvent::MouseUp { x, y };
        // run 손잡이 = run 툴바 왼쪽.
        let rb = d.bar("run").unwrap().bounds();
        let (gx, gy) = (rb.x - GRIP_W / 2, rb.y + rb.h / 2);
        d.on_event(&dn(gx, gy), &mut inv);
        d.on_event(&mv(gx, gy + 100), &mut inv);
        d.on_event(&mv(10, 200), &mut inv);
        d.on_event(&up(10, 200), &mut inv);
        let acts = d.take_actions();
        assert!(
            !acts.iter().any(|a| matches!(a, DockAction::Float { .. })),
            "{acts:?}"
        );
        assert_eq!(d.side_of("run"), Some(DockSide::Left));
        let w = d.side_width(DockSide::Left);
        d.set_side_bounds(Rect::new(0, 28, w, 400), Rect::new(0, 0, 0, 0), &mut inv);
        // 띠의 손잡이(툴바 바로 위 가로 띠)를 끌어 위 도크 안에 놓는다.
        let vb = d.bar("run").unwrap().bounds();
        let (sx, sy) = (vb.x + vb.w / 2, vb.y - GRIP_W / 2);
        d.on_event(&dn(sx, sy), &mut inv);
        d.on_event(&mv(sx + 60, sy + 60), &mut inv);
        assert_eq!(
            d.side_of("run"),
            Some(DockSide::Left),
            "밖에 있는 동안은 띠 소속 그대로"
        );
        d.on_event(&mv(300, 10), &mut inv);
        d.on_event(&up(300, 10), &mut inv);
        assert_eq!(d.side_of("run"), None);
        assert!(!d.bar("run").unwrap().is_vertical());
        assert!(d.take_actions().contains(&DockAction::LayoutChanged));
        // 띠에서 끌어 위 도크 안까지 들어온 뒤 Esc = 원래 띠로(세로 툴바 · 배치 글 그대로).
        d.set_side("run", Some(DockSide::Left));
        d.set_side_bounds(Rect::new(0, 28, w, 400), Rect::new(0, 0, 0, 0), &mut inv);
        let _ = d.take_actions();
        let before = d.layout().serialize();
        let vb = d.bar("run").unwrap().bounds();
        let (sx, sy) = (vb.x + vb.w / 2, vb.y - GRIP_W / 2);
        d.on_event(&dn(sx, sy), &mut inv);
        d.on_event(&mv(sx + 60, sy + 60), &mut inv);
        d.on_event(&mv(300, 10), &mut inv);
        assert_eq!(d.side_of("run"), None, "끄는 동안 위 도크 자리");
        assert!(d.cancel_drag(&mut inv));
        assert_eq!(d.side_of("run"), Some(DockSide::Left));
        assert!(d.bar("run").unwrap().is_vertical());
        assert_eq!(d.layout().serialize(), before);
        d.set_side("run", None);
        d.set_side_bounds(Rect::new(0, 0, 0, 0), Rect::new(0, 0, 0, 0), &mut inv);
        let _ = d.take_actions();
        // 띠 자리도 도크도 아닌 곳에 놓음 = 떼어 내기.
        let rb = d.bar("run").unwrap().bounds();
        let (gx, gy) = (rb.x - GRIP_W / 2, rb.y + rb.h / 2);
        d.on_event(&dn(gx, gy), &mut inv);
        d.on_event(&mv(400, 300), &mut inv);
        d.on_event(&up(400, 300), &mut inv);
        assert!(d
            .take_actions()
            .iter()
            .any(|a| matches!(a, DockAction::Float { .. })));
    }

    /// 간격 설정(117차): 그룹 간격 0 = 그룹이 그립 폭만 두고 맞닿는다 · 행 간격 = 권장 높이와 행 y에 반영 · 칸 여백 0 = 아이콘이 맞닿는다.
    #[test]
    fn gaps_and_padding_are_configurable() {
        let mk = || {
            let g = |id: &str| {
                ToolGroup::new(
                    id,
                    id,
                    vec![
                        ToolItem::new(format!("{id}.1"), ToolIcon::Glyph("a".into())),
                        ToolItem::new(format!("{id}.2"), ToolIcon::Glyph("b".into())),
                    ],
                )
            };
            let mut d = ToolDock::new(vec![g("a"), g("b")]);
            d.set_icon_size(20);
            d
        };
        let mut inv = Invalidations::default();
        let mut d = mk();
        assert_eq!(d.gaps(), (GROUP_GAP, 0));
        let h1 = d.preferred_height();
        d.set_bounds(Rect::new(0, 0, 600, h1), &mut inv);
        let bx_default = d.bar("b").unwrap().bounds().x;
        let a_right = d.bar("a").unwrap().bounds().right();
        assert_eq!(bx_default, a_right + GROUP_GAP + GRIP_W);
        d.set_gaps(0, 6);
        assert_eq!(
            d.bar("b").unwrap().bounds().x,
            a_right + GRIP_W,
            "그룹 간격 0"
        );
        assert!(
            d.take_actions().contains(&DockAction::Resized),
            "행 간격 변화 = 높이 재계산 요청"
        );
        // 두 행으로: 권장 높이 = 2행 + 행 간격 · 둘째 행 y = 행 높이 + 간격.
        d.apply_layout(&DockLayout::parse("a;b"));
        assert_eq!(d.rows(), 2);
        assert_eq!(d.preferred_height(), h1 * 2 + 6);
        d.set_bounds(Rect::new(0, 0, 600, d.preferred_height()), &mut inv);
        assert_eq!(d.bar("b").unwrap().bounds().y, h1 + 6);
        // 항목 간격 0 + 칸 여백 0 = 아이콘이 맞닿는다(폭이 줄어든다).
        let w_before = d.bar("a").unwrap().bounds().w;
        d.set_padding(0, 2);
        d.set_item_gap(0);
        d.set_bounds(Rect::new(0, 0, 600, d.preferred_height()), &mut inv);
        let a = d.bar("a").unwrap();
        assert!(a.bounds().w <= w_before);
        let (r1, r2) = (a.item_rect("a.1").unwrap(), a.item_rect("a.2").unwrap());
        assert_eq!(r1.right(), r2.x, "아이콘 칸 사이 틈 0");
        // 체크 전달.
        assert!(d.set_item_checked("b.1", true, &mut inv));
        assert!(!d.set_item_checked("b.1", true, &mut inv));
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
        assert_eq!(
            d.placeholder_rect(),
            Some(d.bars[0].bounds()),
            "자리 표시 = 그 그룹의 도크 안 자리(227차)"
        );
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
        assert_eq!(d.placeholder_rect(), None, "놓으면 자리 표시 없음");
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
        // 226차: 밖 = 플로팅 **미리보기**(도크는 시작 때 순서 · Float는 아직 아님 · 드래그 계속).
        assert_eq!(
            d.take_actions(),
            vec![DockAction::Resized],
            "밖 = 미리보기만"
        );
        assert!(d.is_dragging());
        assert_eq!(d.rows(), 1);
        assert!(!d.is_floating("run"));
        assert!(d.drag.as_ref().is_some_and(|dr| dr.outside));
        // 놓지 않고 도크 바로 아래(새 행)로 돌아오면 순서 이동으로 복귀.
        d.on_event(
            &InputEvent::MouseMove {
                x: g_run.x + 2,
                y: g_run.y + row_h + 5,
            },
            &mut inv,
        );
        assert_eq!(d.rows(), 2, "복귀 = 다시 새 행 미리보기");
        assert_eq!(d.take_actions(), vec![DockAction::Resized]);
        assert!(d.drag.as_ref().is_some_and(|dr| !dr.outside));
        // 다시 밖으로 → **놓으면** 떼어 내기(좌표 = 놓은 자리) · 순서는 시작 때.
        let (fx, fy) = (g_run.x + 2, g_run.y + row_h * 2 + 60);
        d.on_event(&InputEvent::MouseMove { x: fx, y: fy }, &mut inv);
        assert_eq!(d.take_actions(), vec![DockAction::Resized]);
        d.on_event(&InputEvent::MouseUp { x: fx, y: fy }, &mut inv);
        assert_eq!(
            d.take_actions(),
            vec![DockAction::Float {
                id: "run".into(),
                x: fx,
                y: fy
            }]
        );
        assert!(!d.is_dragging());
        assert_eq!(d.rows(), 1);
        assert_eq!(
            d.layout().order,
            vec!["run", "file", "view"],
            "순서 = 시작 때"
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

    /// ★ 10-01(nexa-sql ⑲): 글자 항목의 글이 길어지면 그룹 사각형이 낡는다 → `relayout_if_stale`가 권장 폭으로 다시.
    #[test]
    fn label_growth_relayouts_group() {
        let g = ToolGroup::new(
            "t",
            "T",
            vec![
                ToolItem::text("a", "ab").with_dropdown(),
                ToolItem::text("b", "cd").with_dropdown(),
            ],
        );
        let mut dock = ToolDock::new(vec![g]);
        let mut inv = Invalidations::default();
        dock.set_bounds(Rect::new(0, 0, 800, 40), &mut inv);
        let w0 = dock.bars[0].bounds().w;
        // 글자 폭 실측이 없는 상태에서는 권장 폭 = 배치 폭.
        assert_eq!(w0, dock.bars[0].preferred_width());
        assert!(!dock.relayout_if_stale(&mut inv), "낡지 않음");
        // 글이 길어졌다고(실측 폭이 커졌다고) 표시 → 낡음.
        dock.bars[0].items_mut_for_test()[1].label_w.set(200);
        dock.stale.set(true);
        assert!(dock.relayout_if_stale(&mut inv));
        assert_eq!(dock.bars[0].bounds().w, dock.bars[0].preferred_width());
        assert!(dock.bars[0].bounds().w > w0);
    }

    /// ★ T-278 그룹 숨김: 숨긴 그룹은 자리를 차지하지 않고(다음 그룹이 당겨짐) 사각형 0 · 다시 보이면 제자리 · 순서·groups()는 그대로.
    #[test]
    fn hidden_group_takes_no_space_and_returns_in_place() {
        let mut d = ToolDock::new(vec![
            ToolGroup::new("a", "A", vec![item("a.1"), item("a.2")]),
            ToolGroup::new("b", "B", vec![item("b.1")]),
        ]);
        d.set_icon_size(16);
        let mut inv = Invalidations::default();
        d.set_bounds(Rect::new(0, 0, 600, d.preferred_height()), &mut inv);
        let before_b = d.bar("b").unwrap().bounds().x;
        assert!(d.set_group_visible("a", false));
        assert!(!d.set_group_visible("a", false), "같은 상태 = false");
        assert!(!d.is_group_visible("a"));
        assert_eq!(d.bar("a").unwrap().bounds().w, 0);
        assert!(
            d.bar("b").unwrap().bounds().x < before_b,
            "앞 그룹이 숨으면 당겨진다"
        );
        let ids: Vec<String> = d.groups().into_iter().map(|(id, _, _)| id).collect();
        assert!(ids.contains(&"a".to_string()), "순서 목록에는 남는다");
        assert!(d.set_group_visible("a", true));
        assert_eq!(d.bar("b").unwrap().bounds().x, before_b);
        assert!(!d.set_group_visible("zzz", false));
    }
}

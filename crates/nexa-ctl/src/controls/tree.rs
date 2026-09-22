//! 트리 컨트롤 — **TreeView(단독)** 와 **TreeGrid(그리드+트리)** 의 공통 추상(사용자 요청 08-08).
//!
//! ## 추상 설계 (ControlBase → TreeControl → TreeView / TreeGrid)
//!
//! 두 컨트롤은 **같은 계층 모델**([`TreeModel`])과 **같은 트리 동작**([`TreeControl`] 기본 메서드)을
//! 공유하고, 표현만 다르다:
//!
//! - [`super::Control`] — 루트 인터페이스(포커스 링·활성·도움말).
//! - [`TreeControl`] — 트리 계층 인터페이스. 평탄화·펼침/접기·선택 이동·히트테스트를
//!   [`TreeModel`] 접근자 + 선택 상태 접근자만으로 **기본 메서드 상속**.
//! - [`TreeView`] — **단일 열** 트리(들여쓰기 + 셰브론 + 라벨).
//! - [`TreeGrid`] — **여러 열** 그리드. 첫 열이 트리(셰브론+라벨), 나머지는 셀 값. 헤더 포함.
//!
//! 확장: 열을 늘리거나(그리드) 셀 렌더를 바꿔도 트리 로직은 그대로 재사용된다(추상 레벨 연결).

use super::{image_fit_contain, BorderSpec, Control, ControlBase, ScrollBars};
use crate::draw::{DrawCtx, FontSlot};
use crate::event::{InputEvent, Key};
use crate::geom::Rect;
use crate::theme::{IconImage, Theme};
use crate::tokens::{hover_alpha, HoverFade};
use crate::widget::{Invalidations, Widget};
use std::rc::Rc;

/// 트리 노드 — 라벨 + (그리드용) 추가 셀 값 + 자식 + 펼침 상태 + 선행 이미지(옵션).
#[derive(Clone, Debug)]
pub struct TreeNode {
    /// 트리 열 라벨.
    pub label: String,
    /// 그리드 추가 열 값(트리 열 제외 · TreeView는 무시).
    pub cells: Vec<String>,
    /// 자식 노드.
    pub children: Vec<TreeNode>,
    /// 펼침 여부.
    pub expanded: bool,
    /// 라벨 앞 이미지 아이콘(옵션 · 펼침 셰브론과 별개).
    pub image: Option<Rc<IconImage>>,
}

impl TreeNode {
    /// 잎 노드(자식 없음).
    #[must_use]
    pub fn leaf(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            cells: Vec::new(),
            children: Vec::new(),
            expanded: false,
            image: None,
        }
    }
    /// 자식 있는 노드(기본 펼침).
    #[must_use]
    pub fn branch(label: impl Into<String>, children: Vec<TreeNode>) -> Self {
        Self {
            label: label.into(),
            cells: Vec::new(),
            children,
            expanded: true,
            image: None,
        }
    }
    /// 그리드 셀 값 지정(체이닝).
    #[must_use]
    pub fn with_cells(mut self, cells: Vec<String>) -> Self {
        self.cells = cells;
        self
    }
    /// 선행 이미지 아이콘 지정(체이닝 · 셰브론과 별개).
    #[must_use]
    pub fn with_image(mut self, image: Rc<IconImage>) -> Self {
        self.image = Some(image);
        self
    }
}

/// 평탄화된 표시 행(현재 펼침 상태 기준).
#[derive(Clone, Debug)]
pub struct FlatRow {
    /// 루트부터의 자식 인덱스 경로(토글 대상).
    pub path: Vec<usize>,
    /// 깊이(들여쓰기).
    pub depth: usize,
    /// 트리 열 라벨.
    pub label: String,
    /// 그리드 셀 값.
    pub cells: Vec<String>,
    /// 자식 존재 여부(셰브론 표시 근거).
    pub has_children: bool,
    /// 펼침 여부.
    pub expanded: bool,
    /// 라벨 앞 이미지 아이콘(옵션).
    pub image: Option<Rc<IconImage>>,
}

/// 트리 계층 모델 — 노드 트리 + 펼침 상태(뷰와 무관 · TreeView/TreeGrid 공유).
#[derive(Clone, Debug, Default)]
pub struct TreeModel {
    /// 루트 노드들 — 바깥에서는 [`Self::roots`]/[`Self::roots_mut`]로만(변경 = 평탄화 캐시 무효화 · docs/37 P-1).
    roots: Vec<TreeNode>,
    /// ★ 평탄화 캐시(09-15 nexa-sql docs/37 P-1) — 4,899행 `flatten()`이 프레임당 3회 × 4.3ms였다.
    /// `Rc`로 돌려주므로 두 번째부터는 복사 0 · `roots_mut`/`toggle`/`set_expanded`가 비운다.
    flat: std::cell::RefCell<Option<Rc<Vec<FlatRow>>>>,
}

impl TreeModel {
    /// 루트 목록으로 만든다.
    #[must_use]
    pub fn new(roots: Vec<TreeNode>) -> Self {
        Self {
            roots,
            flat: std::cell::RefCell::new(None),
        }
    }

    /// 루트 노드들(읽기).
    #[must_use]
    pub fn roots(&self) -> &[TreeNode] {
        &self.roots
    }

    /// 루트 노드들(변경 — 평탄화 캐시를 비운다).
    pub fn roots_mut(&mut self) -> &mut Vec<TreeNode> {
        self.invalidate();
        &mut self.roots
    }

    /// 평탄화 캐시를 비운다(노드를 바깥에서 바꿨을 때).
    pub fn invalidate(&self) {
        *self.flat.borrow_mut() = None;
    }

    /// 현재 펼침 상태 기준 **보이는 행**을 평탄화한다(접힌 가지는 자식 제외 · 캐시).
    #[must_use]
    pub fn flatten(&self) -> Rc<Vec<FlatRow>> {
        if let Some(f) = self.flat.borrow().as_ref() {
            return Rc::clone(f);
        }
        let mut out = Vec::new();
        Self::walk(&self.roots, &mut Vec::new(), 0, &mut out);
        let rc = Rc::new(out);
        *self.flat.borrow_mut() = Some(Rc::clone(&rc));
        rc
    }

    fn walk(nodes: &[TreeNode], path: &mut Vec<usize>, depth: usize, out: &mut Vec<FlatRow>) {
        for (i, n) in nodes.iter().enumerate() {
            path.push(i);
            out.push(FlatRow {
                path: path.clone(),
                depth,
                label: n.label.clone(),
                cells: n.cells.clone(),
                has_children: !n.children.is_empty(),
                expanded: n.expanded,
                image: n.image.clone(),
            });
            if n.expanded && !n.children.is_empty() {
                Self::walk(&n.children, path, depth + 1, out);
            }
            path.pop();
        }
    }

    fn node_at_mut(&mut self, path: &[usize]) -> Option<&mut TreeNode> {
        let (&first, rest) = path.split_first()?;
        let mut node = self.roots.get_mut(first)?;
        for &i in rest {
            node = node.children.get_mut(i)?;
        }
        Some(node)
    }

    /// 경로의 노드 펼침을 토글한다.
    pub fn toggle(&mut self, path: &[usize]) {
        if let Some(n) = self.node_at_mut(path) {
            if !n.children.is_empty() {
                n.expanded = !n.expanded;
                self.invalidate();
            }
        }
    }

    /// 경로의 노드 펼침을 지정한다.
    pub fn set_expanded(&mut self, path: &[usize], on: bool) {
        if let Some(n) = self.node_at_mut(path) {
            if !n.children.is_empty() {
                n.expanded = on;
                self.invalidate();
            }
        }
    }
}

// 레이아웃 상수(논리 px).
const ROW_H: i32 = 24;
const INDENT: i32 = 16;
const CHEV_W: i32 = 16;
const HEADER_H: i32 = 26;

/// 트리 계층 인터페이스 — 평탄화·펼침/접기·선택을 기본 메서드로 상속.
pub trait TreeControl: Control {
    /// 계층 모델(구현 필수).
    fn model(&self) -> &TreeModel;
    /// 계층 모델 가변(구현 필수).
    fn model_mut(&mut self) -> &mut TreeModel;
    /// 선택 행 인덱스(가시 행 기준 · 구현 필수).
    fn selected_row(&self) -> usize;
    /// 선택 행 지정(구현 필수).
    fn set_selected_row(&mut self, i: usize);
    /// 스크롤 오프셋 `(x, y)` 물리 px(구현 필수).
    fn scroll(&self) -> (i32, i32);
    /// 스크롤 오프셋 지정(구현 필수).
    fn set_scroll(&mut self, x: i32, y: i32);
    /// 오버레이 스크롤바 상태(구현 필수).
    fn bars_mut(&mut self) -> &mut ScrollBars;
    /// ★ **커서가 올라간 행의 페이드 상태**(구현 필수 — 상태를 가지므로 기본 구현 불가).
    fn hover(&self) -> &HoverFade;
    /// 위와 같은 것의 가변 참조.
    fn hover_mut(&mut self) -> &mut HoverFade;

    /// hover 페이드 시간을 흘린다 — **밝기가 변했으면 `true`**(그때만 다시 그린다).
    ///
    /// 호스트의 프레임 틱에서 부른다([`TreeView::tick`]가 스크롤바 틱과 함께 묶어 준다).
    fn tick_hover(&mut self, now_ms: u64) -> bool {
        self.hover_mut().tick(now_ms)
    }

    /// 트리 영역의 top(헤더 아래 등 — 그리드가 재정의). 기본 = bounds.y.
    fn tree_top(&self) -> i32 {
        self.bounds().y
    }

    /// 행 클릭이 유효한 가로 폭(기본 = 전폭 · 그리드는 `fit_columns`면 열 합 — 그 밖은 **빈 공간**).
    fn hit_width(&self) -> i32 {
        self.bounds().w
    }

    /// 행이 그려지는 뷰포트(헤더 아래 · 스크롤 대상).
    fn rows_viewport(&self) -> Rect {
        let b = self.bounds();
        let top = self.tree_top();
        Rect::new(b.x, top, b.w, (b.bottom() - top).max(0))
    }

    /// 콘텐츠 총 크기 `(w, h)` — 세로=행수×행높이, 가로 기본=뷰포트 폭(그리드가 열 합으로 재정의).
    fn content_size(&self) -> (i32, i32) {
        let h = self.rows().len() as i32 * self.s(ROW_H);
        (self.rows_viewport().w, h)
    }

    /// 보이는 행(평탄화 캐시 · `Rc` 복사만).
    fn rows(&self) -> Rc<Vec<FlatRow>> {
        self.model().flatten()
    }

    /// 선택을 delta만큼 이동(경계 클램프).
    fn move_selection(&mut self, delta: i32, inv: &mut Invalidations) {
        let n = self.rows().len() as i32;
        if n == 0 {
            return;
        }
        let i = (self.selected_row() as i32 + delta).clamp(0, n - 1);
        self.set_selected_row(i as usize);
        self.reveal_row(i as usize);
        inv.push(self.bounds());
    }

    /// 행 `i`가 뷰포트 안에 들어오도록 세로 스크롤(키보드 이동 · 프로그램 선택 뒤 — nexa-sql 파일 창 사용자 09-22
    /// "선택만 화면 밖으로 사라지고 스크롤이 안 된다"). 뷰포트가 아직 없으면(배치 전) 그대로.
    fn reveal_row(&mut self, i: usize) {
        let vp = self.rows_viewport();
        if vp.h <= 0 {
            return;
        }
        let rh = self.s(ROW_H).max(1);
        let (sx, sy) = self.scroll();
        let top = i as i32 * rh;
        let bottom = top + rh;
        let mut ny = if top < sy {
            top
        } else if bottom > sy + vp.h {
            bottom - vp.h
        } else {
            sy
        };
        let (_, ch) = self.content_size();
        ny = ny.clamp(0, (ch - vp.h).max(0));
        if ny != sy {
            self.set_scroll(sx, ny);
        }
    }

    /// 한 페이지의 행 수(PageUp/PageDown · 최소 1).
    fn page_rows(&self) -> i32 {
        (self.rows_viewport().h / self.s(ROW_H).max(1)).max(1)
    }

    /// 가시 행 i의 펼침 토글.
    fn toggle_row(&mut self, i: usize, inv: &mut Invalidations) {
        if let Some(row) = self.rows().get(i) {
            let path = row.path.clone();
            self.model_mut().toggle(&path);
            inv.push(self.bounds());
        }
    }

    /// →(펼침) / ←(접힘) — 선택 행 기준. ←는 **펼쳐진 폴더면 접고, 아니면(잎·접힌 폴더) 상위 폴더로 이동**(파일 탐색기 관례 ·
    /// Windows 탐색기·VS Code · nexa-sql 사용자 09-19 "좌측 이동이 미동작").
    fn expand_selected(&mut self, on: bool, inv: &mut Invalidations) {
        let i = self.selected_row();
        let rows = self.rows();
        let Some(row) = rows.get(i) else { return };
        if on {
            if row.has_children && !row.expanded {
                let path = row.path.clone();
                self.model_mut().set_expanded(&path, true);
                inv.push(self.bounds());
            }
            return;
        }
        if row.has_children && row.expanded {
            let path = row.path.clone();
            self.model_mut().set_expanded(&path, false);
            inv.push(self.bounds());
            return;
        }
        // 상위 = 위쪽에서 처음 만나는 얕은 행.
        let depth = row.depth;
        if let Some(parent) = (0..i).rev().find(|&j| rows[j].depth < depth) {
            self.set_selected_row(parent);
            inv.push(self.bounds());
        }
    }

    /// (x,y) → (가시 행 인덱스, 셰브론을 눌렀는가). 스크롤 오프셋 반영.
    fn row_hit(&self, x: i32, y: i32) -> Option<(usize, bool)> {
        let rh = self.s(ROW_H).max(1);
        let top = self.tree_top();
        let (sx, sy) = self.scroll();
        // ★ x·y 둘 다 검사(09-15 nexa-sql 설정 창: 오른쪽 카드 클릭이 왼쪽 트리 행을 선택하던 결함 — y만 보고 있었다).
        let b = self.bounds();
        if y < top || y >= b.bottom() || x < b.x || x >= b.right() || x >= b.x + self.hit_width() {
            return None;
        }
        let i = ((y - top + sy) / rh) as usize;
        let rows = self.rows();
        let row = rows.get(i)?;
        // 셰브론 영역: 깊이 들여쓰기 지점(가로 스크롤 반영).
        let chev_x = self.bounds().x + self.s(4) + self.s(INDENT) * row.depth as i32 - sx;
        let on_chev = row.has_children && x >= chev_x && x < chev_x + self.s(CHEV_W);
        Some((i, on_chev))
    }

    /// 트리 열 한 칸을 그린다(들여쓰기 + 셰브론 + 라벨) — TreeView 전체 / TreeGrid 첫 열 공용.
    /// `cell` = 기하(가로 스크롤만큼 왼쪽으로 밀린 칸) · `clip` = 실제로 칠할 수 있는 영역(컨트롤 안쪽).
    /// ★ 둘을 나눈 이유: 가로 스크롤 시 `cell`이 컨트롤 왼쪽 밖으로 나가는데 그대로 클립으로 쓰면 글자·아이콘·셰브론이
    ///   이웃 컨트롤(파일 대화상자 사이드바) 위에 그려진다(09-16 mac 캡처).
    #[allow(clippy::too_many_arguments)] // 기하(cell)·클립(clip)·상태(selected/hover) — 호출 2곳 · 구조체화는 과함
    fn paint_tree_cell(
        &self,
        ctx: &mut dyn DrawCtx,
        theme: &Theme,
        row: &FlatRow,
        cell: Rect,
        clip: Rect,
        selected: bool,
        hover: f32,
    ) {
        let cell_vis = cell.intersection(&clip);
        if selected {
            ctx.fill_rect(
                cell_vis,
                if self.is_active() {
                    theme.sel_bg
                } else {
                    theme.sel_bg_inactive
                },
            );
        }
        // ★ hover — **색을 새로 만들지 않고** 전경색을 알파로 덮는다([docs/25 §3-4]).
        //   진행도(0~1)를 곱하므로 **서서히** 밝아진다(사용자 확정 08-26).
        let a = hover_alpha(selected, hover);
        if a > 0.0 {
            ctx.fill_rect_alpha(cell_vis, theme.text, a);
        }
        let chev_x = cell.x + self.s(4) + self.s(INDENT) * row.depth as i32;
        // dir2 파일 그리드와 같은 90° 셰브론 · 크기 = 글꼴 높이 · 접힘 = 흐림 · 펼침/hover = 본문색(사용자 09-15).
        ctx.select_font(FontSlot::Base, false);
        let cw = ctx.text_height().max(self.s(CHEV_W));
        let cy = cell.y + (cell.h - cw) / 2;
        let chev = Rect::new(chev_x, cy, cw, cw);
        // 셰브론은 선분이라 클립을 못 받는다 — 온전히 안에 들 때만 그린다.
        if row.has_children && chev.intersection(&clip) == chev {
            let color = if row.expanded || hover > 0.0 {
                theme.text
            } else {
                theme.text_dim
            };
            super::draw_chevron_90(ctx, chev, color, row.expanded);
        }
        let mut tx = chev.right() + self.s(4);
        // 선행 이미지(옵션 · 셰브론과 별개) — 공용 아이콘 크기(콤보/버튼과 동일 원천).
        if let Some(img) = row.image.as_deref() {
            // 16px급 원본(OS 셸 아이콘)은 **원본 크기**로(13px로 줄이면 흐려진다) · 더 큰 그림은 공용 크기로 맞춘다.
            let native = img.w.min(img.h).min(16) as i32;
            let isz = self.s(super::LEADING_ICON).max(self.s(native));
            let boxr = Rect::new(tx, cell.y + (cell.h - isz) / 2, isz, isz);
            let fit = image_fit_contain(boxr, img.w as i32, img.h as i32);
            ctx.image_scaled(fit, img, cell_vis);
            tx += isz + self.s(4);
        }
        ctx.select_font(FontSlot::Base, false);
        let ty = ctx.text_center_y(cell.y, cell.h);
        ctx.text(tx, ty, cell_vis, &row.label, theme.text);
    }
}

/// 공통 이벤트 처리(TreeView/TreeGrid 공용).
fn tree_event<T: TreeControl + ?Sized>(t: &mut T, ev: &InputEvent, inv: &mut Invalidations) {
    // 오버레이 스크롤바 먼저(휠·드래그·호버). 소비되면 콘텐츠로 넘기지 않는다.
    let vp = t.rows_viewport();
    let (cw, ch) = t.content_size();
    let (sx, sy) = t.scroll();
    let scale = t.base().scale;
    let (nx, ny, consumed) = t.bars_mut().on_event(ev, vp, cw, ch, sx, sy, scale);
    if nx != sx || ny != sy {
        t.set_scroll(nx, ny);
        inv.push(t.bounds());
    }
    if consumed || matches!(ev, InputEvent::MouseMove { .. }) {
        inv.push(t.bounds());
    }
    // ★ hover 대상 갱신 — 스크롤바가 먹은 이동이면 행 hover는 **끈다**
    //   (막대 위에 있는데 아래 행이 밝아지면 어디를 가리키는지 흐려진다).
    if let InputEvent::MouseMove { x, y } = *ev {
        let target = if consumed {
            None
        } else {
            t.row_hit(x, y).map(|(i, _)| i)
        };
        t.hover_mut().set(target);
    }
    if consumed {
        return;
    }
    match *ev {
        InputEvent::MouseDown { x, y, .. } => {
            if let Some((i, on_chev)) = t.row_hit(x, y) {
                t.set_selected_row(i);
                if on_chev {
                    t.toggle_row(i, inv);
                } else {
                    inv.push(t.bounds());
                }
            }
        }
        InputEvent::Key { key, .. } if t.is_focused() => match key {
            Key::Up => t.move_selection(-1, inv),
            Key::Down => t.move_selection(1, inv),
            Key::PageUp => {
                let p = t.page_rows();
                t.move_selection(-p, inv);
            }
            Key::PageDown => {
                let p = t.page_rows();
                t.move_selection(p, inv);
            }
            Key::Home => {
                let n = t.rows().len() as i32;
                t.move_selection(-n, inv);
            }
            Key::End => {
                let n = t.rows().len() as i32;
                t.move_selection(n, inv);
            }
            Key::Right => t.expand_selected(true, inv),
            Key::Left => t.expand_selected(false, inv),
            Key::Enter | Key::Space => {
                let i = t.selected_row();
                t.toggle_row(i, inv);
            }
            _ => {}
        },
        _ => {}
    }
}

// ───────────────────────────── TreeView(단일 열) ─────────────────────────────

/// 트리뷰 — 단일 열 계층 목록.
#[derive(Debug)]
pub struct TreeView {
    base: ControlBase,
    model: TreeModel,
    selected: usize,
    scroll_x: i32,
    scroll_y: i32,
    bars: ScrollBars,
    border: BorderSpec,
    /// ★ 커서가 올라간 행 — 서서히 밝아진다.
    hover: HoverFade,
}

impl TreeView {
    /// 모델로 만든다.
    #[must_use]
    pub fn new(model: TreeModel) -> Self {
        Self {
            base: ControlBase::default(),
            model,
            selected: 0,
            scroll_x: 0,
            scroll_y: 0,
            bars: ScrollBars::new(),
            border: BorderSpec::default(),
            hover: HoverFade::default(),
        }
    }

    /// 외곽 테두리 설정(두께·색·투명도 · 두께 0 = 없음).
    pub fn set_border(&mut self, border: BorderSpec) {
        self.border = border;
    }
    /// 선택 행의 라벨.
    #[must_use]
    pub fn selected_label(&self) -> Option<String> {
        self.rows().get(self.selected).map(|r| r.label.clone())
    }

    /// 스크롤바 자동숨김 + ★ **hover 페이드** 틱 — 다시 그려야 하면 `true`.
    /// `now_ms`는 호스트 시계(단조).
    pub fn tick(&mut self, now_ms: u64) -> bool {
        // ⚠️ `||`로 묶으면 앞이 참일 때 뒤가 **안 돈다** — 둘 다 시간을 흘려야 한다.
        let bars = self.bars.tick(now_ms);
        let hover = self.tick_hover(now_ms);
        bars || hover
    }
}

impl Control for TreeView {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
}
impl TreeControl for TreeView {
    fn model(&self) -> &TreeModel {
        &self.model
    }
    fn model_mut(&mut self) -> &mut TreeModel {
        &mut self.model
    }
    fn selected_row(&self) -> usize {
        self.selected
    }
    fn set_selected_row(&mut self, i: usize) {
        self.selected = i;
    }
    fn scroll(&self) -> (i32, i32) {
        (self.scroll_x, self.scroll_y)
    }
    fn set_scroll(&mut self, x: i32, y: i32) {
        self.scroll_x = x;
        self.scroll_y = y;
    }
    fn bars_mut(&mut self) -> &mut ScrollBars {
        &mut self.bars
    }
    fn hover(&self) -> &HoverFade {
        &self.hover
    }
    fn hover_mut(&mut self) -> &mut HoverFade {
        &mut self.hover
    }
}

impl Widget for TreeView {
    fn bounds(&self) -> Rect {
        self.base.bounds
    }
    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        self.base.bounds = bounds;
        inv.push(bounds);
    }
    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        tree_event(self, ev, inv);
    }
    fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.base.bounds;
        ctx.fill_rect(b, theme.panel_bg);
        let rh = self.s(ROW_H);
        let top = self.tree_top();
        let bottom = b.bottom();
        // 스크롤 오프셋 반영 · 뷰포트 안의 온전한 행만 — **첫 가시 행부터**(docs/37 P-2 · 전체 순회 금지).
        let rows = self.rows();
        let first = ((self.scroll_y / rh.max(1)).max(0) as usize).min(rows.len());
        let count = ((bottom - top) / rh.max(1)).max(0) as usize + 2;
        for (i, row) in rows.iter().enumerate().skip(first).take(count) {
            let ry = top - self.scroll_y + rh * i as i32;
            if ry < top || ry + rh > bottom {
                continue;
            }
            let cell = Rect::new(b.x - self.scroll_x, ry, b.w + self.scroll_x, rh);
            self.paint_tree_cell(
                ctx,
                theme,
                row,
                cell,
                b,
                i == self.selected,
                self.hover.value(i),
            );
        }
        let (cw, ch) = self.content_size();
        self.bars.paint(
            ctx,
            theme,
            self.rows_viewport(),
            cw,
            ch,
            self.scroll_x,
            self.scroll_y,
            self.base.scale,
        );
        draw_border(ctx, b, self.border, self.base.scale);
    }
}

/// 외곽 테두리 그리기(두께 0이면 생략) — 트리/그리드 공용.
fn draw_border(ctx: &mut dyn DrawCtx, b: Rect, border: BorderSpec, scale: f32) {
    if border.width <= 0.0 {
        return;
    }
    let w = (border.width * scale).max(0.5); // 소수 두께 허용(0.5px 얇은 선)
    ctx.stroke_round_rect_alpha(b, 0, border.color, w, border.alpha);
}

// ───────────────────────────── TreeGrid(그리드+트리) ─────────────────────────────

/// 그리드 열 정의.
#[derive(Clone, Debug)]
pub struct GridColumn {
    /// 헤더 제목.
    pub title: String,
    /// 열 폭(논리 px).
    pub width: i32,
    /// 헤더 오른쪽 끝 배지(정렬 ▲/▼ + 결합 순번 · accent 색 · nexa-sql 결과 그리드와 같은 모양 · 09-15).
    pub badge: Option<String>,
}

impl GridColumn {
    /// 배지 붙이기(빌더 · 빈 문자열 = 없음).
    #[must_use]
    pub fn with_badge(mut self, badge: impl Into<String>) -> Self {
        let b: String = badge.into();
        self.badge = (!b.is_empty()).then_some(b);
        self
    }

    /// (제목, 폭).
    pub fn new(title: impl Into<String>, width: i32) -> Self {
        Self {
            title: title.into(),
            width,
            badge: None,
        }
    }
}

/// 트리 그리드 — 첫 열이 트리, 나머지는 셀 값(같은 [`TreeModel`] 재사용).
#[derive(Debug)]
pub struct TreeGrid {
    base: ControlBase,
    model: TreeModel,
    selected: usize,
    /// 선택이 있는가 — 빈 곳 클릭으로 해제하면 false(캐럿 행 `selected`는 남겨 키 이동의 기준으로 · nexa-dlg 09-22).
    has_sel: bool,
    /// 열 정의(첫 열 = 트리 열).
    columns: Vec<GridColumn>,
    scroll_x: i32,
    scroll_y: i32,
    bars: ScrollBars,
    border: BorderSpec,
    /// ★ 커서가 올라간 행 — 서서히 밝아진다.
    hover: HoverFade,
    /// 선택·hover·클릭을 **열 합 폭까지만**(그 밖은 빈 공간 · 파일 대화상자 · 사용자 09-15).
    fit_columns: bool,
    /// ★ **표시된 행**(다중 선택 · nexa-dlg 열기 모드 · 09-22): 선택 행이 아니어도 선택 배경을 칠한다. 열쇠 = 노드 경로(`FlatRow::path`)
    /// — 가시 행 인덱스가 아니라서 다른 폴더를 펼치거나 접어 행이 밀려도 표시가 따라간다(사용자 09-22 실기).
    marked: std::collections::HashSet<Vec<usize>>,
    /// 다중 선택 모드: 캐럿 행은 채우지 않고 **테두리**만(선택 = `marked`) — 폴더가 "기본 선택"처럼 보이지 않게(사용자 09-22).
    caret_outline: bool,
}

impl TreeGrid {
    /// 선택·hover·클릭 범위를 열 합 폭으로 제한(기본 = 전폭).
    pub fn set_fit_columns(&mut self, on: bool) {
        self.fit_columns = on;
    }

    /// 모델 + 열 정의로 만든다(첫 열이 트리 열).
    #[must_use]
    pub fn new(model: TreeModel, columns: Vec<GridColumn>) -> Self {
        Self {
            base: ControlBase::default(),
            model,
            selected: 0,
            has_sel: true,
            columns,
            scroll_x: 0,
            scroll_y: 0,
            bars: ScrollBars::new(),
            border: BorderSpec::default(),
            hover: HoverFade::default(),
            fit_columns: false,
            marked: std::collections::HashSet::new(),
            caret_outline: false,
        }
    }

    /// 다중 선택 모드(캐럿 = 테두리 · 채움은 `set_marked_paths`만).
    pub fn set_caret_outline(&mut self, on: bool) {
        self.caret_outline = on;
    }

    /// 다중 선택 표시 — 노드 경로 목록(`FlatRow::path` · 빈 목록 = 없음).
    pub fn set_marked_paths(&mut self, paths: Vec<Vec<usize>>) {
        self.marked = paths.into_iter().collect();
    }

    /// 가시 행 `i`가 다중 선택에 들어 있는가.
    #[must_use]
    pub fn is_marked(&self, i: usize) -> bool {
        self.rows()
            .get(i)
            .is_some_and(|r| self.marked.contains(&r.path))
    }

    /// 외곽 테두리 설정(두께·색·투명도 · 두께 0 = 없음).
    pub fn set_border(&mut self, border: BorderSpec) {
        self.border = border;
    }

    /// 열 폭 변경(논리 px · 헤더 경계 드래그 — 모델·스크롤을 유지한 채 폭만).
    pub fn set_column_width(&mut self, i: usize, width: i32) {
        if let Some(c) = self.columns.get_mut(i) {
            c.width = width.max(24);
        }
    }

    /// 전체 열 폭 합(물리 px).
    fn columns_width(&self) -> i32 {
        self.columns.iter().map(|c| self.s(c.width)).sum()
    }

    /// 스크롤바 자동숨김 틱 — 표시 상태 변화 시 `true`. `now_ms`는 호스트 시계(단조).
    pub fn tick(&mut self, now_ms: u64) -> bool {
        // ⚠️ `||`로 묶으면 앞이 참일 때 뒤가 **안 돈다** — 둘 다 시간을 흘려야 한다.
        let bars = self.bars.tick(now_ms);
        let hover = self.tick_hover(now_ms);
        bars || hover
    }
}

impl Control for TreeGrid {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
}
impl TreeGrid {
    /// 선택 해제(빈 곳 클릭) — 강조 행 0 · 키 이동은 마지막 캐럿 행에서 이어진다.
    pub fn clear_selection(&mut self) {
        self.has_sel = false;
    }

    /// 선택된 행이 있는가(`clear_selection` 뒤 false · `set_selected_row`/키 이동으로 다시 true).
    pub fn has_selection(&self) -> bool {
        self.has_sel
    }
}

impl TreeControl for TreeGrid {
    fn model(&self) -> &TreeModel {
        &self.model
    }
    fn model_mut(&mut self) -> &mut TreeModel {
        &mut self.model
    }
    fn selected_row(&self) -> usize {
        self.selected
    }
    fn set_selected_row(&mut self, i: usize) {
        self.selected = i;
        self.has_sel = true;
    }
    fn scroll(&self) -> (i32, i32) {
        (self.scroll_x, self.scroll_y)
    }
    fn set_scroll(&mut self, x: i32, y: i32) {
        self.scroll_x = x;
        self.scroll_y = y;
    }
    fn bars_mut(&mut self) -> &mut ScrollBars {
        &mut self.bars
    }
    fn hover(&self) -> &HoverFade {
        &self.hover
    }
    fn hover_mut(&mut self) -> &mut HoverFade {
        &mut self.hover
    }
    /// 그리드는 헤더 아래부터 트리 행.
    fn tree_top(&self) -> i32 {
        self.base.bounds.y + self.s(HEADER_H)
    }
    fn hit_width(&self) -> i32 {
        if self.fit_columns {
            (self.columns_width() - self.scroll_x).clamp(0, self.base.bounds.w)
        } else {
            self.base.bounds.w
        }
    }
    /// 가로 콘텐츠 = 전체 열 폭 합(길면 좌우 스크롤).
    fn content_size(&self) -> (i32, i32) {
        let h = self.rows().len() as i32 * self.s(ROW_H);
        (self.columns_width(), h)
    }
}

impl Widget for TreeGrid {
    fn bounds(&self) -> Rect {
        self.base.bounds
    }
    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        self.base.bounds = bounds;
        inv.push(bounds);
    }
    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        tree_event(self, ev, inv);
    }
    fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.base.bounds;
        ctx.fill_rect(b, theme.panel_bg);
        let ox = self.scroll_x; // 가로 스크롤(헤더·셀 공통 이동)

        // 헤더(가로 스크롤 반영 · b.w로 텍스트 클립). 텍스트는 헤더 높이의 세로 중앙.
        let header = Rect::new(b.x, b.y, b.w, self.s(HEADER_H));
        ctx.fill_rect(header, theme.chrome_bg);
        let mut cx = b.x - ox;
        ctx.select_font(FontSlot::Status, false);
        let hty = ctx.text_center_y(header.y, header.h); // 잉크 기준 세로 가운데(09-16)
        for col in &self.columns {
            let w = self.s(col.width);
            let cell = Rect::new(cx, header.y, w, header.h).intersection(&header);
            // 배지(정렬 표시) — 오른쪽 끝 · accent · 제목은 배지 폭만큼 잘라 그린다.
            let title_clip = if let Some(bd) = &col.badge {
                let bw = ctx.text_width(bd);
                ctx.text(cx + w - self.s(8) - bw, hty, cell, bd, theme.accent);
                Rect::new(cx, header.y, (w - bw - self.s(16)).max(0), header.h)
                    .intersection(&header)
            } else {
                cell
            };
            ctx.text(cx + self.s(8), hty, title_clip, &col.title, theme.text_dim);
            cx += w;
            ctx.fill_rect(Rect::new(cx - 1, header.y, 1, b.h), theme.border);
        }
        ctx.fill_rect(Rect::new(b.x, header.bottom() - 1, b.w, 1), theme.border);

        // 행(세로 스크롤 반영 · 온전한 행만 · 가로 스크롤 반영).
        let rh = self.s(ROW_H);
        let top = self.tree_top();
        let bottom = b.bottom();
        let tree_w = self.columns.first().map_or(b.w, |c| self.s(c.width));
        // 선택·hover 폭 — 열 합까지만(`fit_columns`) 또는 전폭.
        let row_w = self.hit_width();
        // 본문 클립 — 가로 스크롤로 왼쪽 밖에 나간 부분은 그리지 않는다(09-16).
        let body = Rect::new(b.x, top, b.w, (bottom - top).max(0));
        let rows = self.rows();
        let first = ((self.scroll_y / rh.max(1)).max(0) as usize).min(rows.len());
        let count = ((bottom - top) / rh.max(1)).max(0) as usize + 2;
        for (i, row) in rows.iter().enumerate().skip(first).take(count) {
            let y = top - self.scroll_y + rh * i as i32;
            if y < top || y + rh > bottom {
                continue;
            }
            let fill = self.marked.contains(&row.path)
                || (self.has_sel && i == self.selected && !self.caret_outline);
            if fill {
                ctx.fill_rect(
                    Rect::new(b.x, y, row_w, rh),
                    if self.is_active() {
                        theme.sel_bg
                    } else {
                        theme.sel_bg_inactive
                    },
                );
            }
            if self.caret_outline && self.has_sel && i == self.selected && self.is_active() {
                ctx.stroke_round_rect(Rect::new(b.x, y, row_w, rh), 0, theme.accent, 1.0);
            }
            // ★ 선택 없음(빈 곳 클릭) + 캐럿 행 = 배경 없이 **테두리만**(키보드 이동의 기준점 · nexa-sql 사용자 09-23) — 흐린 색.
            if !self.has_sel && i == self.selected && self.is_active() {
                ctx.stroke_round_rect(Rect::new(b.x, y, row_w, rh), 0, theme.text_dim, 1.0);
            }
            // ★ hover는 **행 전체**에 얹는다(첫 열만 밝아지면 행이 잘려 보인다).
            let a = hover_alpha(self.has_sel && i == self.selected, self.hover.value(i));
            if a > 0.0 {
                ctx.fill_rect_alpha(Rect::new(b.x, y, row_w, rh), theme.text, a);
            }
            // 첫 열 = 트리 셀(배경·hover 재도색 방지 — 위에서 이미 얹었다).
            let tree_cell = Rect::new(b.x - ox, y, tree_w, rh);
            self.paint_tree_cell(ctx, theme, row, tree_cell, body, false, 0.0);
            // 나머지 열 = 셀 값.
            let mut colx = b.x + tree_w - ox;
            for (ci, col) in self.columns.iter().enumerate().skip(1) {
                let w = self.s(col.width);
                if let Some(val) = row.cells.get(ci - 1) {
                    ctx.select_font(FontSlot::Base, false);
                    let ty = ctx.text_center_y(y, rh);
                    ctx.text(
                        colx + self.s(8),
                        ty,
                        Rect::new(colx, y, w, rh).intersection(&body),
                        val,
                        theme.text,
                    );
                }
                colx += w;
            }
        }

        // 오버레이 스크롤바.
        let (cw, ch) = self.content_size();
        self.bars.paint(
            ctx,
            theme,
            self.rows_viewport(),
            cw,
            ch,
            self.scroll_x,
            self.scroll_y,
            self.base.scale,
        );
        draw_border(ctx, b, self.border, self.base.scale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> TreeModel {
        TreeModel::new(vec![
            TreeNode::branch(
                "Path Finder",
                vec![
                    TreeNode::leaf("About Path Finder"),
                    TreeNode::branch("Trash", vec![TreeNode::leaf("Empty Trash")]),
                ],
            ),
            TreeNode::leaf("Show Desktop"),
        ])
    }

    fn view() -> (TreeView, Invalidations) {
        let mut v = TreeView::new(model());
        let mut inv = Invalidations::default();
        v.set_bounds(Rect::new(0, 0, 300, 300), &mut inv);
        (v, inv)
    }
    fn click(x: i32, y: i32) -> InputEvent {
        InputEvent::MouseDown {
            x,
            y,
            shift: false,
            primary: false,
        }
    }
    fn key(k: Key) -> InputEvent {
        InputEvent::Key {
            key: k,
            shift: false,
            primary: false,
        }
    }

    #[test]
    fn flatten_cache_is_reused_and_invalidated() {
        // docs/37 P-1 — 두 번째 호출은 같은 Rc · toggle/roots_mut 뒤에는 새로 만든다.
        let mut m = TreeModel::new(vec![TreeNode::branch("a", vec![TreeNode::leaf("b")])]);
        let f1 = m.flatten();
        let f2 = m.flatten();
        assert!(Rc::ptr_eq(&f1, &f2), "캐시 재사용");
        assert_eq!(f1.len(), 2, "branch는 기본 펼침(a, b)");
        m.toggle(&[0]);
        let f3 = m.flatten();
        assert!(!Rc::ptr_eq(&f1, &f3), "toggle = 무효화");
        assert_eq!(f3.len(), 1);
        m.roots_mut().push(TreeNode::leaf("c"));
        assert_eq!(m.flatten().len(), 2, "roots_mut = 무효화");
    }

    #[test]
    fn flatten_respects_expansion() {
        let m = model();
        // 기본: Path Finder(펼침) → About, Trash(펼침) → Empty Trash, 그리고 Show Desktop = 5행.
        assert_eq!(m.flatten().len(), 5);
    }

    #[test]
    fn collapse_hides_children() {
        let mut m = model();
        m.toggle(&[0]); // Path Finder 접기
        let rows = m.flatten();
        // Path Finder + Show Desktop만.
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "Path Finder");
        assert!(!rows[0].expanded);
    }

    #[test]
    fn click_outside_horizontal_bounds_is_ignored() {
        let mut v = TreeView::new(TreeModel::new(vec![
            TreeNode::branch("a", vec![TreeNode::leaf("a1")]),
            TreeNode::leaf("b"),
        ]));
        let mut inv = Invalidations::default();
        v.set_bounds(Rect::new(10, 10, 200, 300), &mut inv);
        let before = v.selected_row();
        // 트리 오른쪽 바깥(x=500) · 둘째 행 높이 → 무시돼야 한다.
        v.on_event(
            &InputEvent::MouseDown {
                x: 500,
                y: 10 + 24 + 5,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(v.selected_row(), before);
    }

    #[test]
    fn click_chevron_toggles() {
        let (mut v, mut inv) = view();
        assert_eq!(v.rows().len(), 5);
        // Path Finder 행(0)의 셰브론 클릭 — depth 0, chev_x ≈ x+4.
        v.on_event(&click(6, 6), &mut inv);
        assert_eq!(v.rows().len(), 2, "접힘");
    }

    #[test]
    fn keyboard_navigates_and_expands() {
        let (mut v, mut inv) = view();
        v.set_focused(true);
        v.on_event(&key(Key::Down), &mut inv);
        assert_eq!(v.selected_row(), 1);
        // 선택을 Path Finder(0)로 되돌려 접기.
        v.on_event(&key(Key::Up), &mut inv);
        v.on_event(&key(Key::Left), &mut inv);
        assert_eq!(v.rows().len(), 2, "← 접힘");
        v.on_event(&key(Key::Right), &mut inv);
        assert_eq!(v.rows().len(), 5, "→ 펼침");
        // ← 규칙(09-19): 잎/접힌 행에서는 상위로 · 펼쳐진 행에서는 접기.
        v.on_event(&key(Key::Down), &mut inv);
        assert_eq!(v.selected_row(), 1, "자식 잎 행");
        v.on_event(&key(Key::Left), &mut inv);
        assert_eq!(v.selected_row(), 0, "← 잎에서 = 상위 폴더로");
        assert_eq!(v.rows().len(), 5, "상위로 갈 때는 접지 않는다");
        v.on_event(&key(Key::Left), &mut inv);
        assert_eq!(v.rows().len(), 2, "← 펼쳐진 상위에서 = 접기");
        v.on_event(&key(Key::Left), &mut inv);
        assert_eq!(v.selected_row(), 0, "루트에서 ← = 그대로");
    }

    #[test]
    fn tree_grid_shares_model_and_adds_columns() {
        let m = TreeModel::new(vec![TreeNode::branch(
            "Path Finder",
            vec![TreeNode::leaf("Settings…").with_cells(vec!["⌘,".into()])],
        )]);
        let cols = vec![
            GridColumn::new("Menu", 200),
            GridColumn::new("Command", 100),
        ];
        let mut g = TreeGrid::new(m, cols);
        let mut inv = Invalidations::default();
        g.set_bounds(Rect::new(0, 0, 300, 300), &mut inv);
        // 같은 평탄화 로직 재사용.
        assert_eq!(g.rows().len(), 2);
        assert_eq!(g.rows()[1].cells, vec!["⌘,".to_string()]);
        // 헤더 아래부터 트리 행.
        assert!(g.tree_top() > g.bounds().y);
    }

    /// 가로 스크롤 시 그리기 클립이 컨트롤 왼쪽 밖으로 나가지 않는다(09-16 mac 캡처: 파일 목록이 사이드바 위에 겹침).
    #[test]
    fn horizontal_scroll_never_paints_left_of_bounds() {
        use crate::theme::Color;
        #[derive(Default)]
        struct Rec {
            clips: Vec<Rect>,
            fills: Vec<Rect>,
        }
        impl DrawCtx for Rec {
            fn fill_rect(&mut self, r: Rect, _c: Color) {
                self.fills.push(r);
            }
            fn text_opaque(
                &mut self,
                _x: i32,
                _y: i32,
                clip: Rect,
                _t: &str,
                _f: Color,
                _b: Color,
            ) {
                self.clips.push(clip);
            }
            fn text(&mut self, _x: i32, _y: i32, clip: Rect, _t: &str, _f: Color) {
                self.clips.push(clip);
            }
            fn text_width(&mut self, text: &str) -> i32 {
                text.chars().count() as i32 * 7
            }
        }
        let m = TreeModel::new(vec![TreeNode::branch(
            "a long folder name that scrolls",
            vec![TreeNode::leaf("child").with_cells(vec!["cell".into()])],
        )]);
        let mut g = TreeGrid::new(
            m,
            vec![GridColumn::new("Name", 400), GridColumn::new("Size", 100)],
        );
        let mut inv = Invalidations::default();
        let b = Rect::new(200, 0, 150, 300);
        g.set_bounds(b, &mut inv);
        g.set_scroll(120, 0);
        let mut rec = Rec::default();
        g.paint(&mut rec, &Theme::dark());
        assert!(!rec.clips.is_empty());
        for c in &rec.clips {
            assert!(
                c.w == 0 || c.x >= b.x,
                "클립이 컨트롤 왼쪽 밖: {c:?} (bounds {b:?})"
            );
        }
        for f in rec.fills.iter().filter(|f| f.w > 0) {
            assert!(f.x >= b.x - 1, "채우기가 컨트롤 왼쪽 밖: {f:?}");
        }
    }

    /// 키보드 ↓/PageDown/End로 선택이 뷰포트 밖으로 가면 스크롤이 따라온다(↑/Home으로 돌아오면 0) — nexa-sql 파일 창 09-22.
    #[test]
    fn keyboard_selection_scrolls_into_view() {
        let nodes: Vec<TreeNode> = (0..40)
            .map(|i| TreeNode::leaf(format!("row {i}")))
            .collect();
        let mut v = TreeView::new(TreeModel::new(nodes));
        let mut inv = Invalidations::default();
        v.set_bounds(Rect::new(0, 0, 200, 100), &mut inv);
        v.set_focused(true);
        let key = |k: Key| InputEvent::Key {
            key: k,
            shift: false,
            primary: false,
        };
        for _ in 0..10 {
            v.on_event(&key(Key::Down), &mut inv);
        }
        assert_eq!(v.selected_row(), 10);
        let (_, sy) = v.scroll();
        let rh = 24;
        assert!(
            sy > 0 && 10 * rh + rh <= sy + 100,
            "선택 행이 뷰포트 안: sy={sy}"
        );
        v.on_event(&key(Key::End), &mut inv);
        assert_eq!(v.selected_row(), 39);
        let (_, sy) = v.scroll();
        assert_eq!(sy, 40 * rh - 100, "끝 = 최대 스크롤");
        v.on_event(&key(Key::PageUp), &mut inv);
        assert_eq!(v.selected_row(), 39 - 4);
        v.on_event(&key(Key::Home), &mut inv);
        assert_eq!((v.selected_row(), v.scroll().1), (0, 0));
    }

    #[test]
    fn many_rows_scroll_vertically() {
        // 행이 많고 뷰포트가 작으면 세로 스크롤.
        let nodes: Vec<TreeNode> = (0..40)
            .map(|i| TreeNode::leaf(format!("row {i}")))
            .collect();
        let mut v = TreeView::new(TreeModel::new(nodes));
        let mut inv = Invalidations::default();
        v.set_bounds(Rect::new(0, 0, 200, 100), &mut inv);
        let (_cw, ch) = v.content_size();
        assert!(ch > 100, "콘텐츠가 뷰포트보다 큼(40행×24=960)");
        v.set_focused(true);
        v.on_event(&InputEvent::Wheel { delta: -300 }, &mut inv);
        assert_eq!(v.scroll(), (0, 100), "휠 세로 스크롤");
    }

    #[test]
    fn wide_columns_scroll_horizontally() {
        let m = TreeModel::new(vec![
            TreeNode::leaf("A").with_cells(vec!["a".into()]),
            TreeNode::leaf("B").with_cells(vec!["b".into()]),
        ]);
        let cols = vec![
            GridColumn::new("Menu", 300),
            GridColumn::new("Command", 200),
        ];
        let mut g = TreeGrid::new(m, cols);
        let mut inv = Invalidations::default();
        g.set_bounds(Rect::new(0, 0, 300, 300), &mut inv); // 창(300) < 열 합(500)
        let (cw, _ch) = g.content_size();
        assert_eq!(cw, 500, "열 폭 합");
        g.on_event(&InputEvent::HWheel { delta: 300 }, &mut inv);
        assert_eq!(g.scroll().0, 100, "가로 스크롤(delta/3)");
    }

    #[test]
    fn tree_grid_row_hit_accounts_for_header() {
        let m = TreeModel::new(vec![TreeNode::leaf("A"), TreeNode::leaf("B")]);
        let mut g = TreeGrid::new(m, vec![GridColumn::new("Menu", 200)]);
        let mut inv = Invalidations::default();
        g.set_bounds(Rect::new(0, 0, 300, 300), &mut inv);
        g.set_focused(true);
        // 헤더 영역 클릭은 행 아님.
        g.on_event(&click(10, 5), &mut inv);
        // 첫 행(헤더 아래) 클릭.
        let y = g.tree_top() + 2;
        g.on_event(&click(10, y), &mut inv);
        assert_eq!(g.selected_row(), 0);
    }
}

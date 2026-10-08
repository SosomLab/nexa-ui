//! 계층 경로 바 — 원본 NexaPathBar(docs/27) α 이식.
//! 브레드크럼(세그먼트 클릭 이동·현재 비활성·드라이브 `C:`→`C:\`·hover) + 편집 모드
//! (우클릭 진입·타이핑·Enter 제출·Esc 취소). **네비게이션 비종속** — 위젯은 파일시스템을
//! 모르고 `take_navigation()`으로 통지만, 이동·검증은 호스트가 수행(§3 규약).
//! 후속(β/γ): 오버플로 `…`·UNC·자동완성(PATH-SUG)·형제 ▾ 드롭다운·드롭 타깃.

use std::cell::RefCell;

use crate::draw::DrawCtx;
use crate::edit::{EditKey, EditState};
use crate::event::InputEvent;
use crate::geom::{Point, Rect};
use crate::theme::Theme;
use crate::widget::{Invalidations, Widget};
use nexa_ctl::DrawCtx as CtlDrawCtx;

/// 경로 세그먼트(라벨 + 클릭 시 이동할 전체 경로).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Segment {
    pub label: String,
    pub full: String,
}

/// 이 경로의 구분자: `/`로 시작하면 Unix 경로(`/`) · 그 밖(드라이브 `C:\…` 등)은 `\`.
#[must_use]
pub fn path_separator(path: &str) -> &'static str {
    if path.starts_with('/') {
        "/"
    } else {
        "\\"
    }
}

/// 로컬 FS 세그먼터(원본 IPathSegmenter 기본 구현): 드라이브 `C:` → `C:\`, 이후 폴더 누적.
/// Windows 경로는 `\`·`/` 모두 허용, 조립은 `\`. **Unix 경로(`/`로 시작)는 `/`로만 나누고 `/home/user`처럼 절대 경로로 조립**한다
/// (nexa-dir3 Linux 실기 10-03: 종전에는 `home\user`로 조립해 세그먼트 클릭이 없는 경로로 갔다 · 이름에 `\`가 든 파일도 보존).
/// UNC·VFS 스킴은 후속(§4).
pub fn split_path(path: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    if path.starts_with('/') {
        // 루트 `/`가 첫 세그먼트(Windows의 `C:`에 해당 — 128차 · nexa-dir3 사용자 10-03 "/ 위치에서 Breadcrumb에 아무것도 없음 ·
        // / 폴더가 끝 폴더이자 시작 폴더로 보여야"). 루트 뒤에는 구분자를 그리지 않는다(루트 글자가 곧 구분자).
        out.push(Segment {
            label: "/".to_string(),
            full: "/".to_string(),
        });
        let mut acc = String::new();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            acc.push('/');
            acc.push_str(part);
            out.push(Segment {
                label: part.to_string(),
                full: acc.clone(),
            });
        }
        return out;
    }
    let mut acc = String::new();
    for part in path.split(['\\', '/']).filter(|p| !p.is_empty()) {
        if acc.is_empty() {
            // 첫 세그먼트: 드라이브(`C:`)면 루트는 `C:\`
            acc = if part.ends_with(':') {
                format!("{part}\\")
            } else {
                part.to_string()
            };
            out.push(Segment {
                label: part.to_string(),
                full: acc.clone(),
            });
        } else {
            if !acc.ends_with('\\') {
                acc.push('\\');
            }
            acc.push_str(part);
            out.push(Segment {
                label: part.to_string(),
                full: acc.clone(),
            });
        }
    }
    out
}

/// 자동완성 제안 상태(PATH-SUG — 원본 NexaPathBar 계승): 목록·선택·조회 시점 입력(↑ 복원).
struct Suggest {
    items: Vec<String>,
    sel: Option<usize>,
    base: String,
}

/// 계층 경로 바 위젯. 높이 1줄 고정(§5 — 경량 measure).
pub struct PathBar {
    path: String,
    segments: Vec<Segment>,
    bounds: Rect,
    row_h: i32,
    pad_x: i32,
    hover: Option<usize>,
    /// 편집 모드 상태(Some = 편집 중) — 캐럿·선택·클릭 배치(edit.rs 공용 모델).
    edit: Option<EditState>,
    /// 편집 자동완성 팝업(PATH-SUG) — 내용은 호스트가 공급([`Self::set_suggestions`]).
    suggest: Option<Suggest>,
    /// 팝업 하한 y(리스트 영역 바닥 — 호스트가 지정. 0=제한 없음).
    overlay_bottom: i32,
    /// 호스트가 수거할 이동 요청(세그먼트 클릭·편집 제출).
    pending_nav: Option<String>,
    /// 눌린 세그먼트(이동은 **뗄 때** · 같은 세그먼트 위 — nexa-ctl 179차 규약 · nexa-dir3 사용자 10-09).
    pressed_seg: Option<usize>,
    /// 페인트 시 계산한 세그먼트 x 범위(히트 테스트용 — 텍스트 측정은 DrawCtx에서만 가능).
    ranges: RefCell<Vec<(i32, i32)>>,
}

impl std::fmt::Debug for PathBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PathBar")
            .field("path", &self.path)
            .field("bounds", &self.bounds)
            .finish_non_exhaustive()
    }
}

impl PathBar {
    pub fn new(path: impl Into<String>, row_h: i32, pad_x: i32) -> Self {
        let path = path.into();
        PathBar {
            segments: split_path(&path),
            path,
            bounds: Rect::default(),
            row_h: row_h.max(1),
            pad_x,
            hover: None,
            edit: None,
            suggest: None,
            overlay_bottom: 0,
            pending_nav: None,
            pressed_seg: None,
            ranges: RefCell::new(Vec::new()),
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn is_editing(&self) -> bool {
        self.edit.is_some()
    }

    /// IME 조합 창 배치용(M2-7) — 편집 중이면 (캐럿 앞 텍스트, 필드 rect, pad_x).
    /// 캐럿 = `rect.x + pad_x + text_width(캐럿 앞 텍스트)`.
    pub fn edit_info(&self) -> Option<(String, Rect, i32)> {
        self.edit
            .as_ref()
            .map(|e| (e.text_before_caret(), self.bounds, self.pad_x))
    }

    /// 호스트가 수행할 이동 요청 수거(있으면 1회성).
    pub fn take_navigation(&mut self) -> Option<String> {
        self.pending_nav.take()
    }

    /// 호스트가 이동 완료 후 호출 — 브레드크럼 갱신·편집 종료.
    pub fn set_path(&mut self, path: impl Into<String>, inv: &mut Invalidations) {
        self.path = path.into();
        self.segments = split_path(&self.path);
        self.close_suggest(inv);
        self.edit = None;
        self.hover = None;
        inv.push(self.bounds);
    }

    pub fn set_metrics(&mut self, row_h: i32, pad_x: i32, inv: &mut Invalidations) {
        self.row_h = row_h.max(1);
        self.pad_x = pad_x;
        inv.push(self.bounds);
    }

    /// 편집 모드 진입(우클릭) — **전체 선택** 상태로 시작(사용자 지시 07-13·원본 대응).
    pub fn begin_edit(&mut self, inv: &mut Invalidations) {
        self.edit = Some(EditState::new(&self.path, true));
        inv.push(self.bounds);
    }

    /// 편집 취소(Esc·포커스아웃) — 입력 무시·브레드크럼 복귀(§2).
    pub fn cancel_edit(&mut self, inv: &mut Invalidations) {
        self.close_suggest(inv);
        if self.edit.take().is_some() {
            inv.push(self.bounds);
        }
    }

    /// 편집 제출(Enter) — 이동 요청으로 통지(검증·이동은 호스트).
    pub fn submit_edit(&mut self, inv: &mut Invalidations) {
        self.close_suggest(inv);
        if let Some(es) = self.edit.take() {
            let text = es.text();
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                self.pending_nav = Some(trimmed.to_string());
            }
            inv.push(self.bounds);
        }
    }

    // ── 편집 자동완성(PATH-SUG — 원본 NexaPathBar §제안) ─────────

    /// 현재 편집 버퍼(제안 공급자 입력용). 비편집이면 `None`.
    pub fn edit_text(&self) -> Option<String> {
        self.edit.as_ref().map(|e| e.text())
    }

    /// 팝업 하한 y(리스트 영역 바닥) — 패널 레이아웃이 지정.
    pub fn set_overlay_bottom(&mut self, y: i32) {
        self.overlay_bottom = y;
    }

    pub fn suggest_open(&self) -> bool {
        self.suggest.is_some()
    }

    fn suggest_rect_for(&self, n: usize) -> Rect {
        let mut h = n as i32 * self.row_h;
        if self.overlay_bottom > 0 {
            h = h.min((self.overlay_bottom - self.bounds.bottom()).max(0));
        }
        Rect::new(self.bounds.x, self.bounds.bottom(), self.bounds.w, h)
    }

    /// 현재 팝업 영역(없으면 `None`) — 무효화·오버레이 클립용.
    pub fn suggest_rect(&self) -> Option<Rect> {
        self.suggest
            .as_ref()
            .map(|s| self.suggest_rect_for(s.items.len()))
    }

    /// 제안 목록 갱신(호스트 — 편집 텍스트 변경 시). 빈 목록=닫기(원본 규약).
    pub fn set_suggestions(&mut self, items: Vec<String>, inv: &mut Invalidations) {
        if let Some(r) = self.suggest_rect() {
            inv.push(r); // 이전 팝업 영역(줄어들 때 잔상 방지 — 드롭다운 교훈 07-13)
        }
        if items.is_empty() || self.edit.is_none() {
            self.suggest = None;
            return;
        }
        let base = self.edit.as_ref().map(|e| e.text()).unwrap_or_default();
        self.suggest = Some(Suggest {
            items,
            sel: None,
            base,
        });
        if let Some(r) = self.suggest_rect() {
            inv.push(r);
        }
    }

    pub fn close_suggest(&mut self, inv: &mut Invalidations) {
        if let Some(r) = self.suggest_rect() {
            inv.push(r);
        }
        self.suggest = None;
    }

    /// ↑/↓ 제안 이동(원본 규약): 처음 ↓=1번째 · 첫 항목(또는 미선택)에서 ↑=선택 해제 +
    /// **조회 시점 입력 복원**. 선택 항목은 편집기에 미리 채움(캐럿 끝) — Enter로 그대로 이동.
    /// 팝업이 열려 있으면 `true`(호스트가 소비).
    pub fn suggest_move(&mut self, delta: i32, inv: &mut Invalidations) -> bool {
        let Some(sg) = &mut self.suggest else {
            return false;
        };
        let n = sg.items.len();
        if n == 0 {
            return false;
        }
        if delta < 0 && sg.sel.is_none_or(|i| i == 0) {
            sg.sel = None;
            let base = sg.base.clone();
            if let Some(es) = &mut self.edit {
                es.set_text_end(&base);
            }
        } else {
            let i = match sg.sel {
                None => 0,
                Some(i) => (i as i64 + delta as i64).clamp(0, n as i64 - 1) as usize,
            };
            sg.sel = Some(i);
            let t = sg.items[i].clone();
            if let Some(es) = &mut self.edit {
                es.set_text_end(&t);
            }
        }
        if let Some(r) = self.suggest_rect() {
            inv.push(r);
        }
        inv.push(self.bounds);
        true
    }

    /// 팝업 클릭 — 항목이면 그 경로로 **즉시 이동**(제출, 탐색기 동일). 처리했으면 `true`.
    pub fn suggest_click(&mut self, x: i32, y: i32, inv: &mut Invalidations) -> bool {
        let Some(sg) = &self.suggest else {
            return false;
        };
        let rect = self.suggest_rect_for(sg.items.len());
        if !rect.contains(Point { x, y }) {
            return false;
        }
        let idx = ((y - rect.y) / self.row_h.max(1)) as usize;
        if let Some(item) = sg.items.get(idx).cloned() {
            if let Some(es) = &mut self.edit {
                es.set_text_end(&item);
            }
            self.submit_edit(inv); // pending_nav 통지 + 팝업 닫기
        }
        true
    }

    /// 팝업 페인트 — 패널 페인트 **마지막**에 호출(리스트 위 오버레이).
    pub fn paint_suggest(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let Some(sg) = &self.suggest else {
            return;
        };
        let rect = self.suggest_rect_for(sg.items.len());
        if rect.h <= 0 || rect.w <= 2 {
            return;
        }
        ctx.fill_rect(rect, theme.panel_bg);
        // 테두리 1px(팝업 경계 가독)
        ctx.fill_rect(Rect::new(rect.x, rect.y, rect.w, 1), theme.border);
        ctx.fill_rect(
            Rect::new(rect.x, rect.bottom() - 1, rect.w, 1),
            theme.border,
        );
        ctx.fill_rect(Rect::new(rect.x, rect.y, 1, rect.h), theme.border);
        ctx.fill_rect(Rect::new(rect.right() - 1, rect.y, 1, rect.h), theme.border);
        for (i, item) in sg.items.iter().enumerate() {
            let y = rect.y + 1 + i as i32 * self.row_h;
            if y >= rect.bottom() - 1 {
                break; // 하한 클램프 — 넘치는 항목 생략
            }
            let row = Rect::new(
                rect.x + 1,
                y,
                rect.w - 2,
                self.row_h.min(rect.bottom() - 1 - y),
            );
            let bg = if sg.sel == Some(i) {
                theme.sel_bg
            } else {
                theme.panel_bg
            };
            ctx.fill_rect(row, bg);
            let ty = y + (self.row_h - (self.row_h * 4) / 5) / 2;
            let cell = Rect::new(
                row.x + self.pad_x,
                y,
                (row.w - self.pad_x * 2).max(0),
                row.h,
            );
            ctx.text_opaque(cell.x, ty, cell, item, theme.text, bg);
        }
    }

    /// 편집 문자 입력(`'\u{8}'` = Backspace) — 선택이 있으면 대체/삭제.
    pub fn edit_char(&mut self, c: char, inv: &mut Invalidations) {
        let Some(es) = &mut self.edit else {
            return;
        };
        if c == '\u{8}' {
            es.backspace();
        } else if !c.is_control() {
            es.insert(c);
        }
        inv.push(self.bounds);
    }

    /// 편집 키(←→/Home/End/Shift 선택·Ctrl+A·Delete — 실기 QA 07-13).
    pub fn edit_key(&mut self, k: EditKey, shift: bool, inv: &mut Invalidations) {
        if let Some(es) = &mut self.edit {
            es.key(k, shift);
            inv.push(self.bounds);
        }
    }

    /// 편집 선택 텍스트(Ctrl+C — QA 07-14). 선택 없으면 `None`.
    pub fn edit_selected_text(&self) -> Option<String> {
        self.edit.as_ref()?.selected_text()
    }

    /// 편집 선택 잘라내기(Ctrl+X) — 선택 텍스트 반환 후 삭제.
    pub fn edit_cut(&mut self, inv: &mut Invalidations) -> Option<String> {
        let t = self.edit.as_mut()?.cut_selection()?;
        inv.push(self.bounds);
        Some(t)
    }

    /// 편집 붙여넣기(Ctrl+V) — 선택 대체 삽입. 제어 문자는 호출자가 필터.
    pub fn edit_paste(&mut self, s: &str, inv: &mut Invalidations) {
        if let Some(es) = &mut self.edit {
            es.insert_str(s);
            inv.push(self.bounds);
        }
    }

    /// 편집 컨텍스트 메뉴 상태(10-01 — 네이티브 EDIT 메뉴 대응): (실행 취소 가능, 선택 있음, 비어 있음).
    /// 편집 중이 아니면 `None`.
    pub fn edit_menu_state(&self) -> Option<(bool, bool, bool)> {
        let es = self.edit.as_ref()?;
        Some((es.can_undo(), es.has_selection(), es.is_empty()))
    }

    /// 편집 필드 안 좌표인가(우클릭 컨텍스트 메뉴 판정 — 편집 중이 아니면 false).
    pub fn edit_hit(&self, x: i32, y: i32) -> bool {
        self.edit.as_ref().is_some_and(|es| es.hit(x, y))
    }

    /// 편집 실행 취소(단일 단계). 복귀했으면 `true`.
    pub fn edit_undo(&mut self, inv: &mut Invalidations) -> bool {
        let Some(es) = &mut self.edit else {
            return false;
        };
        let did = es.undo();
        if did {
            inv.push(self.bounds);
        }
        did
    }

    /// 편집 선택 삭제(컨텍스트 메뉴 "삭제"). 지웠으면 `true`.
    pub fn edit_delete(&mut self, inv: &mut Invalidations) -> bool {
        let Some(es) = &mut self.edit else {
            return false;
        };
        let did = es.delete_selected();
        if did {
            inv.push(self.bounds);
        }
        did
    }

    /// 좌표의 세그먼트 인덱스(페인트가 캐시한 범위 기준).
    fn segment_at(&self, x: i32, y: i32) -> Option<usize> {
        if !self.bounds.contains(Point { x, y }) {
            return None;
        }
        self.ranges
            .borrow()
            .iter()
            .position(|&(lo, hi)| x >= lo && x < hi)
    }
}

impl Widget for PathBar {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        if self.bounds != bounds {
            let old = self.bounds;
            self.bounds = bounds;
            inv.push(old.union(&bounds));
        }
    }

    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        match *ev {
            InputEvent::MouseDown { x, y, .. } => {
                if let Some(es) = &mut self.edit {
                    // 필드 안 클릭 = 캐럿 배치(실기 QA 07-13). 밖 = 호스트가 포커스아웃(cancel)
                    if es.hit(x, y) {
                        es.click(x);
                        inv.push(self.bounds);
                    }
                    return;
                }
                // 세그먼트 누름 = 기억만 · 이동은 뗄 때(179차) · 현재(마지막) 세그먼트는 비활성(§1-3).
                self.pressed_seg = self
                    .segment_at(x, y)
                    .filter(|&i| i + 1 < self.segments.len());
            }
            InputEvent::RightDown { x, y } => {
                if self.bounds.contains(Point { x, y }) && !self.is_editing() {
                    self.begin_edit(inv);
                }
            }
            InputEvent::MouseMove { x, y } => {
                if let Some(es) = &mut self.edit {
                    // 드래그 선택(click~release — QA 07-13)
                    if es.drag(x) {
                        inv.push(self.bounds);
                    }
                    return;
                }
                let hover = self
                    .segment_at(x, y)
                    .filter(|&i| i + 1 < self.segments.len()); // 마지막(현재)은 hover 없음(§1-3)
                if hover != self.hover {
                    self.hover = hover;
                    inv.push(self.bounds);
                }
            }
            InputEvent::MouseUp { x, y } => {
                if let Some(es) = &mut self.edit {
                    es.release();
                    return;
                }
                if let Some(i) = self.pressed_seg.take() {
                    if self.segment_at(x, y) == Some(i) {
                        self.pending_nav = Some(self.segments[i].full.clone());
                    }
                }
            }
            _ => {}
        }
    }

    fn paint(&self, ctx: &mut dyn CtlDrawCtx, theme: &Theme) {
        let mut a = crate::draw::Adapt(ctx);
        self.paint_bar(&mut a, theme);
    }
}

impl PathBar {
    /// 그리기 본체(dir2 `Widget::paint` 그대로) — 그리드 어휘 백엔드를 직접 넘길 때(시험 기록기).
    pub fn paint_bar(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        ctx.select_font(crate::draw::FontSlot::Base, false, false); // 폰트 슬롯(X-12)
        let b = self.bounds;
        let ty = b.y + (b.h - (b.h * 4) / 5) / 2;

        if let Some(es) = &self.edit {
            // 편집 모드 — 공용 필드 페인트(선택 하이라이트·세로바 캐럿·끝 정렬 오버플로)
            es.paint_field(ctx, b, self.pad_x, theme);
            self.ranges.borrow_mut().clear();
            return;
        }

        // 브레드크럼 — 구분자 `\`·여백 최소(사용자 지시 07-13: 실제 경로 문자열처럼 조밀하게),
        // hover 강조. x 범위 캐시(히트 테스트)
        const SEG_PAD: i32 = 2; // 세그먼트 좌우 미세 여백(hover 가독 최소치)
        let mut ranges = Vec::with_capacity(self.segments.len());
        let last = self.segments.len().saturating_sub(1);
        // 남은 영역 배경 먼저(세그먼트가 덮어씀)
        ctx.fill_rect(b, theme.chrome_bg);
        // 오버플로 = 끝 정렬(사용자 지시 07-13): 뒤(최근 폴더)부터 역으로 채워 시작 인덱스 결정,
        // 잘린 앞부분은 "…" 표시(원본 breadcrumb 긴 경로 끝 정렬 계승)
        let sep = path_separator(&self.path);
        let sep_w = ctx.text_width(sep);
        let widths: Vec<i32> = self
            .segments
            .iter()
            .map(|s| ctx.text_width(&s.label) + SEG_PAD * 2)
            .collect();
        let avail = b.w - SEG_PAD * 2;
        let mut start = 0usize;
        // 세그먼트 뒤 구분자 폭(마지막 · Unix 루트 `/` 뒤 = 0 — `/ home`이지 `/ / home`이 아니다).
        let seps: Vec<i32> = self
            .segments
            .iter()
            .enumerate()
            .map(|(i, s)| {
                if i == last || s.label == "/" {
                    0
                } else {
                    sep_w
                }
            })
            .collect();
        let total: i32 = widths.iter().sum::<i32>() + seps.iter().sum::<i32>();
        if total > avail {
            let ell_w = ctx.text_width("…");
            let mut acc = ell_w + sep_w;
            start = self.segments.len(); // 최소한 마지막 세그먼트는 그린다(아래 min)
            for i in (0..self.segments.len()).rev() {
                acc += widths[i] + seps[i];
                if acc > avail {
                    break;
                }
                start = i;
            }
            start = start.min(last);
        }
        let mut x = b.x + SEG_PAD;
        if start > 0 {
            // 생략 표지 — 클릭 불가(범위 캐시에는 빈 항목으로 채워 인덱스 정렬 유지)
            let ell = Rect::new(x, b.y, ctx.text_width("…").min(b.right() - x), b.h);
            ctx.text_opaque(ell.x, ty, ell, "…", theme.text_dim, theme.chrome_bg);
            x += ell.w;
            let sep_cell = Rect::new(x, b.y, sep_w.min((b.right() - x).max(0)), b.h);
            if sep_cell.w > 0 {
                ctx.text_opaque(
                    sep_cell.x,
                    ty,
                    sep_cell,
                    sep,
                    theme.text_dim,
                    theme.chrome_bg,
                );
            }
            x += sep_w;
            ranges.extend(std::iter::repeat_n((0, 0), start)); // 생략 세그먼트 = 빈 범위
        }
        for (i, seg) in self.segments.iter().enumerate().skip(start) {
            let w = widths[i];
            let cell = Rect::new(x, b.y, w.min(b.right() - x), b.h);
            if cell.w <= 0 {
                ranges.push((x, x)); // 화면 밖 — 빈 범위
                continue;
            }
            let bg = if self.hover == Some(i) {
                theme.sel_bg // hover 강조(클릭 가능 세그먼트만 — MouseMove에서 필터)
            } else {
                theme.chrome_bg
            };
            let fg = if i == last {
                theme.text
            } else {
                theme.text_dim
            };
            ctx.text_opaque(cell.x + SEG_PAD, ty, cell, &seg.label, fg, bg);
            ranges.push((cell.x, cell.x + w));
            x += w;
            if seps[i] > 0 {
                let sep_cell = Rect::new(x, b.y, sep_w.min((b.right() - x).max(0)), b.h);
                if sep_cell.w > 0 {
                    ctx.text_opaque(
                        sep_cell.x,
                        ty,
                        sep_cell,
                        sep,
                        theme.text_dim,
                        theme.chrome_bg,
                    );
                }
                x += sep_w;
            }
        }
        // 하단 경계선(영역 구분 — 원본 docs/39 §2 "경계선 + 명도 차")
        ctx.fill_rect(Rect::new(b.x, b.bottom() - 1, b.w, 1), theme.border);
        *self.ranges.borrow_mut() = ranges;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Color;

    #[test]
    fn split_drive_and_folders() {
        let segs = split_path("C:\\Users\\kiros33");
        assert_eq!(segs.len(), 3);
        assert_eq!(
            (segs[0].label.as_str(), segs[0].full.as_str()),
            ("C:", "C:\\")
        );
        assert_eq!(segs[1].full, "C:\\Users");
        assert_eq!(segs[2].full, "C:\\Users\\kiros33");
        // 슬래시·끝 구분자 허용
        assert_eq!(split_path("C:/a/b/")[2].full, "C:\\a\\b");
        assert!(split_path("").is_empty());
    }

    /// Unix 경로: `/`로만 나누고 절대 경로로 조립 · 구분자 글자 = `/` · 이름 안의 `\`는 그대로.
    #[test]
    fn split_unix_paths_keep_leading_slash() {
        let segs = split_path("/home/kiros33/Projects");
        assert_eq!(segs.len(), 4);
        // 루트가 첫 세그먼트(128차) — 클릭 = `/`로.
        assert_eq!((segs[0].label.as_str(), segs[0].full.as_str()), ("/", "/"));
        assert_eq!(
            (segs[1].label.as_str(), segs[1].full.as_str()),
            ("home", "/home")
        );
        assert_eq!(segs[3].full, "/home/kiros33/Projects");
        assert_eq!(split_path("/a//b/")[2].full, "/a/b");
        assert_eq!(split_path("/tmp/we\\ird")[2].label, "we\\ird");
        // 루트 자신 = 세그먼트 하나(시작이자 끝).
        let root = split_path("/");
        assert_eq!(root.len(), 1);
        assert_eq!((root[0].label.as_str(), root[0].full.as_str()), ("/", "/"));
        assert_eq!(path_separator("/home"), "/");
        assert_eq!(path_separator("C:\\Users"), "\\");
    }

    #[test]
    fn suggest_cycle_and_restore_and_click() {
        // 원본 NexaPathBar 규약: ↓=선택+미리 채움 · 첫 항목에서 ↑=해제+조회 시점 입력 복원 ·
        // 클릭=즉시 이동(제출). 빈 목록=닫기.
        let mut inv = Invalidations::default();
        let mut pb = PathBar::new("C:\\U", 20, 6);
        pb.set_bounds(Rect::new(0, 0, 300, 20), &mut inv);
        pb.set_overlay_bottom(400);
        pb.begin_edit(&mut inv);
        pb.edit_char('\\', &mut inv); // "C:\U\" 유사 — 버퍼 조작만
        let items = vec!["C:\\U\\Alpha".to_string(), "C:\\U\\Beta".to_string()];
        pb.set_suggestions(items, &mut inv);
        assert!(pb.suggest_open());
        let base = pb.edit_text().unwrap();
        assert!(pb.suggest_move(1, &mut inv));
        assert_eq!(pb.edit_text().unwrap(), "C:\\U\\Alpha", "↓=1번째 미리 채움");
        pb.suggest_move(1, &mut inv);
        assert_eq!(pb.edit_text().unwrap(), "C:\\U\\Beta");
        pb.suggest_move(-1, &mut inv);
        pb.suggest_move(-1, &mut inv);
        assert_eq!(
            pb.edit_text().unwrap(),
            base,
            "첫 항목에서 ↑=조회 시점 입력 복원"
        );
        // 팝업 클릭(1행) = 그 경로 제출
        assert!(
            pb.suggest_click(10, 20 + 10, &mut inv),
            "팝업 영역 클릭 처리"
        );
        assert_eq!(pb.take_navigation().as_deref(), Some("C:\\U\\Alpha"));
        assert!(
            !pb.suggest_open() && !pb.is_editing(),
            "제출 = 닫기+편집 종료"
        );
        // 빈 목록 = 닫기
        pb.begin_edit(&mut inv);
        pb.set_suggestions(vec!["x".into()], &mut inv);
        pb.set_suggestions(Vec::new(), &mut inv);
        assert!(!pb.suggest_open());
    }

    struct Probe;
    impl DrawCtx for Probe {
        fn fill_rect(&mut self, _r: Rect, _c: Color) {}
        fn text_opaque(&mut self, _x: i32, _y: i32, _c: Rect, _t: &str, _f: Color, _b: Color) {}
        fn text_width(&mut self, text: &str) -> i32 {
            text.chars().count() as i32 * 8
        }
    }

    fn bar() -> (PathBar, Invalidations) {
        let mut inv = Invalidations::default();
        let mut p = PathBar::new("C:\\Users\\kiros33", 20, 6);
        p.set_bounds(Rect::new(0, 0, 600, 24), &mut inv);
        p.paint_bar(&mut Probe, &Theme::dark()); // 히트 테스트 범위 캐시
        (p, inv)
    }

    #[test]
    fn segment_click_requests_navigation_but_last_is_inert() {
        let (mut p, mut inv) = bar();
        // 세그먼트 0("C:"): x = pad(6)..6+2*8+12=34
        p.on_event(
            &InputEvent::MouseDown {
                x: 10,
                y: 5,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(p.take_navigation(), None, "누름만 = 이동 없음(179차)");
        p.on_event(&InputEvent::MouseUp { x: 400, y: 5 }, &mut inv);
        assert_eq!(p.take_navigation(), None, "다른 곳에서 뗌 = 취소");
        p.on_event(
            &InputEvent::MouseDown {
                x: 10,
                y: 5,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        p.on_event(&InputEvent::MouseUp { x: 10, y: 5 }, &mut inv);
        assert_eq!(p.take_navigation().as_deref(), Some("C:\\"));
        // 마지막 세그먼트(현재)는 무동작
        let last_x = p.ranges.borrow().last().unwrap().0 + 2;
        p.on_event(
            &InputEvent::MouseDown {
                x: last_x,
                y: 5,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(p.take_navigation(), None);
    }

    #[test]
    fn right_click_edits_enter_submits_esc_cancels() {
        let (mut p, mut inv) = bar();
        p.on_event(&InputEvent::RightDown { x: 100, y: 5 }, &mut inv);
        assert!(p.is_editing());
        p.edit_key(EditKey::End, false, &mut inv); // 전체 선택 해제 — 끝 편집
        p.edit_char('\u{8}', &mut inv); // "…kiros3"
        p.edit_char('2', &mut inv);
        p.submit_edit(&mut inv);
        assert!(!p.is_editing());
        assert_eq!(p.take_navigation().as_deref(), Some("C:\\Users\\kiros32"));

        p.on_event(&InputEvent::RightDown { x: 100, y: 5 }, &mut inv);
        p.edit_char('x', &mut inv);
        p.cancel_edit(&mut inv); // Esc — 입력 무시·복귀(§2)
        assert!(!p.is_editing());
        assert_eq!(p.take_navigation(), None);
        assert_eq!(p.path(), "C:\\Users\\kiros33");
    }

    #[test]
    fn edit_starts_fully_selected_and_click_places_caret() {
        let (mut p, mut inv) = bar();
        p.on_event(&InputEvent::RightDown { x: 100, y: 5 }, &mut inv);
        p.edit_char('D', &mut inv); // 전체 선택 → 대체(사용자 지시: 진입=전체 선택)
        p.edit_char(':', &mut inv);
        p.submit_edit(&mut inv);
        assert_eq!(p.take_navigation().as_deref(), Some("D:"));

        // 클릭 캐럿 배치: 편집 필드 재진입 → paint로 오프셋 캐시 → 클릭 → 그 위치에 삽입
        p.set_path("C:\\ab", &mut inv);
        p.on_event(&InputEvent::RightDown { x: 100, y: 5 }, &mut inv);
        p.paint_bar(&mut Probe, &Theme::dark());
        p.on_event(
            &InputEvent::MouseDown {
                x: 6 + 8 * 3, // 문자 폭 8 — "C:\" 뒤 경계
                y: 5,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert!(p.is_editing(), "필드 안 클릭은 편집 유지(캐럿 배치)");
        p.edit_char('Z', &mut inv);
        p.submit_edit(&mut inv);
        assert_eq!(p.take_navigation().as_deref(), Some("C:\\Zab"));
    }

    #[test]
    fn hover_tracks_clickable_segments_only() {
        let (mut p, mut inv) = bar();
        p.on_event(&InputEvent::MouseMove { x: 10, y: 5 }, &mut inv);
        assert_eq!(p.hover, Some(0));
        let last_x = p.ranges.borrow().last().unwrap().0 + 2;
        p.on_event(&InputEvent::MouseMove { x: last_x, y: 5 }, &mut inv);
        assert_eq!(p.hover, None, "현재 세그먼트는 hover 없음");
        p.on_event(&InputEvent::MouseMove { x: 10, y: 100 }, &mut inv);
        assert_eq!(p.hover, None, "영역 밖");
    }

    #[test]
    fn set_path_rebuilds_and_exits_edit() {
        let (mut p, mut inv) = bar();
        p.begin_edit(&mut inv);
        p.set_path("D:\\Work", &mut inv);
        assert!(!p.is_editing());
        assert_eq!(p.path(), "D:\\Work");
        assert_eq!(p.segments.len(), 2);
    }
}

//! 가상화 행 리스트 — 스크롤·가시 행(M1-1) → 트리 표시(M1-3) → **컬럼 시스템(M1-4)**.
//! 원본 docs/23 계승: 헤더 행(정렬 3상태 ▲/▼ 앞·다중 순번 ①② 뒤·Shift 다중열·드래그 리사이즈)·
//! 가로 스크롤. 컬럼 의미는 모른다 — 셀 값·정렬은 [`RowSource`]에 위임(key 불투명).

use crate::columns::{order_badge, Align, Column};
use crate::draw::DrawCtx;
use crate::event::{InputEvent, Key, WheelAccum};
use crate::fastscroll::FastScroller;
use crate::geom::{Point, Rect};
use crate::theme::Theme;
// 타입어헤드 버퍼 = nexa-ctl 부품(157차 — 한글 조합기 내장 · nexa-sql 탐색기와 같은 규칙). 이 크레이트의 옛 `typeahead`
// 모듈(dir2 규칙 · 같은 키 반복 = 순환)은 공개 API로만 남는다.
use crate::widget::{Invalidations, Widget};
use nexa_ctl::typeahead::{TypeAhead, TYPEAHEAD_TIMEOUT_MS};
use nexa_ctl::DrawCtx as CtlDrawCtx;

/// 휠 1노치당 스크롤 행 수(M0-7 계승).
/// 가로 휠 1"행"당 픽셀.
const HSCROLL_PX: i32 = 16;
/// 리사이즈 핸들 판정 폭 — 컬럼 오른쪽 경계 기준 [right-6, right+2).
const RESIZE_ZONE_L: i32 = 6;
const RESIZE_ZONE_R: i32 = 2;
/// 오버레이 스크롤바(09-04 — 설정 창 방식을 파일 목록에 확산, X-47): 평소 숨김 →
/// 스크롤 순간 반투명 → 유지 → 단계 페이드. 썸 호버/드래그 = 두꺼운 바·페이드 보류.
/// 호스트 틱 = 40ms(`Invalidations::request_tick`) — 유지 22틱 ≈ 900ms(그리드 07-18 규약).
const BAR_THIN: i32 = 6;
const BAR_WIDE: i32 = 10;
const THUMB_MIN: i32 = 24;
const BAR_ALPHA: u8 = 120;
const BAR_ALPHA_HOT: u8 = 210;
const BAR_HOLD_TICKS: u8 = 22;
const BAR_FADE_STEP: u8 = 24;

/// 행 왼쪽의 펼침 상태 마커.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Marker {
    /// 펼칠 수 없음(파일) — 마커 없음, 자리는 유지(정렬).
    None,
    /// 접힌 디렉터리(▸).
    Collapsed,
    /// 펼친 디렉터리(▾).
    Expanded,
}

impl Marker {
    /// 디스클로저 글리프(사용자 확정 07-18 — 원본 규약: Segoe MDL2 Assets
    /// ChevronRight U+E76C[닫힘]/ChevronDown U+E70D[열림] — 백엔드
    /// glyph_opaque가 PUA 대역을 MDL2 폰트로 라우팅).
    fn glyph(self) -> &'static str {
        let (collapsed, expanded) = MARKER_GLYPHS.with(std::cell::Cell::get);
        match self {
            Marker::None => "",
            Marker::Collapsed => collapsed,
            Marker::Expanded => expanded,
        }
    }
}

thread_local! {
    /// (닫힘, 열림) 디스클로저 글리프 — 기본 = Segoe MDL2 ChevronRight/ChevronDown.
    static MARKER_GLYPHS: std::cell::Cell<(&'static str, &'static str)> =
        const { std::cell::Cell::new(("\u{E76C}", "\u{E70D}")) };
}

/// 디스클로저 글리프 교체(115차 · nexa-dir3 10-03 "쉐브론이 두부로") — 아이콘 글꼴(MDL2/Fluent)이 없는 OS에서 호스트가
/// 자기 글꼴이 가진 글리프(예 `›` `⌄`)로 바꾼다. UI 스레드 전용(thread_local).
pub fn set_marker_glyphs(collapsed: &'static str, expanded: &'static str) {
    MARKER_GLYPHS.with(|g| g.set((collapsed, expanded)));
}

/// 지금 디스클로저 글리프 (닫힘, 열림) — 호스트의 두부 점검용.
#[must_use]
pub fn marker_glyphs() -> (&'static str, &'static str) {
    MARKER_GLYPHS.with(std::cell::Cell::get)
}

thread_local! {
    /// 디스클로저를 글리프 대신 **선으로** 그릴까 — 기본 꺼짐([`set_marker_vector`]).
    static MARKER_VECTOR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 디스클로저를 글꼴 글리프 대신 꺾은선 쉐브론으로 그린다(123차 · nexa-dir3 Linux 실기 10-03 "쉐브론이 윈도우와 다르게 작다").
/// 아이콘 글꼴(Segoe MDL2)이 없는 OS에서 호스트가 켠다 — 글꼴과 무관하게 MDL2 쉐브론(em 9)과 같은 크기·모양이 된다.
/// 기본 꺼짐(종전 = 글리프). UI 스레드 전용(thread_local).
pub fn set_marker_vector(on: bool) {
    MARKER_VECTOR.with(|v| v.set(on));
}

/// 지금 디스클로저를 선으로 그리는가.
#[must_use]
pub fn marker_vector() -> bool {
    MARKER_VECTOR.with(std::cell::Cell::get)
}

/// 선 쉐브론의 꼭짓점 3개(순수): `cell` = 마커 칸(폭 = 들여쓰기 폭). 긴 변 = 칸 폭의 절반(16 → 8 · 짝수) · 짧은 변 = 그 절반
/// (팔 45°) — Segoe MDL2 ChevronRight/ChevronDown을 em 9로 그린 잉크(약 4.5×8.5)에 맞춘 값. 칸 가운데.
#[must_use]
pub fn marker_chevron_points(cell: Rect, expanded: bool) -> [(i32, i32); 3] {
    let long = ((cell.w / 2).max(4) / 2) * 2;
    let short = long / 2;
    let (cx, cy) = (cell.x + cell.w / 2, cell.y + cell.h / 2);
    if expanded {
        let (x0, y0) = (cx - long / 2, cy - short / 2);
        [(x0, y0), (x0 + short, y0 + short), (x0 + long, y0)]
    } else {
        let (x0, y0) = (cx - short / 2, cy - long / 2);
        [(x0, y0), (x0 + short, y0 + short), (x0, y0 + long)]
    }
}

/// 트리 컬럼(key 0) 한 행의 표시 데이터.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RowItem {
    pub text: String,
    /// 폴더 행인가 — `marker`와 분리(X-43): 빈 폴더는 글리프가 억제돼도(marker=None)
    /// 폴더 서식(굵게·리네임 전체 선택)은 유지해야 한다.
    pub is_dir: bool,
    /// 트리 깊이(들여쓰기 단위 수).
    pub depth: u32,
    pub marker: Marker,
}

/// 클릭 선택 방식(원본 docs/07 §1-2 — 교차폴더 다중 선택).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectOp {
    /// 단일 선택(기존 해제) + anchor 갱신.
    Single,
    /// Ctrl — 비연속 토글.
    Toggle,
    /// Shift — anchor~행 가시 범위.
    RangeTo,
}

/// 행 데이터 공급자. 위젯은 가시 행에 대해서만 호출한다.
pub trait RowSource {
    fn len(&self) -> usize;
    /// 트리 컬럼(key 0)의 행 데이터.
    fn row(&self, index: usize) -> RowItem;
    /// 그 외 컬럼의 셀 텍스트(key = Column.key). 기본 = 빈 값.
    fn cell(&self, index: usize, key: u32) -> String {
        let _ = (index, key);
        String::new()
    }
    /// 그 외 컬럼의 **셀 아이콘** `(키, 힌트)`(132차 · nexa-dir3 "상태" 열) — `Some`이면 글 대신 아이콘을 칸 가운데에 그린다
    /// (호스트 리졸버 [`crate::draw::set_icon_resolver`]가 키로 이미지를 준다 · 못 주면 [`RowSource::cell`] 글로 폴백).
    /// 기본 = 없음(글만).
    fn cell_icon(&self, index: usize, key: u32) -> Option<(String, String)> {
        let _ = (index, key);
        None
    }
    /// 행 활성화(펼침 마커 클릭) — 목록 구조가 바뀌었으면 `true`(위젯이 전체 무효화).
    fn toggle(&mut self, index: usize) -> bool {
        let _ = index;
        false
    }
    /// 정렬 적용(우선순위 순 `(key, desc)`, 빈 목록 = 열거 순서). 반영했으면 `true`.
    fn set_sort(&mut self, keys: &[(u32, bool)]) -> bool {
        let _ = keys;
        false
    }
    // ── 선택(기본 = 선택 없음 소스) ──
    fn is_selected(&self, index: usize) -> bool {
        let _ = index;
        false
    }
    /// 행을 흐리게 표시하는가(잘라내기 대기 항목 — 탐색기 반투명 관례, X-32).
    /// 기본 = 없음.
    fn is_ghosted(&self, index: usize) -> bool {
        let _ = index;
        false
    }
    fn select(&mut self, index: usize, op: SelectOp) -> bool {
        let _ = (index, op);
        false
    }
    /// 가시 범위 `lo..=hi`로 선택을 대체(러버밴드).
    fn select_span(&mut self, lo: usize, hi: usize) -> bool {
        let _ = (lo, hi);
        false
    }
    fn select_all(&mut self) -> bool {
        false
    }
    /// 선택 전체 해제(빈 영역 클릭). 해제했으면 `true`.
    fn clear_selection(&mut self) -> bool {
        false
    }
    /// 타입어헤드 매칭(원본 docs/32 §6 — 가시 스트림 위치상대 starts-with + wrap).
    /// `caret` 다음부터 검색(코어 `find_prefix` 규약). 기본 = 매치 없음.
    fn find_prefix(&self, caret: Option<usize>, prefix: &str) -> Option<usize> {
        let _ = (caret, prefix);
        None
    }
    /// 타입어헤드 **역방향** 매칭(157차 · 입력 중 ↑ = 이전 일치 항목): `caret` 바로 앞부터 위로 · 처음을 지나면 끝에서 이어 돈다.
    /// 기본 = 보이는 행의 이름(`row(i).text`) 접두 비교(대소문자 무시) — 소스가 더 싼 길이 있으면 재정의한다.
    fn find_prefix_rev(&self, caret: Option<usize>, prefix: &str) -> Option<usize> {
        let n = self.len();
        if n == 0 || prefix.is_empty() {
            return None;
        }
        let lower = prefix.to_lowercase();
        let start = caret.filter(|&c| c < n).unwrap_or(0);
        (1..=n)
            .map(|k| (start + n - k) % n)
            .find(|&i| self.row(i).text.to_lowercase().starts_with(&lower))
    }
    /// 행 아이콘 `(키, 로드 힌트)` — DrawCtx가 해석(M1-7 셸 아이콘). 기본 = 아이콘 없음.
    /// 타일 보기 보조 정보 — (보조 줄 텍스트, 사용량 0.0~1.0[드라이브 용량 바 — X-17]).
    /// 기본 = 없음. 소스가 종류/용량 등으로 구체화한다.
    fn tile_info(&self, _index: usize) -> (String, Option<f32>) {
        (String::new(), None)
    }

    fn icon(&self, index: usize) -> Option<(String, String)> {
        let _ = index;
        None
    }
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// 인라인 이름변경 필드의 안쪽 여백 — 필드를 텍스트 왼쪽으로 확장(테두리+여백이 필드
/// 안쪽)해 편집 진입 시 이름 x가 일반 표시와 동일(밀림 없음, QA 07-13 4차).
const RENAME_FIELD_PAD: i32 = 3;

/// 프로그램적 선택 시 뷰 내 배치 위치(사용자 QA 07-15 — Alt+↑ 떠난 폴더 자동 선택).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ScrollAlign {
    Top,
    #[default]
    Center,
    Bottom,
}

/// 헤더 드래그 재배열 상태(07-19 사용자 — 탭 드래그와 동일 UX).
/// 활성 중엔 **라이브 미리보기**(스냅 위치로 실제 재배열해 표시)·커서 추종
/// 고스트 헤더·ESC 취소([`VirtualRows::cancel_col_drag`] — orig로 복원).
#[derive(Clone, Debug)]
struct ColDrag {
    col: usize,
    press_x: i32,
    cur_x: i32,
    /// 임계(5px) 초과 이동 후 활성 — 활성 전 MouseUp = 정렬 클릭.
    active: bool,
    shift: bool,
    /// 드래그 시작 시 key 순서(ESC 취소 복원용).
    orig: Vec<u32>,
}

/// 리사이즈 드래그 상태 — **단독 조절**(사용자 확정 07-22): 해당 컬럼 너비만 변하고
/// 이웃은 그대로, 컬럼 총폭이 늘거나 준다(초과분은 가로 스크롤 — 탐색기 규약).
/// (구 QA 07-15의 한 쌍 동시 조절[총폭 보존]은 폐지.)
#[derive(Clone, Copy, Debug)]
struct ResizeDrag {
    col: usize,
    start_x: i32,
    start_w: i32,
}

/// 보기 모드(사용자 요청 07-16 — 원본 FR-A4 뷰 모드의 dir2 1차):
/// Tree = 계층(인라인 펼침 마커 — 기존 기본), Flat = 일반 폴더(펼침 없음·목록 동일),
/// Tiles = 타일(아이콘 32px + 이름/보조 줄 그리드 — 탐색기 '타일' 보기).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ViewMode {
    #[default]
    Tree,
    Flat,
    Tiles,
}

/// 러버밴드 드래그 상태(본문 빈 영역에서 시작).
#[derive(Clone, Copy, Debug)]
struct BandDrag {
    ox: i32,
    oy: i32,
    cx: i32,
    cy: i32,
}

impl BandDrag {
    fn rect(&self) -> Rect {
        let x = self.ox.min(self.cx);
        let y = self.oy.min(self.cy);
        Rect::new(x, y, (self.ox - self.cx).abs(), (self.oy - self.cy).abs())
    }
}

/// 세로 가상화 행 리스트 + 컬럼 헤더 — 프레임 비용은 bounds 높이에만 비례(docs/01 §3).
pub struct VirtualRows<S> {
    src: S,
    bounds: Rect,
    scroll_row: usize,
    scroll_x: i32,
    /// 부분 행 픽셀 오프셋(0..grid_h — 10-02 트랙패드 픽셀 스크롤). 행 단위 조작(키·노치·
    /// scroll_into_view)은 0으로 스냅.
    scroll_frac: i32,
    /// 트랙패드 픽셀 누적(노치 미만 delta → px).
    wheel_px: WheelAccum,
    row_h: i32,
    pad_x: i32,
    /// 트리 깊이 1단계의 가로 들여쓰기(px). 마커 폭도 이 값을 쓴다.
    indent_w: i32,
    wheel: WheelAccum,
    hwheel: WheelAccum,
    /// 고속 스크롤(10-02 — nexa-sql ScrollAccel 이식): 휠 노치·↑↓ 자동 반복 연속 시 배수 + ×N 배지.
    fast: FastScroller,
    /// 컬럼 정의(비면 헤더 없는 단일 트리 컬럼 — M1-3 호환).
    columns: Vec<Column>,
    /// 정렬 상태(우선순위 순). 빈 목록 = 소스 기본 정렬.
    sort: Vec<(u32, bool)>,
    resize: Option<ResizeDrag>,
    /// 헤더 라벨 드래그 재배열(07-19) — 활성 시 페인트가 삽입 인디케이터.
    col_drag: Option<ColDrag>,
    /// 컬럼 폭이 사용자 리사이즈로 변경됨(호스트 폴링 — take_col_resized).
    col_resized: bool,
    /// 컬럼 순서가 드래그로 변경됨(호스트 폴링 — take_col_reordered).
    col_reordered: bool,
    band: Option<BandDrag>,
    /// 캐럿(키보드 네비 기준 행 — docs/07 §8·docs/32).
    caret: Option<usize>,
    typeahead: TypeAhead,
    /// 타입어헤드 옵션(원본 docs/32 §7 — 설정 노출 07-15): 특수문자/공백/Backspace 허용,
    /// HUD 배지 위치(0..8 = 3×3, 행=pos/3 열=pos%3 — 기본 6=좌하).
    ta_special: bool,
    ta_space: bool,
    ta_backspace: bool,
    ta_hud_pos: u8,
    /// 타입어헤드 끔(157차 · 기본 false = 켜짐 — 끄면 글자 키를 무시한다).
    ta_off: bool,
    /// 마지막으로 본 시각(ms · `tick`/글자 입력이 갱신) — 시각이 없는 키 사건(↑/↓ 순환)의 유지 시간 리셋에 쓴다.
    ta_clock: u64,
    /// 인라인 이름변경(M3-2, 원본 B-6) — Some((행, 편집 상태)). 캐럿·선택은 edit.rs 공용 모델.
    rename: Option<(usize, crate::edit::EditState)>,
    /// 기선택 행 프레스(무수정키) — 클릭 확정(MouseUp·무드래그) 시 단일 선택으로 붕괴
    /// (프레스 시점 유지 = 다중 선택 드래그 DnD, 탐색기 규약 — QA 07-13).
    press_pending: Option<usize>,
    /// 호스트 패널 포커스 — 비활성 패널의 선택 하이라이트는 무채색(`sel_bg_inactive`)으로 구분.
    focused: bool,
    /// 보기 모드(07-16) — Tree(계층)/Flat(일반)/Tiles(타일 그리드).
    mode: ViewMode,
    /// 폰트 장식(X-12): (폴더 이름 굵게, 헤더 굵게, 헤더 이탤릭).
    font_decor: (bool, bool, bool),
    /// 컬럼을 끄는 동안 놓일 자리 표식(131차 · [`Self::set_col_drag_marker`]) — 기본 꺼짐(고스트만).
    col_drag_marker: bool,
    /// 정렬 표시를 헤더 칸 **오른쪽 끝**에(130차 · [`Self::set_sort_mark_trailing`]) — 기본 꺼짐(▲ 이름 ①).
    sort_mark_trailing: bool,
    /// 오버레이 스크롤바(09-04 X-47) — **축별 독립**(사용자 확정: 세로 스크롤 = 세로만,
    /// 가로 스크롤 = 가로만, 가로 표시 중 세로 스크롤 = 둘 다): `[세로, 가로]` 썸 알파
    /// (0=숨김)·유지 틱 잔량, 호버 축, 드래그.
    bar_alpha: [u8; 2],
    bar_hold: [u8; 2],
    bar_hover: Option<Axis>,
    bar_drag: Option<BarDrag>,
}

/// 오버레이 바 축 인덱스(`bar_alpha`/`bar_hold`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Axis {
    V = 0,
    H = 1,
}

/// 오버레이 썸 드래그(09-04): 세로 = (시작 y, 시작 scroll_row) · 가로 = (시작 x, 시작 scroll_x).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BarDrag {
    V(i32, usize),
    H(i32, i32),
}

impl<S> std::fmt::Debug for VirtualRows<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VirtualRows")
            .field("bounds", &self.bounds)
            .field("scroll_row", &self.scroll_row)
            .field("caret", &self.caret)
            .field("mode", &self.mode)
            .finish_non_exhaustive()
    }
}

impl<S: RowSource> VirtualRows<S> {
    pub fn new(src: S, row_h: i32, pad_x: i32, indent_w: i32) -> Self {
        VirtualRows {
            src,
            bounds: Rect::default(),
            scroll_row: 0,
            scroll_x: 0,
            scroll_frac: 0,
            wheel_px: WheelAccum::default(),
            row_h: row_h.max(1),
            pad_x,
            indent_w: indent_w.max(1),
            wheel: WheelAccum::default(),
            hwheel: WheelAccum::default(),
            fast: FastScroller::for_grid(), // 파일 그리드 = 한 단계 더 빠른 설정(10-02)
            columns: Vec::new(),
            sort: Vec::new(),
            resize: None,
            col_drag: None,
            col_resized: false,
            col_reordered: false,
            band: None,
            caret: None,
            typeahead: TypeAhead::new(TYPEAHEAD_TIMEOUT_MS),
            ta_special: true,
            ta_space: true,
            ta_backspace: true,
            ta_hud_pos: 6,
            ta_off: false,
            ta_clock: 0,
            rename: None,
            press_pending: None,
            focused: true,
            mode: ViewMode::default(),
            font_decor: (false, false, false),
            col_drag_marker: false,
            sort_mark_trailing: false,
            bar_alpha: [0, 0],
            bar_hold: [0, 0],
            bar_hover: None,
            bar_drag: None,
        }
    }

    /// 컬럼 드래그 표식(131차 · nexa-dir3 사용자 10-03 "컬럼 이동 시 탭 이동처럼 옮겨갈 위치에 표식"): 켜면 끄는 동안
    /// 컬럼이 **놓일 자리**(라이브 미리 보기 위치의 열 전체 — 머리 + 본문)를 강조색으로 옅게 덮고 좌우에 1 px 강조선을
    /// 긋는다(탭 바의 드래그 표식과 같은 모양). 끄면(기본) 종전처럼 고스트 머리만.
    pub fn set_col_drag_marker(&mut self, on: bool) {
        self.col_drag_marker = on;
    }

    /// 지금 컬럼을 끌고 있는가(임계를 넘은 드래그) — 호스트의 Esc 취소 판정용.
    #[must_use]
    pub fn col_dragging(&self) -> bool {
        self.col_drag.as_ref().is_some_and(|d| d.active)
    }

    /// 끄는 컬럼이 지금 놓여 있는 자리(표식 rect · 머리 + 본문 · 표식이 꺼져 있거나 끄는 중이 아니면 `None`).
    #[must_use]
    pub fn col_drag_slot(&self) -> Option<Rect> {
        let d = self
            .col_drag
            .as_ref()
            .filter(|d| d.active && self.col_drag_marker)?;
        let col = self.columns.get(d.col)?;
        let b = self.bounds;
        let x0 = self.col_x(d.col).max(b.x);
        let x1 = (self.col_x(d.col) + col.width).min(b.right());
        (x1 > x0).then(|| Rect::new(x0, b.y, x1 - x0, b.h))
    }

    /// 정렬 표시 자리(130차 · nexa-dir3 사용자 10-03 "정렬 인디케이터는 가장 우측 · 다중 정렬 순번은 인디케이터 우측" —
    /// nexa-sql 결과 그리드 모양): 켜면 제목은 왼쪽 그대로 · ▲/▼는 칸 오른쪽 끝 · **다중 정렬일 때만** 그 오른쪽에 순번 숫자
    /// (`이름 ▲2`). 끄면(기본) 종전 `▲ 이름 ①`. 클릭 규칙(단순 클릭 = 단일 3상태 · Shift = 추가/방향/제거)은 같다.
    pub fn set_sort_mark_trailing(&mut self, on: bool, inv: &mut Invalidations) {
        if self.sort_mark_trailing != on {
            self.sort_mark_trailing = on;
            inv.push(self.bounds);
        }
    }

    /// 헤더 칸 오른쪽 끝에 그릴 정렬 표시(끝 정렬 모양일 때만): `▲` · 다중 정렬이면 `▲2`(1부터). 정렬되지 않은 열 = `None`.
    #[must_use]
    pub fn sort_mark(&self, key: u32) -> Option<String> {
        if !self.sort_mark_trailing {
            return None;
        }
        let order = self.sort.iter().position(|(k, _)| *k == key)?;
        let mut s = String::from(if self.sort[order].1 { "▼" } else { "▲" });
        if self.sort.len() > 1 {
            s.push_str(&(order + 1).to_string());
        }
        Some(s)
    }

    /// 폰트 장식 설정(X-12) — 폴더 이름 굵게 / 헤더 굵게·이탤릭.
    pub fn set_font_decor(
        &mut self,
        folder_bold: bool,
        hdr_bold: bool,
        hdr_italic: bool,
        inv: &mut Invalidations,
    ) {
        let v = (folder_bold, hdr_bold, hdr_italic);
        if self.font_decor != v {
            self.font_decor = v;
            inv.push(self.bounds);
        }
    }

    /// 보기 모드 전환(07-16). 타일 진입 시 이름변경·밴드 상태는 정리.
    pub fn set_view_mode(&mut self, mode: ViewMode, inv: &mut Invalidations) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        self.band = None;
        if mode == ViewMode::Tiles && self.rename.is_some() {
            self.cancel_rename(inv);
        }
        self.clamp_scroll();
        if let Some(c) = self.caret {
            self.scroll_into_view_idx(c);
        }
        inv.push(self.bounds);
    }

    pub fn view_mode(&self) -> ViewMode {
        self.mode
    }

    // ── 타일 그리드 기하(07-16) — 리스트 모드는 열 1·행 높이 row_h로 수렴 ──

    /// 타일 셀 크기(row_h 비례 = DPI 추종): 폭 12행높이·높이 3행높이.
    fn tile_wh(&self) -> (i32, i32) {
        (self.row_h * 12, self.row_h * 3)
    }

    /// 그리드 열 수(리스트 = 1).
    fn grid_cols(&self) -> usize {
        if self.mode != ViewMode::Tiles {
            return 1;
        }
        let (tw, _) = self.tile_wh();
        ((self.bounds.w / tw).max(1)) as usize
    }

    /// 그리드 단위 높이(리스트 = row_h·타일 = tile_h).
    fn grid_h(&self) -> i32 {
        if self.mode == ViewMode::Tiles {
            self.tile_wh().1
        } else {
            self.row_h
        }
    }

    /// 그리드 행 수 = ceil(항목 / 열).
    fn grid_len(&self) -> usize {
        let cols = self.grid_cols();
        self.src.len().div_ceil(cols.max(1))
    }

    /// 항목 인덱스의 타일 rect(가시 여부 무관 — 스크롤 반영).
    fn tile_rect(&self, index: usize) -> Rect {
        let cols = self.grid_cols();
        let (tw, th) = self.tile_wh();
        let gr = index / cols;
        let gc = index % cols;
        Rect::new(
            self.bounds.x + gc as i32 * tw,
            self.row_origin() + (gr as i32 - self.scroll_row as i32) * th,
            tw,
            th,
        )
    }

    /// 컬럼 경계 리사이즈 존 히트(호스트 커서 변경용 — QA 07-15: 경계=수평 리사이즈 커서).
    /// 드래그 중에는 존 밖에서도 유지.
    pub fn resize_hot(&self, x: i32, y: i32) -> bool {
        self.resize.is_some() || matches!(self.header_hit(x, y), Some((_, true)))
    }

    /// 호스트 패널 포커스 상태 반영 — 선택 하이라이트 색만 바뀐다(선택 자체는 유지).
    pub fn set_focused(&mut self, focused: bool, inv: &mut Invalidations) {
        if self.focused != focused {
            self.focused = focused;
            inv.push(self.bounds);
        }
    }

    // ── 인라인 이름변경(M3-2, 원본 B-6) ─────────────────────────────

    pub fn is_renaming(&self) -> bool {
        self.rename.is_some()
    }

    /// 편집 상태 (행, 현재 버퍼) — IME 배치·호스트 표시용.
    pub fn rename_state(&self) -> Option<(usize, String)> {
        self.rename.as_ref().map(|(r, e)| (*r, e.text()))
    }

    /// 리네임 **편집 필드 안** 좌표인가(QA 07-26 — 호스트 슬로우클릭 판정용:
    /// 필드 안 클릭 = 캐럿 배치라 행 클릭이 아님·밖 클릭 = 취소 후 정상 행 클릭).
    pub fn rename_field_hit(&self, x: i32, y: i32) -> bool {
        self.rename.as_ref().is_some_and(|(_, es)| es.hit(x, y))
    }

    /// 인라인 이름변경 시작 — 캐럿을 그 행으로, 가시 범위로 스크롤.
    /// 초기 선택 = 파일이면 **이름부**(마지막 `.` 앞), 폴더면 전체(탐색기 관례 — QA 07-13).
    pub fn begin_rename(&mut self, row: usize, initial: &str, inv: &mut Invalidations) {
        if self.mode == ViewMode::Tiles {
            return; // 타일 보기 인라인 편집은 β(필드 기하가 리스트 전용)
        }
        if row >= self.src.len() || self.tree_col().is_none() {
            return; // 트리(이름) 열이 숨겨지면 필드를 둘 곳이 없다(A29 — 방어)
        }
        self.caret = Some(row);
        self.scroll_into_view(row);
        self.press_pending = None; // 리네임 진입 — 클릭 확정 붕괴 취소
        let is_dir = self.src.row(row).is_dir; // marker 아님(X-43 — 빈 폴더도 폴더 규약)
        let sel_to = if is_dir {
            initial.chars().count()
        } else {
            match initial.rfind('.') {
                Some(i) if i > 0 => initial[..i].chars().count(),
                _ => initial.chars().count(),
            }
        };
        self.rename = Some((
            row,
            crate::edit::EditState::with_selection_to(initial, sel_to),
        ));
        inv.push(self.bounds);
    }

    /// 편집 문자 입력(`'\u{8}'` = Backspace, 그 외 제어 문자 무시) — 선택은 대체/삭제.
    pub fn rename_char(&mut self, c: char, inv: &mut Invalidations) {
        let Some((_, es)) = &mut self.rename else {
            return;
        };
        if c == '\u{8}' {
            es.backspace();
        } else if !c.is_control() {
            es.insert(c);
        } else {
            return;
        }
        inv.push(self.bounds);
    }

    /// 편집 키(←→/Home/End/Shift 선택·Ctrl+A·Delete — 실기 QA 07-13).
    pub fn rename_key(&mut self, k: crate::edit::EditKey, shift: bool, inv: &mut Invalidations) {
        if let Some((_, es)) = &mut self.rename {
            es.key(k, shift);
            inv.push(self.bounds);
        }
    }

    /// 편집 선택 텍스트(Ctrl+C — QA 07-14). 선택 없으면 `None`.
    pub fn rename_selected_text(&self) -> Option<String> {
        self.rename.as_ref()?.1.selected_text()
    }

    /// 편집 선택 잘라내기(Ctrl+X) — 선택 텍스트 반환 후 삭제.
    pub fn rename_cut(&mut self, inv: &mut Invalidations) -> Option<String> {
        let t = self.rename.as_mut()?.1.cut_selection()?;
        inv.push(self.bounds);
        Some(t)
    }

    /// 편집 붙여넣기(Ctrl+V) — 선택 대체 삽입. 제어 문자는 호출자가 필터.
    pub fn rename_paste(&mut self, s: &str, inv: &mut Invalidations) {
        if let Some((_, es)) = &mut self.rename {
            es.insert_str(s);
            inv.push(self.bounds);
        }
    }

    /// 이름변경 실행 취소(단일 단계 — Edit 메뉴/컨텍스트 메뉴, 10-01). 복귀했으면 `true`.
    pub fn rename_undo(&mut self, inv: &mut Invalidations) -> bool {
        let Some((_, es)) = &mut self.rename else {
            return false;
        };
        let did = es.undo();
        if did {
            inv.push(self.bounds);
        }
        did
    }

    /// 이름변경 선택 삭제(컨텍스트 메뉴 "삭제"). 지웠으면 `true`.
    pub fn rename_delete(&mut self, inv: &mut Invalidations) -> bool {
        let Some((_, es)) = &mut self.rename else {
            return false;
        };
        let did = es.delete_selected();
        if did {
            inv.push(self.bounds);
        }
        did
    }

    /// 이름변경 컨텍스트 메뉴 상태: (실행 취소 가능, 선택 있음, 비어 있음). 편집 중 아니면 `None`.
    pub fn rename_menu_state(&self) -> Option<(bool, bool, bool)> {
        let (_, es) = self.rename.as_ref()?;
        Some((es.can_undo(), es.has_selection(), es.is_empty()))
    }

    /// 이름변경 필드 안 좌표인가(우클릭 컨텍스트 메뉴 판정).
    pub fn rename_hit(&self, x: i32, y: i32) -> bool {
        self.rename.as_ref().is_some_and(|(_, es)| es.hit(x, y))
    }

    /// 편집 취소(Esc·외부 클릭) — 입력 무시.
    pub fn cancel_rename(&mut self, inv: &mut Invalidations) {
        if self.rename.take().is_some() {
            inv.push(self.bounds);
        }
    }

    /// IME 조합 창 배치용(M5-3 — pathbar `edit_info`와 동일 계약): 편집 중이고 행이
    /// 가시 범위면 (캐럿 앞 텍스트, 필드 rect, pad). 캐럿 = `rect.x + pad + text_width`.
    pub fn rename_edit_info(&self) -> Option<(String, Rect, i32)> {
        let (row, es) = self.rename.as_ref()?;
        if *row < self.scroll_row {
            return None;
        }
        let y = self.row_origin() + (*row - self.scroll_row) as i32 * self.row_h;
        if y >= self.bounds.bottom() {
            return None;
        }
        let rc = self.rename_field_rect(*row, y);
        Some((es.text_before_caret(), rc, RENAME_FIELD_PAD))
    }

    /// 인라인 이름변경 필드 rect(paint와 단일 기하 — M5-3에서 추출).
    /// 트리 열 위치는 [`Self::tree_col`](현재 열 순서 — A29: 첫 열 가정 제거).
    fn rename_field_rect(&self, row: usize, y: i32) -> Rect {
        let b = self.bounds;
        let (tc_x, tc_w) = self.tree_col().unwrap_or((b.x, b.w));
        let item = self.src.row(row);
        let mut fx = tc_x + self.pad_x + item.depth as i32 * self.indent_w + self.indent_w;
        if self.src.icon(row).is_some() {
            fx += self.indent_w + self.pad_x / 2;
        }
        let fw = (tc_x + tc_w - fx).max(self.indent_w * 3);
        Rect::new(fx - RENAME_FIELD_PAD, y, fw + RENAME_FIELD_PAD, self.row_h)
    }

    /// 프로그램적 선택(M5-3 UIA SelectionItem) — 캐럿 동반·가시 범위로 스크롤.
    pub fn select_program(&mut self, row: usize, op: SelectOp, inv: &mut Invalidations) {
        if row >= self.src.len() {
            return;
        }
        self.caret = Some(row);
        self.src.select(row, op);
        self.scroll_into_view(row);
        inv.push(self.bounds);
    }

    /// 프로그램적 선택 + **뷰 정렬 스크롤**(사용자 QA 07-15 — Alt+↑ 자동 선택 위치):
    /// 선택 행을 뷰의 상단/중단/하단에 배치(최소 이동 규칙 대신 명시 위치).
    pub fn select_program_aligned(
        &mut self,
        row: usize,
        op: SelectOp,
        align: ScrollAlign,
        inv: &mut Invalidations,
    ) {
        if row >= self.src.len() {
            return;
        }
        self.caret = Some(row);
        self.src.select(row, op);
        let full = ((self.body_h() / self.row_h).max(1)) as usize;
        let target = match align {
            ScrollAlign::Top => row,
            ScrollAlign::Center => row.saturating_sub(full / 2),
            ScrollAlign::Bottom => row.saturating_sub(full.saturating_sub(1)),
        };
        self.scroll_row = target.min(self.max_scroll());
        inv.push(self.bounds);
    }

    /// 편집 제출(Enter) — (행, 새 이름) 반환. 실제 rename·재로드는 호스트 책임.
    pub fn submit_rename(&mut self, inv: &mut Invalidations) -> Option<(usize, String)> {
        let taken = self.rename.take().map(|(r, e)| (r, e.text()));
        if taken.is_some() {
            inv.push(self.bounds);
        }
        taken
    }

    /// 타입어헤드 확정 버퍼(조합 중인 한글 글자는 빠진다 — 화면 표시는 [`Self::typeahead_composing`]).
    pub fn typeahead_text(&self) -> &str {
        self.typeahead.text()
    }

    /// 타입어헤드 접두사(확정 글자 + 조합 중 글자 · 157차) — HUD에 보이는 그대로. 빈 값 = 비활성.
    pub fn typeahead_composing(&self) -> String {
        self.typeahead.composing()
    }

    /// 타입어헤드 입력 중인가(버퍼나 한글 조합이 살아 있다 · 157차).
    pub fn typeahead_active(&self) -> bool {
        self.typeahead.is_active()
    }

    /// 타입어헤드 켬/끔(157차 · 기본 켜짐). 끄면 글자 키를 무시하고 진행 중인 입력도 지운다.
    pub fn set_typeahead_enabled(&mut self, on: bool, inv: &mut Invalidations) {
        self.ta_off = !on;
        if !on && self.typeahead.is_active() {
            self.typeahead.clear();
            inv.push(self.bounds);
        }
    }

    /// 타입어헤드 즉시 취소(Esc · 포커스 이탈 · 157차) — 지웠으면 `true`.
    pub fn typeahead_cancel(&mut self, inv: &mut Invalidations) -> bool {
        if !self.typeahead.is_active() {
            return false;
        }
        self.typeahead.clear();
        inv.push(self.bounds);
        true
    }

    /// 입력 중 ↑/↓ = **접두사가 같은 항목 사이**로만 이동(157차 · nexa-sql 탐색기 규칙): 아래 = 다음 일치 · 위 = 이전 일치
    /// (끝에서 처음으로 돈다). 움직일 때마다 유지 시간을 다시 잰다. 처리했으면 `true`(입력 중이 아니면 `false`).
    fn typeahead_cycle(&mut self, down: bool, inv: &mut Invalidations) -> bool {
        if !self.typeahead.is_active() {
            return false;
        }
        let prefix = self.typeahead.composing();
        let hit = if down {
            self.src.find_prefix(self.caret, &prefix)
        } else {
            self.src.find_prefix_rev(self.caret, &prefix)
        };
        if let Some(idx) = hit {
            self.caret = Some(idx);
            self.src.select(idx, SelectOp::Single);
            self.scroll_into_view(idx);
        }
        self.typeahead.touch(self.ta_clock);
        inv.push(self.bounds);
        true
    }

    /// 타입어헤드 옵션 적용(설정 — 07-15): 리셋 ms·특수문자·공백·Backspace·HUD 위치.
    pub fn set_typeahead_opts(
        &mut self,
        reset_ms: u64,
        special: bool,
        space: bool,
        backspace: bool,
        hud_pos: u8,
        inv: &mut Invalidations,
    ) {
        self.typeahead.set_timeout(reset_ms);
        self.ta_special = special;
        self.ta_space = space;
        self.ta_backspace = backspace;
        if self.ta_hud_pos != hud_pos.min(8) {
            self.ta_hud_pos = hud_pos.min(8);
            inv.push(self.bounds);
        }
    }

    /// 주기 점검(WM_TIMER) — 타입어헤드 타임아웃 소거 + 오버레이 바 유지/페이드(09-04).
    pub fn tick(&mut self, now_ms: u64, inv: &mut Invalidations) {
        self.ta_clock = self.ta_clock.max(now_ms);
        if self.typeahead.tick(now_ms) {
            inv.push(self.bounds);
        }
        self.fast.tick(self.bounds, inv); // 속도 배지 유지/페이드(10-02)
        for axis in [Axis::V, Axis::H] {
            if self.axis_hot(axis) {
                continue; // 호버/드래그 중인 축은 페이드 보류(이탈 시 flash_bar가 재개)
            }
            let i = axis as usize;
            if self.bar_hold[i] > 0 {
                self.bar_hold[i] -= 1;
                inv.request_tick();
            } else if self.bar_alpha[i] > 0 {
                self.bar_alpha[i] = self.bar_alpha[i].saturating_sub(BAR_FADE_STEP);
                self.push_bar_strip(axis, inv);
                if self.bar_alpha[i] > 0 {
                    inv.request_tick();
                }
            }
        }
    }

    // ── 오버레이 스크롤바(09-04 X-47 — 설정 창 컨테이너와 같은 규약, 세로+가로) ──

    /// 세로 썸 rect — 콘텐츠가 뷰포트에 들어가면 None. `wide` = 호버/드래그.
    fn thumb_rect(&self, wide: bool) -> Option<Rect> {
        let body_h = self.body_h();
        let gh = self.grid_h().max(1);
        let total = self.grid_len();
        let full = (body_h / gh).max(0) as usize;
        if body_h <= 0 || total <= full || full == 0 {
            return None;
        }
        let th = ((body_h as i64 * full as i64 / total as i64) as i32)
            .max(THUMB_MIN)
            .min(body_h);
        let gh_px = gh as i64;
        let max_s = self.max_scroll().max(1) as i64 * gh_px;
        let pos = self.scroll_row as i64 * gh_px + self.scroll_frac as i64;
        let ty = self.body_top() + ((body_h - th) as i64 * pos / max_s) as i32;
        let w = if wide { BAR_WIDE } else { BAR_THIN };
        Some(Rect::new(self.bounds.right() - w - 2, ty, w, th))
    }

    /// 가로 썸 rect(본문 하단) — 컬럼 총폭이 위젯 폭 이하면 None.
    fn hthumb_rect(&self, wide: bool) -> Option<Rect> {
        let b = self.bounds;
        let total = self.total_w();
        if b.w <= 0 || total <= b.w {
            return None;
        }
        let tw = ((b.w as i64 * b.w as i64 / total as i64) as i32)
            .max(THUMB_MIN)
            .min(b.w);
        let max_x = (total - b.w).max(1) as i64;
        let tx = b.x + ((b.w - tw) as i64 * self.scroll_x as i64 / max_x) as i32;
        let h = if wide { BAR_WIDE } else { BAR_THIN };
        Some(Rect::new(tx, b.bottom() - h - 2, tw, h))
    }

    /// 축 트랙 스트립(세로 = 우측·가로 = 하단) 무효화.
    fn push_bar_strip(&self, axis: Axis, inv: &mut Invalidations) {
        let b = self.bounds;
        inv.push(match axis {
            Axis::V => Rect::new(
                b.right() - BAR_WIDE - 4,
                self.body_top(),
                BAR_WIDE + 4,
                self.body_h(),
            ),
            Axis::H => Rect::new(b.x, b.bottom() - BAR_WIDE - 4, b.w, BAR_WIDE + 4),
        });
    }

    /// 축이 호버/드래그 중(두꺼운 바·페이드 보류).
    fn axis_hot(&self, axis: Axis) -> bool {
        self.bar_hover == Some(axis)
            || matches!(
                (axis, self.bar_drag),
                (Axis::V, Some(BarDrag::V(..))) | (Axis::H, Some(BarDrag::H(..)))
            )
    }

    /// 축 바가 그려지는가(알파 > 0 또는 hot).
    fn axis_visible(&self, axis: Axis) -> bool {
        self.bar_alpha[axis as usize] > 0 || self.axis_hot(axis)
    }

    /// 스크롤 직후 **그 축만** 표시(반투명) + 유지 → 페이드. 다른 축은 자기 상태 유지
    /// (가로가 보이는 중 세로 스크롤 = 둘 다 — 사용자 확정 09-04).
    fn flash_bar(&mut self, axis: Axis, inv: &mut Invalidations) {
        self.bar_alpha[axis as usize] = BAR_ALPHA;
        self.bar_hold[axis as usize] = BAR_HOLD_TICKS;
        self.push_bar_strip(axis, inv);
        inv.request_tick();
    }

    fn in_thumb(&self, x: i32, y: i32) -> bool {
        self.thumb_rect(true)
            .is_some_and(|t| x >= t.x - 2 && x < t.right() + 2 && y >= t.y && y < t.bottom())
    }

    fn in_hthumb(&self, x: i32, y: i32) -> bool {
        self.hthumb_rect(true)
            .is_some_and(|t| x >= t.x && x < t.right() && y >= t.y - 2 && y < t.bottom() + 2)
    }

    /// MouseMove — 드래그 중이면 비례 스크롤(true = 소비), 아니면 호버 갱신(false).
    fn bar_mouse_move(&mut self, x: i32, y: i32, inv: &mut Invalidations) -> bool {
        match self.bar_drag {
            Some(BarDrag::V(sy, s0)) => {
                if let Some(t) = self.thumb_rect(true) {
                    let denom = (self.body_h() - t.h).max(1) as i64;
                    let max_s = self.max_scroll() as i64;
                    let ny = s0 as i64 + (y - sy) as i64 * max_s / denom;
                    self.scroll_to(ny.max(0) as isize, inv);
                }
                return true;
            }
            Some(BarDrag::H(sx, x0)) => {
                if let Some(t) = self.hthumb_rect(true) {
                    let denom = (self.bounds.w - t.w).max(1) as i64;
                    let max_x = (self.total_w() - self.bounds.w).max(0) as i64;
                    let nx = (x0 as i64 + (x - sx) as i64 * max_x / denom) as i32;
                    self.hscroll_to(nx, inv);
                }
                return true;
            }
            None => {}
        }
        // 호버는 **보이는** 축의 썸에만(숨겨진 바 자리에 올려도 드러나지 않음 — 축 독립 규약)
        let over = if self.axis_visible(Axis::V) && self.in_thumb(x, y) {
            Some(Axis::V)
        } else if self.axis_visible(Axis::H) && self.in_hthumb(x, y) {
            Some(Axis::H)
        } else {
            None
        };
        if over != self.bar_hover {
            let prev = self.bar_hover;
            self.bar_hover = over;
            if let Some(ax) = prev {
                self.flash_bar(ax, inv); // 이탈 = 유지 → 페이드 재개
            }
            if let Some(ax) = over {
                self.push_bar_strip(ax, inv);
            }
        }
        false
    }

    /// 가로 오프셋 설정(클램프) + 가로 바 표시.
    fn hscroll_to(&mut self, x: i32, inv: &mut Invalidations) {
        let old = self.scroll_x;
        self.scroll_x = x;
        self.clamp_scroll_x();
        if self.scroll_x != old {
            inv.push(self.bounds);
            self.flash_bar(Axis::H, inv);
        }
    }

    /// MouseDown — 썸 = 드래그 시작, 트랙 = 페이지 이동(그 축이 표시 중일 때만). true = 소비.
    fn bar_mouse_down(&mut self, x: i32, y: i32, inv: &mut Invalidations) -> bool {
        if y < self.body_top() || y >= self.bounds.bottom() {
            return false;
        }
        if self.axis_visible(Axis::V) {
            if self.in_thumb(x, y) {
                self.bar_drag = Some(BarDrag::V(y, self.scroll_row));
                self.push_bar_strip(Axis::V, inv);
                return true;
            }
            if let Some(t) = self.thumb_rect(true) {
                if x >= t.x - 2 {
                    let full = (self.body_h() / self.grid_h().max(1)).max(1) as isize;
                    let page = (full - 1).max(1);
                    let cur = self.scroll_row as isize;
                    self.scroll_to(if y < t.y { cur - page } else { cur + page }, inv);
                    return true;
                }
            }
        }
        if self.axis_visible(Axis::H) {
            if self.in_hthumb(x, y) {
                self.bar_drag = Some(BarDrag::H(x, self.scroll_x));
                self.push_bar_strip(Axis::H, inv);
                return true;
            }
            if let Some(t) = self.hthumb_rect(true) {
                if y >= t.y - 2 {
                    let page = self.bounds.w.max(1);
                    let cur = self.scroll_x;
                    self.hscroll_to(if x < t.x { cur - page } else { cur + page }, inv);
                    return true;
                }
            }
        }
        false
    }

    /// MouseUp — 드래그 종료(true = 소비).
    fn bar_mouse_up(&mut self, inv: &mut Invalidations) -> bool {
        match self.bar_drag.take() {
            Some(BarDrag::V(..)) => self.flash_bar(Axis::V, inv),
            Some(BarDrag::H(..)) => self.flash_bar(Axis::H, inv),
            None => return false,
        }
        true
    }

    /// 오버레이 썸 페인트(본문 위 마지막 — 알파 합성). 축별로 보이는 것만.
    fn paint_bar(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        if self.axis_visible(Axis::V) {
            let hot = self.axis_hot(Axis::V);
            if let Some(t) = self.thumb_rect(hot) {
                if matches!(self.bar_drag, Some(BarDrag::V(..))) {
                    ctx.fill_round_rect_alpha(
                        Rect::new(t.x - 1, self.body_top(), t.w + 2, self.body_h()),
                        0,
                        theme.text,
                        28,
                    );
                }
                let alpha = if hot {
                    BAR_ALPHA_HOT
                } else {
                    self.bar_alpha[0]
                };
                ctx.fill_round_rect_alpha(t, t.w / 2, theme.text, alpha);
            }
        }
        if self.axis_visible(Axis::H) {
            let hot = self.axis_hot(Axis::H);
            if let Some(t) = self.hthumb_rect(hot) {
                if matches!(self.bar_drag, Some(BarDrag::H(..))) {
                    ctx.fill_round_rect_alpha(
                        Rect::new(self.bounds.x, t.y - 1, self.bounds.w, t.h + 2),
                        0,
                        theme.text,
                        28,
                    );
                }
                let alpha = if hot {
                    BAR_ALPHA_HOT
                } else {
                    self.bar_alpha[1]
                };
                ctx.fill_round_rect_alpha(t, t.h / 2, theme.text, alpha);
            }
        }
    }

    pub fn caret(&self) -> Option<usize> {
        self.caret
    }

    pub fn scroll_row(&self) -> usize {
        self.scroll_row
    }

    /// 뷰포트 행 구간 `[start, end)`(부분 행 포함·소스 길이로 클램프) — 호스트의
    /// "눈에 보이는 영역" 판정(X-44 — 화면에 보이는 펼침 폴더만 저비용 프로브).
    pub fn viewport(&self) -> (usize, usize) {
        let start = self.scroll_row;
        (start, (start + self.visible_rows()).min(self.src.len()))
    }

    pub fn scroll_x(&self) -> i32 {
        self.scroll_x
    }

    /// 데이터 공급자 접근(호스트가 트리 상태를 조회할 때).
    pub fn source(&self) -> &S {
        &self.src
    }

    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// 현재 정렬 상태(우선순위 순).
    pub fn sort(&self) -> &[(u32, bool)] {
        &self.sort
    }

    pub fn set_columns(&mut self, columns: Vec<Column>, inv: &mut Invalidations) {
        self.columns = columns;
        self.clamp_scroll_x();
        inv.push(self.bounds);
    }

    /// DPI 변화 등에 따른 행 높이·패딩·들여쓰기 갱신(WM_DPICHANGED 경로).
    pub fn set_metrics(&mut self, row_h: i32, pad_x: i32, indent_w: i32, inv: &mut Invalidations) {
        let row_h = row_h.max(1);
        let indent_w = indent_w.max(1);
        if self.row_h != row_h || self.pad_x != pad_x || self.indent_w != indent_w {
            self.row_h = row_h;
            self.pad_x = pad_x;
            self.indent_w = indent_w;
            self.clamp_scroll();
            inv.push(self.bounds);
        }
    }

    // ── 기하 ─────────────────────────────────────────────────────

    /// 헤더 높이(컬럼 없으면 0 — M1-3 호환).
    fn header_h(&self) -> i32 {
        if self.columns.is_empty() || self.mode == ViewMode::Tiles {
            0 // 타일 보기 = 컬럼 헤더 없음(탐색기 규약)
        } else {
            self.row_h
        }
    }

    fn body_top(&self) -> i32 {
        self.bounds.y + self.header_h()
    }

    fn body_h(&self) -> i32 {
        (self.bounds.h - self.header_h()).max(0)
    }

    /// 첫 가시 행(scroll_row)의 y — 부분 행 오프셋만큼 본문 상단보다 위(10-02 픽셀 스크롤).
    fn row_origin(&self) -> i32 {
        self.body_top() - self.scroll_frac
    }

    /// 픽셀 단위 세로 스크롤(트랙패드 — 10-02): 행 + 부분 오프셋을 px 위치로 환산해 이동·클램프.
    fn scroll_by_px(&mut self, dy: i32, inv: &mut Invalidations) {
        let gh = self.grid_h().max(1) as i64;
        let max_px = self.max_scroll() as i64 * gh;
        let pos =
            (self.scroll_row as i64 * gh + self.scroll_frac as i64 + dy as i64).clamp(0, max_px);
        let (row, frac) = ((pos / gh) as usize, (pos % gh) as i32);
        if row != self.scroll_row || frac != self.scroll_frac {
            self.scroll_row = row;
            self.scroll_frac = frac;
            inv.push(self.bounds);
            self.flash_bar(Axis::V, inv);
        }
    }

    /// 전체 컬럼 폭 합(컬럼 없으면 위젯 폭).
    fn total_w(&self) -> i32 {
        if self.columns.is_empty() {
            self.bounds.w
        } else {
            self.columns.iter().map(|c| c.width).sum()
        }
    }

    /// 컬럼 `i`의 x 시작(스크롤 반영).
    fn col_x(&self, i: usize) -> i32 {
        let before: i32 = self.columns[..i].iter().map(|c| c.width).sum();
        self.bounds.x - self.scroll_x + before
    }

    /// 트리(이름) 열의 (x, 폭) — **현재 열 순서** 기준(A29 — X3-07). 컬럼 미설정이면
    /// 위젯 전체 폭(M1-3 호환), `key == 0` 열이 숨겨졌으면 `None`. 이름변경 필드·
    /// 마커 존·페인트(`col.key == 0`)가 같은 열을 가리키도록 하는 단일 기하.
    fn tree_col(&self) -> Option<(i32, i32)> {
        if self.columns.is_empty() {
            return Some((self.bounds.x, self.bounds.w));
        }
        self.columns
            .iter()
            .position(|c| c.key == 0)
            .map(|i| (self.col_x(i), self.columns[i].width))
    }

    /// 컬럼 총폭의 오른쪽 경계 — 이 오른쪽은 **빈 본문**으로 판정(행 아님. 원본 B-4
    /// "행 히트영역=컬럼 총폭" — 클릭=해제·드래그=러버밴드, QA 07-13).
    fn columns_right(&self) -> i32 {
        if self.columns.is_empty() {
            self.bounds.right()
        } else {
            self.col_x(self.columns.len() - 1) + self.columns.last().map_or(0, |c| c.width)
        }
    }

    /// 현재 높이에서 그릴 그리드 행 수(부분 행 포함).
    fn visible_rows(&self) -> usize {
        let gh = self.grid_h();
        ((self.body_h() + self.scroll_frac + gh - 1) / gh).max(0) as usize
    }

    /// 스크롤 상한 = 전체 그리드 행 - 완전 가시 그리드 행 수.
    fn max_scroll(&self) -> usize {
        let full = (self.body_h() / self.grid_h()).max(0) as usize;
        self.grid_len().saturating_sub(full)
    }

    fn clamp_scroll(&mut self) {
        let max = self.max_scroll();
        if self.scroll_row >= max {
            self.scroll_row = max;
            self.scroll_frac = 0;
        }
        self.scroll_frac = self.scroll_frac.clamp(0, self.grid_h().max(1) - 1);
        self.clamp_scroll_x();
    }

    fn clamp_scroll_x(&mut self) {
        let max_x = (self.total_w() - self.bounds.w).max(0);
        self.scroll_x = self.scroll_x.clamp(0, max_x);
    }

    fn scroll_to(&mut self, target: isize, inv: &mut Invalidations) {
        let clamped = target.clamp(0, self.max_scroll() as isize) as usize;
        if clamped != self.scroll_row || self.scroll_frac != 0 {
            self.scroll_row = clamped;
            self.scroll_frac = 0; // 행 단위 이동 = 부분 오프셋 스냅
            inv.push(self.bounds); // 전 행 이동 — 위젯 영역 전체 무효화
            self.flash_bar(Axis::V, inv); // 세로 오버레이 바 표시(09-04)
        }
    }

    /// 행이 보이도록 세로 스크롤 조정(원본 ScrollIndexIntoView 대응).
    /// 그리드(타일)에서는 항목의 그리드 행 기준.
    fn scroll_into_view(&mut self, row: usize) {
        self.scroll_into_view_idx(row);
    }

    fn scroll_into_view_idx(&mut self, index: usize) {
        let grow = index / self.grid_cols().max(1);
        let full = ((self.body_h() / self.grid_h()).max(1)) as usize;
        // 부분 행 오프셋(10-02)을 px로 환산해 완전 가시 여부 판정 — 가려져 있으면 행 단위로 스냅
        let gh = self.grid_h().max(1) as i64;
        let pos = self.scroll_row as i64 * gh + self.scroll_frac as i64;
        let (r0, r1) = (grow as i64 * gh, (grow as i64 + 1) * gh);
        if r0 < pos {
            self.scroll_row = grow;
            self.scroll_frac = 0;
        } else if r1 > pos + self.body_h() as i64 {
            self.scroll_row = (grow + 1).saturating_sub(full);
            self.scroll_frac = 0;
        }
        self.scroll_row = self.scroll_row.min(self.max_scroll());
    }

    /// 캐럿 이동 + 선택 규약(탐색기): 평이동=단일 선택, Shift=범위, Ctrl=캐럿만.
    fn move_caret(&mut self, target: usize, shift: bool, primary: bool, inv: &mut Invalidations) {
        self.caret = Some(target);
        if shift {
            self.src.select(target, SelectOp::RangeTo);
        } else if !primary {
            self.src.select(target, SelectOp::Single);
        }
        let before = self.scroll_row;
        self.scroll_into_view(target);
        inv.push(self.bounds);
        if self.scroll_row != before {
            self.flash_bar(Axis::V, inv); // 키보드 스크롤도 세로 바 표시(09-04)
        }
    }

    /// 가시 목록에서 `row`의 부모 행(더 얕은 깊이의 직전 행). 최상위면 `None`.
    fn parent_row(&self, row: usize) -> Option<usize> {
        let depth = self.src.row(row).depth;
        if depth == 0 {
            return None;
        }
        (0..row).rev().find(|&i| self.src.row(i).depth < depth)
    }

    /// 타입어헤드 검색 실행 — 매치 시 단일 선택+캐럿+스크롤(원본 docs/32 §6).
    fn typeahead_find(&mut self, prefix: &str, include_caret: bool, inv: &mut Invalidations) {
        // find_prefix는 caret "다음"부터 — 현재 행 포함 재평가는 caret-1 기준
        let base = if include_caret {
            self.caret.and_then(|c| c.checked_sub(1))
        } else {
            self.caret
        };
        if let Some(idx) = self.src.find_prefix(base, prefix) {
            self.caret = Some(idx);
            self.src.select(idx, SelectOp::Single);
            self.scroll_into_view(idx);
        }
        inv.push(self.bounds); // 매치 없어도 HUD(버퍼) 갱신
    }

    /// 소스 가변 접근 — 재로드 상태 복원(펼침·선택) 등 호스트 주도 변형용(M3-6 선행).
    pub fn source_mut(&mut self) -> &mut S {
        &mut self.src
    }

    /// 재로드 후 뷰 상태 복원(M3-6 무간섭 갱신 선행) — 캐럿·스크롤(범위 밖은 clamp).
    /// 선택·펼침은 소스([`Self::source_mut`])가 복원한다.
    pub fn restore_view(
        &mut self,
        caret: Option<usize>,
        scroll_row: usize,
        scroll_x: i32,
        inv: &mut Invalidations,
    ) {
        self.caret = caret.filter(|&c| c < self.src.len());
        self.scroll_row = scroll_row;
        self.scroll_x = scroll_x;
        self.clamp_scroll();
        inv.push(self.bounds);
    }

    /// 진행 중 프레스 취소(10-02 X3-03) — 호스트가 OLE 드래그(`dnd::begin_drag`)에서 돌아온
    /// 직후 호출. DoDragDrop이 버튼 해제를 소비해 위젯에 MouseUp이 오지 않으므로 클릭 확정
    /// 보류(`press_pending`)와 밴드를 여기서 지운다. **선택 집합은 불변**(다중 선택 드래그 후
    /// 선택 유지 규약 — 단일화하지 않는다).
    pub fn abort_press(&mut self) {
        self.press_pending = None;
        self.band = None;
    }

    /// 데이터 공급자 교체(네비게이션 — M1-8). 스크롤·캐럿·타입어헤드는 리셋,
    /// 컬럼·정렬 상태는 유지하고 새 소스에 재적용(원본 PanelView.SortKeys 지속 규약).
    pub fn replace_source(&mut self, src: S, inv: &mut Invalidations) {
        self.src = src;
        self.scroll_row = 0;
        self.scroll_x = 0;
        self.caret = None;
        self.band = None;
        self.press_pending = None; // 보조 리셋(10-02 X3-03) — 낡은 인덱스가 새 소스에 작용 금지
        self.typeahead.clear();
        // 위젯 정렬이 **명시된 경우에만** 새 소스에 재적용(07-15 수정) — 빈 상태(미지정)로
        // set_sort(&[])를 호출하면 소스 기본 정렬(이름 오름차순)이 열거 순서로 퇴행해
        // 정렬 옵션(대소문자 등)이 무효화된다. 헤더 3상태 '없음'은 헤더 클릭 경로가 처리.
        let keys = self.sort.clone();
        if !keys.is_empty() {
            self.src.set_sort(&keys);
        }
        self.clamp_scroll();
        inv.push(self.bounds);
    }

    /// 클라이언트 좌표 → 본문 행 인덱스(범위 밖이면 `None`). 호스트의 더블클릭 진입 판정에도 사용.
    /// 마지막 컬럼 오른쪽 공간은 행이 아니라 **빈 본문**(원본 B-4 — QA 07-13).
    pub fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        if !self.bounds.contains(Point { x, y }) || y < self.body_top() {
            return None;
        }
        if self.mode == ViewMode::Tiles {
            let (tw, th) = self.tile_wh();
            let gc = ((x - self.bounds.x) / tw) as usize;
            if gc >= self.grid_cols() {
                return None; // 마지막 열 오른쪽 잔여 = 빈 본문
            }
            let gr = self.scroll_row + ((y - self.row_origin()) / th) as usize;
            let idx = gr * self.grid_cols() + gc;
            return (idx < self.src.len()).then_some(idx);
        }
        if x >= self.columns_right() {
            return None;
        }
        let row = self.scroll_row + ((y - self.row_origin()) / self.row_h) as usize;
        (row < self.src.len()).then_some(row)
    }

    /// DnD 엣지 자동 스크롤(X-32) — 드래그 y가 본문 상/하단 **한 행 높이** 이내면 1행 이동
    /// (탐색기 관례 — 호스트 타이머가 반복 호출해 연속 스크롤). 스크롤했으면 `true`.
    pub fn drag_scroll_edge(&mut self, y: i32, inv: &mut Invalidations) -> bool {
        let top = self.body_top();
        let bottom = self.bounds.bottom();
        if y < top || y >= bottom {
            return false;
        }
        let before = self.scroll_row;
        if y < top + self.row_h {
            self.scroll_to(self.scroll_row as isize - 1, inv);
        } else if y >= bottom - self.row_h {
            self.scroll_to(self.scroll_row as isize + 1, inv);
        }
        self.scroll_row != before
    }

    /// 접힌 폴더 행인가(X-32 — DnD 호버 펼침 후보 판정). 트리 보기 전용
    /// (Flat/Tiles는 펼침 개념 없음).
    pub fn is_collapsed_dir(&self, row: usize) -> bool {
        self.mode == ViewMode::Tree
            && row < self.src.len()
            && self.src.row(row).marker == Marker::Collapsed
    }

    /// DnD 호버 펼침(X-32) — 접힌 폴더 행이면 펼친다(접기 없음 — 토글 아님).
    /// 펼쳤으면 `true`(목록 구조 변경 — 전체 무효화).
    pub fn hover_expand(&mut self, row: usize, inv: &mut Invalidations) -> bool {
        if !self.is_collapsed_dir(row) {
            return false;
        }
        if self.src.toggle(row) {
            self.clamp_scroll();
            inv.push(self.bounds);
            true
        } else {
            false
        }
    }

    /// 행의 클라이언트 앵커 좌표(가시 범위 내일 때만) — 키보드 컨텍스트 메뉴 위치(M3-4 Apps 키).
    pub fn row_anchor(&self, row: usize) -> Option<Point> {
        if row >= self.src.len() {
            return None;
        }
        if self.mode == ViewMode::Tiles {
            let rc = self.tile_rect(row);
            let visible = rc.y >= self.body_top() && rc.bottom() <= self.bounds.bottom();
            return visible.then_some(Point {
                x: rc.x + self.pad_x,
                y: rc.y + rc.h / 2,
            });
        }
        if row < self.scroll_row {
            return None;
        }
        let y = self.row_origin() + ((row - self.scroll_row) as i32) * self.row_h;
        (y >= self.body_top() && y + self.row_h <= self.bounds.bottom()).then_some(Point {
            x: self.bounds.x + self.pad_x,
            y: y + self.row_h / 2,
        })
    }

    /// 좌표가 본문(헤더 아래) 영역인가 — 호스트의 빈 영역 판정(M3-4 배경 셸 메뉴).
    pub fn in_body(&self, x: i32, y: i32) -> bool {
        self.bounds.contains(Point { x, y }) && y >= self.body_top()
    }

    /// 좌표가 펼침 마커 위인가 — 호스트가 더블클릭 진입과 마커 토글을 구분할 때 사용.
    pub fn marker_hit(&self, x: i32, y: i32) -> bool {
        self.row_at(x, y)
            .is_some_and(|row| self.in_marker_zone(row, x))
    }

    /// 클릭 x가 해당 행의 펼침 마커 영역인가(트리 컬럼 안 들여쓰기 자리·마커 있는 행만).
    fn in_marker_zone(&self, row: usize, x: i32) -> bool {
        if self.mode != ViewMode::Tree {
            return false; // 일반/타일 보기 = 인라인 펼침 없음(07-16)
        }
        let Some((tc_x, tc_w)) = self.tree_col() else {
            return false; // 트리 열 숨김
        };
        let item = self.src.row(row);
        if item.marker == Marker::None {
            return false;
        }
        let indent = tc_x + self.pad_x + item.depth as i32 * self.indent_w;
        x >= indent && x < (indent + self.indent_w).min(tc_x + tc_w)
    }

    /// 헤더 명중 판정: `Some((컬럼 인덱스, 리사이즈 핸들 여부))`.
    fn header_hit(&self, x: i32, y: i32) -> Option<(usize, bool)> {
        if self.columns.is_empty() || !self.bounds.contains(Point { x, y }) || y >= self.body_top()
        {
            return None;
        }
        // 핸들 우선: 컬럼 오른쪽 경계 [right-6, right+2)
        for i in 0..self.columns.len() {
            let right = self.col_x(i) + self.columns[i].width;
            if self.columns[i].resizable && x >= right - RESIZE_ZONE_L && x < right + RESIZE_ZONE_R
            {
                return Some((i, true));
            }
        }
        for i in 0..self.columns.len() {
            let cx = self.col_x(i);
            if x >= cx && x < cx + self.columns[i].width {
                return Some((i, false));
            }
        }
        None
    }

    // ── 정렬 (원본 docs/23 §4: 3상태 순환·Shift 다중열) ────────────

    fn dir_of(&self, key: u32) -> Option<bool> {
        self.sort.iter().find(|(k, _)| *k == key).map(|(_, d)| *d)
    }

    /// 헤더 클릭 정렬. 단순 클릭 = 단일 정렬 리셋 + 3상태 순환(없음→▲→▼→없음).
    /// Shift+클릭 & 기존 정렬 ≥1 = 키 추가/방향 순환/제거(다중열).
    fn apply_sort(&mut self, key: u32, shift: bool, inv: &mut Invalidations) {
        let cur = self.dir_of(key);
        if shift && !self.sort.is_empty() {
            match cur {
                None => self.sort.push((key, false)), // 추가 = 오름
                Some(false) => {
                    if let Some(e) = self.sort.iter_mut().find(|(k, _)| *k == key) {
                        e.1 = true;
                    }
                }
                Some(true) => self.sort.retain(|(k, _)| *k != key), // 없음 = 제거(순번 당김)
            }
        } else {
            self.sort = match cur {
                None => vec![(key, false)],
                Some(false) => vec![(key, true)],
                Some(true) => Vec::new(), // 없음 = 열거 순서
            };
        }
        let keys = self.sort.clone();
        self.src.set_sort(&keys);
        self.clamp_scroll(); // 정렬로 행 수는 불변이지만 방어
        inv.push(self.bounds); // 헤더 글리프 + 본문 전체
    }

    // ── 페인트 보조 ──────────────────────────────────────────────

    /// 트리 컬럼(마커+들여쓰기+아이콘+이름)을 `cell` 안에 그린다.
    /// `fg` = 이름 색(잘라내기 대기 행은 text_dim — X-32 흐림 표시).
    #[allow(clippy::too_many_arguments)] // 행 페인트 색 2종(bg/fg) 전달(구조체화는 후속)
    fn paint_tree_cell(
        &self,
        ctx: &mut dyn DrawCtx,
        theme: &Theme,
        item: &RowItem,
        icon: Option<&(String, String)>,
        cell: Rect,
        bg: crate::theme::Color,
        fg: crate::theme::Color,
    ) {
        // 텍스트 세로 위치: 행 높이의 4/5를 글자 높이로 보고 중앙 정렬(M0-7 계승)
        let ty = cell.y + (cell.h - (cell.h * 4) / 5) / 2;
        let indent = cell.x + self.pad_x + item.depth as i32 * self.indent_w;
        // 셀 배경 선도장(기존 마커 text_opaque의 전체 셀 필 규약 유지) 후
        // 디스클로저 = MDL2 글리프(glyph_opaque — 07-18 원본 규약)
        ctx.text_opaque(indent, ty, cell, "", theme.text_dim, bg);
        if item.marker != Marker::None {
            let mrc = Rect::new(indent, cell.y, self.indent_w, cell.h);
            if marker_vector() {
                // 선 쉐브론(123차): 글꼴과 무관한 크기·모양 · 굵기 = 칸 폭 16당 1 px.
                ctx.fill_rect(mrc, bg);
                let pts = marker_chevron_points(mrc, item.marker == Marker::Expanded);
                let width = (self.indent_w as f32 / 16.0).max(1.0);
                ctx.polyline(&pts, theme.text_dim, width);
            } else {
                ctx.glyph_opaque(mrc, item.marker.glyph(), theme.text_dim, bg);
            }
        }
        let mut name_x = indent + self.indent_w;
        if let Some((key, hint)) = icon {
            // 아이콘 크기 = 들여쓰기 폭(16px@96dpi) — 셸 스몰 아이콘 규격
            let isz = self.indent_w;
            let iy = cell.y + (cell.h - isz) / 2;
            ctx.draw_icon(name_x, iy, isz, key, hint);
            name_x += isz + self.pad_x / 2;
        }
        if name_x < cell.right() {
            let name_rc = Rect::new(name_x, cell.y, cell.right() - name_x, cell.h);
            // 폴더 이름 굵게(X-12) — 폴더 행만(트리 보기 한정 = 현행 유지. X-43:
            // 빈 폴더는 marker가 억제되므로 is_dir로 판정)
            let folder_bold = self.font_decor.0 && item.is_dir && self.mode == ViewMode::Tree;
            if folder_bold {
                ctx.select_font(crate::draw::FontSlot::List, true, false);
            }
            // 칸보다 긴 이름 = 끝 말줄임(154차 · dir2 RENDER-010 — 종전 = 글자 중간에서 잘렸다). 굵은 글꼴을 고른 뒤에 잰다.
            let shown = crate::draw::ellipsize_end(ctx, &item.text, name_rc.w);
            ctx.text_opaque(name_x, ty, name_rc, &shown, fg, bg);
            if folder_bold {
                ctx.select_font(crate::draw::FontSlot::List, false, false);
            }
        }
    }

    /// 드래그 x에서 가장 가까운 컬럼 경계 index(0..=len — 삽입 위치).
    fn nearest_boundary(&self, x: i32) -> usize {
        let n = self.columns.len();
        let mut best = 0usize;
        let mut best_d = i32::MAX;
        for i in 0..=n {
            let bx = if i < n {
                self.col_x(i)
            } else {
                self.columns_right()
            };
            let d = (x - bx).abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        best
    }

    /// ESC = 드래그 취소(07-19 사용자) — 시작 시 순서(orig)로 복원.
    /// 활성 드래그가 있었으면 `true`(호스트가 키 소비).
    pub fn cancel_col_drag(&mut self, inv: &mut Invalidations) -> bool {
        let Some(d) = self.col_drag.take() else {
            return false;
        };
        if !d.active {
            return false;
        }
        // orig key 순서로 재배열(폭 등 Column 값은 그대로 이동)
        let mut restored: Vec<Column> = Vec::with_capacity(self.columns.len());
        for k in &d.orig {
            if let Some(pos) = self.columns.iter().position(|c| c.key == *k) {
                restored.push(self.columns.remove(pos));
            }
        }
        restored.append(&mut self.columns); // 방어(orig에 없던 열)
        self.columns = restored;
        self.clamp_scroll_x();
        inv.push(self.bounds);
        true
    }

    /// 컬럼 순서 드래그 변경 수거(1회성 — 호스트가 탭 상속/좌우 동기, 07-19).
    pub fn take_col_reordered(&mut self) -> bool {
        std::mem::take(&mut self.col_reordered)
    }

    /// 컬럼 리사이즈 발생 여부 수거(1회성 — 호스트가 폭 동기화에 사용, 07-18).
    pub fn take_col_resized(&mut self) -> bool {
        std::mem::take(&mut self.col_resized)
    }

    /// 컬럼 폭 일괄 적용(순서 대응·개수 부족분 무시 — 패널 상속/좌우 동기, 07-18).
    pub fn set_col_widths(&mut self, widths: &[i32], inv: &mut Invalidations) {
        let mut changed = false;
        for (c, w) in self.columns.iter_mut().zip(widths) {
            let w = (*w).max(c.min_width);
            if c.width != w {
                c.width = w;
                changed = true;
            }
        }
        if changed {
            self.clamp_scroll_x();
            inv.push(self.bounds);
        }
    }

    /// 좌표가 헤더 행인가(07-19 — 우클릭 컬럼 설정 팝업 판정).
    pub fn header_area(&self, x: i32, y: i32) -> bool {
        self.mode != ViewMode::Tiles
            && !self.columns.is_empty()
            && self.bounds.contains(Point { x, y })
            && y < self.body_top()
    }

    /// 더블클릭 auto-fit 대상(07-19 사용자): 헤더 **리사이즈 핸들** 좌표면
    /// 해당 컬럼 index(타일 보기 = 헤더 없음 — 제외).
    pub fn autofit_col_at(&self, x: i32, y: i32) -> Option<usize> {
        if self.mode == ViewMode::Tiles {
            return None;
        }
        match self.header_hit(x, y) {
            Some((i, true)) if self.columns[i].resizable => Some(i),
            _ => None,
        }
    }

    /// auto-fit 측정 자료(07-19): 대상 컬럼의 **가시 행 + 헤더** 텍스트와
    /// 텍스트 외 부가 폭(들여쓰기·아이콘·패딩) 목록 — 폭 실측은 호스트
    /// (DwCtx text_width, List 폰트)가 수행.
    pub fn autofit_texts(&self, col: usize) -> Vec<(String, i32)> {
        let Some(c) = self.columns.get(col) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        // 헤더 라벨(정렬 화살표·순번 포함 — 끝 정렬 모양이면 표시 + 사이 여백만큼 더).
        let label = match self.sort_mark(c.key) {
            Some(mark) => format!("{} {mark}", self.header_label(c)),
            None => self.header_label(c),
        };
        out.push((label, self.pad_x * 2));
        let first = self.scroll_row;
        let count = self
            .visible_rows()
            .min(self.src.len().saturating_sub(first));
        for i in 0..count {
            let row = first + i;
            if c.key == 0 {
                // 트리 셀: paint_tree_cell 배치 재현 — pad + depth·마커 존 +
                // (아이콘 폭 + pad/2) + 텍스트 + 우측 pad
                let item = self.src.row(row);
                let mut extra =
                    self.pad_x + item.depth as i32 * self.indent_w + self.indent_w + self.pad_x;
                if self.src.icon(row).is_some() {
                    extra += self.indent_w + self.pad_x / 2;
                }
                out.push((item.text, extra));
            } else {
                out.push((self.src.cell(row, c.key), self.pad_x * 2));
            }
        }
        out
    }

    /// 단일 컬럼 폭 적용(min 클램프·`col_resized` 마크 — auto-fit 07-19).
    /// 최대 제한은 호스트가 설정값으로 적용 후 호출.
    pub fn set_col_width(&mut self, col: usize, w: i32, inv: &mut Invalidations) {
        if let Some(c) = self.columns.get_mut(col) {
            let w = w.max(c.min_width);
            if c.width != w {
                c.width = w;
                self.col_resized = true;
                self.clamp_scroll_x();
                inv.push(self.bounds);
            }
        }
    }

    /// 헤더 셀 제목: ▲/▼는 이름 앞, 정렬 순번(①②…)은 이름 뒤(원본 docs/23 §4).
    /// 순번은 **정렬 시작부터 상시 표시**(사용자 확정 07-18 — 단일 정렬 = ①,
    /// Ctrl/Shift로 추가한 컬럼 = ② 순차).
    fn header_label(&self, col: &Column) -> String {
        if self.sort_mark_trailing {
            return col.title.clone(); // 정렬 표시는 칸 오른쪽 끝에 따로 그린다([`Self::sort_mark`])
        }
        let mut s = String::new();
        if let Some(desc) = self.dir_of(col.key) {
            s.push_str(if desc { "▼ " } else { "▲ " });
        }
        s.push_str(&col.title);
        if let Some(order) = self.sort.iter().position(|(k, _)| *k == col.key) {
            s.push(' ');
            s.push_str(order_badge(order));
        }
        s
    }

    /// 타일 보기 페인트(07-16 — 탐색기 '타일'): 셀 = 아이콘(2행높이 — 32px@96dpi 라지
    /// 아이콘) + 이름/보조 줄(종류 또는 드라이브 용량 텍스트) + 용량 바(내 PC — X-17).
    fn paint_tiles(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.bounds;
        ctx.fill_rect(b, theme.panel_bg);
        let cols = self.grid_cols();
        let first_idx = self.scroll_row * cols;
        let last_idx = ((self.scroll_row + self.visible_rows() + 1) * cols).min(self.src.len());
        let isz = self.row_h * 2 - 4; // 아이콘 변(행높이 20 기준 36 → 라지 32 근사 확대)
        for idx in first_idx..last_idx {
            let rc = self.tile_rect(idx);
            if rc.y >= b.bottom() || rc.bottom() <= self.body_top() {
                continue;
            }
            let selected = self.src.is_selected(idx);
            let bg = if selected {
                if self.focused {
                    theme.sel_bg
                } else {
                    theme.sel_bg_inactive
                }
            } else {
                theme.panel_bg
            };
            // 셀 배경(안쪽 1px 여백 — 타일 간 시각 분리)
            let cell = Rect::new(rc.x + 2, rc.y + 2, rc.w - 4, rc.h - 4);
            ctx.fill_rect(cell, bg);
            // 아이콘(라지 32px — "L|" 키 네임스페이스, icons.rs 로더 분기)
            let item = self.src.row(idx);
            let folder_bold = self.font_decor.0 && item.is_dir; // X-12(판정은 X-43 is_dir)
            let ix = cell.x + self.pad_x;
            let iy = cell.y + (cell.h - isz) / 2;
            if let Some((key, hint)) = self.src.icon(idx) {
                ctx.draw_icon(ix, iy, isz, &format!("L|{key}"), &hint);
            }
            // 텍스트 영역(아이콘 오른쪽): 이름 / 보조 줄 / (있으면) 용량 바
            let tx = ix + isz + self.pad_x;
            let tw_text = (cell.right() - self.pad_x - tx).max(0);
            if tw_text > 0 {
                let (line2, bar) = self.src.tile_info(idx);
                let lh = self.row_h * 4 / 5; // 줄 높이(글자 높이 근사)
                let lines = 1 + (!line2.is_empty() as i32) + (bar.is_some() as i32);
                let mut ly = cell.y + (cell.h - lines * lh - (lines - 1) * 2).max(0) / 2;
                let name_rc = Rect::new(tx, ly, tw_text, lh);
                if folder_bold {
                    ctx.select_font(crate::draw::FontSlot::List, true, false);
                }
                // 잘라내기 대기 타일은 이름 흐리게(X-32 — 리스트와 동일 규약)
                let fg = if self.src.is_ghosted(idx) {
                    theme.text_dim
                } else {
                    theme.text
                };
                ctx.text_opaque(tx, ly, name_rc, &item.text, fg, bg);
                if folder_bold {
                    ctx.select_font(crate::draw::FontSlot::List, false, false);
                }
                ly += lh + 2;
                if let Some(frac) = bar {
                    // 드라이브 용량 바(X-17 — 탐색기 내 PC 타일): 트랙 + 사용분(>90% 경고색)
                    let bar_h = (lh / 2).max(4);
                    let track = Rect::new(tx, ly + (lh - bar_h) / 2, tw_text, bar_h);
                    ctx.fill_rect(track, crate::theme::header_bg(theme));
                    let used_w = ((tw_text as f32) * frac.clamp(0.0, 1.0)) as i32;
                    if used_w > 0 {
                        let col = if frac > 0.9 {
                            crate::theme::Color::from_rgb(0xD3, 0x3F, 0x3F) // 경고(탐색기 빨강)
                        } else {
                            theme.accent
                        };
                        ctx.fill_rect(Rect::new(track.x, track.y, used_w, bar_h), col);
                    }
                    ly += lh + 2;
                }
                if !line2.is_empty() {
                    let rc2 = Rect::new(tx, ly, tw_text, lh);
                    ctx.text_opaque(tx, ly, rc2, &line2, theme.text_dim, bg);
                }
            }
            // 캐럿 테두리(1px — 리스트와 동일 규약)
            if self.caret == Some(idx) {
                let cc = if self.focused {
                    theme.accent
                } else {
                    theme.text_dim
                };
                ctx.fill_rect(Rect::new(cell.x, cell.y, cell.w, 1), cc);
                ctx.fill_rect(Rect::new(cell.x, cell.bottom() - 1, cell.w, 1), cc);
                ctx.fill_rect(Rect::new(cell.x, cell.y, 1, cell.h), cc);
                ctx.fill_rect(Rect::new(cell.right() - 1, cell.y, 1, cell.h), cc);
            }
        }
        // 러버밴드 외곽선(리스트와 동일)
        if let Some(band) = self.band {
            let r = band.rect();
            if r.w > 0 && r.h > 0 {
                ctx.fill_rect(Rect::new(r.x, r.y, r.w, 1), theme.accent);
                ctx.fill_rect(Rect::new(r.x, r.bottom() - 1, r.w, 1), theme.accent);
                ctx.fill_rect(Rect::new(r.x, r.y, 1, r.h), theme.accent);
                ctx.fill_rect(Rect::new(r.right() - 1, r.y, 1, r.h), theme.accent);
            }
        }
        // 타입어헤드 HUD(리스트와 동일 — 위치 규약 공유)
        if self.typeahead.is_active() {
            let label = format!("찾기: {}", self.typeahead.composing());
            let tw_hud = ctx.text_width(&label);
            let hw = tw_hud + self.pad_x * 2;
            let hx = match self.ta_hud_pos % 3 {
                0 => b.x + self.pad_x,
                1 => b.x + (b.w - hw) / 2,
                _ => b.right() - hw - self.pad_x,
            };
            let hy = match self.ta_hud_pos / 3 {
                0 => self.body_top() + self.pad_x,
                1 => b.y + (b.h - self.row_h) / 2,
                _ => b.bottom() - self.row_h - self.pad_x,
            };
            let hud = Rect::new(hx, hy, hw, self.row_h);
            let hty = hud.y + (self.row_h - (self.row_h * 4) / 5) / 2;
            ctx.text_opaque(
                hud.x + self.pad_x,
                hty,
                hud,
                &label,
                theme.text,
                crate::theme::header_bg(theme),
            );
            ctx.fill_rect(Rect::new(hud.x, hud.y, hud.w, 1), theme.accent);
            ctx.fill_rect(Rect::new(hud.x, hud.bottom() - 1, hud.w, 1), theme.accent);
            ctx.fill_rect(Rect::new(hud.x, hud.y, 1, hud.h), theme.accent);
            ctx.fill_rect(Rect::new(hud.right() - 1, hud.y, 1, hud.h), theme.accent);
        }
    }
}

impl<S: RowSource> Widget for VirtualRows<S> {
    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        if self.bounds != bounds {
            let old = self.bounds;
            self.bounds = bounds;
            self.clamp_scroll();
            inv.push(old.union(&bounds));
        }
    }

    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        // 새 프레스 = 이전 보류 폐기(10-02 G6-01·X3-03) — OLE 드래그(DoDragDrop 모달)가
        // 버튼 해제를 소비하면 MouseUp이 오지 않아 `press_pending`이 잔존하고, 다음 클릭의
        // MouseUp이 낡은 행을 단일 선택해 선택이 되돌아가던 결함. 선택 집합은 건드리지 않는다.
        if matches!(
            *ev,
            InputEvent::MouseDown { .. } | InputEvent::RightDown { .. }
        ) {
            self.press_pending = None;
        }
        // 오버레이 스크롤바(09-04 X-47) — 썸 드래그/트랙 클릭/호버는 다른 처리보다 우선
        match *ev {
            InputEvent::MouseMove { x, y } => {
                if self.bar_mouse_move(x, y, inv) {
                    return;
                }
            }
            InputEvent::MouseDown { x, y, .. } => {
                if self.bar_mouse_down(x, y, inv) {
                    return;
                }
            }
            InputEvent::MouseUp { .. } if self.bar_mouse_up(inv) => return,
            _ => {}
        }
        // 인라인 이름변경 중: 문자는 버퍼로, 키 네비는 차단, 필드 안 클릭=캐럿 배치·
        // 밖 클릭=취소 후 정상 처리(M3-2·QA 07-13)
        if self.is_renaming() {
            match *ev {
                InputEvent::Char { c, .. } => {
                    self.rename_char(c, inv);
                    return;
                }
                InputEvent::Key { .. } => return, // Enter/Esc·편집 키는 호스트가 라우팅
                InputEvent::MouseDown { x, y, .. } => {
                    if let Some((_, es)) = &mut self.rename {
                        if es.hit(x, y) {
                            es.click(x);
                            inv.push(self.bounds);
                            return;
                        }
                    }
                    self.cancel_rename(inv);
                }
                InputEvent::MouseMove { x, .. } => {
                    // 드래그 선택(click~release — QA 07-13)
                    if let Some((_, es)) = &mut self.rename {
                        if es.drag(x) {
                            inv.push(self.bounds);
                        }
                        return;
                    }
                }
                InputEvent::MouseUp { .. } => {
                    if let Some((_, es)) = &mut self.rename {
                        es.release();
                        return;
                    }
                }
                _ => {}
            }
        }
        let cur = self.scroll_row as isize;
        let page = (self.body_h() / self.row_h).max(1) as isize;
        match *ev {
            InputEvent::Wheel { delta } => {
                if delta.abs() >= crate::event::WHEEL_DELTA {
                    // 마우스 노치 = 행 단위(시스템 줄 수) + 고속 스크롤 배수
                    let lines = self.wheel.add(delta, crate::event::wheel_lines());
                    let lines = self.fast.wheel(delta, lines) as isize;
                    if lines != 0 {
                        self.scroll_to(cur - lines, inv);
                    }
                } else {
                    // 정밀 터치패드(노치 미만 delta) = **픽셀** 스크롤(10-02 사용자 — 조금 움직여도 즉시)
                    let px = self
                        .wheel_px
                        .add(delta, crate::event::wheel_lines() * self.grid_h());
                    if px != 0 {
                        self.scroll_by_px(-px, inv);
                    }
                }
                if self.fast.hud_visible() {
                    inv.push(self.bounds);
                    inv.request_tick();
                }
            }
            InputEvent::HWheel { delta } => {
                // 가로 = 항상 픽셀(노치당 줄 수×16px · 트랙패드는 비례)
                let px = self
                    .hwheel
                    .add(delta, crate::event::wheel_lines() * HSCROLL_PX);
                let px = self.fast.wheel(delta, px);
                if px != 0 {
                    let x = self.scroll_x + px;
                    self.hscroll_to(x, inv); // 가로 오버레이 바 표시(09-04)
                }
                if self.fast.hud_visible() {
                    inv.push(self.bounds);
                    inv.request_tick();
                }
            }
            InputEvent::Key {
                key,
                shift,
                primary,
            } => {
                let len = self.src.len();
                if len == 0 {
                    return;
                }
                // 타입어헤드 입력 중(157차): ↑/↓ = 일치 항목 사이로만 이동 · Esc = 입력 취소. 수식키가 있으면 평소 이동.
                if !shift && !primary {
                    match key {
                        Key::Down if self.typeahead_cycle(true, inv) => return,
                        Key::Up if self.typeahead_cycle(false, inv) => return,
                        Key::Escape if self.typeahead_cancel(inv) => return,
                        _ => {}
                    }
                }
                let caret = self.caret.unwrap_or(self.scroll_row).min(len - 1);
                // 고속 스크롤(10-02): ↑/↓ 자동 반복이 짧은 간격으로 이어지면 한 번에 k행
                let k = match key {
                    Key::Up => self.fast.key(-1) as isize,
                    Key::Down => self.fast.key(1) as isize,
                    _ => 1,
                };
                if self.fast.hud_visible() {
                    inv.push(self.bounds);
                    inv.request_tick();
                }
                // 타일 그리드(07-16): ↑/↓ = ±열 수, ←/→ = ∓1/+1 (탐색기 아이콘 뷰 규약)
                if self.mode == ViewMode::Tiles {
                    let cols = self.grid_cols() as isize;
                    let cur = caret as isize;
                    let target = match key {
                        Key::Up => cur - cols * k,
                        Key::Down => cur + cols * k,
                        Key::Left => cur - 1,
                        Key::Right => cur + 1,
                        Key::PageUp => cur - page * cols,
                        Key::PageDown => cur + page * cols,
                        Key::Home => 0,
                        Key::End => len as isize - 1,
                        Key::Space => {
                            if !self.ta_space || !self.typeahead.is_active() {
                                self.caret = Some(caret);
                                self.src.select(caret, SelectOp::Toggle);
                                inv.push(self.bounds);
                            }
                            return;
                        }
                        // nexa-ctl Key의 그 밖 변형(Enter·Esc·Delete·단어 이동)은 호스트가 명령으로 처리한다.
                        _ => return,
                    }
                    .clamp(0, len as isize - 1) as usize;
                    self.move_caret(target, shift, primary, inv);
                    return;
                }
                match key {
                    // 캐럿 이동(탐색기 규약: 평이동=단일 선택·Shift=범위·Ctrl=캐럿만)
                    Key::Up | Key::Down | Key::PageUp | Key::PageDown | Key::Home | Key::End => {
                        let cur = caret as isize;
                        let target = match key {
                            Key::Up => cur - k,
                            Key::Down => cur + k,
                            Key::PageUp => cur - page,
                            Key::PageDown => cur + page,
                            Key::Home => 0,
                            _ => len as isize - 1, // End
                        }
                        .clamp(0, len as isize - 1) as usize;
                        self.move_caret(target, shift, primary, inv);
                    }
                    // → = 펼침, 이미 펼침이면 첫 자식으로(docs/07 §8) — Flat = 무동작
                    Key::Right if self.mode == ViewMode::Flat => {}
                    Key::Left if self.mode == ViewMode::Flat => {}
                    Key::Right => {
                        let item = self.src.row(caret);
                        match item.marker {
                            Marker::Collapsed => {
                                if self.src.toggle(caret) {
                                    self.caret = Some(caret);
                                    self.clamp_scroll();
                                    inv.push(self.bounds);
                                }
                            }
                            Marker::Expanded => {
                                if caret + 1 < len && self.src.row(caret + 1).depth > item.depth {
                                    self.move_caret(caret + 1, shift, primary, inv);
                                }
                            }
                            Marker::None => {}
                        }
                    }
                    // ← = 접힘, 접힘/파일이면 부모로(docs/07 §8)
                    Key::Left => {
                        if self.src.row(caret).marker == Marker::Expanded {
                            if self.src.toggle(caret) {
                                self.caret = Some(caret);
                                self.clamp_scroll();
                                inv.push(self.bounds);
                            }
                        } else if let Some(parent) = self.parent_row(caret) {
                            self.move_caret(parent, shift, primary, inv);
                        }
                    }
                    // Space/Ctrl+Space = 캐럿 행 선택 토글(docs/32 §7 결정 1)
                    // 접두사 입력 중 + 공백 포함 옵션이면 토글 대신 문자(Char 경로 처리)
                    Key::Space if !self.ta_space || !self.typeahead.is_active() => {
                        self.caret = Some(caret);
                        self.src.select(caret, SelectOp::Toggle);
                        inv.push(self.bounds);
                    }
                    // nexa-ctl Key의 그 밖 변형(Enter·Esc·Delete·단어 이동)은 호스트가 명령으로 처리한다.
                    _ => {}
                }
            }
            InputEvent::Char { c, now_ms } => {
                self.ta_clock = self.ta_clock.max(now_ms);
                if self.ta_off {
                    return;
                }
                if c == '\u{8}' {
                    // Backspace — 접두사 축소(옵션 off면 무시 — 원본 §7 체크), 비면 HUD 소거
                    if self.ta_backspace {
                        match self.typeahead.backspace(now_ms) {
                            Some(q) => self.typeahead_find(&q.prefix, q.include_caret, inv),
                            None => inv.push(self.bounds),
                        }
                    }
                } else if c == ' ' {
                    // 공백 = 접두사 입력 중일 때만 문자(원본 "Include Space while typing")
                    if self.ta_space && self.typeahead.is_active() {
                        let q = self.typeahead.push(c, now_ms);
                        self.typeahead_find(&q.prefix, q.include_caret, inv);
                    }
                } else if !c.is_control()
                    && (self.ta_special || c.is_alphanumeric() || nexa_ctl::hangul::is_jamo(c))
                {
                    let q = self.typeahead.push(c, now_ms);
                    self.typeahead_find(&q.prefix, q.include_caret, inv);
                }
            }
            InputEvent::SelectAll => {
                if self.src.select_all() {
                    inv.push(self.bounds);
                }
            }
            InputEvent::MouseDown {
                x,
                y,
                shift,
                primary,
            } => {
                if let Some((i, handle)) = self.header_hit(x, y) {
                    if handle {
                        // 단독 조절(사용자 확정 07-22) — 이웃 불변·총폭 가변
                        self.resize = Some(ResizeDrag {
                            col: i,
                            start_x: x,
                            start_w: self.columns[i].width,
                        });
                    } else {
                        // 헤더 라벨 프레스 = 드래그 재배열 후보(07-19 — 탭과
                        // 동일 UX). 정렬은 MouseUp 무드래그 클릭에서 확정
                        // (다중열 = Shift 전용 — 07-18 규약 유지).
                        self.col_drag = Some(ColDrag {
                            col: i,
                            press_x: x,
                            cur_x: x,
                            active: false,
                            shift,
                            orig: self.columns.iter().map(|c| c.key).collect(),
                        });
                    }
                } else if let Some(row) = self.row_at(x, y) {
                    if self.in_marker_zone(row, x) {
                        // 삼각형 = 인라인 펼침/접힘(docs/07 §1-1) — 선택과 분리
                        if self.src.toggle(row) {
                            self.clamp_scroll();
                            inv.push(self.bounds);
                        }
                    } else {
                        let was_selected = self.src.is_selected(row);
                        if was_selected && !shift && !primary {
                            // 기선택 행 프레스 = **선택 유지**(다중 선택 드래그 DnD — 탐색기
                            // 규약, QA 07-13). 단일화는 클릭 확정(MouseUp·무드래그)에서.
                            self.caret = Some(row);
                            self.press_pending = Some(row);
                        } else {
                            let op = if shift {
                                SelectOp::RangeTo
                            } else if primary {
                                SelectOp::Toggle
                            } else {
                                SelectOp::Single
                            };
                            self.src.select(row, op);
                            self.caret = Some(row);
                            // **미선택 행** 프레스(수정키 없음) = 러버밴드 시작(원본 B-4 —
                            // 드래그=다중 선택·클릭만=단일 선택)
                            if !was_selected && !shift && !primary {
                                self.band = Some(BandDrag {
                                    ox: x,
                                    oy: y,
                                    cx: x,
                                    cy: y,
                                });
                            }
                        }
                        inv.push(self.bounds); // 하이라이트·캐럿 갱신
                    }
                } else if y >= self.body_top() && self.bounds.contains(Point { x, y }) {
                    // 빈 본문 영역 — 러버밴드 시작(기존 선택 해제)
                    self.band = Some(BandDrag {
                        ox: x,
                        oy: y,
                        cx: x,
                        cy: y,
                    });
                    self.src.clear_selection();
                    inv.push(self.bounds);
                }
            }
            InputEvent::MouseMove { x, y } => {
                if let Some(drag) = self.resize {
                    // 단독 조절(07-22) — min_width 하한만, 이웃 불변·총폭 가변
                    let dx =
                        (x - drag.start_x).max(self.columns[drag.col].min_width - drag.start_w);
                    let w = drag.start_w + dx;
                    if w != self.columns[drag.col].width {
                        self.columns[drag.col].width = w;
                        self.col_resized = true; // 호스트 동기 폴링(07-18)
                        self.clamp_scroll_x();
                        inv.push(self.bounds);
                    }
                } else if let Some(mut d) = self.col_drag.take() {
                    d.cur_x = x;
                    if !d.active && (x - d.press_x).abs() > 5 {
                        d.active = true; // 임계 초과 — 재배열 모드 진입
                    }
                    if d.active {
                        // 라이브 미리보기(07-19 사용자): 스냅 위치로 즉시
                        // 재배열해 "옮겨진 것처럼" 표시 — 확정은 MouseUp,
                        // 취소는 ESC(orig 복원)
                        let ins = self.nearest_boundary(x);
                        let to = if ins > d.col { ins - 1 } else { ins };
                        if to != d.col && to < self.columns.len() {
                            let c = self.columns.remove(d.col);
                            self.columns.insert(to, c);
                            d.col = to;
                        }
                        inv.push(self.bounds); // 고스트·미리보기 갱신
                    }
                    self.col_drag = Some(d);
                } else if let Some(mut band) = self.band {
                    band.cx = x;
                    band.cy = y;
                    self.band = Some(band);
                    // 최소 이동 임계 미만 = 클릭 지터 — 선택 미변경(높이 0 rect가
                    // clear_selection으로 떨어져 선택이 풀리던 결함, QA 07-13 4차)
                    if (band.cx - band.ox).abs() < 4 && (band.cy - band.oy).abs() < 4 {
                        return;
                    }
                    let r = band.rect();
                    if self.mode == ViewMode::Tiles {
                        // 타일 = 밴드 rect와 교차하는 타일만(사각 영역 선택 — 탐색기 규약)
                        self.src.clear_selection();
                        if !self.src.is_empty() {
                            let cols = self.grid_cols();
                            let full = self.visible_rows();
                            let first = self.scroll_row * cols;
                            let last = ((self.scroll_row + full + 1) * cols).min(self.src.len());
                            for idx in first..last {
                                if self.tile_rect(idx).intersects(&r) {
                                    self.src.select(idx, SelectOp::Toggle);
                                }
                            }
                        }
                        inv.push(self.bounds);
                        return;
                    }
                    // 밴드 세로 범위와 교차하는 가시 행 범위로 선택 대체
                    let top = r.y.max(self.body_top());
                    let bot = r.bottom().min(self.bounds.bottom());
                    if bot > top && !self.src.is_empty() {
                        let lo =
                            self.scroll_row + ((top - self.row_origin()) / self.row_h) as usize;
                        let hi = (self.scroll_row
                            + ((bot - 1 - self.row_origin()) / self.row_h) as usize)
                            .min(self.src.len() - 1);
                        if lo <= hi && lo < self.src.len() {
                            self.src.select_span(lo, hi);
                        } else {
                            self.src.clear_selection();
                        }
                    } else {
                        self.src.clear_selection();
                    }
                    inv.push(self.bounds);
                }
            }
            InputEvent::MouseUp { .. } => {
                self.resize = None;
                if let Some(d) = self.col_drag.take() {
                    if d.active {
                        // 미리보기 확정(07-19) — 이동이 있었으면 호스트 동기
                        let now: Vec<u32> = self.columns.iter().map(|c| c.key).collect();
                        if now != d.orig {
                            self.col_reordered = true; // 호스트 동기 폴링
                            self.clamp_scroll_x();
                        }
                        inv.push(self.bounds);
                    } else if self.columns[d.col].sortable {
                        // 무드래그 클릭 = 정렬(다중열 = Shift 전용 — 07-18)
                        let key = self.columns[d.col].key;
                        self.apply_sort(key, d.shift, inv);
                    }
                }
                // 클릭 확정(무드래그) — 기선택 행 프레스를 단일 선택으로 붕괴(탐색기 규약).
                // 파일 DnD가 시작됐으면 MouseUp이 오지 않아 다중 선택이 유지된다.
                if let Some(row) = self.press_pending.take() {
                    if row < self.src.len() {
                        self.src.select(row, SelectOp::Single);
                        inv.push(self.bounds);
                    }
                }
                if self.band.take().is_some() {
                    inv.push(self.bounds); // 밴드 사각형 지우기
                }
            }
            InputEvent::RightDown { x, y } => {
                // 선택 규약만 처리(탐색기: 미선택 행=단독 선택·선택 행=유지) — 메뉴 표시는 호스트(M3-4).
                if let Some(row) = self.row_at(x, y) {
                    if !self.in_marker_zone(row, x) {
                        if !self.src.is_selected(row) {
                            self.src.select(row, SelectOp::Single);
                        }
                        self.caret = Some(row);
                        inv.push(self.bounds);
                    }
                } else if y >= self.body_top() && self.bounds.contains(Point { x, y }) {
                    // 빈 본문 영역 = 선택 해제(배경 셸 메뉴는 S3)
                    self.src.clear_selection();
                    inv.push(self.bounds);
                }
            }
            // Undo/Redo/DoubleClick/MiddleDown/XButton(nexa-ctl 변형) — 호스트가 명령·행 판정으로 처리한다.
            _ => {}
        }
    }

    fn paint(&self, ctx: &mut dyn CtlDrawCtx, theme: &Theme) {
        let mut a = crate::draw::Adapt(ctx);
        self.paint_grid(&mut a, theme);
    }
}

impl<S: RowSource> VirtualRows<S> {
    /// 그리기 본체(dir2 `Widget::paint` 그대로) — 호스트가 그리드 어휘 백엔드를 직접 넘길 때(시험 기록기)도 쓴다.
    pub fn paint_grid(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        // 셀·배지·HUD 전부 자기 경계 안에서만(UIC-310 클립 스택 · dir2 10-02 가로 스크롤 번짐 차단 ·
        // nexa-dir3 10-03 RecordCtx 시험 적발 "열이 패널을 넘침"). 백엔드가 no-op이면 종전과 같다.
        ctx.push_clip(self.bounds);
        self.paint_grid_inner(ctx, theme);
        ctx.pop_clip();
    }

    fn paint_grid_inner(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        ctx.select_font(crate::draw::FontSlot::List, false, false); // 파일 목록 슬롯(X-12)
        let b = self.bounds;
        if self.mode == ViewMode::Tiles {
            self.paint_tiles(ctx, theme);
            self.paint_bar(ctx, theme);
            return;
        }
        let first = self.scroll_row;
        let count = self
            .visible_rows()
            .min(self.src.len().saturating_sub(first));
        let body_top = self.row_origin(); // 부분 행 오프셋(10-02 픽셀 스크롤) — 헤더가 뒤에 덮는다

        // ── 본문 행 ──
        for i in 0..count {
            let row = first + i;
            let y = body_top + i as i32 * self.row_h;
            // 선택 하이라이트 > 교대 음영(docs/07 §7 다중·비연속·교차폴더 하이라이트)
            let bg = if self.src.is_selected(row) {
                if self.focused {
                    theme.sel_bg
                } else {
                    theme.sel_bg_inactive
                }
            } else if row % 2 == 0 {
                theme.panel_bg
            } else {
                theme.panel_bg_alt
            };
            // 텍스트 세로 위치: 행 높이의 4/5를 글자 높이로 보고 중앙 정렬(M0-7 계승)
            let ty = y + (self.row_h - (self.row_h * 4) / 5) / 2;
            // 잘라내기 대기 행은 흐리게(X-32 — 탐색기 반투명 관례)
            let fg = if self.src.is_ghosted(row) {
                theme.text_dim
            } else {
                theme.text
            };

            if self.columns.is_empty() {
                // M1-3 호환: 단일 트리 컬럼이 전체 폭
                let mut item = self.src.row(row);
                if self.mode == ViewMode::Flat {
                    item.marker = Marker::None; // 일반 폴더 보기 = 펼침 마커 숨김(07-16)
                }
                let icon = self.src.icon(row);
                let rc = Rect::new(b.x, y, b.w, self.row_h);
                self.paint_tree_cell(ctx, theme, &item, icon.as_ref(), rc, bg, fg);
            } else {
                for (ci, col) in self.columns.iter().enumerate() {
                    let cx = self.col_x(ci);
                    if cx >= b.right() || cx + col.width <= b.x {
                        continue; // 가로 스크롤로 화면 밖
                    }
                    let cell = Rect::new(cx, y, col.width, self.row_h);
                    if col.key == 0 {
                        let mut item = self.src.row(row);
                        if self.mode == ViewMode::Flat {
                            item.marker = Marker::None; // 일반 폴더 보기(07-16)
                        }
                        let icon = self.src.icon(row);
                        self.paint_tree_cell(ctx, theme, &item, icon.as_ref(), cell, bg, fg);
                    } else if let Some((key, hint)) = self.src.cell_icon(row, col.key) {
                        // 아이콘 셀(132차): 배경 → 칸 가운데에 들여쓰기 폭 크기 아이콘 · 리졸버가 못 주면 글로.
                        ctx.text_opaque(cell.x, ty, cell, "", fg, bg);
                        let isz = self.indent_w.min(cell.w).min(cell.h);
                        let ix = cell.x + (cell.w - isz) / 2;
                        let iy = cell.y + (cell.h - isz) / 2;
                        if !ctx.draw_icon(ix, iy, isz, &key, &hint) {
                            let text = self.src.cell(row, col.key);
                            ctx.text_opaque(cell.x + self.pad_x, ty, cell, &text, fg, bg);
                        }
                    } else {
                        let text = self.src.cell(row, col.key);
                        // 칸보다 긴 값 = 끝 말줄임(154차 · 왼쪽 안쪽 여백을 뺀 폭 기준 — 좌·우 정렬 공통).
                        let text = crate::draw::ellipsize_end(ctx, &text, cell.w - self.pad_x);
                        let tx = match col.align {
                            Align::Left => cell.x + self.pad_x,
                            Align::Right => {
                                let w = ctx.text_width(&text);
                                (cell.right() - self.pad_x - w).max(cell.x + self.pad_x)
                            }
                        };
                        ctx.text_opaque(tx, ty, cell, &text, fg, bg);
                    }
                }
                // 마지막 컬럼 오른쪽 잔여 = 빈 본문(선택 하이라이트 제외 — 원본 B-4·QA 07-13)
                let cols_right = self.columns_right();
                if cols_right < b.right() {
                    let empty_bg = if row % 2 == 0 {
                        theme.panel_bg
                    } else {
                        theme.panel_bg_alt
                    };
                    ctx.fill_rect(
                        Rect::new(cols_right, y, b.right() - cols_right, self.row_h),
                        empty_bg,
                    );
                }
            }

            // 캐럿 행 테두리(1px accent) — 선택과 독립(키보드 기준점 표시). 폭 = 컬럼 총폭.
            // 비활성 패널(터미널 포커스 포함)은 무채색(text_dim)으로 낮춘다(QA 07-15).
            if self.caret == Some(row) {
                let cc = if self.focused {
                    theme.accent
                } else {
                    theme.text_dim
                };
                let cr = self.columns_right().min(b.right());
                let cw = (cr - b.x).max(1);
                ctx.fill_rect(Rect::new(b.x, y, cw, 1), cc);
                ctx.fill_rect(Rect::new(b.x, y + self.row_h - 1, cw, 1), cc);
                ctx.fill_rect(Rect::new(b.x, y, 1, self.row_h), cc);
                ctx.fill_rect(Rect::new(cr - 1, y, 1, self.row_h), cc);
            }
        }

        // 마지막 행 아래 잔여 영역
        let drawn_h = count as i32 * self.row_h;
        if body_top + drawn_h < b.bottom() {
            ctx.fill_rect(
                Rect::new(
                    b.x,
                    body_top + drawn_h,
                    b.w,
                    b.bottom() - (body_top + drawn_h),
                ),
                theme.panel_bg,
            );
        }

        // ── 헤더(본문 위에 그려 스크롤과 무관하게 고정) ──
        if !self.columns.is_empty() {
            // 헤더 장식(X-12 — 굵게/이탤릭)
            ctx.select_font(
                crate::draw::FontSlot::List,
                self.font_decor.1,
                self.font_decor.2,
            );
            let hy = b.y;
            let hty = hy + (self.row_h - (self.row_h * 4) / 5) / 2;
            for (ci, col) in self.columns.iter().enumerate() {
                let cx = self.col_x(ci);
                if cx >= b.right() || cx + col.width <= b.x {
                    continue;
                }
                let cell = Rect::new(cx, hy, col.width, self.row_h);
                // 끝 정렬 정렬 표시(130차): 칸 오른쪽 끝에 강조색으로 · 제목은 그 왼쪽까지만(겹치지 않게 클립).
                let mark = self.sort_mark(col.key);
                let mark_w = mark
                    .as_deref()
                    .map_or(0, |m| (ctx.text_width(m) + self.pad_x).min(cell.w));
                let title_cell = Rect::new(cell.x, cell.y, cell.w - mark_w, cell.h);
                ctx.text_opaque(
                    cell.x + self.pad_x,
                    hty,
                    title_cell,
                    &self.header_label(col),
                    theme.text,
                    crate::theme::header_bg(theme),
                );
                if let Some(m) = mark.as_deref() {
                    let mc = Rect::new(title_cell.right(), cell.y, mark_w, cell.h);
                    ctx.text_opaque(
                        mc.x,
                        hty,
                        mc,
                        m,
                        theme.accent,
                        crate::theme::header_bg(theme),
                    );
                }
                // 컬럼 경계선(헤더 안, 오른쪽 1px)
                let sep_x = cell.right() - 1;
                if sep_x >= b.x && sep_x < b.right() {
                    ctx.fill_rect(Rect::new(sep_x, hy, 1, self.row_h), theme.border);
                }
            }
            let cols_right =
                self.col_x(self.columns.len() - 1) + self.columns.last().map_or(0, |c| c.width);
            if cols_right < b.right() {
                ctx.fill_rect(
                    Rect::new(cols_right, hy, b.right() - cols_right, self.row_h),
                    crate::theme::header_bg(theme),
                );
            }
            // 놓일 자리 표식(131차 · 켜져 있을 때): 그 열 전체에 강조색 옅게 + 좌우 1 px 강조선 — 고스트보다 먼저(아래) 그린다.
            if let Some(slot) = self.col_drag_slot() {
                ctx.fill_round_rect_alpha(slot, 0, theme.accent, 31);
                ctx.fill_rect(Rect::new(slot.x, slot.y, 1, slot.h), theme.accent);
                ctx.fill_rect(Rect::new(slot.right() - 1, slot.y, 1, slot.h), theme.accent);
            }
            // 드래그 고스트 헤더(07-19 사용자): 커서 x 추종·세로 = 헤더 행
            // 고정 — 헤더 셀 모양(배경+테두리+라벨) 복제. 본체는 이미
            // 미리보기 위치에 렌더된다.
            if let Some(d) = &self.col_drag {
                if d.active {
                    let col = &self.columns[d.col];
                    let w = col.width;
                    let gx = (d.cur_x - w / 2).clamp(b.x, (b.right() - w).max(b.x));
                    let cell = Rect::new(gx, hy, w, self.row_h);
                    ctx.fill_rect(cell, crate::theme::header_bg(theme));
                    // 1px 테두리(시안 — 떠 있는 헤더 박스)
                    ctx.fill_rect(Rect::new(cell.x, cell.y, cell.w, 1), theme.border);
                    ctx.fill_rect(
                        Rect::new(cell.x, cell.bottom() - 1, cell.w, 1),
                        theme.border,
                    );
                    ctx.fill_rect(Rect::new(cell.x, cell.y, 1, cell.h), theme.border);
                    ctx.fill_rect(Rect::new(cell.right() - 1, cell.y, 1, cell.h), theme.border);
                    ctx.text_opaque(
                        cell.x + self.pad_x,
                        hty,
                        Rect::new(cell.x + 1, cell.y + 1, (cell.w - 2).max(0), cell.h - 2),
                        &self.header_label(col),
                        theme.text,
                        crate::theme::header_bg(theme),
                    );
                }
            }
            ctx.select_font(crate::draw::FontSlot::List, false, false); // 장식 복원
        }

        // ── 러버밴드 외곽선(드래그 중) ──
        if let Some(band) = self.band {
            let r = band.rect();
            if r.w > 0 && r.h > 0 {
                ctx.fill_rect(Rect::new(r.x, r.y, r.w, 1), theme.accent);
                ctx.fill_rect(Rect::new(r.x, r.bottom() - 1, r.w, 1), theme.accent);
                ctx.fill_rect(Rect::new(r.x, r.y, 1, r.h), theme.accent);
                ctx.fill_rect(Rect::new(r.right() - 1, r.y, 1, r.h), theme.accent);
            }
        }

        // ── 인라인 이름변경 필드(M3-2) — 트리 컬럼 이름부 위 오버레이,
        //    공용 필드 페인트(선택 하이라이트·세로바 캐럿·캐럿 가시 정렬) ──
        if let Some((row, es)) = &self.rename {
            if *row >= first && *row < first + count {
                let y = body_top + (*row - first) as i32 * self.row_h;
                let rc = self.rename_field_rect(*row, y);
                es.paint_field(ctx, rc, RENAME_FIELD_PAD, theme);
            }
        }

        // ── 타입어헤드 HUD(본문 좌하단 플로팅 배지 — 원본 docs/32 §7-A) ──
        if self.typeahead.is_active() {
            let label = format!("찾기: {}", self.typeahead.composing());
            let tw = ctx.text_width(&label);
            // HUD 배지 위치(원본 §7-A 3×3 피커 — 설정 07-15): 행=pos/3·열=pos%3
            let hw = tw + self.pad_x * 2;
            let hx = match self.ta_hud_pos % 3 {
                0 => b.x + self.pad_x,
                1 => b.x + (b.w - hw) / 2,
                _ => b.right() - hw - self.pad_x,
            };
            let hy = match self.ta_hud_pos / 3 {
                0 => self.body_top() + self.pad_x,
                1 => b.y + (b.h - self.row_h) / 2,
                _ => b.bottom() - self.row_h - self.pad_x,
            };
            let hud = Rect::new(hx, hy, hw, self.row_h);
            let hty = hud.y + (self.row_h - (self.row_h * 4) / 5) / 2;
            ctx.text_opaque(
                hud.x + self.pad_x,
                hty,
                hud,
                &label,
                theme.text,
                crate::theme::header_bg(theme),
            );
            ctx.fill_rect(Rect::new(hud.x, hud.y, hud.w, 1), theme.accent);
            ctx.fill_rect(Rect::new(hud.x, hud.bottom() - 1, hud.w, 1), theme.accent);
            ctx.fill_rect(Rect::new(hud.x, hud.y, 1, hud.h), theme.accent);
            ctx.fill_rect(Rect::new(hud.right() - 1, hud.y, 1, hud.h), theme.accent);
        }
        self.paint_bar(ctx, theme); // 오버레이 스크롤바(09-04) — 본문 위 마지막
        self.fast
            .paint(ctx, theme, self.bounds, self.row_h, self.pad_x); // ×N 배지(10-02)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Color;
    use std::cell::RefCell;

    #[test]
    fn tiles_grid_geometry_and_keys() {
        // 타일 보기(07-16): row_h 20 → 타일 240×60. 폭 500 → 열 2.
        let mut v = VirtualRows::new(
            Rows {
                n: 7,
                sorts: RefCell::new(Vec::new()),
            },
            20,
            6,
            16,
        );
        let mut inv = Invalidations::default();
        v.set_bounds(Rect::new(0, 0, 500, 130), &mut inv);
        v.set_view_mode(ViewMode::Tiles, &mut inv);
        assert_eq!(v.grid_cols(), 2);
        assert_eq!(v.grid_len(), 4, "7항목/2열 = 4그리드 행");
        // 히트: (250, 65) = 2행째(그리드 행 1)·열 1 → 인덱스 3. 헤더 없음(body_top=0).
        assert_eq!(v.row_at(250, 65), Some(3));
        assert_eq!(v.row_at(10, 5), Some(0));
        assert_eq!(v.row_at(490, 5), None, "마지막 열 오른쪽 잔여 = 빈 본문");
        // 키: ↓ = +열수(2) — 0 → 2 → 4, → = +1
        v.on_event(
            &InputEvent::MouseDown {
                x: 10,
                y: 5,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(v.caret(), Some(0));
        let down = InputEvent::Key {
            key: Key::Down,
            shift: false,
            primary: false,
        };
        v.on_event(&down, &mut inv);
        assert_eq!(v.caret(), Some(2), "↓ = +2(열 수)");
        v.on_event(
            &InputEvent::Key {
                key: Key::Right,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(v.caret(), Some(3), "→ = +1");
        // 스크롤 상한: 4그리드 행·가시 2행(130/60) → max 2
        assert_eq!(v.max_scroll(), 2);
        // 리스트 복귀 — 기하 원복
        v.set_view_mode(ViewMode::Tree, &mut inv);
        assert_eq!(v.grid_cols(), 1);
        assert_eq!(v.max_scroll(), 1, "7행·가시 6행(130/20)");
    }

    #[test]
    fn flat_mode_blocks_expansion() {
        // 일반 폴더 보기(07-16): 마커 존·←/→ 펼침 무동작
        let mut v = VirtualRows::new(
            Rows {
                n: 3,
                sorts: RefCell::new(Vec::new()),
            },
            20,
            6,
            16,
        );
        let mut inv = Invalidations::default();
        v.set_bounds(Rect::new(0, 0, 300, 200), &mut inv);
        v.set_view_mode(ViewMode::Flat, &mut inv);
        assert!(!v.marker_hit(8, 10), "Flat = 마커 존 없음");
        v.on_event(
            &InputEvent::MouseDown {
                x: 100,
                y: 10,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        v.on_event(
            &InputEvent::Key {
                key: Key::Right,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(v.caret(), Some(0), "→ 펼침 무동작(캐럿 유지)");
    }

    #[test]
    fn inline_rename_flow_and_key_block() {
        let mut v = VirtualRows::new(
            Rows {
                n: 5,
                sorts: RefCell::new(Vec::new()),
            },
            20,
            6,
            16,
        );
        let mut inv = Invalidations::default();
        v.set_bounds(Rect::new(0, 0, 300, 200), &mut inv);
        v.begin_rename(2, "행 2", &mut inv);
        assert_eq!(v.rename_state(), Some((2, "행 2".to_string())));
        assert_eq!(v.caret(), Some(2), "편집 행으로 캐럿 이동");
        // IME 배치 정보(M5-3) — 필드 rect가 편집 행 y에, pad=RENAME_FIELD_PAD
        let (_, rc, pad) = v.rename_edit_info().expect("편집 중 IME 정보");
        assert_eq!(rc.y, 2 * 20, "헤더 없음(컬럼 미설정) — 행 2의 y");
        assert_eq!(rc.h, 20);
        assert_eq!(pad, RENAME_FIELD_PAD);
        // 시작 = 선택 상태(확장자 없음 → 전체) — End로 접어 끝 편집으로 전환
        v.rename_key(crate::edit::EditKey::End, false, &mut inv);
        // 문자 입력·Backspace — Char 이벤트 경유(타입어헤드 대신 버퍼로)
        v.on_event(
            &InputEvent::Char {
                c: '\u{8}',
                now_ms: 0,
            },
            &mut inv,
        );
        v.on_event(&InputEvent::Char { c: '3', now_ms: 0 }, &mut inv);
        assert_eq!(v.rename_state(), Some((2, "행 3".to_string())));
        assert_eq!(v.typeahead_text(), "", "편집 중 타입어헤드 미동작");
        // 키 네비 차단
        v.on_event(
            &InputEvent::Key {
                key: Key::Down,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(v.caret(), Some(2), "편집 중 캐럿 이동 차단");
        // 제출 — (행, 새 이름), 편집 종료
        assert_eq!(v.submit_rename(&mut inv), Some((2, "행 3".to_string())));
        assert!(!v.is_renaming());
        // 취소 경로
        v.begin_rename(1, "x", &mut inv);
        v.cancel_rename(&mut inv);
        assert!(!v.is_renaming());
        assert_eq!(v.submit_rename(&mut inv), None);
    }

    /// A29(X3-07): 이름변경 필드·마커 존은 **현재 열 순서의 트리(key 0) 열** 위에 놓인다
    /// — 헤더 드래그로 '크기'를 맨 앞에 둬도 첫 열 위에 뜨지 않는다.
    #[test]
    fn rename_field_follows_tree_column_after_reorder() {
        // `rename_hit`은 paint 캐시 기준 — 무출력 DrawCtx로 한 번 그린다
        struct Nop;
        impl DrawCtx for Nop {
            fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
            fn text_opaque(&mut self, _x: i32, _y: i32, _c: Rect, _t: &str, _f: Color, _b: Color) {}
            fn text_width(&mut self, text: &str) -> i32 {
                text.chars().count() as i32 * 8
            }
        }
        // 기본 순서(이름 첫 열) = 종전 좌표 그대로(동작 불변)
        let (mut v, mut inv) = list_with_cols(5, 200);
        v.begin_rename(0, "row-0", &mut inv);
        let (_, rc, _) = v.rename_edit_info().expect("편집 중");
        let pad = 12;
        let indent = 16;
        assert_eq!(rc.x, v.col_x(0) + pad + indent - RENAME_FIELD_PAD);
        assert_eq!(rc.w, 200 - (pad + indent) + RENAME_FIELD_PAD);
        assert_eq!(rc.y, 20, "헤더 1행 아래 행 0");
        v.paint_grid(&mut Nop, &Theme::dark());
        assert!(v.rename_hit(rc.x + 2, rc.y + 2));
        v.cancel_rename(&mut inv);

        // 재배열: [크기, 이름, 수정한 날짜] — 이름 열이 둘째
        v.set_columns(
            vec![
                Column::new(2, "크기", 100).right_aligned(),
                Column::new(0, "이름", 200),
                Column::new(3, "수정한 날짜", 150),
            ],
            &mut inv,
        );
        assert_eq!(v.tree_col(), Some((100, 200)));
        v.begin_rename(0, "row-0", &mut inv);
        let (_, rc, _) = v.rename_edit_info().expect("편집 중");
        assert_eq!(
            rc.x,
            v.col_x(1) + pad + indent - RENAME_FIELD_PAD,
            "필드는 둘째(이름) 열 위"
        );
        assert_eq!(rc.w, 200 - (pad + indent) + RENAME_FIELD_PAD);
        v.paint_grid(&mut Nop, &Theme::dark());
        assert!(!v.rename_hit(50, rc.y + 2), "크기 열(첫 열) 위는 필드 아님");
        assert!(v.rename_hit(rc.x + 2, rc.y + 2));
        v.cancel_rename(&mut inv);

        // 가로 스크롤 반영 — 열 x가 함께 밀린다
        v.set_bounds(Rect::new(0, 0, 150, 200), &mut inv);
        v.hscroll_to(80, &mut inv);
        assert_eq!(v.scroll_x(), 80);
        v.begin_rename(0, "row-0", &mut inv);
        let (_, rc, _) = v.rename_edit_info().expect("편집 중");
        assert_eq!(rc.x, 100 - 80 + pad + indent - RENAME_FIELD_PAD);
        v.cancel_rename(&mut inv);

        // 트리 열 숨김(방어 경로) — 필드를 둘 곳이 없으면 편집 진입 거부
        v.set_columns(vec![Column::new(2, "크기", 100)], &mut inv);
        assert_eq!(v.tree_col(), None);
        v.begin_rename(0, "row-0", &mut inv);
        assert!(!v.is_renaming());

        // 컬럼 미설정 = 전체 폭(M1-3 호환)
        v.set_columns(Vec::new(), &mut inv);
        assert_eq!(v.tree_col(), Some((0, 150)));
    }

    /// A29: 마커 존도 같은 트리 열 기하 — 재배열 후 둘째 열의 들여쓰기 자리.
    #[test]
    fn marker_zone_follows_tree_column_after_reorder() {
        let mut inv = Invalidations::default();
        let mut v = VirtualRows::new(Expandable { expanded: false }, 20, 12, 16);
        v.set_bounds(Rect::new(0, 0, 400, 200), &mut inv);
        v.set_columns(cols(), &mut inv);
        // 기본 순서: 행 0(헤더 아래 y=20..40) 마커 = [pad, pad+indent) = [12, 28)
        assert!(v.marker_hit(20, 30));
        assert!(!v.marker_hit(30, 30));
        v.set_columns(
            vec![
                Column::new(2, "크기", 100).right_aligned(),
                Column::new(0, "이름", 200),
                Column::new(3, "수정한 날짜", 150),
            ],
            &mut inv,
        );
        assert!(!v.marker_hit(20, 30), "첫 열(크기) 위는 마커 아님");
        assert!(v.marker_hit(100 + 20, 30), "둘째(이름) 열의 들여쓰기 자리");
        assert!(!v.marker_hit(100 + 30, 30));
    }

    /// 정적 N행 소스(토글 없음) + set_sort 기록.
    struct Rows {
        n: usize,
        sorts: RefCell<Vec<Vec<(u32, bool)>>>,
    }
    impl Rows {
        fn new(n: usize) -> Rows {
            Rows {
                n,
                sorts: RefCell::new(Vec::new()),
            }
        }
    }
    impl RowSource for Rows {
        fn len(&self) -> usize {
            self.n
        }
        fn row(&self, index: usize) -> RowItem {
            RowItem {
                text: format!("row-{index}"),
                is_dir: false,
                depth: 0,
                marker: Marker::None,
            }
        }
        fn cell(&self, index: usize, key: u32) -> String {
            format!("c{key}-{index}")
        }
        fn set_sort(&mut self, keys: &[(u32, bool)]) -> bool {
            self.sorts.borrow_mut().push(keys.to_vec());
            true
        }
    }

    /// 토글 가능한 소스 — index 0을 토글하면 5행이 늘었다 줄었다 한다(트리 펼침 모사).
    struct Expandable {
        expanded: bool,
    }
    impl RowSource for Expandable {
        fn len(&self) -> usize {
            if self.expanded {
                6
            } else {
                1
            }
        }
        fn row(&self, index: usize) -> RowItem {
            RowItem {
                text: format!("row-{index}"),
                is_dir: index == 0,
                depth: u32::from(index > 0),
                marker: if index == 0 {
                    if self.expanded {
                        Marker::Expanded
                    } else {
                        Marker::Collapsed
                    }
                } else {
                    Marker::None
                },
            }
        }
        fn toggle(&mut self, index: usize) -> bool {
            if index == 0 {
                self.expanded = !self.expanded;
                true
            } else {
                false
            }
        }
    }

    fn list(total: usize, h: i32) -> (VirtualRows<Rows>, Invalidations) {
        let mut inv = Invalidations::default();
        let mut v = VirtualRows::new(Rows::new(total), 20, 12, 16);
        v.set_bounds(Rect::new(0, 0, 400, h), &mut inv);
        inv.drain().for_each(drop);
        (v, inv)
    }

    fn cols() -> Vec<Column> {
        vec![
            Column::new(0, "이름", 200),
            Column::new(2, "크기", 100).right_aligned(),
            Column::new(3, "수정한 날짜", 150),
        ]
    }

    fn list_with_cols(total: usize, h: i32) -> (VirtualRows<Rows>, Invalidations) {
        let (mut v, mut inv) = list(total, h);
        v.set_columns(cols(), &mut inv);
        inv.drain().for_each(drop);
        (v, inv)
    }

    fn down(v: &mut VirtualRows<Rows>, inv: &mut Invalidations, x: i32, y: i32, shift: bool) {
        v.on_event(
            &InputEvent::MouseDown {
                x,
                y,
                shift,
                primary: false,
            },
            inv,
        );
    }

    /// 완전 클릭(down+up) — 정렬은 MouseUp 무드래그에서 확정(07-19 드래그
    /// 재배열 도입으로 이동).
    fn click(v: &mut VirtualRows<Rows>, inv: &mut Invalidations, x: i32, y: i32, shift: bool) {
        down(v, inv, x, y, shift);
        v.on_event(&InputEvent::MouseUp { x, y }, inv);
    }

    fn key(k: Key) -> InputEvent {
        InputEvent::Key {
            key: k,
            shift: false,
            primary: false,
        }
    }

    // ── M1-3 계승(컬럼 없음 = 헤더 없음) ──

    #[test]
    fn scroll_clamps_to_total_minus_full_rows() {
        let (mut v, mut inv) = list(100, 200); // 완전 가시 10행
        v.on_event(&key(Key::End), &mut inv);
        assert_eq!(v.scroll_row(), 90); // 캐럿 99가 보이도록 스크롤 추적
        assert_eq!(v.caret(), Some(99));
        v.on_event(&key(Key::Home), &mut inv);
        assert_eq!(v.scroll_row(), 0);
        assert_eq!(v.caret(), Some(0));
    }

    #[test]
    fn fast_scroll_multiplies_rapid_wheel_notches_but_not_trackpad() {
        // 10-02 고속 스크롤(nexa-sql 이식): 노치(120) 연타 = 6번째부터 ×2(step 5) · 분수 delta = 그대로
        let (mut v, mut inv) = list(100, 200); // 완전 가시 10행
        for _ in 0..8 {
            v.on_event(&InputEvent::Wheel { delta: -120 }, &mut inv);
        }
        assert_eq!(
            v.scroll_row(),
            3 * 3 + 3 * 6 + 2 * 9,
            "3회 ×1 + 3회 ×2 + 2회 ×3(step 3)"
        );
        assert!(inv.tick_requested(), "배지 = 틱 요청");
        let (mut v2, mut inv2) = list(100, 200);
        for _ in 0..15 {
            v2.on_event(&InputEvent::Wheel { delta: -8 }, &mut inv2); // 트랙패드 = 픽셀
        }
        assert_eq!(
            v2.scroll_row(),
            3,
            "노치 미만 delta는 가속 없음(120 = 3행 = 60px)"
        );
        assert_eq!(v2.scroll_frac, 0);
        // 반 노치(60 = 30px = 1.5행) 더 → 행 4 + 10px 부분 오프셋 · 행 위치·히트가 10px 위로
        v2.on_event(&InputEvent::Wheel { delta: -60 }, &mut inv2);
        assert_eq!((v2.scroll_row(), v2.scroll_frac), (4, 10));
        assert_eq!(
            v2.row_at(10, v2.body_top()),
            Some(4),
            "부분 행(10px 가려짐)도 첫 행"
        );
        assert_eq!(
            v2.row_at(10, v2.body_top() + 10),
            Some(5),
            "20px 행: 10px 아래 = 다음 행"
        );
        v2.on_event(&key(Key::End), &mut inv2); // 스크롤이 필요한 키 이동 = 행 단위 스냅
        assert_eq!(v2.scroll_frac, 0);
    }

    /// 선 쉐브론 꼭짓점: 칸 16 = 닫힘 4×8 · 열림 8×4 · 칸 가운데 · 배율 2 = 두 배 · 좁은 칸도 최소 4.
    #[test]
    fn marker_chevron_points_match_mdl2_ink() {
        let cell = Rect::new(10, 100, 16, 22);
        assert_eq!(
            marker_chevron_points(cell, false),
            [(16, 107), (20, 111), (16, 115)]
        );
        assert_eq!(
            marker_chevron_points(cell, true),
            [(14, 109), (18, 113), (22, 109)]
        );
        let big = marker_chevron_points(Rect::new(0, 0, 32, 44), false);
        assert_eq!((big[1].0 - big[0].0, big[2].1 - big[0].1), (8, 16));
        let tiny = marker_chevron_points(Rect::new(0, 0, 5, 10), true);
        assert_eq!((tiny[2].0 - tiny[0].0, tiny[1].1 - tiny[0].1), (4, 2));
        // 스위치 기본 = 꺼짐(종전 글리프) · 켜고 끄기.
        assert!(!marker_vector());
        set_marker_vector(true);
        assert!(marker_vector());
        set_marker_vector(false);
    }

    #[test]
    fn marker_click_toggles_row_without_columns() {
        let mut inv = Invalidations::default();
        let mut v = VirtualRows::new(Expandable { expanded: false }, 20, 12, 16);
        v.set_bounds(Rect::new(0, 0, 400, 200), &mut inv);
        inv.drain().for_each(drop);
        // 헤더 없음 → y=5는 0행. 마커 존 = [12, 28)
        v.on_event(
            &InputEvent::MouseDown {
                x: 15,
                y: 5,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(v.source().len(), 6);
        // 마커 존 밖 클릭은 펼침이 아니라 선택(캐럿만 이동)
        v.on_event(
            &InputEvent::MouseDown {
                x: 100,
                y: 5,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(v.source().len(), 6, "본문 클릭은 토글 아님");
        assert_eq!(v.caret(), Some(0));
    }

    // ── 헤더·본문 오프셋 ──

    #[test]
    fn header_shifts_body_rows_down() {
        let (mut v, mut inv) = list_with_cols(100, 220); // 헤더 20 + 본문 200 = 완전 가시 10행
        assert_eq!(v.max_scroll(), 90);
        // y=5 → 헤더(정렬 클릭), y=25 → 0행
        assert_eq!(v.row_at(10, 5), None);
        assert_eq!(v.row_at(10, 25), Some(0));
        v.on_event(&key(Key::End), &mut inv);
        assert_eq!(v.scroll_row(), 90);
    }

    // ── 정렬: 3상태 순환·단일 리셋·Shift 다중열(원본 docs/23 §4) ──

    #[test]
    fn header_click_cycles_three_states() {
        let (mut v, mut inv) = list_with_cols(10, 220);
        let name_x = 50; // 이름 컬럼(0..200)
        click(&mut v, &mut inv, name_x, 5, false);
        assert_eq!(v.sort(), &[(0, false)]); // ▲
        click(&mut v, &mut inv, name_x, 5, false);
        assert_eq!(v.sort(), &[(0, true)]); // ▼
        click(&mut v, &mut inv, name_x, 5, false);
        assert_eq!(v.sort(), &[]); // 없음(열거)
        assert_eq!(
            *v.source().sorts.borrow(),
            vec![vec![(0, false)], vec![(0, true)], vec![]]
        );
    }

    #[test]
    fn plain_click_resets_to_single_sort() {
        let (mut v, mut inv) = list_with_cols(10, 220);
        click(&mut v, &mut inv, 50, 5, false); // 이름 ▲
        click(&mut v, &mut inv, 250, 5, true); // Shift+크기 → 다중 [이름▲, 크기▲]
        assert_eq!(v.sort(), &[(0, false), (2, false)]);
        click(&mut v, &mut inv, 250, 5, false); // 단순 클릭 = 단일 리셋 + 크기의 3상태(▲→▼)
        assert_eq!(v.sort(), &[(2, true)]);
    }

    #[test]
    fn shift_click_adds_cycles_and_removes_keys() {
        let (mut v, mut inv) = list_with_cols(10, 220);
        click(&mut v, &mut inv, 50, 5, true); // 정렬 없음 + Shift = 단일로 동작
        assert_eq!(v.sort(), &[(0, false)]);
        click(&mut v, &mut inv, 250, 5, true); // 크기 추가(오름)
        click(&mut v, &mut inv, 350, 5, true); // 날짜 추가(오름 — 날짜 컬럼 300..450 중 가시 범위)
        assert_eq!(v.sort(), &[(0, false), (2, false), (3, false)]);
        click(&mut v, &mut inv, 250, 5, true); // 크기 방향 순환 ▲→▼
        assert_eq!(v.sort(), &[(0, false), (2, true), (3, false)]);
        click(&mut v, &mut inv, 250, 5, true); // 크기 ▼→없음(제거, 뒤 순번 당김)
        assert_eq!(v.sort(), &[(0, false), (3, false)]);
    }

    #[test]
    fn header_label_shows_arrow_before_and_badge_after() {
        let (mut v, mut inv) = list_with_cols(10, 220);
        click(&mut v, &mut inv, 50, 5, false); // 이름 ▲
                                               // 순번 = 정렬 시작부터 상시 표시(사용자 확정 07-18 — 단일 = ①)
        assert_eq!(v.header_label(&v.columns()[0]), "▲ 이름 ①");
        click(&mut v, &mut inv, 250, 5, true); // + 크기
        assert_eq!(v.header_label(&v.columns()[0]), "▲ 이름 ①");
        assert_eq!(v.header_label(&v.columns()[1]), "▲ 크기 ②");
        assert_eq!(v.header_label(&v.columns()[2]), "수정한 날짜");
    }

    /// 컬럼 드래그(131차): 임계를 넘으면 라이브로 순서가 바뀌고 · 표식 = 그 열이 놓인 자리(머리 + 본문) · 놓으면
    /// take_col_reordered · Esc(cancel_col_drag) = 원래 순서 · 표식은 기본 꺼짐.
    #[test]
    fn col_drag_marker_follows_live_slot_and_cancel_restores() {
        let (mut v, mut inv) = list_with_cols(10, 220);
        let keys =
            |v: &VirtualRows<Rows>| -> Vec<u32> { v.columns().iter().map(|c| c.key).collect() };
        let orig = keys(&v);
        let w: Vec<i32> = v.columns().iter().map(|c| c.width).collect();
        let down = |v: &mut VirtualRows<Rows>, inv: &mut Invalidations, x: i32| {
            v.on_event(
                &InputEvent::MouseDown {
                    x,
                    y: 5,
                    shift: false,
                    primary: false,
                },
                inv,
            );
        };
        // 첫 열 머리 가운데를 잡아 둘째 열 너머로.
        down(&mut v, &mut inv, w[0] / 2);
        assert!(!v.col_dragging(), "누르기만 = 후보");
        v.on_event(
            &InputEvent::MouseMove {
                x: w[0] + w[1] + 10,
                y: 5,
            },
            &mut inv,
        );
        assert!(v.col_dragging());
        assert_eq!(v.col_drag_slot(), None, "표식 기본 꺼짐");
        v.set_col_drag_marker(true);
        let slot = v.col_drag_slot().expect("slot");
        let now = keys(&v);
        assert_ne!(now, orig, "라이브 미리 보기로 순서가 바뀐다");
        let at = now.iter().position(|k| *k == orig[0]).unwrap();
        assert_eq!((slot.x, slot.w), (v.col_x(at), v.columns()[at].width));
        assert_eq!(
            (slot.y, slot.h),
            (v.bounds().y, v.bounds().h),
            "머리 + 본문"
        );
        let mut rec = nexa_ctl::RecordCtx::with_surface(800, 300);
        v.paint(&mut rec, &Theme::dark());
        assert!(rec.round_rects.iter().any(|r| r.0 == slot), "표식 채움");
        // Esc = 원래 순서 · 통지 없음.
        assert!(v.cancel_col_drag(&mut inv));
        assert_eq!(keys(&v), orig);
        assert!(!v.col_dragging() && !v.take_col_reordered());
        // 다시 끌어 놓으면 통지.
        down(&mut v, &mut inv, w[0] / 2);
        v.on_event(
            &InputEvent::MouseMove {
                x: w[0] + w[1] + 10,
                y: 5,
            },
            &mut inv,
        );
        v.on_event(
            &InputEvent::MouseUp {
                x: w[0] + w[1] + 10,
                y: 5,
            },
            &mut inv,
        );
        assert!(v.take_col_reordered() && !v.col_dragging());
        assert_ne!(keys(&v), orig);
    }

    /// 끝 정렬 정렬 표시(130차): 제목만 왼쪽 · ▲/▼는 오른쪽 끝 · 다중 정렬일 때만 순번 · Shift 순환(오름 → 내림 → 없음).
    #[test]
    fn trailing_sort_mark_and_shift_cycle() {
        let (mut v, mut inv) = list_with_cols(10, 220);
        let k: Vec<u32> = v.columns().iter().map(|c| c.key).collect();
        assert_eq!(v.sort_mark(k[0]), None, "기본 = 종전 모양");
        v.set_sort_mark_trailing(true, &mut inv);
        click(&mut v, &mut inv, 50, 5, false); // 이름 ▲
        assert_eq!(v.header_label(&v.columns()[0]), "이름");
        assert_eq!(
            v.sort_mark(k[0]).as_deref(),
            Some("▲"),
            "단일 정렬 = 순번 없음"
        );
        click(&mut v, &mut inv, 250, 5, true); // Shift+크기 → 다중
        assert_eq!(v.sort_mark(k[0]).as_deref(), Some("▲1"));
        assert_eq!(v.sort_mark(k[1]).as_deref(), Some("▲2"));
        assert_eq!(v.sort_mark(k[2]), None);
        click(&mut v, &mut inv, 250, 5, true); // Shift 다시 = 내림
        assert_eq!(v.sort_mark(k[1]).as_deref(), Some("▼2"));
        click(&mut v, &mut inv, 250, 5, true); // Shift 또 = 없음
        assert_eq!(v.sort_mark(k[1]), None);
        assert_eq!(
            v.sort_mark(k[0]).as_deref(),
            Some("▲"),
            "하나 남으면 순번 없음"
        );
        // 그리기: 표시는 칸 오른쪽 끝(제목과 따로) · 자동 맞춤 글에는 표시가 들어간다.
        click(&mut v, &mut inv, 250, 5, true);
        let mut rec = nexa_ctl::RecordCtx::with_surface(600, 300);
        v.paint(&mut rec, &Theme::dark());
        assert!(rec.drew_text("이름") && rec.drew_text("▲1") && rec.drew_text("▲2"));
        assert!(!rec.drew_text("▲ 이름 ①"));
        assert_eq!(v.autofit_texts(0)[0].0, "이름 ▲1");
    }

    // ── 리사이즈 드래그 ──

    #[test]
    fn drag_handle_resizes_column_with_min_width() {
        let (mut v, mut inv) = list_with_cols(10, 220);
        let neighbor_w = v.columns()[1].width;
        let total0: i32 = v.columns().iter().map(|c| c.width).sum();
        // 이름 컬럼 오른쪽 경계 x=200 → 핸들 [194, 202)
        down(&mut v, &mut inv, 197, 5, false);
        assert!(v.sort().is_empty(), "핸들 클릭은 정렬이 아님");
        v.on_event(&InputEvent::MouseMove { x: 257, y: 5 }, &mut inv);
        assert_eq!(v.columns()[0].width, 260); // +60
                                               // 단독 조절(07-22): 이웃 불변·총폭 가변(초과분 = 가로 스크롤)
        assert_eq!(v.columns()[1].width, neighbor_w, "이웃 컬럼 너비 불변");
        let total: i32 = v.columns().iter().map(|c| c.width).sum();
        assert_eq!(total, total0 + 60, "총폭이 늘어난다");
        v.on_event(&InputEvent::MouseMove { x: -500, y: 5 }, &mut inv);
        assert_eq!(v.columns()[0].width, 40); // min_width 고정
        assert_eq!(v.columns()[1].width, neighbor_w, "축소 시에도 이웃 불변");
        v.on_event(&InputEvent::MouseUp { x: -500, y: 5 }, &mut inv);
        v.on_event(&InputEvent::MouseMove { x: 300, y: 5 }, &mut inv);
        assert_eq!(v.columns()[0].width, 40, "업 이후엔 리사이즈 없음");
    }

    // ── 선택(원본 docs/07 §1-2·§8): 단일·Ctrl 토글·Shift 범위·Ctrl+A·러버밴드 ──

    struct SelRows {
        n: usize,
        sel: std::collections::HashSet<usize>,
        anchor: usize,
        names: Vec<String>,
    }
    impl SelRows {
        fn new(n: usize) -> SelRows {
            SelRows {
                n,
                sel: Default::default(),
                anchor: 0,
                names: Vec::new(),
            }
        }
    }
    impl SelRows {
        fn named(names: &[&str]) -> SelRows {
            let mut s = SelRows::new(names.len());
            s.names = names.iter().map(|n| n.to_string()).collect();
            s
        }
        fn name(&self, i: usize) -> String {
            self.names
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("row-{i}"))
        }
    }
    impl RowSource for SelRows {
        fn len(&self) -> usize {
            self.n
        }
        fn row(&self, index: usize) -> RowItem {
            RowItem {
                text: self.name(index),
                is_dir: false,
                depth: 0,
                marker: Marker::None,
            }
        }
        fn find_prefix(&self, caret: Option<usize>, prefix: &str) -> Option<usize> {
            // 코어 find_prefix(VisibleStream) 축약 모사: caret+1부터 + wrap, 대소문자 무시
            if prefix.is_empty() || self.n == 0 {
                return None;
            }
            let lower = prefix.to_lowercase();
            let start = caret.filter(|&c| c < self.n).map_or(0, |c| c + 1);
            (start..self.n)
                .chain(0..start)
                .find(|&i| self.name(i).to_lowercase().starts_with(&lower))
        }
        fn is_selected(&self, index: usize) -> bool {
            self.sel.contains(&index)
        }
        fn select(&mut self, index: usize, op: SelectOp) -> bool {
            match op {
                SelectOp::Single => {
                    self.sel.clear();
                    self.sel.insert(index);
                    self.anchor = index;
                }
                SelectOp::Toggle => {
                    if !self.sel.remove(&index) {
                        self.sel.insert(index);
                    }
                    self.anchor = index;
                }
                SelectOp::RangeTo => {
                    let (lo, hi) = if self.anchor <= index {
                        (self.anchor, index)
                    } else {
                        (index, self.anchor)
                    };
                    self.sel = (lo..=hi).collect();
                }
            }
            true
        }
        fn select_span(&mut self, lo: usize, hi: usize) -> bool {
            self.sel = (lo..=hi).collect();
            true
        }
        fn select_all(&mut self) -> bool {
            self.sel = (0..self.n).collect();
            true
        }
        fn clear_selection(&mut self) -> bool {
            let had = !self.sel.is_empty();
            self.sel.clear();
            had
        }
    }

    fn sel_list(n: usize, h: i32) -> (VirtualRows<SelRows>, Invalidations) {
        let mut inv = Invalidations::default();
        let mut v = VirtualRows::new(SelRows::new(n), 20, 12, 16);
        v.set_bounds(Rect::new(0, 0, 400, h), &mut inv);
        inv.drain().for_each(drop);
        (v, inv)
    }

    fn sdown(
        v: &mut VirtualRows<SelRows>,
        inv: &mut Invalidations,
        y: i32,
        shift: bool,
        primary: bool,
    ) {
        v.on_event(
            &InputEvent::MouseDown {
                x: 100,
                y,
                shift,
                primary,
            },
            inv,
        );
    }

    #[test]
    fn click_selects_single_ctrl_toggles_shift_ranges() {
        let (mut v, mut inv) = sel_list(10, 200); // 헤더 없음 — 행 y = i*20
        sdown(&mut v, &mut inv, 5, false, false); // 0행 단일
        assert!(v.source().is_selected(0) && v.source().sel.len() == 1);
        assert_eq!(v.caret(), Some(0));
        sdown(&mut v, &mut inv, 45, false, true); // Ctrl+2행 토글 → {0,2} (비연속)
        assert_eq!(v.source().sel.len(), 2);
        assert!(v.source().is_selected(2));
        sdown(&mut v, &mut inv, 85, true, false); // Shift+4행 → anchor(2)~4 범위
        assert_eq!(
            v.source().sel,
            [2usize, 3, 4].into_iter().collect(),
            "가시 순서 범위 선택"
        );
        assert_eq!(v.caret(), Some(4));
        sdown(&mut v, &mut inv, 45, false, true); // Ctrl 토글 해제
        assert!(!v.source().is_selected(2));
    }

    #[test]
    fn select_program_moves_caret_and_ignores_out_of_range() {
        let (mut v, mut inv) = sel_list(10, 200);
        v.select_program(3, SelectOp::Single, &mut inv); // UIA Select(M5-3)
        assert!(v.source().is_selected(3) && v.source().sel.len() == 1);
        assert_eq!(v.caret(), Some(3));
        v.select_program(5, SelectOp::Toggle, &mut inv); // AddToSelection
        assert_eq!(v.source().sel.len(), 2);
        v.select_program(99, SelectOp::Single, &mut inv); // 낡은 스냅샷 인덱스 — 무시
        assert_eq!(v.caret(), Some(5), "범위 밖 무시");
    }

    // ── OLE 드래그 뒤 낡은 press_pending(10-02 G6-01·X3-03) ──

    #[test]
    fn press_pending_does_not_survive_replace_source() {
        let (mut v, mut inv) = sel_list(5, 200);
        sdown(&mut v, &mut inv, 5, false, false); // 0행 단일
        v.on_event(&InputEvent::MouseUp { x: 30, y: 5 }, &mut inv);
        sdown(&mut v, &mut inv, 45, true, false); // Shift+2행 → {0,1,2}
        v.on_event(&InputEvent::MouseUp { x: 30, y: 45 }, &mut inv);
        // 기선택 1행 프레스 = 보류(다중 선택 드래그 DnD) → OLE 드래그로 MouseUp 없음
        sdown(&mut v, &mut inv, 25, false, false);
        assert_eq!(v.source().sel.len(), 3, "프레스 시점 선택 유지");
        v.replace_source(SelRows::new(5), &mut inv); // 드롭 후 재로드
                                                     // 다른 행 B(3행) 클릭 — MouseUp이 낡은 A(1행)로 되돌리면 안 된다
        sdown(&mut v, &mut inv, 65, false, false);
        v.on_event(&InputEvent::MouseUp { x: 30, y: 65 }, &mut inv);
        assert!(
            v.source().is_selected(3) && v.source().sel.len() == 1,
            "선택 = {{3}}, 실제 {:?}",
            v.source().sel
        );
        assert_eq!(v.caret(), Some(3));
    }

    #[test]
    fn new_press_discards_stale_pending_without_replace_source() {
        // 재로드 없이(드래그 취소·ESC) 다음 클릭도 낡은 보류를 폐기해야 한다 — 좌·우 프레스 모두
        let (mut v, mut inv) = sel_list(5, 200);
        sdown(&mut v, &mut inv, 5, false, false);
        v.on_event(&InputEvent::MouseUp { x: 30, y: 5 }, &mut inv);
        sdown(&mut v, &mut inv, 45, true, false); // {0,1,2}
        v.on_event(&InputEvent::MouseUp { x: 30, y: 45 }, &mut inv);
        sdown(&mut v, &mut inv, 25, false, false); // 1행 보류, MouseUp 없음
        sdown(&mut v, &mut inv, 65, false, false); // 새 프레스 = 3행 단일
        v.on_event(&InputEvent::MouseUp { x: 30, y: 65 }, &mut inv);
        assert_eq!(
            v.source().sel,
            std::collections::HashSet::from([3]),
            "좌 프레스가 보류 폐기"
        );

        sdown(&mut v, &mut inv, 65, false, false); // 기선택 3행 프레스 보류
        v.on_event(&InputEvent::RightDown { x: 30, y: 85 }, &mut inv); // 4행 우클릭 단독 선택
        v.on_event(&InputEvent::MouseUp { x: 30, y: 85 }, &mut inv);
        assert_eq!(
            v.source().sel,
            std::collections::HashSet::from([4]),
            "우 프레스가 보류 폐기"
        );
    }

    #[test]
    fn abort_press_clears_pending_and_keeps_selection() {
        let (mut v, mut inv) = sel_list(5, 200);
        sdown(&mut v, &mut inv, 5, false, false);
        v.on_event(&InputEvent::MouseUp { x: 30, y: 5 }, &mut inv);
        sdown(&mut v, &mut inv, 45, true, false); // {0,1,2}
        v.on_event(&InputEvent::MouseUp { x: 30, y: 45 }, &mut inv);
        sdown(&mut v, &mut inv, 25, false, false); // 1행 보류
        v.abort_press(); // 호스트: begin_drag 반환 직후
        assert_eq!(v.source().sel.len(), 3, "선택 집합 불변(단일화 안 함)");
        v.on_event(&InputEvent::MouseUp { x: 30, y: 25 }, &mut inv); // 늦게 온 해제도 무해
        assert_eq!(v.source().sel.len(), 3, "낡은 보류 소비 없음");
    }

    #[test]
    fn right_down_selects_unselected_keeps_selection_clears_on_empty() {
        let (mut v, mut inv) = sel_list(5, 200);
        // 미선택 행 우클릭 = 단독 선택 + 캐럿(탐색기 규약, M3-4)
        v.on_event(&InputEvent::RightDown { x: 30, y: 45 }, &mut inv); // 2행
        assert!(v.source().is_selected(2) && v.source().sel.len() == 1);
        assert_eq!(v.caret(), Some(2));
        // 기존 다중 선택 위 우클릭 = 선택 유지(축소 안 함)
        sdown(&mut v, &mut inv, 5, false, false); // 0행 단일
        sdown(&mut v, &mut inv, 45, true, false); // Shift+2행 → {0,1,2}
        v.on_event(&InputEvent::RightDown { x: 30, y: 25 }, &mut inv); // 선택된 1행
        assert_eq!(v.source().sel.len(), 3, "선택 유지");
        assert_eq!(v.caret(), Some(1));
        // 빈 본문 영역 우클릭 = 선택 해제(배경 메뉴는 S3)
        v.on_event(&InputEvent::RightDown { x: 30, y: 150 }, &mut inv);
        assert!(v.source().sel.is_empty());
    }

    #[test]
    fn ctrl_a_selects_all_visible() {
        let (mut v, mut inv) = sel_list(7, 200);
        v.on_event(&InputEvent::SelectAll, &mut inv);
        assert_eq!(v.source().sel.len(), 7);
        assert!(!inv.is_empty());
    }

    #[test]
    fn rubber_band_selects_intersecting_rows_and_ends_on_up() {
        let (mut v, mut inv) = sel_list(3, 200); // 행 3개(0..60), 아래 빈 영역
        sdown(&mut v, &mut inv, 5, false, false); // 미리 선택해 둔 0행이
        sdown(&mut v, &mut inv, 100, false, false); // 빈 영역 클릭 → 밴드 시작 + 해제
        assert!(v.source().sel.is_empty());
        v.on_event(&InputEvent::MouseMove { x: 50, y: 30 }, &mut inv); // 위로 드래그: 30..100
        assert_eq!(
            v.source().sel,
            [1usize, 2].into_iter().collect(),
            "밴드 세로 범위와 교차하는 행"
        );
        v.on_event(&InputEvent::MouseUp { x: 50, y: 30 }, &mut inv);
        assert_eq!(v.source().sel.len(), 2, "업 후 선택 유지");
        // 업 이후 이동은 밴드 아님
        v.on_event(&InputEvent::MouseMove { x: 50, y: 5 }, &mut inv);
        assert_eq!(v.source().sel.len(), 2);
    }

    #[test]
    fn right_left_keys_toggle_expansion_at_caret() {
        let mut inv = Invalidations::default();
        let mut v = VirtualRows::new(Expandable { expanded: false }, 20, 12, 16);
        v.set_bounds(Rect::new(0, 0, 400, 200), &mut inv);
        v.on_event(
            &InputEvent::MouseDown {
                x: 100,
                y: 5,
                shift: false,
                primary: false,
            },
            &mut inv,
        ); // 본문 클릭 → 캐럿 0
        assert_eq!(v.caret(), Some(0));
        v.on_event(&key(Key::Right), &mut inv);
        assert_eq!(v.source().len(), 6, "→ = 인라인 펼침");
        v.on_event(&key(Key::Right), &mut inv);
        assert_eq!(v.caret(), Some(1), "펼침 상태에서 → = 첫 자식으로");
        v.on_event(&key(Key::Left), &mut inv);
        assert_eq!(v.caret(), Some(0), "자식(파일)에서 ← = 부모로");
        v.on_event(&key(Key::Left), &mut inv);
        assert_eq!(v.source().len(), 1, "펼친 부모에서 ← = 접힘");
    }

    // ── 캐럿 키보드 네비(M1-6, 탐색기 규약) ──

    #[test]
    fn caret_moves_select_and_scroll_follows() {
        let (mut v, mut inv) = sel_list(100, 200); // 완전 가시 10행
        v.on_event(&key(Key::Down), &mut inv); // 캐럿 없음 → scroll_row(0) 기준 +1
        assert_eq!(v.caret(), Some(1));
        assert!(v.source().is_selected(1) && v.source().sel.len() == 1);
        v.on_event(&key(Key::PageDown), &mut inv);
        assert_eq!(v.caret(), Some(11));
        assert_eq!(v.scroll_row(), 2, "캐럿이 보이도록 스크롤 추적");
        v.on_event(&key(Key::Home), &mut inv);
        assert_eq!((v.caret(), v.scroll_row()), (Some(0), 0));
    }

    #[test]
    fn shift_moves_range_and_ctrl_moves_caret_only() {
        let (mut v, mut inv) = sel_list(20, 200);
        sdown(&mut v, &mut inv, 25, false, false); // 1행 클릭(anchor)
        v.on_event(
            &InputEvent::Key {
                key: Key::Down,
                shift: true,
                primary: false,
            },
            &mut inv,
        );
        v.on_event(
            &InputEvent::Key {
                key: Key::Down,
                shift: true,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(v.source().sel, [1usize, 2, 3].into_iter().collect());
        v.on_event(
            &InputEvent::Key {
                key: Key::Down,
                shift: false,
                primary: true,
            },
            &mut inv,
        ); // Ctrl+↓ = 캐럿만
        assert_eq!(v.caret(), Some(4));
        assert_eq!(v.source().sel.len(), 3, "Ctrl 이동은 선택 불변");
        v.on_event(
            &InputEvent::Key {
                key: Key::Space,
                shift: false,
                primary: true,
            },
            &mut inv,
        ); // Ctrl+Space = 토글
        assert_eq!(v.source().sel.len(), 4);
        assert!(v.source().is_selected(4));
    }

    // ── 타입어헤드(M1-6, 원본 docs/32 §6) ──

    /// 157차 규칙(nexa-ctl 부품 · nexa-sql 탐색기와 같다): 글자는 **누적**만 한다(같은 키 반복도 누적 — 자동 순환 없음) ·
    /// 입력 중 ↑/↓ = 접두사가 같은 항목 사이로만 순환 · Backspace = 축소.
    #[test]
    fn typeahead_accumulates_and_arrows_cycle_matches() {
        let mut inv = Invalidations::default();
        let mut v = VirtualRows::new(
            SelRows::named(&["apple", "apricot", "banana", "aardvark"]),
            20,
            12,
            16,
        );
        v.set_bounds(Rect::new(0, 0, 400, 200), &mut inv);
        let ch = |c: char, now_ms: u64| InputEvent::Char { c, now_ms };
        v.on_event(&ch('a', 0), &mut inv);
        assert_eq!(v.caret(), Some(0), "첫 'a' = apple");
        assert!(v.source().is_selected(0) && v.typeahead_active());
        // ↓ = 다음 일치(banana는 건너뛴다) · 끝에서 처음으로 · ↑ = 이전 일치.
        v.on_event(&key(Key::Down), &mut inv);
        assert_eq!(v.caret(), Some(1), "↓ = apricot");
        v.on_event(&key(Key::Down), &mut inv);
        assert_eq!(v.caret(), Some(3), "↓ = aardvark(banana 건너뜀)");
        v.on_event(&key(Key::Down), &mut inv);
        assert_eq!(v.caret(), Some(0), "↓ = 처음으로(apple)");
        v.on_event(&key(Key::Up), &mut inv);
        assert_eq!(v.caret(), Some(3), "↑ = 이전 일치(aardvark)");
        v.on_event(&key(Key::Up), &mut inv);
        assert_eq!(v.caret(), Some(1), "↑ = apricot");
        assert_eq!(v.typeahead_text(), "a", "순환은 접두사를 바꾸지 않는다");
        // 누적: 'p' → "ap" = 지금 행(apricot)이 여전히 일치 → 그대로.
        v.on_event(&ch('p', 300), &mut inv);
        assert_eq!(v.typeahead_text(), "ap");
        assert_eq!(v.caret(), Some(1));
        // 같은 키 반복도 누적("app") = apple로.
        v.on_event(&ch('p', 400), &mut inv);
        assert_eq!(v.typeahead_text(), "app");
        assert_eq!(v.caret(), Some(0));
        // Backspace → "ap" · 지금 행 포함 재평가 → apple 유지.
        v.on_event(&ch('\u{8}', 500), &mut inv);
        assert_eq!(v.typeahead_text(), "ap");
        assert_eq!(v.caret(), Some(0));
        // 일치가 하나뿐이면 ↑/↓는 제자리(다른 행으로 새지 않는다).
        v.on_event(&ch('p', 600), &mut inv);
        v.on_event(&key(Key::Down), &mut inv);
        assert_eq!(v.caret(), Some(0), "유일한 일치 = 제자리");
        // Esc = 입력 취소 → 그 뒤 ↓는 평소 이동.
        v.on_event(&key(Key::Escape), &mut inv);
        assert!(!v.typeahead_active());
        v.on_event(&key(Key::Down), &mut inv);
        assert_eq!(v.caret(), Some(1), "입력이 끝나면 평소 한 칸 이동");
    }

    /// 유지 시간(157차 · 기본 2000 ms): 마지막 활동 뒤 시간이 지나면 지운다 · **↑/↓로 한 번 움직일 때마다 다시 잰다** ·
    /// 공백은 입력 중일 때만 글자 · 끄면 글자 키를 무시한다.
    #[test]
    fn typeahead_timeout_resets_on_arrow_moves() {
        let mut inv = Invalidations::default();
        let mut v = VirtualRows::new(SelRows::named(&["alpha", "beta", "bravo"]), 20, 12, 16);
        v.set_bounds(Rect::new(0, 0, 400, 200), &mut inv);
        let ch = |c: char, now_ms: u64| InputEvent::Char { c, now_ms };
        v.on_event(&ch('b', 0), &mut inv);
        assert_eq!(v.typeahead_text(), "b");
        v.tick(1500, &mut inv);
        assert!(v.typeahead_active(), "유지 시간 전");
        // 1500 ms에 ↓로 이동 = 그때부터 다시 2000 ms.
        v.on_event(&key(Key::Down), &mut inv);
        assert_eq!(v.caret(), Some(2), "beta → bravo");
        v.tick(3000, &mut inv);
        assert!(
            v.typeahead_active(),
            "이동이 유지 시간을 되돌렸다(3000 − 1500 < 2000)"
        );
        v.tick(3600, &mut inv);
        assert!(!v.typeahead_active(), "마지막 이동 뒤 2000 ms 경과 = 소거");
        // 공백: 입력 중이 아니면 글자가 아니다.
        v.on_event(&ch(' ', 4000), &mut inv);
        assert!(!v.typeahead_active(), "빈 상태의 공백 = 타입어헤드 아님");
        // 유지 시간 설정.
        v.set_typeahead_opts(500, true, true, true, 6, &mut inv);
        v.on_event(&ch('a', 5000), &mut inv);
        v.tick(5600, &mut inv);
        assert!(!v.typeahead_active(), "설정한 500 ms");
        // 끔 = 글자 키 무시 · 진행 중이던 입력도 지운다.
        v.on_event(&ch('b', 6000), &mut inv);
        assert!(v.typeahead_active());
        v.set_typeahead_enabled(false, &mut inv);
        assert!(!v.typeahead_active());
        let before = v.caret();
        v.on_event(&ch('a', 6100), &mut inv);
        assert!(!v.typeahead_active() && v.caret() == before, "끔 = 무시");
    }

    /// 한글(157차): 자모가 오면 그리드 안에서 조합한다(IME 없이) — 조합 중인 글자도 접두사에 들어간다.
    #[test]
    fn typeahead_composes_hangul_jamo() {
        let mut inv = Invalidations::default();
        let mut v = VirtualRows::new(
            SelRows::named(&["apple", "가방", "강아지", "나무"]),
            20,
            12,
            16,
        );
        v.set_bounds(Rect::new(0, 0, 400, 200), &mut inv);
        let ch = |c: char, now_ms: u64| InputEvent::Char { c, now_ms };
        v.on_event(&ch('ㄱ', 0), &mut inv);
        assert_eq!(v.typeahead_composing(), "ㄱ");
        assert!(v.typeahead_active(), "조합 중 = 입력 중");
        v.on_event(&ch('ㅏ', 100), &mut inv);
        assert_eq!(v.typeahead_composing(), "가");
        assert_eq!(v.caret(), Some(1), "가 → 가방");
        v.on_event(&ch('ㅇ', 200), &mut inv);
        assert_eq!(v.typeahead_composing(), "강");
        assert_eq!(v.caret(), Some(2), "강 → 강아지");
        // Backspace = 자모 단위("강" → "가") → 지금 행(강아지)은 "가"로 시작하지 않으므로 가방으로.
        v.on_event(&ch('\u{8}', 300), &mut inv);
        assert_eq!(v.typeahead_composing(), "가");
        assert_eq!(v.caret(), Some(1));
        // 특수문자 제외 설정이어도 자모는 받는다.
        v.on_event(&key(Key::Escape), &mut inv);
        v.set_typeahead_opts(2000, false, true, true, 6, &mut inv);
        v.on_event(&ch('ㄴ', 5000), &mut inv);
        v.on_event(&ch('ㅏ', 5100), &mut inv);
        assert_eq!(v.caret(), Some(3), "나 → 나무");
    }

    // ── 소스 교체(M1-8 네비게이션) ──

    #[test]
    fn replace_source_resets_view_but_keeps_sort() {
        let (mut v, mut inv) = list_with_cols(100, 220);
        click(&mut v, &mut inv, 50, 5, false); // 이름 ▲ 정렬
        v.on_event(&key(Key::End), &mut inv); // 스크롤·캐럿 이동
        assert!(v.scroll_row() > 0 && v.caret().is_some());

        v.replace_source(Rows::new(5), &mut inv); // "다른 폴더 진입"
        assert_eq!((v.scroll_row(), v.caret()), (0, None), "뷰 상태 리셋");
        assert_eq!(v.sort(), &[(0, false)], "정렬 상태 유지");
        assert_eq!(
            *v.source().sorts.borrow(),
            vec![vec![(0, false)]],
            "새 소스에 정렬 재적용"
        );
    }

    // ── 가로 스크롤 ──

    #[test]
    fn hwheel_scrolls_and_clamps_to_total_width() {
        let (mut v, mut inv) = list_with_cols(10, 220); // 총폭 450, 위젯 400 → max 50
        v.on_event(&InputEvent::HWheel { delta: 120 }, &mut inv); // 3행 × 16px = 48
        assert_eq!(v.scroll_x(), 48);
        v.on_event(&InputEvent::HWheel { delta: 120 }, &mut inv);
        assert_eq!(v.scroll_x(), 50); // 클램프
        v.on_event(&InputEvent::HWheel { delta: -1200 }, &mut inv);
        assert_eq!(v.scroll_x(), 0);
    }

    #[test]
    fn widening_bounds_reclamps_scroll_x() {
        let (mut v, mut inv) = list_with_cols(10, 220);
        v.on_event(&InputEvent::HWheel { delta: 120 }, &mut inv);
        assert_eq!(v.scroll_x(), 48);
        v.set_bounds(Rect::new(0, 0, 1000, 220), &mut inv); // 총폭 450 < 1000 → 0
        assert_eq!(v.scroll_x(), 0);
    }

    // ── 페인트 ──

    #[test]
    fn paint_draws_header_cells_and_right_aligned_size() {
        struct Probe {
            texts: Vec<(i32, i32, String)>,
            fills: Vec<Rect>,
        }
        impl DrawCtx for Probe {
            fn fill_rect(&mut self, rect: Rect, _color: Color) {
                self.fills.push(rect);
            }
            fn text_opaque(
                &mut self,
                x: i32,
                y: i32,
                _clip: Rect,
                text: &str,
                _fg: Color,
                _bg: Color,
            ) {
                self.texts.push((x, y, text.to_string()));
            }
            fn text_width(&mut self, text: &str) -> i32 {
                text.chars().count() as i32 * 8
            }
        }
        let (v, _) = list_with_cols(1, 220);
        let mut p = Probe {
            texts: vec![],
            fills: vec![],
        };
        v.paint_grid(&mut p, &Theme::dark());
        // 본문 0행: 트리(마커+이름), 크기(우측 정렬), 날짜 — 이후 헤더 3개
        let texts: Vec<&str> = p.texts.iter().map(|(_, _, t)| t.as_str()).collect();
        assert!(texts.contains(&"row-0"));
        assert!(texts.contains(&"이름") && texts.contains(&"크기"));
        // 크기 셀 "c2-0"(폭 8*4=32): x = 300(right) - 12(pad) - 32 = 256
        let size_cell = p.texts.iter().find(|(_, _, t)| t == "c2-0").unwrap();
        assert_eq!(size_cell.0, 256);
        // 헤더는 y=0행에 그려짐
        let hdr = p.texts.iter().find(|(_, _, t)| t == "이름").unwrap();
        assert!(hdr.1 < 20);
    }

    #[test]
    fn ghosted_row_paints_name_dim() {
        // 잘라내기 대기 행 흐림(X-32) — is_ghosted 행의 이름은 text_dim
        struct Ghosty;
        impl RowSource for Ghosty {
            fn len(&self) -> usize {
                2
            }
            fn row(&self, index: usize) -> RowItem {
                RowItem {
                    text: format!("g-{index}"),
                    is_dir: false,
                    depth: 0,
                    marker: Marker::None,
                }
            }
            fn is_ghosted(&self, index: usize) -> bool {
                index == 0
            }
        }
        struct Probe {
            named: Vec<(String, Color)>,
        }
        impl DrawCtx for Probe {
            fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
            fn text_opaque(
                &mut self,
                _x: i32,
                _y: i32,
                _clip: Rect,
                text: &str,
                fg: Color,
                _bg: Color,
            ) {
                self.named.push((text.to_string(), fg));
            }
            fn text_width(&mut self, text: &str) -> i32 {
                text.chars().count() as i32 * 8
            }
        }
        let mut v = VirtualRows::new(Ghosty, 20, 6, 16);
        let mut inv = Invalidations::default();
        v.set_bounds(Rect::new(0, 0, 300, 100), &mut inv);
        let mut p = Probe { named: vec![] };
        let th = Theme::dark();
        v.paint_grid(&mut p, &th);
        let fg_of = |name: &str| p.named.iter().find(|(t, _)| t == name).unwrap().1;
        assert_eq!(fg_of("g-0"), th.text_dim, "잘라내기 대기 행 = 흐림");
        assert_eq!(fg_of("g-1"), th.text, "일반 행 = 기본 색");
    }

    #[test]
    fn drag_scroll_edge_moves_one_row_within_edge_zone() {
        // DnD 엣지 자동 스크롤(X-32) — 상/하단 한 행 높이 존에서만 ±1행
        let mut v = VirtualRows::new(Rows::new(50), 20, 6, 16);
        let mut inv = Invalidations::default();
        v.set_bounds(Rect::new(0, 0, 300, 100), &mut inv); // 헤더 없음 — 가시 5행
        assert!(!v.drag_scroll_edge(50, &mut inv), "중앙 = 무동작");
        assert!(v.drag_scroll_edge(95, &mut inv), "하단 엣지 = +1");
        assert_eq!(v.scroll_row(), 1);
        assert!(v.drag_scroll_edge(5, &mut inv), "상단 엣지 = -1");
        assert_eq!(v.scroll_row(), 0);
        assert!(
            !v.drag_scroll_edge(5, &mut inv),
            "최상단에서 위 = 클램프 무변"
        );
        assert!(!v.drag_scroll_edge(150, &mut inv), "본문 밖 = 무동작");
    }

    #[test]
    fn hover_expand_only_collapsed_dirs_in_tree_mode() {
        // DnD 호버 펼침(X-32) — 접힌 폴더만, 펼침 전용(토글 아님), Flat 모드 제외
        struct Dirs {
            expanded: bool,
        }
        impl RowSource for Dirs {
            fn len(&self) -> usize {
                2
            }
            fn row(&self, index: usize) -> RowItem {
                if index == 0 {
                    RowItem {
                        text: "dir".into(),
                        is_dir: true,
                        depth: 0,
                        marker: if self.expanded {
                            Marker::Expanded
                        } else {
                            Marker::Collapsed
                        },
                    }
                } else {
                    RowItem {
                        text: "file".into(),
                        is_dir: false,
                        depth: 0,
                        marker: Marker::None,
                    }
                }
            }
            fn toggle(&mut self, index: usize) -> bool {
                if index == 0 {
                    self.expanded = !self.expanded;
                    true
                } else {
                    false
                }
            }
        }
        let mut v = VirtualRows::new(Dirs { expanded: false }, 20, 6, 16);
        let mut inv = Invalidations::default();
        v.set_bounds(Rect::new(0, 0, 300, 100), &mut inv);
        assert!(v.is_collapsed_dir(0) && !v.is_collapsed_dir(1));
        assert!(!v.hover_expand(1, &mut inv), "파일 = 무동작");
        assert!(v.hover_expand(0, &mut inv), "접힌 폴더 = 펼침");
        assert!(!v.is_collapsed_dir(0), "펼침 후 = 후보 아님");
        assert!(
            !v.hover_expand(0, &mut inv),
            "이미 펼침 = 무동작(접기 없음)"
        );
        // Flat 모드 = 펼침 개념 없음 — 접힌 폴더라도 후보 아님
        let mut f = VirtualRows::new(Dirs { expanded: false }, 20, 6, 16);
        f.set_bounds(Rect::new(0, 0, 300, 100), &mut inv);
        f.set_view_mode(ViewMode::Flat, &mut inv);
        assert!(!f.is_collapsed_dir(0));
        assert!(!f.hover_expand(0, &mut inv));
    }
}

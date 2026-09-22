//! nexa-dlg — 대화상자 **조립층**(docs/20 §1·§3). 1차 = [`FilePicker`](파일 열기·저장).
//!
//! - 새 원자 컨트롤 없이 nexa-ctl 컨트롤(Button · TextBox · Combo · Checkbox · TreeView · TreeGrid)을 놓는다.
//! - I/O·OS 차이는 [`nexa_fs`]에만 — 여기에는 `cfg(target_os)`가 없다.
//! - 문자열은 전부 앱이 [`PickerLabels`]로 주입(i18n) · 색·간격은 테마/배율만.
//! - 호스트(앱)가 창을 만들고 `set_bounds`/`on_event`/`paint`를 부른다 — 창 안 오버레이든 별도 창이든 같은 코드.
//!
//! 동작(docs/20 §3 교집합): 경로 상자 편집(Enter = 이동/확정 · `~`·환경변수·상대 경로) · ↑ 상위 · 장소 사이드바 ·
//! 목록 헤더 클릭 정렬(자연 정렬 · 폴더 먼저) · 더블클릭/Enter = 폴더 진입·파일 확정 · Backspace = 상위 ·
//! 파일명 상자 Enter = 확정 · 확장자 필터 · 숨김 표시 · 새 폴더 · 저장은 **덮어쓰기 2단 확인**(같은 이름으로 한 번 더) ·
//! 확장자 자동 부여 · 파일명 규칙 즉시 검증.

use nexa_ctl::controls::{glyph, ContextMenu, CtxItem, GlyphKind, LabelSide, MenuIcon};
use nexa_ctl::IconImage;
use nexa_ctl::{
    Button, Checkbox, Combo, ComboControl, ComboItem, Control, ControlBase, DrawCtx, FontSlot,
    GridColumn, InputEvent, Invalidations, Key, Point, Rect, TextBox, Theme, TimeoutButton,
    TreeControl, TreeGrid, TreeModel, TreeNode, TreeView, Widget,
};
use nexa_fs::shell::{IconKey, IconService, Lookup};
use nexa_fs::{Entry, History, ListHandle, ListMsg, ListOpts, Place, PlaceKind, SortKey};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// 열기 / 저장.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerMode {
    /// 기존 파일 열기.
    Open,
    /// 파일 저장(이름 입력 · 덮어쓰기 확인).
    Save,
    /// ★ **폴더 고르기**(nexa-sql 사용자 09-21 — Oracle 클라이언트 폴더): 목록에 **폴더만** 보인다(파일은 보일 필요가 없다) ·
    /// 확정 버튼 = 고른 폴더(없으면 지금 폴더) · 더블클릭/Enter = 그 폴더로 들어가기(열기 모드와 같다).
    Folder,
}

/// 확장자 필터 한 줄(`exts`는 점 없는 소문자 · 비면 전체).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFilter {
    /// 표시 라벨(예 `SQL (*.sql)`).
    pub label: String,
    /// 확장자 목록.
    pub exts: Vec<String>,
}

impl FileFilter {
    /// 라벨 + 확장자.
    #[must_use]
    pub fn new(label: impl Into<String>, exts: &[&str]) -> Self {
        Self {
            label: label.into(),
            exts: exts.iter().map(|e| e.to_lowercase()).collect(),
        }
    }
}

/// 앱이 주입하는 문자열(전부 i18n 키의 번역 결과).
#[derive(Clone, Debug, Default)]
pub struct PickerLabels {
    /// "File name:".
    pub file_name: String,
    /// "File type:".
    pub file_type: String,
    /// 확정 버튼(열기).
    pub ok_open: String,
    /// 확정 버튼(저장).
    pub ok_save: String,
    /// 확정 버튼(폴더 고르기).
    pub ok_folder: String,
    /// "Folder:"(폴더 고르기의 이름 상자 라벨).
    pub folder_name: String,
    /// 취소.
    pub cancel: String,
    /// 새 폴더 버튼.
    pub new_folder: String,
    /// 새 폴더 기본 이름.
    pub new_folder_name: String,
    /// 숨김 파일 표시.
    pub show_hidden: String,
    /// 점 파일(`.git` …) 표시.
    pub show_dot: String,
    /// 열 제목.
    pub col_name: String,
    /// 열 제목.
    pub col_modified: String,
    /// 열 제목.
    pub col_size: String,
    /// 열 제목.
    pub col_kind: String,
    /// 종류 셀 — 폴더.
    pub kind_folder: String,
    /// 종류 셀 — 파일(`{EXT} File`의 뒷말).
    pub kind_file: String,
    /// 장소 라벨.
    pub place_home: String,
    /// 장소 라벨.
    pub place_desktop: String,
    /// 장소 라벨.
    pub place_documents: String,
    /// 장소 라벨.
    pub place_downloads: String,
    /// 장소 그룹(= 가상 최상위 "내 PC" · 클릭 = 드라이브 목록).
    pub place_drives: String,
    /// 드라이브 행의 종류 셀.
    pub kind_drive: String,
    /// 장소 그룹.
    pub place_recent: String,
    /// 경로 상자 placeholder.
    pub path_hint: String,
    /// 오류 — 파일 없음.
    pub err_not_found: String,
    /// 안내 — 같은 이름 존재(한 번 더 누르면 덮어씀).
    pub err_exists: String,
    /// 덮어쓰기 확인 카드 문구(`{0}` = 파일 이름) · 확인 버튼(09-16 · nexa-sql 사용자 "경고 창 + 확인 절차").
    pub overwrite_ask: String,
    pub overwrite_yes: String,
    /// 오류 — 파일명 규칙 위반.
    pub err_bad_name: String,
    /// 오류 — 폴더를 읽을 수 없음.
    pub err_list: String,
    /// 오류 — 새 폴더 실패.
    pub err_mkdir: String,
    /// 우클릭 메뉴 — 열기.
    pub menu_open: String,
    /// 우클릭 메뉴 — 경로 복사.
    pub menu_copy_path: String,
    /// 우클릭 메뉴 — 이름 복사.
    pub menu_copy_name: String,
    /// 우클릭 메뉴 — 새로 고침.
    pub menu_refresh: String,
    /// 다중 선택 안내(`{0}` = 파일 수 · `{1}` = 합계 크기).
    pub multi_selected: String,
}

/// 선택기 결과(1회성 · [`FilePicker::take_action`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerAction {
    /// 없음.
    None,
    /// 확정(열기·저장 경로).
    Confirm(PathBuf),
    /// ★ 열기 모드 다중 확정(고른 순서 · 파일만 · nexa-sql 사용자 09-22).
    ConfirmMany(Vec<PathBuf>),
    /// 취소.
    Cancel,
    /// 클립보드에 쓸 텍스트(경로/이름 복사 — 클립보드는 호스트 몫).
    CopyText(String),
}

/// 파일 선택기 — 컨트롤 6개를 한 패널에 조립한 **복합 컨트롤**.
#[derive(Debug)]
pub struct FilePicker {
    base: ControlBase,
    mode: PickerMode,
    labels: PickerLabels,
    filters: Vec<FileFilter>,
    dir: PathBuf,
    entries: Vec<Entry>,
    /// 현재 폴더 열거(백그라운드 · 배치 도착마다 목록 갱신 · Drop = 취소).
    loader: Option<ListHandle>,
    /// 지연 펼침 로더 — (그리드? · 노드 경로 · 모은 항목 · 핸들). 노드가 접히거나 목록이 바뀌면 버린다.
    sub_loaders: Vec<SubLoad>,
    /// 사이드바 장소 프로브(셰브론 유무).
    probe: Option<ListHandle>,
    /// 프로브 결과 — 경로 → (보여줄 자식 있음, 하위 폴더 있음). 폴더 이동 때 비운다.
    probes: HashMap<PathBuf, (bool, bool)>,
    /// 열거 끝난 뒤 선택할 이름(새 폴더·↑ 복귀).
    select_after: Option<String>,
    /// 로딩 중 마지막 재구성 시각(docs/37 P-4 — 배치마다 전체를 다시 만들지 않는다).
    last_rebuild: Option<Instant>,
    /// 우클릭 메뉴(열기 · 경로/이름 복사 · 새 폴더 · 새로 고침 · 숨김 표시).
    menu: ContextMenu,
    menu_row: Option<usize>,
    /// 결합 정렬 키(클릭 = 단일 3단 ▲→▼→해제 · Shift+클릭 = 키 추가/방향/제거 · 비면 이름 ▲ · 결과 그리드와 같은 규약).
    sort_keys: Vec<(SortKey, bool)>,
    /// 표시 순서(논리 컬럼 1=수정 · 2=크기 · 3=유형 · 이름은 항상 첫 열) — 헤더 드래그로 바꾼다.
    col_order: Vec<usize>,
    /// 헤더 드래그(표시 위치 · 시작 x · 현재 x · 4px 이상 움직임 · Shift).
    hdr_drag: Option<(usize, i32, i32, bool, bool)>,
    /// 헤더 경계 드래그 = 컬럼 폭 조절(논리 컬럼 · 시작 x · 시작 폭 · 사용자 09-15).
    hdr_resize: Option<(usize, i32, i32)>,
    show_hidden: bool,
    places: Vec<Place>,
    recent: Vec<PathBuf>,
    // 컨트롤
    home_btn: Button,
    back_btn: Button,
    fwd_btn: Button,
    up_btn: Button,
    /// 경로 편집 상자 — 브레드크럼 위에 겹치며 **편집 중일 때만** 보인다(우클릭/빈 곳 클릭 = 편집 · Enter = 이동 · Esc = 취소).
    path_box: TextBox,
    /// 브레드크럼 편집 중인가.
    path_editing: bool,
    /// 브레드크럼 세그먼트 x 범위(페인트 캐시 · 히트테스트).
    crumb_ranges: std::cell::RefCell<Vec<(i32, i32)>>,
    crumb_hover: Option<usize>,
    /// 탐색 히스토리(뒤로/앞으로).
    history: History,
    new_folder_btn: Button,
    places_view: TreeView,
    grid: TreeGrid,
    name_box: TextBox,
    /// ★ **다중 선택**(열기 모드만 · nexa-sql 사용자 09-22 · nexa-dir2 규약): **고른 순서**의 파일 경로(폴더는 들어가지 않는다).
    /// 클릭 = 단일 · Ctrl(⌘)+클릭/Space = 토글 · Shift+클릭/Shift+방향키 = 기준 행~여기 범위 · Ctrl+A = 보이는 파일 전부.
    marks: Vec<PathBuf>,
    /// Shift 범위의 기준 행(가시 행 인덱스).
    anchor: Option<usize>,
    /// 이름 상자에 넣어 둔 다중 선택 표기(사용자가 고치면 선택을 푼다).
    marks_text: String,
    /// ★ Ctrl+드래그 스윕(사용자 09-22): (마지막으로 지난 행, 추가인가) — 누른 행이 토글로 선택됐으면 지나는 파일을 **추가**,
    /// 해제됐으면 **해제**. MouseUp(어디서든)에 끝난다.
    drag_sweep: Option<(usize, bool)>,
    /// ★ 러버밴드(사용자 09-22 "빈 공간에서 좌클릭 드래그로 다중 선택"): (시작점, 지금 점, 시작 때의 선택 — Ctrl이면 유지·아니면 빈 것).
    /// 밴드의 세로 범위와 겹치는 행의 파일 = 선택. MouseUp(어디서든)에 끝난다.
    band: Option<(Point, Point, Vec<PathBuf>)>,
    filter_combo: Combo,
    hidden_chk: Checkbox,
    dot_chk: Checkbox,
    show_dot: bool,
    /// 하단 부가 콤보(앱이 주입 · 예: 인코딩 — Golden/DBeaver 하단 줄 · 사용자 09-15) — (라벨, 콤보).
    extra: Option<(String, Combo)>,
    ok_btn: Button,
    cancel_btn: Button,
    // 상태
    cursor: (i32, i32),
    message: Option<(String, bool)>, // (본문, 오류인가)
    pending_overwrite: Option<PathBuf>,
    /// 덮어쓰기 확인 카드(열려 있으면 모달 · Enter = 덮어쓰기 · Esc = 취소).
    overwrite_ask: Option<PathBuf>,
    overwrite_yes_btn: Button,
    overwrite_no_btn: Button,
    /// ★ 덮어쓰기 **무장**(nexa-sql 사용자 09-19 "타임아웃 버튼으로 · 두 번 눌러야 저장"): [덮어쓰기]를 처음 누르면 그 자리에
    /// 빨간 타이머 버튼이 뜨고, 시간 안에 **한 번 더** 눌러야 확정된다. 시간이 다 되면 원래 버튼으로 돌아간다(확정 아님).
    overwrite_arm: Option<TimeoutButton>,
    /// 무장 시간(ms · 호스트 설정 · 0 = 무장 없이 한 번에).
    overwrite_confirm_ms: u64,
    /// 마지막 틱 시각(무장 시작 시각으로 쓴다).
    now_ms: u64,
    last_row_click: Option<(usize, Instant)>,
    last_place_row: usize,
    action: PickerAction,
    /// 헤더 열 폭(논리 px) — 정렬 히트테스트 근거.
    col_w: [i32; 4],
    grid_rect: Rect,
    /// 아이콘 변환 캐시(서비스의 RGBA → 이 창의 `IconImage` · 키 = 서비스 키) · 폴백 그림 2종.
    icons: HashMap<IconKey, Rc<IconImage>>,
    fallback_dir: Rc<IconImage>,
    fallback_file: Rc<IconImage>,
    /// 마지막으로 반영한 서비스 버전 — 바뀌면(아이콘/종류 이름 도착) 제자리 갱신.
    icon_version: u64,
}

/// 그리드 행 숨은 셀 위치(보이는 3열 뒤): 경로 · `d`/`f` · 확장자.
/// 점 파일(`.`으로 시작) 토글을 **Windows에서만** 보인다(사용자 09-16) — macOS·Linux는 점 파일이 곧 숨김 파일이라
/// (`nexa-fs::is_hidden_meta` = 점 접두) "Show hidden" 하나로 충분하고, 그 OS에서 `show_dot`은 항상 켜짐으로 고정한다.
const DOT_TOGGLE: bool = cfg!(windows);

const CELL_PATH: usize = 3;
const CELL_KIND: usize = 4;
const CELL_EXT: usize = 5;

/// 지연 펼침 로더 한 건.
#[derive(Debug)]
struct SubLoad {
    /// 사이드바(폴더만)인가 · 아니면 목록 그리드.
    sidebar: bool,
    /// 대상 노드 경로(트리 인덱스).
    node: Vec<usize>,
    /// 지금까지 도착한 항목.
    items: Vec<Entry>,
    /// 새 배치가 있어 다시 반영해야 하는가.
    dirty: bool,
    handle: ListHandle,
}

/// 자리표시 자식이 "로딩 중"으로 바뀐 상태(같은 노드를 두 번 시작하지 않게).
const LOADING: &str = "\u{0}loading";

/// 폴더 트리 자리표시 자식(펼칠 때 실제 하위 폴더로 교체).
const PENDING: &str = "\u{0}pending";

/// 더블클릭 간격.
const DOUBLE_CLICK: Duration = Duration::from_millis(500);
const PLACES_W: i32 = 190;
const ROW: i32 = 30;
const GAP: i32 = 8;
const PAD: i32 = 12;
const LABEL_W: i32 = 90;
const FILTER_W: i32 = 220;
const BTN_W: i32 = 96;
const HEADER_H: i32 = 26;

impl Drop for FilePicker {
    /// 대화상자가 사라지면 아이콘 워커 스레드도 지금 거둔다(캐시는 남는다 · 다음 열기가 새 워커를 만든다 · 사용자 09-15 "사용 후 회수").
    fn drop(&mut self) {
        IconService::global().release_worker();
    }
}

impl FilePicker {
    /// 새 선택기 — `start`가 폴더가 아니면 홈(그마저 없으면 현재 작업 폴더).
    #[must_use]
    pub fn new(
        mode: PickerMode,
        start: Option<&Path>,
        filters: Vec<FileFilter>,
        labels: PickerLabels,
    ) -> Self {
        let dir = start
            .filter(|p| p.is_dir())
            .map(Path::to_path_buf)
            .or_else(nexa_fs::home_dir)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let filters = if filters.is_empty() {
            vec![FileFilter::new("*.*", &[])]
        } else {
            filters
        };
        let items: Vec<ComboItem> = filters
            .iter()
            .enumerate()
            .map(|(i, f)| ComboItem::new(i.to_string(), f.label.clone()))
            .collect();
        let ok_label = match mode {
            PickerMode::Open => labels.ok_open.clone(),
            PickerMode::Save => labels.ok_save.clone(),
            PickerMode::Folder => labels.ok_folder.clone(),
        };
        let mut p = FilePicker {
            base: ControlBase::default(),
            mode,
            filters,
            dir: dir.clone(),
            entries: Vec::new(),
            menu: ContextMenu::new(),
            menu_row: None,
            sort_keys: Vec::new(),
            col_order: vec![2, 1, 3],
            hdr_drag: None,
            hdr_resize: None,
            show_hidden: false,
            places: nexa_fs::places(),
            recent: Vec::new(),
            home_btn: Button::glyph(glyph(GlyphKind::Home)),
            // Material 도형(사용자 SVG 09-16) — 글자 ←→↑는 글꼴마다 굵기·위치가 달랐다.
            back_btn: Button::glyph(glyph(GlyphKind::ArrowBack)),
            fwd_btn: Button::glyph(glyph(GlyphKind::ArrowForward)),
            up_btn: Button::glyph(glyph(GlyphKind::ArrowUp)),
            path_box: TextBox::new(labels.path_hint.clone()),
            path_editing: false,
            crumb_ranges: std::cell::RefCell::new(Vec::new()),
            crumb_hover: None,
            history: History::new(dir.clone()),
            new_folder_btn: Button::new(labels.new_folder.clone()),
            places_view: TreeView::new(TreeModel::new(Vec::new())),
            grid: TreeGrid::new(TreeModel::new(Vec::new()), Vec::new()),
            name_box: TextBox::new(String::new()),
            marks: Vec::new(),
            anchor: None,
            marks_text: String::new(),
            drag_sweep: None,
            band: None,
            filter_combo: Combo::new(items, 0),
            hidden_chk: Checkbox::new(labels.show_hidden.clone(), false)
                .with_label_side(LabelSide::Right),
            dot_chk: Checkbox::new(labels.show_dot.clone(), true).with_label_side(LabelSide::Right),
            show_dot: true,
            extra: None,
            ok_btn: Button::new(ok_label),
            cancel_btn: Button::new(labels.cancel.clone()),
            cursor: (0, 0),
            message: None,
            pending_overwrite: None,
            overwrite_ask: None,
            overwrite_yes_btn: Button::new(labels.overwrite_yes.clone()),
            overwrite_no_btn: Button::new(labels.cancel.clone()),
            overwrite_arm: None,
            overwrite_confirm_ms: 5000,
            now_ms: 0,
            last_row_click: None,
            last_place_row: usize::MAX,
            action: PickerAction::None,
            col_w: [320, 150, 90, 110],
            grid_rect: Rect::default(),
            icons: HashMap::new(),
            fallback_dir: Rc::new(fallback_icon(true)),
            fallback_file: Rc::new(fallback_icon(false)),
            icon_version: IconService::global().version(),
            loader: None,
            sub_loaders: Vec::new(),
            probe: None,
            probes: HashMap::new(),
            select_after: None,
            last_rebuild: None,
            labels,
        };
        p.rebuild_places();
        p.go(&dir);
        p
    }

    /// 저장 기본 파일명(저장 모드).
    pub fn set_default_name(&mut self, name: &str) {
        self.name_box.set_text(name);
    }

    /// 최근 폴더(앱이 기억 · 사이드바 아래에 표시).
    pub fn set_recent(&mut self, recent: Vec<PathBuf>) {
        self.recent = recent.into_iter().filter(|p| p.is_dir()).take(8).collect();
        self.rebuild_places();
    }

    /// 숨김 파일 표시 초기값(앱 설정).
    pub fn set_show_hidden(&mut self, on: bool) {
        self.show_hidden = on;
        self.hidden_chk.set_checked(on);
        self.reload();
    }

    /// 점 파일 표시 초기값(앱 설정).
    pub fn set_show_dot(&mut self, on: bool) {
        let on = on || !DOT_TOGGLE;
        self.show_dot = on;
        self.dot_chk.set_checked(on);
        self.reload();
        self.rebuild_places_keep_selection();
    }

    /// 점 파일 표시 상태(앱이 설정에 되돌려 저장).
    #[must_use]
    pub fn show_dot(&self) -> bool {
        self.show_dot
    }

    fn rebuild_places_keep_selection(&mut self) {
        let sel = self.places_view.selected_row();
        self.rebuild_places();
        self.places_view.set_selected_row(sel);
        self.last_place_row = sel;
    }

    /// 하단 부가 콤보(예: 인코딩) — `(값, 라벨)` 목록과 초기 선택. 확정 뒤 [`Self::extra_value`]로 읽는다.
    pub fn set_extra(&mut self, label: impl Into<String>, items: &[(&str, &str)], selected: usize) {
        let items: Vec<ComboItem> = items.iter().map(|(v, l)| ComboItem::new(*v, *l)).collect();
        let mut c = Combo::new(items, selected);
        c.set_scale(self.base.scale);
        self.extra = Some((label.into(), c));
        self.layout();
    }

    /// 부가 콤보의 현재 값(없으면 None).
    #[must_use]
    pub fn extra_value(&self) -> Option<String> {
        self.extra.as_ref().map(|(_, c)| c.value())
    }

    fn extra_open(&self) -> bool {
        self.extra.as_ref().is_some_and(|(_, c)| c.is_open())
    }

    /// 현재 폴더.
    #[must_use]
    pub fn current_dir(&self) -> &Path {
        &self.dir
    }

    /// 숨김 표시 상태(앱이 설정에 되돌려 저장).
    #[must_use]
    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    /// 1회성 결과.
    pub fn take_action(&mut self) -> PickerAction {
        std::mem::replace(&mut self.action, PickerAction::None)
    }

    /// 콤보 드롭다운·편집 메뉴가 열려 있는가(호스트 Esc 가드).
    #[must_use]
    pub fn popup_open(&self) -> bool {
        self.filter_combo.is_open() || self.extra_open() || self.path_editing || self.menu.is_open()
    }

    /// 프레임 틱 — 다시 그려야 하면 true(아이콘/종류 이름 도착 포함).
    /// 덮어쓰기 무장 시간(ms · 0 = 한 번에 확정 — 종전 동작).
    pub fn set_overwrite_confirm_ms(&mut self, ms: u64) {
        self.overwrite_confirm_ms = ms;
    }

    /// 덮어쓰기 버튼을 눌렀다(클릭 · Enter) — 무장 전이면 무장하고 false · 무장 중이면(또는 무장 없음 설정) true = 확정.
    fn overwrite_press(&mut self) -> bool {
        if self.overwrite_confirm_ms == 0 || self.overwrite_arm.is_some() {
            self.overwrite_arm = None;
            return true;
        }
        let mut tb =
            TimeoutButton::new(self.labels.overwrite_yes.clone(), self.overwrite_confirm_ms)
                .with_warn(true)
                .with_show_remaining(false);
        let mut inv = Invalidations::default();
        tb.set_bounds(self.overwrite_yes_btn.bounds(), &mut inv);
        tb.set_scale(self.base.scale);
        tb.start(self.now_ms);
        // 가려지는 원래 버튼의 일시 상태(hover·눌림)를 비운다(포커스 규칙 ⑤).
        self.overwrite_yes_btn.clear_transient();
        self.overwrite_arm = Some(tb);
        false
    }

    fn overwrite_disarm(&mut self) {
        self.overwrite_arm = None;
    }

    pub fn tick(&mut self, now_ms: u64) -> bool {
        self.now_ms = now_ms;
        // 무장 버튼 — 게이지를 돌리고, 시간이 다 되면 원래 버튼으로(확정하지 않는다).
        let mut armed_redraw = false;
        if let Some(tb) = self.overwrite_arm.as_mut() {
            armed_redraw = tb.tick(now_ms);
            if tb.expired() {
                self.overwrite_arm = None;
                armed_redraw = true;
            }
        }
        armed_redraw
            | self.poll_loaders()
            | self.apply_icon_updates()
            | self.up_btn.tick(now_ms)
            | self.home_btn.tick(now_ms)
            | self.back_btn.tick(now_ms)
            | self.fwd_btn.tick(now_ms)
            | self.new_folder_btn.tick(now_ms)
            | self.ok_btn.tick(now_ms)
            | self.cancel_btn.tick(now_ms)
            | self.path_box.tick(now_ms)
            | self.name_box.tick(now_ms)
            | self.places_view.tick(now_ms)
            | self.grid.tick(now_ms)
            | self.filter_combo.tick_hover(now_ms)
            | self
                .extra
                .as_mut()
                .is_some_and(|(_, c)| c.tick_hover(now_ms))
    }

    /// 아이콘/종류 이름 조회가 아직 진행 중인가(도착을 받으려면 호스트 타이머가 살아 있어야 한다).
    #[must_use]
    pub fn icons_pending(&self) -> bool {
        IconService::global().pending() > 0
    }

    /// 애니메이션 진행 중(호스트가 타이머를 유지할 근거) — 아이콘 조회 중도 포함.
    #[must_use]
    pub fn animating(&self) -> bool {
        self.overwrite_arm.is_some()
            || self.loading()
            || self.icons_pending()
            || self.up_btn.is_animating()
            || self.home_btn.is_animating()
            || self.back_btn.is_animating()
            || self.fwd_btn.is_animating()
            || self.new_folder_btn.is_animating()
            || self.ok_btn.is_animating()
            || self.cancel_btn.is_animating()
            || self.path_box.is_animating()
            || self.name_box.is_animating()
            || self.filter_combo.hover_animating()
            || self
                .extra
                .as_ref()
                .is_some_and(|(_, c)| c.hover_animating())
    }

    /// 포커스 텍스트 박스(IME 배선).
    pub fn focused_textbox(&mut self) -> Option<&mut TextBox> {
        if self.path_editing && self.path_box.is_focused() {
            Some(&mut self.path_box)
        } else if self.name_box.is_focused() {
            Some(&mut self.name_box)
        } else {
            None
        }
    }

    // ───────────────────────── 데이터 ─────────────────────────

    fn rebuild_places(&mut self) {
        let mut nodes: Vec<TreeNode> = Vec::new();
        let label_of = |p: &Place, l: &PickerLabels| -> String {
            match p.kind {
                PlaceKind::Home => l.place_home.clone(),
                PlaceKind::Desktop => l.place_desktop.clone(),
                PlaceKind::Documents => l.place_documents.clone(),
                PlaceKind::Downloads => l.place_downloads.clone(),
                PlaceKind::Drive | PlaceKind::Recent => p.name.clone(),
            }
        };
        let places = self.places.clone();
        let mut std_places: Vec<TreeNode> = Vec::new();
        for p in places.iter().filter(|p| p.kind != PlaceKind::Drive) {
            let img = self.path_icon(&p.path);
            std_places.push(self.folder_node(label_of(p, &self.labels), &p.path, img));
        }
        nodes.extend(std_places);
        let mut drives: Vec<TreeNode> = Vec::new();
        for p in places.iter().filter(|p| p.kind == PlaceKind::Drive) {
            let img = self.path_icon(&p.path);
            drives.push(self.folder_node(p.name.clone(), &p.path, img));
        }
        if !drives.is_empty() {
            // 그룹 행 자체가 가상 최상위(클릭 = 드라이브 목록 · 탐색기 "내 PC").
            let mut b = TreeNode::branch(self.labels.place_drives.clone(), drives)
                .with_cells(vec![nexa_fs::VIRTUAL_ROOT.into()]);
            b.expanded = true;
            nodes.push(b);
        }
        if !self.recent.is_empty() {
            let folder = self.kind_icon(true, "");
            let kids: Vec<TreeNode> = self
                .recent
                .iter()
                .map(|p| self.folder_node(nexa_fs::path::display(p), p, folder.clone()))
                .collect();
            let mut b = TreeNode::branch(self.labels.place_recent.clone(), kids);
            b.expanded = true;
            nodes.push(b);
        }
        *self.places_view.model_mut() = TreeModel::new(nodes);
        self.places_view.set_selected_row(usize::MAX);
        self.last_place_row = usize::MAX;
        // 셰브론 유무는 백그라운드 프로브(장소 + 드라이브 + 최근 · 이미 아는 것은 제외).
        let mut paths: Vec<PathBuf> = self.places.iter().map(|p| p.path.clone()).collect();
        paths.extend(self.recent.iter().cloned());
        paths.retain(|p| !self.probes.contains_key(p));
        if !paths.is_empty() {
            let opts = self.list_opts(true);
            self.probe = Some(ListHandle::probe(paths, opts));
        }
        self.apply_probes();
    }

    /// 장소 가시 행 → 경로(그룹 행은 None).
    /// 폴더 노드 — `cells[0]` = 전체 경로 · 자리표시 자식 1개(펼치면 하위 폴더를 읽어 채운다 · 없으면 글리프 제거).
    /// 사이드바 폴더 노드 — 하위 **폴더**가 하나라도 있을 때만 자리표시 자식(= 셰브론) · 숨김 규칙은 현재 설정.
    fn folder_node(&self, label: String, path: &Path, icon: Rc<IconImage>) -> TreeNode {
        // 기본 = 셰브론(자리표시) · 프로브가 "하위 폴더 없음"이면 `apply_probes`가 지운다(UI 스레드 프로브 0).
        let kids = match self.probes.get(path) {
            Some(&(_, has_dir)) if !has_dir => Vec::new(),
            _ => vec![TreeNode::leaf(String::new()).with_cells(vec![PENDING.into()])],
        };
        let mut n = TreeNode::branch(label, kids)
            .with_cells(vec![path.to_string_lossy().into_owned()])
            .with_image(icon);
        n.expanded = false;
        n
    }

    /// 가시 행 → 경로(그룹 행·자리표시 행은 None).
    fn place_path(&self, row: usize) -> Option<PathBuf> {
        let rows = self.places_view.rows();
        let r = rows.get(row)?;
        let p = r.cells.first()?;
        if p.is_empty() || p == PENDING {
            return None;
        }
        Some(PathBuf::from(p))
    }

    /// 펼쳐진 노드 중 아직 자리표시 자식만 가진 것을 **지연 열거**(하위 폴더만 · 자연 정렬 · 숨김 규칙 동일).
    fn lazy_load_places(&mut self) {
        let show_hidden = self.show_hidden;
        let show_dot = self.show_dot;
        let folder_icon = self.kind_icon(true, "");
        let mut todo: Vec<Vec<usize>> = Vec::new();
        for row in self.places_view.rows().iter() {
            if !row.expanded {
                continue;
            }
            if let Some(n) = Self::node_at(self.places_view.model().roots(), &row.path) {
                let pending = n.children.len() == 1
                    && n.children[0].cells.first().map(String::as_str) == Some(PENDING);
                if pending && n.cells.first().is_some_and(|c| !c.is_empty()) {
                    todo.push(row.path.clone());
                }
            }
        }
        if todo.is_empty() {
            return;
        }
        let _ = (show_hidden, show_dot, folder_icon);
        for path in todo {
            let Some(dir) = Self::node_at(self.places_view.model().roots(), &path)
                .and_then(|n| n.cells.first().cloned())
            else {
                continue;
            };
            if let Some(n) = Self::node_at_mut(self.places_view.model_mut().roots_mut(), &path) {
                if let Some(c) = n.children.first_mut() {
                    c.cells = vec![LOADING.into()];
                }
            }
            let opts = self.list_opts(true);
            self.sub_loaders.push(SubLoad {
                sidebar: true,
                node: path,
                items: Vec::new(),
                dirty: false,
                handle: ListHandle::start(PathBuf::from(dir), opts),
            });
        }
    }

    fn node_at<'a>(roots: &'a [TreeNode], path: &[usize]) -> Option<&'a TreeNode> {
        let (first, rest) = path.split_first()?;
        let mut n = roots.get(*first)?;
        for &i in rest {
            n = n.children.get(i)?;
        }
        Some(n)
    }

    fn node_at_mut<'a>(roots: &'a mut [TreeNode], path: &[usize]) -> Option<&'a mut TreeNode> {
        let (first, rest) = path.split_first()?;
        let mut n = roots.get_mut(*first)?;
        for &i in rest {
            n = n.children.get_mut(i)?;
        }
        Some(n)
    }

    /// 서비스 키의 아이콘 — 캐시 적중/도착이면 OS 아이콘, 아니면(조회 중·없음) 자체 그림. **절대 막지 않는다**.
    fn icon_for(&mut self, key: IconKey, is_dir: bool) -> Rc<IconImage> {
        if let Some(img) = self.icons.get(&key) {
            return img.clone();
        }
        match IconService::global().icon(&key, false) {
            Lookup::Ready(Some(ic)) => {
                let rc = Rc::new(IconImage::from_rgba(ic.w, ic.h, ic.rgba.clone()));
                self.icons.insert(key, rc.clone());
                rc
            }
            Lookup::Ready(None) | Lookup::Pending => {
                if is_dir {
                    self.fallback_dir.clone()
                } else {
                    self.fallback_file.clone()
                }
            }
        }
    }

    fn kind_icon(&mut self, is_dir: bool, ext: &str) -> Rc<IconImage> {
        self.icon_for(
            IconKey::Kind {
                ext: ext.to_string(),
                is_dir,
            },
            is_dir,
        )
    }

    fn path_icon(&mut self, path: &Path) -> Rc<IconImage> {
        self.icon_for(IconKey::Path(path.to_path_buf()), true)
    }

    /// OS 종류 이름(도착 전·없음 = None → 호출자 폴백 문구).
    fn kind_name(&mut self, is_dir: bool, ext: &str) -> Option<String> {
        match IconService::global().kind_name(ext, is_dir) {
            Lookup::Ready(v) => v,
            Lookup::Pending => None,
        }
    }

    /// 조회 결과가 도착했으면(서비스 버전 변화) 목록·장소의 아이콘/종류 이름을 **제자리에서** 갱신(스크롤·선택 유지).
    fn apply_icon_updates(&mut self) -> bool {
        let v = IconService::global().version();
        if v == self.icon_version {
            return false;
        }
        self.icon_version = v;
        let mut changed = false;
        // 트리 전체(펼친 하위 폴더 포함)의 (폴더?, 확장자) 조합 → 아이콘·종류 이름을 먼저 모은다(가변 차용 분리).
        fn collect(nodes: &[TreeNode], out: &mut Vec<(bool, String)>) {
            for n in nodes {
                if n.cells
                    .get(CELL_PATH)
                    .is_some_and(|p| !p.is_empty() && p != PENDING)
                {
                    let is_dir = n.cells.get(CELL_KIND).map(String::as_str) == Some("d");
                    let ext = n.cells.get(CELL_EXT).cloned().unwrap_or_default();
                    out.push((is_dir, ext));
                }
                collect(&n.children, out);
            }
        }
        let mut combos = Vec::new();
        collect(self.grid.model().roots(), &mut combos);
        combos.sort();
        combos.dedup();
        let mut icons: HashMap<(bool, String), Rc<IconImage>> = HashMap::new();
        let mut names: HashMap<(bool, String), String> = HashMap::new();
        for (is_dir, ext) in combos {
            icons.insert((is_dir, ext.clone()), self.kind_icon(is_dir, &ext));
            if let Some(k) = self.kind_name(is_dir, &ext) {
                names.insert((is_dir, ext), k);
            }
        }
        let kind_pos = self.col_order.iter().position(|&c| c == 3);
        fn apply(
            nodes: &mut [TreeNode],
            icons: &HashMap<(bool, String), Rc<IconImage>>,
            names: &HashMap<(bool, String), String>,
            kind_pos: Option<usize>,
            changed: &mut bool,
        ) {
            for n in nodes {
                let is_drive = n
                    .cells
                    .get(CELL_PATH)
                    .is_some_and(|p| std::path::Path::new(p).parent().is_none());
                if !is_drive
                    && n.cells
                        .get(CELL_PATH)
                        .is_some_and(|p| !p.is_empty() && p != PENDING)
                {
                    let is_dir = n.cells.get(CELL_KIND).map(String::as_str) == Some("d");
                    let ext = n.cells.get(CELL_EXT).cloned().unwrap_or_default();
                    if let Some(img) = icons.get(&(is_dir, ext.clone())) {
                        if n.image.as_ref().map(Rc::as_ptr) != Some(Rc::as_ptr(img)) {
                            n.image = Some(img.clone());
                            *changed = true;
                        }
                    }
                    if let (Some(k), Some(pos)) = (names.get(&(is_dir, ext)), kind_pos) {
                        if n.cells.get(pos) != Some(k) {
                            if let Some(c) = n.cells.get_mut(pos) {
                                *c = k.clone();
                                *changed = true;
                            }
                        }
                    }
                }
                apply(&mut n.children, icons, names, kind_pos, changed);
            }
        }
        apply(
            self.grid.model_mut().roots_mut(),
            &icons,
            &names,
            kind_pos,
            &mut changed,
        );
        // 장소(특수 폴더·드라이브·최근) — 수가 적어 통째로 다시 만든다(선택 행 보존).
        let sel = self.places_view.selected_row();
        self.rebuild_places();
        self.places_view.set_selected_row(sel);
        self.last_place_row = sel;
        changed || true
    }

    /// 브레드크럼 그리기(dir2 경로 바 규약: 끝 정렬 · 앞이 넘치면 `…` · 마지막 = 현재(비활성) · hover 강조 · 구분자 `›`).
    fn paint_crumbs(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.path_box.bounds();
        ctx.fill_round_rect(b, self.s(6), theme.field_bg);
        ctx.stroke_round_rect(b, self.s(6), theme.border, 1.0);
        ctx.select_font(FontSlot::Base, false);
        let ty = ctx.text_center_y(b.y, b.h);
        let crumbs = self.crumbs();
        let seg_pad = self.s(6);
        let sep = " › ";
        let sep_w = ctx.text_width(sep);
        let widths: Vec<i32> = crumbs
            .iter()
            .map(|(l, _)| ctx.text_width(l) + seg_pad * 2)
            .collect();
        let last = crumbs.len().saturating_sub(1);
        let avail = b.w - self.s(8);
        let total: i32 = widths.iter().sum::<i32>() + sep_w * last as i32;
        let mut start = 0usize;
        if total > avail && !crumbs.is_empty() {
            let mut acc = ctx.text_width("…") + sep_w;
            start = crumbs.len();
            for i in (0..crumbs.len()).rev() {
                acc += widths[i] + if i == last { 0 } else { sep_w };
                if acc > avail {
                    break;
                }
                start = i;
            }
            start = start.min(last);
        }
        let mut ranges = vec![(0, 0); start];
        let mut x = b.x + self.s(4);
        let inner = Rect::new(b.x + 1, b.y + 1, b.w - 2, b.h - 2);
        if start > 0 {
            ctx.text(x, ty, inner, "…", theme.text_dim);
            x += ctx.text_width("…");
            ctx.text(x, ty, inner, sep, theme.text_dim);
            x += sep_w;
        }
        for (i, (label, _)) in crumbs.iter().enumerate().skip(start) {
            let w = widths[i];
            let cell = Rect::new(x, b.y + 2, w, b.h - 4).intersection(&inner);
            if self.crumb_hover == Some(i) {
                ctx.fill_round_rect(cell, self.s(4), theme.sel_bg);
            }
            let fg = if i == last {
                theme.text
            } else {
                theme.text_dim
            };
            ctx.text(x + seg_pad, ty, inner, label, fg);
            ranges.push((x, x + w));
            x += w;
            if i != last {
                ctx.text(x, ty, inner, sep, theme.text_dim);
                x += sep_w;
            }
        }
        *self.crumb_ranges.borrow_mut() = ranges;
    }

    fn current_filter(&self) -> &FileFilter {
        let i = self
            .filter_combo
            .value()
            .parse::<usize>()
            .unwrap_or(0)
            .min(self.filters.len() - 1);
        &self.filters[i]
    }

    /// 폴더 이동(읽기 실패 = 메시지 · 현재 폴더 유지).
    fn go(&mut self, dir: &Path) {
        self.marks.clear();
        self.anchor = None;
        self.marks_text.clear();
        if self.go_no_history(dir) {
            self.history.push(self.dir.clone());
            self.sync_nav_buttons();
        }
    }

    /// 히스토리에 넣지 않는 이동(뒤로/앞으로) — 성공하면 true.
    fn go_no_history(&mut self, dir: &Path) -> bool {
        if !nexa_fs::is_virtual_root(dir) && !dir.is_dir() {
            self.message = Some((self.labels.err_list.clone(), true));
            return false;
        }
        self.dir = dir.to_path_buf();
        self.entries.clear();
        self.probes.clear();
        self.sub_loaders.clear(); // Drop = 취소
        self.last_rebuild = None;
        self.message = None;
        self.pending_overwrite = None;
        self.path_box.set_text(&nexa_fs::path::display(&self.dir));
        self.end_path_edit();
        self.refresh_grid(0);
        self.start_loader();
        true
    }

    fn list_opts(&self, dirs_only: bool) -> ListOpts {
        ListOpts {
            show_hidden: self.show_hidden,
            show_dot: self.show_dot,
            exts: self.current_filter().exts.clone(),
            dirs_only,
            batch: 0,
            skip_probe: !crate::probe_chevrons_enabled(),
        }
    }

    /// 현재 폴더 열거를 백그라운드로 시작(이전 것은 Drop으로 취소).
    fn start_loader(&mut self) {
        // 폴더 고르기 = 폴더만 열거한다(파일을 읽지도 그리지도 않는다 — 큰 폴더에서 더 빠르다).
        let opts = self.list_opts(self.mode == PickerMode::Folder);
        self.loader = Some(ListHandle::start(self.dir.clone(), opts));
    }

    /// 열거 진행 중인가(호스트 타이머 유지 근거).
    fn loading(&self) -> bool {
        self.loader.as_ref().is_some_and(|h| !h.is_done())
            || self.sub_loaders.iter().any(|l| !l.handle.is_done())
            || self.probe.as_ref().is_some_and(|h| !h.is_done())
    }

    /// 로더 메시지 소비(프레임당 상한 — UI가 막히지 않게) · 바뀌었으면 true.
    fn poll_loaders(&mut self) -> bool {
        const MAX_MSGS: usize = 8;
        let mut changed = false;
        // 현재 폴더.
        let mut got_batch = false;
        let mut finished: Option<Result<usize, String>> = None;
        if let Some(h) = self.loader.as_mut() {
            for _ in 0..MAX_MSGS {
                match h.try_recv() {
                    Some(ListMsg::Batch(v)) => {
                        self.entries.extend(v);
                        got_batch = true;
                    }
                    Some(ListMsg::Probe(p, a, b)) => {
                        self.probes.insert(p, (a, b));
                        changed = true;
                    }
                    Some(ListMsg::Done(r)) => {
                        finished = Some(r);
                        break;
                    }
                    None => break,
                }
            }
        }
        // 재구성은 첫 배치 즉시 · 이후 150ms 간격 · Done에서 1회(누적 전체 재정렬·재복제 반복 방지 · docs/37 P-4).
        const REBUILD_EVERY: Duration = Duration::from_millis(150);
        let due = finished.is_some()
            || (got_batch
                && self
                    .last_rebuild
                    .is_none_or(|t| t.elapsed() >= REBUILD_EVERY));
        if due {
            let sel = self.grid.selected_row();
            self.refresh_grid(sel);
            self.last_rebuild = if finished.is_some() {
                None
            } else {
                Some(Instant::now())
            };
            changed = true;
        }
        if let Some(r) = finished {
            match r {
                Ok(_) => {
                    if let Some(name) = self.select_after.take() {
                        self.select_name(&name);
                    }
                }
                Err(e) => {
                    self.message = Some((format!("{} — {e}", self.labels.err_list), true));
                }
            }
        }
        // 프로브 결과를 행에 반영(자식 없음 = 셰브론 제거 · 사이드바는 하위 폴더 기준).
        if changed {
            self.apply_probes();
        }
        // 지연 펼침 로더들.
        let mut done_idx: Vec<usize> = Vec::new();
        for (i, l) in self.sub_loaders.iter_mut().enumerate() {
            for _ in 0..MAX_MSGS {
                match l.handle.try_recv() {
                    Some(ListMsg::Batch(v)) => {
                        l.items.extend(v);
                        l.dirty = true;
                    }
                    Some(ListMsg::Probe(p, a, b)) => {
                        self.probes.insert(p, (a, b));
                        changed = true;
                    }
                    Some(ListMsg::Done(_)) => {
                        l.dirty = true;
                        done_idx.push(i);
                        break;
                    }
                    None => break,
                }
            }
        }
        let dirty: Vec<usize> = self
            .sub_loaders
            .iter()
            .enumerate()
            .filter(|(_, l)| l.dirty)
            .map(|(i, _)| i)
            .collect();
        for i in dirty {
            self.apply_sub_loader(i);
            changed = true;
        }
        for i in done_idx.into_iter().rev() {
            if i < self.sub_loaders.len() {
                self.sub_loaders.remove(i);
            }
        }
        // 사이드바 프로브.
        let mut probe_changed = false;
        if let Some(h) = self.probe.as_mut() {
            for _ in 0..MAX_MSGS {
                match h.try_recv() {
                    Some(ListMsg::Probe(p, a, b)) => {
                        self.probes.insert(p, (a, b));
                        probe_changed = true;
                    }
                    Some(ListMsg::Done(_)) | None => break,
                    Some(ListMsg::Batch(_)) => {}
                }
            }
        }
        if probe_changed {
            self.apply_probes();
            changed = true;
        }
        changed
    }

    /// 프로브 결과 → 그리드/사이드바 노드의 자리표시 자식 제거(자식 없음 = 셰브론 없음).
    fn apply_probes(&mut self) {
        fn walk(nodes: &mut [TreeNode], probes: &HashMap<PathBuf, (bool, bool)>, sidebar: bool) {
            for n in nodes {
                let key = if sidebar { 0 } else { CELL_PATH };
                if let Some(p) = n.cells.get(key) {
                    if !p.is_empty() && p != PENDING {
                        if let Some(&(has_any, has_dir)) = probes.get(Path::new(p)) {
                            let keep = if sidebar { has_dir } else { has_any };
                            let placeholder = n.children.len() == 1
                                && n.children[0].cells.first().map(String::as_str) == Some(PENDING);
                            if !keep && placeholder {
                                n.children.clear();
                            }
                        }
                    }
                }
                walk(&mut n.children, probes, sidebar);
            }
        }
        let probes = std::mem::take(&mut self.probes);
        walk(self.grid.model_mut().roots_mut(), &probes, false);
        walk(self.places_view.model_mut().roots_mut(), &probes, true);
        self.probes = probes;
    }

    /// 지연 펼침 결과를 그 노드의 자식으로(도착할 때마다 정렬해 교체 · 접혔으면 버린다).
    fn apply_sub_loader(&mut self, i: usize) {
        let (sidebar, path, mut items) = {
            let l = &mut self.sub_loaders[i];
            l.dirty = false;
            (l.sidebar, l.node.clone(), l.items.clone())
        };
        nexa_fs::sort_by(&mut items, if sidebar { &[] } else { &self.sort_keys });
        let kids: Vec<TreeNode> = if sidebar {
            let folder = self.kind_icon(true, "");
            items
                .iter()
                .filter(|e| e.is_dir)
                .map(|e| self.folder_node(e.name.clone(), &e.path, folder.clone()))
                .collect()
        } else {
            items.iter().map(|e| self.make_row(e)).collect()
        };
        let roots = if sidebar {
            self.places_view.model_mut().roots_mut()
        } else {
            self.grid.model_mut().roots_mut()
        };
        if let Some(n) = Self::node_at_mut(roots, &path) {
            if n.expanded {
                n.children = kids;
            }
        }
        self.apply_probes();
    }

    fn sync_nav_buttons(&mut self) {
        self.back_btn.set_enabled(self.history.can_back());
        self.fwd_btn.set_enabled(self.history.can_forward());
    }

    fn go_back(&mut self) {
        if let Some(p) = self.history.back().map(Path::to_path_buf) {
            if !self.go_no_history(&p) {
                let _ = self.history.forward();
            }
        }
        self.sync_nav_buttons();
    }

    fn go_forward(&mut self) {
        if let Some(p) = self.history.forward().map(Path::to_path_buf) {
            if !self.go_no_history(&p) {
                let _ = self.history.back();
            }
        }
        self.sync_nav_buttons();
    }

    fn go_home(&mut self) {
        if let Some(h) = nexa_fs::home_dir() {
            self.go(&h);
        }
    }

    /// 브레드크럼 세그먼트(루트 → 현재 · 라벨 = 폴더 이름 · 루트는 표기 그대로).
    fn crumbs(&self) -> Vec<(String, std::path::PathBuf)> {
        nexa_fs::path::parent_chain(&self.dir)
            .into_iter()
            .map(|p| {
                let label = if nexa_fs::is_virtual_root(&p) {
                    self.labels.place_drives.clone()
                } else {
                    p.file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| {
                            // 루트: Windows `C:\` → `C:` · unix `/` → **`/`**(비우면 빈 조각이 남는다 · 사용자 09-16).
                            let full = p.to_string_lossy();
                            let t = full.trim_end_matches(['\\', '/']);
                            if t.is_empty() {
                                full.into_owned()
                            } else {
                                t.to_string()
                            }
                        })
                };
                (label, p)
            })
            .collect()
    }

    /// 경로 편집 시작(우클릭 전용) — 전체 경로를 상자에 넣고 전체 선택.
    fn begin_path_edit(&mut self, inv: &mut Invalidations) {
        self.path_editing = true;
        self.path_box.set_text(&self.dir.to_string_lossy());
        self.path_box.set_focused(true);
        self.path_box.on_event(&InputEvent::SelectAll, inv);
    }

    fn end_path_edit(&mut self) {
        self.path_editing = false;
        self.path_box.set_focused(false);
    }

    fn reload(&mut self) {
        let name = self
            .grid
            .rows()
            .get(self.grid.selected_row())
            .filter(|r| r.depth == 0)
            .map(|r| r.label.clone());
        self.select_after = name;
        let dir = self.dir.clone();
        self.entries.clear();
        self.probes.clear();
        self.sub_loaders.clear();
        self.refresh_grid(0);
        let _ = dir;
        self.start_loader();
    }

    /// 필터·정렬 적용 → 그리드 모델 재구성(정렬 표시는 헤더 제목에).
    /// 항목 → 그리드 행(보이는 셀 = 표시 순서 · **숨은 셀** = 경로 · `d`/`f` · 확장자 — 열 수보다 많은 셀은 그리드가 무시한다).
    /// 폴더는 자리표시 자식을 가져 인라인으로 펼쳐진다(dir2 파일 그리드 · 사용자 09-15).
    fn make_row(&mut self, e: &Entry) -> TreeNode {
        let modified = e
            .modified
            .map(|t| nexa_fs::local_time(t).short())
            .unwrap_or_default();
        let size = if e.is_dir {
            String::new()
        } else {
            nexa_fs::fmt_size(e.size)
        };
        let ext = e.ext();
        let is_drive = e.is_dir && e.path.parent().is_none();
        let kind = if is_drive {
            self.labels.kind_drive.clone()
        } else {
            self.kind_name(e.is_dir, &ext).unwrap_or_else(|| {
                if e.is_dir {
                    self.labels.kind_folder.clone()
                } else if ext.is_empty() {
                    self.labels.kind_file.clone()
                } else {
                    format!("{} {}", ext.to_uppercase(), self.labels.kind_file)
                }
            })
        };
        let icon = if is_drive {
            self.path_icon(&e.path)
        } else {
            self.kind_icon(e.is_dir, &ext)
        };
        let logical = [modified, size, kind];
        let mut cells: Vec<String> = self
            .col_order
            .iter()
            .map(|&c| logical[c - 1].clone())
            .collect();
        cells.push(e.path.to_string_lossy().into_owned());
        cells.push(if e.is_dir { "d".into() } else { "f".into() });
        cells.push(ext);
        // ★ 현재 보기(숨김 · 확장자 필터)로 보여줄 자식이 없으면 셰브론 없음(dir2 X-43 · 사용자 09-15) —
        //   숨김 표시를 켜면 `reload`가 다시 판정해 바로 나타난다.
        let children = if e.is_dir {
            match self.probes.get(&e.path) {
                Some(&(has_any, _)) if !has_any => Vec::new(),
                _ => vec![TreeNode::leaf(String::new()).with_cells(vec![PENDING.into()])],
            }
        } else {
            Vec::new()
        };
        let mut n = TreeNode::branch(e.name.clone(), children)
            .with_cells(cells)
            .with_image(icon);
        n.expanded = false;
        n
    }

    /// 행의 숨은 셀 → (경로, 폴더?, 이름).
    fn row_item(cells: &[String], label: &str) -> Option<(PathBuf, bool)> {
        let path = cells.get(CELL_PATH)?;
        if path.is_empty() || path == PENDING {
            return None;
        }
        let _ = label;
        Some((
            PathBuf::from(path),
            cells.get(CELL_KIND).map(String::as_str) == Some("d"),
        ))
    }

    /// 펼쳐진 폴더 행 중 자리표시 자식만 가진 것을 지연 열거(사이드바와 같은 규약).
    fn lazy_load_grid(&mut self) {
        let mut todo: Vec<(Vec<usize>, PathBuf)> = Vec::new();
        for row in self.grid.rows().iter() {
            if !row.expanded {
                continue;
            }
            if let Some(n) = Self::node_at(self.grid.model().roots(), &row.path) {
                let pending = n.children.len() == 1
                    && n.children[0].cells.first().map(String::as_str) == Some(PENDING);
                if pending {
                    if let Some(p) = n.cells.get(CELL_PATH) {
                        todo.push((row.path.clone(), PathBuf::from(p)));
                    }
                }
            }
        }
        for (path, dir) in todo {
            // 자리표시를 "로딩"으로 바꿔 두 번 시작하지 않게 · 로더는 백그라운드 · 결과는 tick에서.
            if let Some(n) = Self::node_at_mut(self.grid.model_mut().roots_mut(), &path) {
                if let Some(c) = n.children.first_mut() {
                    c.cells = vec![LOADING.into()];
                }
            }
            let opts = self.list_opts(self.mode == PickerMode::Folder);
            self.sub_loaders.push(SubLoad {
                sidebar: false,
                node: path,
                items: Vec::new(),
                dirty: false,
                handle: ListHandle::start(dir, opts),
            });
        }
    }

    fn refresh_grid(&mut self, select: usize) {
        nexa_fs::sort_by(&mut self.entries, &self.sort_keys);
        let exts = self.current_filter().exts.clone();
        let top: Vec<Entry> = self
            .entries
            .iter()
            .filter(|e| e.is_dir || exts.is_empty() || exts.contains(&e.ext()))
            .cloned()
            .collect();
        let nodes: Vec<TreeNode> = top.iter().map(|e| self.make_row(e)).collect();
        let n_rows = nodes.len();
        // 정렬 배지 — ▲/▼ + 결합 순번(키가 둘 이상일 때) · 키가 비면 이름 ▲.
        let keys = &self.sort_keys;
        let mark = |k: SortKey| -> String {
            // 키가 비면 배지 없음(기본 = 정렬 없음 · 사용자 09-15 — 이름 ▲가 보여 3단 해제가 안 되는 것처럼 보였다).
            if keys.is_empty() {
                return String::new();
            }
            match keys.iter().position(|(kk, _)| *kk == k) {
                Some(i) => {
                    let arrow = if keys[i].1 { "▼" } else { "▲" };
                    // 결과 그리드와 같은 배지: ▲/▼ + 결합 순번(키 2개 이상) · 헤더 오른쪽 끝 accent.
                    if keys.len() > 1 {
                        format!("{arrow}{}", i + 1)
                    } else {
                        arrow.to_string()
                    }
                }
                None => String::new(),
            }
        };
        let mut cols = vec![GridColumn::new(self.labels.col_name.clone(), self.col_w[0])
            .with_badge(mark(SortKey::Name))];
        for &c in &self.col_order {
            let (label, k) = match c {
                1 => (&self.labels.col_modified, SortKey::Modified),
                2 => (&self.labels.col_size, SortKey::Size),
                _ => (&self.labels.col_kind, SortKey::Kind),
            };
            cols.push(GridColumn::new(label.clone(), self.col_w[c]).with_badge(mark(k)));
        }
        let focused = self.grid.is_focused();
        let mut grid = TreeGrid::new(TreeModel::new(nodes), cols);
        grid.set_fit_columns(true); // 열 합 밖 = 빈 공간(선택·hover·클릭 없음 · 사용자 09-15)
        grid.set_scale(self.base.scale);
        grid.set_focused(focused);
        grid.set_selected_row(select.min(n_rows.saturating_sub(1)));
        let mut inv = Invalidations::default();
        grid.set_bounds(self.grid_rect, &mut inv);
        grid.reveal_row(grid.selected_row());
        self.grid = grid;
        self.last_row_click = None;
        if self.multi() && !self.marks.is_empty() {
            self.sync_marks();
        }
    }

    // ───────────────────────── 다중 선택(열기 모드 · 09-22) ──────────────────

    /// 다중 선택을 쓰는 모드인가(열기만 · 저장·폴더 = 단일).
    fn multi(&self) -> bool {
        self.mode == PickerMode::Open
    }

    /// 가시 행 → 파일 경로(폴더·자리표시는 None).
    fn row_file(&self, row: usize) -> Option<PathBuf> {
        let rows = self.grid.rows();
        let r = rows.get(row)?;
        match Self::row_item(&r.cells, &r.label) {
            Some((p, false)) => Some(p),
            _ => None,
        }
    }

    /// 고른 파일들(고른 순서).
    #[must_use]
    pub fn marked_files(&self) -> &[PathBuf] {
        &self.marks
    }

    /// 단일 선택(기존 해제 · 기준 행 갱신) — 폴더면 빈 선택.
    fn mark_single(&mut self, row: usize) {
        self.marks = self.row_file(row).into_iter().collect();
        self.anchor = Some(row);
        self.sync_marks();
    }

    /// Ctrl — 토글(폴더는 기준 행만 옮긴다).
    fn mark_toggle(&mut self, row: usize) {
        if let Some(p) = self.row_file(row) {
            if let Some(i) = self.marks.iter().position(|m| *m == p) {
                self.marks.remove(i);
            } else {
                self.marks.push(p);
            }
        }
        self.anchor = Some(row);
        self.sync_marks();
    }

    /// Shift — 기준 행~`row`의 가시 파일 전부(행 순서 · 기준 행은 그대로).
    fn mark_range(&mut self, row: usize) {
        let a = self.anchor.unwrap_or(row);
        let (lo, hi) = (a.min(row), a.max(row));
        self.marks = (lo..=hi).filter_map(|r| self.row_file(r)).collect();
        if self.anchor.is_none() {
            self.anchor = Some(row);
        }
        self.sync_marks();
    }

    /// Ctrl+드래그 — 마지막으로 지난 행에서 `row`까지의 파일을 추가/해제(폴더는 건너뜀).
    fn sweep_to(&mut self, row: usize) {
        let Some((last, add)) = self.drag_sweep else {
            return;
        };
        if last == row {
            return;
        }
        let (lo, hi) = (last.min(row), last.max(row));
        let mut changed = false;
        for r in lo..=hi {
            let Some(p) = self.row_file(r) else { continue };
            let at = self.marks.iter().position(|m| *m == p);
            match (add, at) {
                (true, None) => {
                    self.marks.push(p);
                    changed = true;
                }
                (false, Some(i)) => {
                    self.marks.remove(i);
                    changed = true;
                }
                _ => {}
            }
        }
        self.drag_sweep = Some((row, add));
        if changed {
            self.sync_marks();
        }
    }

    /// 러버밴드 시작(빈 공간 좌클릭) — Ctrl이면 지금 선택을 밑바탕으로 · 아니면 선택을 비운다.
    fn band_start(&mut self, x: i32, y: i32, keep: bool) {
        let base = if keep { self.marks.clone() } else { Vec::new() };
        if !keep && !self.marks.is_empty() {
            self.marks.clear();
            self.sync_marks();
        }
        self.band = Some((Point { x, y }, Point { x, y }, base));
    }

    /// 러버밴드 끌기 — 밴드 세로 범위와 겹치는 행의 파일 = 밑바탕 ∪ 그 파일들(행 순서 · 폴더 제외).
    fn band_to(&mut self, x: i32, y: i32) {
        let Some((o, _, base)) = self.band.clone() else {
            return;
        };
        let (y0, y1) = (o.y.min(y), o.y.max(y));
        let vp = self.grid.rows_viewport();
        let (_, ch) = self.grid.content_size();
        let n = self.grid.rows().len();
        let (_, sy) = self.grid.scroll();
        let mut marks = base;
        if n > 0 && ch > 0 {
            let rh = (ch / n as i32).max(1);
            for r in 0..n {
                let top = vp.y - sy + r as i32 * rh;
                let bottom = top + rh;
                if bottom <= y0 || top >= y1 || bottom <= vp.y || top >= vp.bottom() {
                    continue;
                }
                if let Some(p) = self.row_file(r) {
                    if !marks.contains(&p) {
                        marks.push(p);
                    }
                }
            }
        }
        self.band = Some((
            o,
            Point { x, y },
            self.band.as_ref().map(|b| b.2.clone()).unwrap_or_default(),
        ));
        if marks != self.marks {
            self.marks = marks;
            self.sync_marks();
        }
    }

    /// Ctrl+A — 보이는 파일 전부(행 순서).
    fn mark_all(&mut self) {
        let n = self.grid.rows().len();
        self.marks = (0..n).filter_map(|r| self.row_file(r)).collect();
        self.sync_marks();
    }

    /// 선택 → 그리드 강조 · 이름 상자(`"a" "b"` · Windows 관례) · 안내 줄(개수 · 합계 크기).
    fn sync_marks(&mut self) {
        let rows = self.grid.rows();
        // 강조는 노드 경로로(행 인덱스 아님) — 다른 폴더를 펼쳐 행이 밀려도 그대로.
        let marked: Vec<Vec<usize>> = rows
            .iter()
            .filter(|r| {
                Self::row_item(&r.cells, &r.label)
                    .is_some_and(|(p, is_dir)| !is_dir && self.marks.contains(&p))
            })
            .map(|r| r.path.clone())
            .collect();
        self.grid.set_marked_paths(marked);
        if self.marks.len() > 1 {
            let text = self
                .marks
                .iter()
                .map(|p| {
                    format!(
                        "\"{}\"",
                        p.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join(" ");
            self.name_box.set_text(&text);
            self.marks_text = text;
            let total: u64 = self
                .marks
                .iter()
                .map(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
                .sum();
            self.message = Some((
                self.labels
                    .multi_selected
                    .replace("{0}", &self.marks.len().to_string())
                    .replace("{1}", &nexa_fs::fmt_size(total)),
                false,
            ));
        } else {
            if !self.marks_text.is_empty() && self.name_box.text() == self.marks_text {
                // 다중 표기가 남아 있으면 단일 이름(또는 빈 칸)으로 되돌린다.
                let one = self
                    .marks
                    .first()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.name_box.set_text(&one);
            }
            self.marks_text.clear();
            if matches!(self.message, Some((_, false))) {
                self.message = None;
            }
        }
    }

    /// 선택 행 → (경로, 폴더?).
    fn selected_item(&self) -> Option<(PathBuf, bool)> {
        let rows = self.grid.rows();
        let r = rows.get(self.grid.selected_row())?;
        Self::row_item(&r.cells, &r.label)
    }

    fn select_name(&mut self, name: &str) {
        if let Some(row) = self
            .grid
            .rows()
            .iter()
            .position(|r| r.depth == 0 && r.label == name)
        {
            self.grid.set_selected_row(row);
            self.grid.reveal_row(row);
        }
    }

    /// 행 활성화(더블클릭·Enter) — 폴더 진입 / 파일 확정.
    fn activate_row(&mut self, row: usize) {
        let rows = self.grid.rows();
        let Some(r) = rows.get(row) else { return };
        let Some((path, is_dir)) = Self::row_item(&r.cells, &r.label) else {
            return;
        };
        let name = r.label.clone();
        if is_dir {
            self.go(&path);
        } else {
            // 펼친 하위 폴더 안 파일이면 그 폴더로 옮긴 뒤 확정(파일명 상자는 현재 폴더 기준).
            if let Some(parent) = path.parent() {
                if parent != self.dir {
                    self.go(parent);
                }
            }
            self.name_box.set_text(&name);
            self.confirm();
        }
    }

    fn go_up(&mut self) {
        if nexa_fs::is_virtual_root(&self.dir) {
            return;
        }
        if let Some(parent) = self.dir.parent().map(Path::to_path_buf) {
            let child = self
                .dir
                .file_name()
                .map(|s| s.to_string_lossy().into_owned());
            self.go(&parent);
            if let Some(c) = child {
                self.select_name(&c);
            }
        } else {
            // 드라이브/볼륨 루트 위 = 가상 최상위(dir2 X-17 · 세 OS 동일).
            let here = self.dir.clone();
            self.go(Path::new(nexa_fs::VIRTUAL_ROOT));
            let name = nexa_fs::drive_entries()
                .into_iter()
                .find(|e| e.path == here)
                .map(|e| e.name);
            if let Some(n) = name {
                self.select_name(&n);
            }
        }
    }

    /// 경로 상자 Enter — 폴더면 이동 · 파일이면 확정 · 없으면 오류.
    fn commit_path(&mut self, text: &str) {
        let p = nexa_fs::path::resolve(text, &self.dir);
        if nexa_fs::is_virtual_root(&p) || p.is_dir() {
            self.go(&p);
        } else if p.is_file() {
            if let Some(parent) = p.parent() {
                self.go(parent);
            }
            if let Some(n) = p.file_name() {
                self.name_box.set_text(&n.to_string_lossy());
            }
            self.confirm();
        } else {
            self.message = Some((self.labels.err_not_found.clone(), true));
        }
    }

    /// 폴더 고르기의 확정 버튼: 이름 상자의 폴더(적었거나 목록에서 클릭한 것) → **지금 폴더** 순. 가상 최상위("내 PC")는 고를 수 없다.
    fn confirm_folder(&mut self) {
        // 이름 상자(적었거나 클릭으로 들어온 폴더) → 없으면 **지금 폴더**. 목록의 선택만으로는 고르지 않는다 — 열자마자 첫 줄이
        // 자동으로 잡혀 있어서, 그것을 따르면 "지금 폴더를 고르려고" 누른 확정이 첫 하위 폴더를 골라 버린다.
        let typed = self.name_box.text().trim().to_string();
        let target = if typed.is_empty() {
            self.dir.clone()
        } else {
            nexa_fs::path::resolve(&typed, &self.dir)
        };
        if nexa_fs::is_virtual_root(&target) || !target.is_dir() {
            self.message = Some((self.labels.err_not_found.clone(), true));
            return;
        }
        self.action = PickerAction::Confirm(target);
    }

    /// 확정 — 파일명 상자 → 경로. 저장은 덮어쓰기 2단 확인 · 확장자 자동 부여 · 이름 검증.
    fn confirm(&mut self) {
        // ★ 다중 선택(열기 모드): 이름 상자가 다중 표기 그대로면 고른 순서대로 통째로 확정.
        if self.multi() && self.marks.len() > 1 && self.name_box.text() == self.marks_text {
            self.action = PickerAction::ConfirmMany(self.marks.clone());
            return;
        }
        let mut name = self.name_box.text().trim().to_string();
        if name.is_empty() {
            if let Some((path, is_dir)) = self.selected_item() {
                if is_dir {
                    self.go(&path);
                    return;
                }
                name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
            } else {
                return;
            }
        }
        // 폴더 이름(또는 경로)을 치면 그리로 이동.
        let as_path = nexa_fs::path::resolve(&name, &self.dir);
        if as_path.is_dir() {
            self.go(&as_path);
            self.name_box.set_text("");
            return;
        }
        // 경로가 섞여 있으면 폴더 부분은 이동, 이름만 남긴다.
        let (dir, leaf) = match (as_path.parent(), as_path.file_name()) {
            (Some(d), Some(f)) if d != self.dir && d.is_dir() => {
                (d.to_path_buf(), f.to_string_lossy().into_owned())
            }
            _ => (self.dir.clone(), name),
        };
        if dir != self.dir {
            self.go(&dir);
        }
        match self.mode {
            PickerMode::Open => {
                let p = dir.join(&leaf);
                if p.is_file() {
                    self.action = PickerAction::Confirm(p);
                } else {
                    self.message = Some((self.labels.err_not_found.clone(), true));
                }
            }
            PickerMode::Save => {
                if nexa_fs::naming::validate(&leaf).is_err() {
                    self.message = Some((self.labels.err_bad_name.clone(), true));
                    return;
                }
                let mut leaf = leaf;
                let exts = self.current_filter().exts.clone();
                if !leaf.contains('.') {
                    if let Some(first) = exts.first() {
                        leaf = format!("{leaf}.{first}");
                    }
                }
                let p = dir.join(&leaf);
                if p.exists() && self.pending_overwrite.as_deref() != Some(p.as_path()) {
                    // ★ 확인 카드(모달)로 묻는다 — 확인하면 저장 · 취소하면 이름 상자로(사용자 09-16).
                    self.pending_overwrite = Some(p.clone());
                    self.overwrite_ask = Some(p);
                    self.layout_overwrite();
                    return;
                }
                self.action = PickerAction::Confirm(p);
            }
            // 폴더 고르기에서 여기까지 왔다 = 적은 이름이 폴더가 아니다(폴더였으면 위에서 그리로 들어갔다).
            PickerMode::Folder => {
                self.message = Some((self.labels.err_not_found.clone(), true));
            }
        }
    }

    fn make_new_folder(&mut self) {
        let base = self.labels.new_folder_name.clone();
        let mut name = base.clone();
        let mut n = 2;
        while self.dir.join(&name).exists() {
            name = format!("{base} ({n})");
            n += 1;
        }
        match std::fs::create_dir(self.dir.join(&name)) {
            Ok(()) => {
                self.reload();
                self.select_after = Some(name);
                self.message = None;
            }
            Err(e) => self.message = Some((format!("{} — {e}", self.labels.err_mkdir), true)),
        }
    }

    /// 헤더 클릭 → 정렬 열/방향.
    /// 표시 위치 → 논리 컬럼(0 = 이름 · 1 = 수정 · 2 = 크기 · 3 = 유형).
    fn logical_col(&self, pos: usize) -> usize {
        if pos == 0 {
            0
        } else {
            self.col_order.get(pos - 1).copied().unwrap_or(0)
        }
    }

    fn sort_key_of(col: usize) -> SortKey {
        match col {
            0 => SortKey::Name,
            1 => SortKey::Modified,
            2 => SortKey::Size,
            _ => SortKey::Kind,
        }
    }

    /// 우클릭 메뉴(dir2 파일 그리드 관례): 열기 · 경로 복사 · 이름 복사 · ─ · 새 폴더 · 새로 고침 · ─ · 숨김 파일 표시(✓).
    fn open_menu(&mut self, x: i32, y: i32) {
        let has = self.menu_row.and_then(|r| {
            let rows = self.grid.rows();
            rows.get(r)
                .and_then(|rr| Self::row_item(&rr.cells, &rr.label))
        });
        // 아이콘: 새 폴더 = OS 폴더 아이콘(색 그대로 · 없으면 도형) · 나머지 = 코드 도형 · 토글 = 켜짐/꺼짐 도형(사용자 09-15).
        let folder_icon = match IconService::global().icon(
            &IconKey::Kind {
                ext: String::new(),
                is_dir: true,
            },
            false,
        ) {
            Lookup::Ready(Some(ic)) => MenuIcon::from_rgba(ic.w, ic.h, &ic.rgba),
            _ => glyph(GlyphKind::FolderNew),
        };
        // 항목 위 = 항목 메뉴(열기·복사 + 폴더 메뉴) · 빈 공간 = 폴더 메뉴(새 폴더·새로 고침·표시 토글)만.
        let mut items = Vec::new();
        if has.is_some() {
            items.push(
                CtxItem::item("open", self.labels.menu_open.clone())
                    .with_icon(Some(glyph(GlyphKind::Open))),
            );
            items.push(
                CtxItem::item("copy_path", self.labels.menu_copy_path.clone())
                    .with_icon(Some(glyph(GlyphKind::Link))),
            );
            items.push(
                CtxItem::item("copy_name", self.labels.menu_copy_name.clone())
                    .with_icon(Some(glyph(GlyphKind::Text))),
            );
            items.push(CtxItem::Separator);
        }
        items.push(
            CtxItem::item("new_folder", self.labels.new_folder.clone())
                .with_icon(Some(folder_icon)),
        );
        items.push(
            CtxItem::item("refresh", self.labels.menu_refresh.clone())
                .with_icon(Some(glyph(GlyphKind::Refresh))),
        );
        items.push(CtxItem::Separator);
        items.push(
            CtxItem::item("hidden", self.labels.show_hidden.clone()).with_checked(self.show_hidden),
        );
        if DOT_TOGGLE {
            items.push(
                CtxItem::item("dot", self.labels.show_dot.clone()).with_checked(self.show_dot),
            );
        }
        self.menu.set_scale(self.base.scale);
        self.menu
            .open_at(x, y, items, self.base.bounds, self.s(170));
    }

    fn menu_pick(&mut self, id: &str) {
        let item = self.menu_row.and_then(|r| {
            let rows = self.grid.rows();
            rows.get(r).and_then(|rr| {
                Self::row_item(&rr.cells, &rr.label).map(|(p, d)| (p, d, rr.label.clone()))
            })
        });
        match id {
            "open" => {
                if let Some(r) = self.menu_row {
                    self.activate_row(r);
                }
            }
            "copy_path" => {
                if let Some((p, _, _)) = item {
                    self.action = PickerAction::CopyText(p.to_string_lossy().into_owned());
                }
            }
            "copy_name" => {
                if let Some((_, _, name)) = item {
                    self.action = PickerAction::CopyText(name);
                }
            }
            "new_folder" => self.make_new_folder(),
            "refresh" => self.reload(),
            "hidden" => {
                self.show_hidden = !self.show_hidden;
                self.hidden_chk.set_checked(self.show_hidden);
                self.reload();
                self.rebuild_places_keep_selection();
            }
            "dot" => {
                self.show_dot = !self.show_dot;
                self.dot_chk.set_checked(self.show_dot);
                self.reload();
                self.rebuild_places_keep_selection();
            }
            _ => {}
        }
    }

    /// 헤더 표시 위치별 x 범위(가로 스크롤 반영).
    fn header_cells(&self) -> Vec<(i32, i32)> {
        let g = self.grid_rect;
        let (sx, _) = self.grid.scroll();
        let mut cx = g.x - sx;
        let mut out = Vec::with_capacity(4);
        for pos in 0..=self.col_order.len() {
            let w = self.s(self.col_w[self.logical_col(pos)]);
            out.push((cx, cx + w));
            cx += w;
        }
        out
    }

    /// 헤더 경계(오른쪽 끝 ±4px) 위인가 → 그 표시 위치(폭 조절 대상).
    fn header_edge_at(&self, x: i32, y: i32) -> Option<usize> {
        let g = self.grid_rect;
        if y < g.y || y >= g.y + self.s(HEADER_H) {
            return None;
        }
        let tol = self.s(4);
        self.header_cells()
            .iter()
            .position(|&(_, x1)| (x - x1).abs() <= tol)
    }

    /// 커서가 헤더 경계 위인가(호스트가 ↔ 커서를 보여 줄 근거).
    #[must_use]
    pub fn header_edge_hover(&self, x: i32, y: i32) -> bool {
        self.hdr_resize.is_some() || self.header_edge_at(x, y).is_some()
    }

    /// 헤더 클릭 → 표시 위치.
    fn header_hit(&self, x: i32, y: i32) -> Option<usize> {
        let g = self.grid_rect;
        if y < g.y || y >= g.y + self.s(HEADER_H) || x < g.x || x >= g.right() {
            return None;
        }
        self.header_cells()
            .iter()
            .position(|&(x0, x1)| x >= x0 && x < x1)
    }

    /// 드래그 목표 위치(현재 x 기준 · 컬럼 중앙을 넘으면 그 다음 · 이름 열 앞으로는 못 간다).
    fn drop_pos_at(&self, x: i32) -> usize {
        let cells = self.header_cells();
        for (pos, &(x0, x1)) in cells.iter().enumerate().skip(1) {
            if x < (x0 + x1) / 2 {
                return pos;
            }
        }
        cells.len()
    }

    /// 정렬 토글(결과 그리드 규약): 클릭 = 그 키만 ▲→▼→해제 · Shift = 키 추가 / 방향 순환 / 제거.
    fn toggle_sort(&mut self, k: SortKey, shift: bool) {
        let pos = self.sort_keys.iter().position(|(kk, _)| *kk == k);
        if shift && !self.sort_keys.is_empty() {
            match pos {
                None => self.sort_keys.push((k, false)),
                Some(i) if !self.sort_keys[i].1 => self.sort_keys[i].1 = true,
                Some(i) => {
                    self.sort_keys.remove(i);
                }
            }
        } else {
            self.sort_keys = match pos {
                Some(i) if self.sort_keys.len() == 1 && !self.sort_keys[i].1 => vec![(k, true)],
                Some(i) if self.sort_keys.len() == 1 && self.sort_keys[i].1 => Vec::new(),
                _ => vec![(k, false)],
            };
        }
        let sel = self.grid.selected_row();
        self.refresh_grid(sel);
    }

    // ───────────────────────── 배치 ─────────────────────────

    /// 덮어쓰기 카드 사각형(가운데 · 폭 420 · 높이 120 논리 px).
    fn overwrite_card(&self) -> Rect {
        let sc = |v: f32| (v * self.base.scale).round() as i32;
        let (w, h) = (sc(420.0).min(self.base.bounds.w - sc(20.0)), sc(120.0));
        Rect::new(
            self.base.bounds.x + (self.base.bounds.w - w) / 2,
            self.base.bounds.y + (self.base.bounds.h - h) / 2,
            w,
            h,
        )
    }

    /// 카드 안 버튼 배치(오른쪽 아래 · 취소 | 덮어쓰기).
    fn layout_overwrite(&mut self) {
        let card = self.overwrite_card();
        let sc = |v: f32| (v * self.base.scale).round() as i32;
        let (bw, bh, gap, pad) = (sc(96.0), sc(28.0), sc(8.0), sc(12.0));
        let mut inv = Invalidations::default();
        let y = card.bottom() - pad - bh;
        self.overwrite_yes_btn
            .set_bounds(Rect::new(card.right() - pad - bw, y, bw, bh), &mut inv);
        self.overwrite_no_btn.set_bounds(
            Rect::new(card.right() - pad - bw * 2 - gap, y, bw, bh),
            &mut inv,
        );
        self.overwrite_yes_btn.set_scale(self.base.scale);
        self.overwrite_no_btn.set_scale(self.base.scale);
        if let Some(tb) = self.overwrite_arm.as_mut() {
            tb.set_bounds(self.overwrite_yes_btn.bounds(), &mut inv);
            tb.set_scale(self.base.scale);
        }
    }

    fn layout(&mut self) {
        if self.overwrite_ask.is_some() {
            self.layout_overwrite();
        }
        let b = self.base.bounds;
        let s = self.base.scale;
        let mut inv = Invalidations::default();
        for c in [
            &mut self.home_btn,
            &mut self.back_btn,
            &mut self.fwd_btn,
            &mut self.up_btn,
            &mut self.new_folder_btn,
            &mut self.ok_btn,
            &mut self.cancel_btn,
        ] {
            c.set_scale(s);
        }
        self.path_box.set_scale(s);
        self.name_box.set_scale(s);
        self.places_view.set_scale(s);
        self.grid.set_scale(s);
        self.filter_combo.set_scale(s);
        self.hidden_chk.set_scale(s);
        self.dot_chk.set_scale(s);
        let pad = self.s(PAD);
        let gap = self.s(GAP);
        let row = self.s(ROW);
        let x0 = b.x + pad;
        let x1 = b.right() - pad;
        // 1행: ⌂ ← → ↑ · 브레드크럼(편집 = 같은 자리 상자) · 새 폴더 (nexa-dir2 경로 바 · 사용자 09-15)
        let y = b.y + pad;
        let nav_w = row;
        let nf_w = self.s(BTN_W + 20);
        let mut nx = x0;
        let step = nav_w + self.s(2);
        for btn in [
            &mut self.home_btn,
            &mut self.back_btn,
            &mut self.fwd_btn,
            &mut self.up_btn,
        ] {
            btn.set_bounds(Rect::new(nx, y, nav_w, row), &mut inv);
            nx += step;
        }
        nx += gap - (step - nav_w);
        self.new_folder_btn
            .set_bounds(Rect::new(x1 - nf_w, y, nf_w, row), &mut inv);
        self.path_box
            .set_bounds(Rect::new(nx, y, (x1 - nf_w - gap) - nx, row), &mut inv);
        self.sync_nav_buttons();
        // 하단 2행(아래에서 위로)
        let y_btn = b.bottom() - pad - row;
        let y_name = y_btn - gap - row;
        let bw = self.s(BTN_W);
        self.cancel_btn
            .set_bounds(Rect::new(x1 - bw, y_btn, bw, row), &mut inv);
        self.ok_btn
            .set_bounds(Rect::new(x1 - bw * 2 - gap, y_btn, bw, row), &mut inv);
        self.hidden_chk
            .set_bounds(Rect::new(x0, y_btn, self.s(180), row), &mut inv);
        self.dot_chk.set_scale(s);
        // Windows 외 = 폭 0(안 그리고 · 입력 없음 · 메시지는 숨김 체크 바로 오른쪽부터).
        let dot_w = if DOT_TOGGLE { self.s(160) } else { 0 };
        self.dot_chk.set_bounds(
            Rect::new(x0 + self.s(180) + gap, y_btn, dot_w, row),
            &mut inv,
        );
        let ew = self.s(170);
        if let Some((_, c)) = &mut self.extra {
            c.set_scale(s);
            c.set_bounds(
                Rect::new(x1 - bw * 2 - gap * 2 - ew, y_btn, ew, row),
                &mut inv,
            );
        }
        let lw = self.s(LABEL_W);
        let fw = self.s(FILTER_W);
        self.filter_combo
            .set_bounds(Rect::new(x1 - fw, y_name, fw, row), &mut inv);
        // ★ 드롭다운은 창 바닥을 넘지 않는다(넘치면 위로 펼침 · 사용자 09-15 "확장자 콤보가 숨겨져 선택 불가").
        self.filter_combo.set_viewport_bottom(b.bottom());
        if let Some((_, c)) = &mut self.extra {
            c.set_viewport_bottom(b.bottom());
        }
        self.name_box.set_bounds(
            Rect::new(x0 + lw, y_name, (x1 - fw - gap) - (x0 + lw), row),
            &mut inv,
        );
        // 가운데: 장소 | 목록
        let y_mid = y + row + gap;
        let h_mid = (y_name - gap - y_mid).max(self.s(80));
        let pw = self.s(PLACES_W);
        self.places_view
            .set_bounds(Rect::new(x0, y_mid, pw, h_mid), &mut inv);
        self.grid_rect = Rect::new(x0 + pw + gap, y_mid, x1 - (x0 + pw + gap), h_mid);
        self.grid.set_bounds(self.grid_rect, &mut inv);
    }

    /// 포커스 규칙 — MouseDown마다 누른 곳 하나만.
    fn own_focus(&mut self, p: Point) {
        let up = self.up_btn.bounds().contains(p);
        let nf = self.new_folder_btn.bounds().contains(p);
        let ok = self.ok_btn.bounds().contains(p);
        let cancel = self.cancel_btn.bounds().contains(p);
        let path = self.path_editing && self.path_box.bounds().contains(p);
        if self.path_editing && !self.path_box.bounds().contains(p) {
            self.end_path_edit();
        }
        let name = self.name_box.bounds().contains(p);
        let places = self.places_view.bounds().contains(p);
        let grid = self.grid.bounds().contains(p);
        let combo = self.filter_combo.bounds().contains(p);
        let chk = self.hidden_chk.bounds().contains(p);
        let dot = self.dot_chk.bounds().contains(p);
        self.dot_chk.set_focused(dot);
        let ex = self
            .extra
            .as_ref()
            .is_some_and(|(_, c)| c.bounds().contains(p));
        if let Some((_, c)) = &mut self.extra {
            c.set_focused(ex);
        }
        self.up_btn.set_focused(up);
        let home = self.home_btn.bounds().contains(p);
        let back = self.back_btn.bounds().contains(p);
        let fwd = self.fwd_btn.bounds().contains(p);
        self.home_btn.set_focused(home);
        self.back_btn.set_focused(back);
        self.fwd_btn.set_focused(fwd);
        self.new_folder_btn.set_focused(nf);
        self.ok_btn.set_focused(ok);
        self.cancel_btn.set_focused(cancel);
        self.path_box.set_focused(path);
        self.name_box.set_focused(name);
        self.places_view.set_focused(places);
        self.grid.set_focused(grid);
        self.filter_combo.set_focused(combo);
        self.hidden_chk.set_focused(chk);
    }

    fn any_focused(&self) -> bool {
        self.path_box.is_focused()
            || self.name_box.is_focused()
            || self.places_view.is_focused()
            || self.grid.is_focused()
            || self.filter_combo.is_focused()
            || self.extra.as_ref().is_some_and(|(_, c)| c.is_focused())
    }

    /// 사건 뒤 1회성 신호 수거(버튼 클릭 · 콤보 변경 · 체크 · Enter).
    fn collect(&mut self) {
        if self.cancel_btn.take_clicked() {
            self.action = PickerAction::Cancel;
        }
        if self.ok_btn.take_clicked() {
            if self.mode == PickerMode::Folder {
                self.confirm_folder();
            } else {
                self.confirm();
            }
        }
        if self.up_btn.take_clicked() {
            self.go_up();
        }
        if self.home_btn.take_clicked() {
            self.go_home();
        }
        if self.back_btn.take_clicked() {
            self.go_back();
        }
        if self.fwd_btn.take_clicked() {
            self.go_forward();
        }
        if self.new_folder_btn.take_clicked() {
            self.make_new_folder();
        }
        if let Some(text) = self.path_box.take_committed() {
            self.end_path_edit();
            self.commit_path(&text);
        }
        if self.name_box.take_committed().is_some() {
            self.confirm();
        }
        if self.name_box.take_changed().is_some() {
            // 이름을 고치면 덮어쓰기 확인은 무효 · 다중 선택 표기를 고쳤으면 선택도 푼다.
            self.pending_overwrite = None;
            if self.marks.len() > 1 && self.name_box.text() != self.marks_text {
                self.marks.clear();
                self.anchor = None;
                self.marks_text.clear();
                self.grid.set_marked_paths(Vec::new());
            }
            if matches!(self.message, Some((_, false))) {
                self.message = None;
            }
        }
        if self.filter_combo.take_changed().is_some() {
            // ★ 확장자 필터는 **열거 단계**(nexa-fs `lister` · 배경 스레드)에서 걸러진다 → 캐시된 항목을 다시 거르는
            //   것만으로는 넓어진 필터의 파일이 나타나지 않는다(사용자 09-16 "콤보를 바꿔도 바로 반영 안 됨") → 다시 열거.
            self.reload();
        }
        if let Some(on) = self.hidden_chk.take_toggled() {
            self.show_hidden = on;
            self.reload();
            // 사이드바 트리도 다시 판정(빈 폴더 글리프 · 숨김 하위 폴더).
            self.rebuild_places_keep_selection();
        }
        if let Some(on) = self.dot_chk.take_toggled() {
            self.show_dot = on;
            self.reload();
            self.rebuild_places_keep_selection();
        }
        // 장소 선택 변화 → 이동.
        let pr = self.places_view.selected_row();
        if pr != self.last_place_row {
            self.last_place_row = pr;
            if let Some(p) = self.place_path(pr) {
                self.go(&p);
            }
        }
    }
}

impl Control for FilePicker {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
}

impl Widget for FilePicker {
    fn bounds(&self) -> Rect {
        self.base.bounds
    }

    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        self.base.bounds = bounds;
        self.layout();
        inv.push(bounds);
    }

    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        if matches!(ev, InputEvent::MouseUp { .. }) {
            self.drag_sweep = None;
            if self.band.take().is_some() {
                inv.push(self.base.bounds);
            }
        }
        if let InputEvent::MouseMove { x, y }
        | InputEvent::MouseDown { x, y, .. }
        | InputEvent::MouseUp { x, y }
        | InputEvent::RightDown { x, y } = *ev
        {
            self.cursor = (x, y);
        }
        let p = Point {
            x: self.cursor.0,
            y: self.cursor.1,
        };
        // ★ 덮어쓰기 확인 카드가 열려 있으면 모달 — 두 버튼과 Enter/Esc만.
        if let Some(path) = self.overwrite_ask.clone() {
            match *ev {
                InputEvent::Key {
                    key: Key::Enter, ..
                } => {
                    // Enter도 같은 규칙: 첫 번째 = 무장 · 두 번째 = 확정.
                    if self.overwrite_press() {
                        self.overwrite_ask = None;
                        self.action = PickerAction::Confirm(path);
                    }
                }
                InputEvent::Key {
                    key: Key::Escape, ..
                } => {
                    self.overwrite_disarm();
                    self.overwrite_ask = None;
                    self.pending_overwrite = None;
                }
                InputEvent::MouseDown { .. }
                | InputEvent::MouseUp { .. }
                | InputEvent::MouseMove { .. } => {
                    // 마우스 라우팅 규칙 — 커서 아래 버튼에만(놓기는 둘 다).
                    let up = matches!(*ev, InputEvent::MouseUp { .. });
                    // 무장 중이면 그 자리는 타이머 버튼의 것(원래 버튼은 가려져 사건을 받지 않는다).
                    let mut armed_click = false;
                    if let Some(tb) = self.overwrite_arm.as_mut() {
                        if up || tb.bounds().contains(p) {
                            tb.on_event(ev, inv);
                        }
                        armed_click = tb.take_fired() == Some(nexa_ctl::controls::FiredBy::Click);
                    } else if up || self.overwrite_yes_btn.bounds().contains(p) {
                        self.overwrite_yes_btn.on_event(ev, inv);
                    }
                    if up || self.overwrite_no_btn.bounds().contains(p) {
                        self.overwrite_no_btn.on_event(ev, inv);
                    }
                    if armed_click || self.overwrite_yes_btn.take_clicked() {
                        if self.overwrite_press() {
                            self.overwrite_ask = None;
                            self.action = PickerAction::Confirm(path);
                        }
                    } else if self.overwrite_no_btn.take_clicked() {
                        self.overwrite_disarm();
                        self.overwrite_ask = None;
                        self.pending_overwrite = None;
                    }
                }
                _ => {}
            }
            inv.push(self.base.bounds);
            return;
        }
        let is_mouse = matches!(
            ev,
            InputEvent::MouseDown { .. }
                | InputEvent::MouseUp { .. }
                | InputEvent::MouseMove { .. }
                | InputEvent::RightDown { .. }
        );
        let is_wheel = matches!(ev, InputEvent::Wheel { .. } | InputEvent::HWheel { .. });
        // 열린 우클릭 메뉴 = 모달(바깥 클릭은 닫고 통과 · 항목 선택은 그 클릭으로 끝).
        if self.menu.is_open() {
            self.menu.set_scale(self.base.scale);
            let consumed = self.menu.on_event(ev);
            if let Some(id) = self.menu.take_picked() {
                self.menu_pick(&id);
                inv.push(self.base.bounds);
                return;
            }
            if consumed || self.menu.is_open() || !matches!(ev, InputEvent::MouseDown { .. }) {
                inv.push(self.base.bounds);
                return;
            }
        }
        // 열린 콤보 = 모달(바깥 클릭은 닫고 그 클릭을 계속 진행 — 팝업 UX 규칙).
        if self.filter_combo.is_open() {
            self.filter_combo.on_event(ev, inv);
            self.collect();
            if self.filter_combo.is_open() || !matches!(ev, InputEvent::MouseDown { .. }) {
                inv.push(self.base.bounds);
                return;
            }
        }
        if self.extra_open() {
            if let Some((_, c)) = &mut self.extra {
                c.on_event(ev, inv);
            }
            if self.extra_open() || !matches!(ev, InputEvent::MouseDown { .. }) {
                inv.push(self.base.bounds);
                return;
            }
        }
        if matches!(
            ev,
            InputEvent::MouseDown { .. } | InputEvent::RightDown { .. }
        ) {
            self.own_focus(p);
        }
        // 헤더: 클릭 = 정렬(Shift = 결합) · 드래그 = 컬럼 이동(이름 열은 고정 · 사용자 09-15).
        match *ev {
            InputEvent::MouseDown { x, y, shift, .. } => {
                if let Some(pos) = self.header_edge_at(x, y) {
                    let col = self.logical_col(pos);
                    self.hdr_resize = Some((col, x, self.col_w[col]));
                    inv.push(self.base.bounds);
                    return;
                }
                if let Some(pos) = self.header_hit(x, y) {
                    self.hdr_drag = Some((pos, x, x, false, shift));
                    inv.push(self.base.bounds);
                    return;
                }
            }
            InputEvent::MouseMove { x, .. } if self.hdr_resize.is_some() => {
                if let Some((col, x0, w0)) = self.hdr_resize {
                    let dw = ((x - x0) as f32 / self.base.scale).round() as i32;
                    let w = (w0 + dw).max(24);
                    self.col_w[col] = w;
                    // 표시 위치 = 이름(0) 또는 col_order 안 인덱스 + 1.
                    let pos = if col == 0 {
                        0
                    } else {
                        self.col_order
                            .iter()
                            .position(|&c| c == col)
                            .map_or(0, |i| i + 1)
                    };
                    self.grid.set_column_width(pos, w);
                }
                inv.push(self.base.bounds);
                return;
            }
            InputEvent::MouseUp { .. } if self.hdr_resize.is_some() => {
                self.hdr_resize = None;
                inv.push(self.base.bounds);
                return;
            }
            InputEvent::MouseMove { x, .. } if self.hdr_drag.is_some() => {
                let thresh = self.s(4);
                if let Some(d) = self.hdr_drag.as_mut() {
                    d.2 = x;
                    if (x - d.1).abs() > thresh && d.0 > 0 {
                        d.3 = true;
                    }
                }
                inv.push(self.base.bounds);
                return;
            }
            InputEvent::MouseUp { x, .. } if self.hdr_drag.is_some() => {
                let (pos, _, _, moved, shift) =
                    self.hdr_drag.take().unwrap_or((0, 0, 0, false, false));
                if moved {
                    let to = self.drop_pos_at(x).max(1);
                    // 표시 위치(1..) ↔ col_order 인덱스(0..)
                    let from_i = pos - 1;
                    let mut to_i = to - 1;
                    if from_i < self.col_order.len() {
                        let c = self.col_order.remove(from_i);
                        if to_i > from_i {
                            to_i -= 1;
                        }
                        self.col_order.insert(to_i.min(self.col_order.len()), c);
                        let sel = self.grid.selected_row();
                        self.refresh_grid(sel);
                    }
                } else {
                    let k = Self::sort_key_of(self.logical_col(pos));
                    self.toggle_sort(k, shift);
                }
                inv.push(self.base.bounds);
                return;
            }
            _ => {}
        }
        // 마우스/휠 = 커서 아래 컨트롤 · 키 = 포커스 컨트롤.
        let route = |r: Rect, focused: bool| -> bool {
            if is_mouse || is_wheel {
                r.contains(p)
            } else {
                focused
            }
        };
        // 버튼은 마우스 사건을 항상 받는다(hover 정리).
        if is_mouse {
            self.up_btn.on_event(ev, inv);
            self.home_btn.on_event(ev, inv);
            self.back_btn.on_event(ev, inv);
            self.fwd_btn.on_event(ev, inv);
            self.new_folder_btn.on_event(ev, inv);
            self.ok_btn.on_event(ev, inv);
            self.cancel_btn.on_event(ev, inv);
            self.hidden_chk.on_event(ev, inv);
            self.dot_chk.on_event(ev, inv);
            self.filter_combo.on_event(ev, inv);
            if let Some((_, c)) = &mut self.extra {
                c.on_event(ev, inv);
            }
        } else if self.extra.as_ref().is_some_and(|(_, c)| c.is_focused()) {
            if let Some((_, c)) = &mut self.extra {
                c.on_event(ev, inv);
            }
        } else if self.dot_chk.is_focused() {
            self.dot_chk.on_event(ev, inv);
        } else if self.hidden_chk.is_focused() {
            self.hidden_chk.on_event(ev, inv);
        } else if self.filter_combo.is_focused() {
            self.filter_combo.on_event(ev, inv);
        } else if self.ok_btn.is_focused() || self.cancel_btn.is_focused() {
            self.ok_btn.on_event(ev, inv);
            self.cancel_btn.on_event(ev, inv);
        }
        // 브레드크럼(편집 아님): 세그먼트 클릭 = 이동 · 빈 곳 클릭/우클릭 = 편집 · hover 강조.
        let pb = self.path_box.bounds();
        if !self.path_editing && (is_mouse && pb.contains(p)) {
            match *ev {
                InputEvent::MouseMove { x, .. } => {
                    let n = self.crumbs().len();
                    let h = self
                        .crumb_ranges
                        .borrow()
                        .iter()
                        .position(|&(x0, x1)| x >= x0 && x < x1)
                        .filter(|&i| i + 1 < n);
                    if h != self.crumb_hover {
                        self.crumb_hover = h;
                        inv.push(pb);
                    }
                }
                InputEvent::MouseDown { x, .. } => {
                    let crumbs = self.crumbs();
                    let hit = self
                        .crumb_ranges
                        .borrow()
                        .iter()
                        .position(|&(x0, x1)| x >= x0 && x < x1);
                    match hit {
                        Some(i) if i + 1 < crumbs.len() => {
                            let target = crumbs[i].1.clone();
                            self.go(&target);
                        }
                        // 좌클릭은 이동만 — 빈 곳/현재 조각 클릭은 아무것도 하지 않는다(편집은 우클릭 전용 · 사용자 09-15).
                        Some(_) | None => {}
                    }
                }
                InputEvent::RightDown { .. } => {
                    // 첫 우클릭 = 편집 모드 진입만 — 같은 사건을 상자에 넘기면 편집 메뉴까지 뜬다(사용자 09-16).
                    // 편집 중의 우클릭은 아래에서 상자로 가서 메뉴가 뜬다.
                    self.begin_path_edit(inv);
                    inv.push(pb);
                    return;
                }
                _ => {}
            }
        } else if !pb.contains(p) && self.crumb_hover.is_some() {
            self.crumb_hover = None;
            inv.push(pb);
        }
        if self.path_editing {
            if matches!(
                ev,
                InputEvent::Key {
                    key: Key::Escape,
                    ..
                }
            ) {
                // Esc = 편집 취소 — 입력 중이던 글자를 버리고 진입 시 경로로 되돌린다(사용자 09-16).
                self.path_box.set_text(&nexa_fs::path::display(&self.dir));
                self.end_path_edit();
                inv.push(pb);
                return;
            }
            if route(pb, self.path_box.is_focused()) {
                self.path_box.on_event(ev, inv);
            }
        }
        if route(self.name_box.bounds(), self.name_box.is_focused()) {
            self.name_box.on_event(ev, inv);
        }
        if route(self.places_view.bounds(), self.places_view.is_focused()) {
            let chev = match *ev {
                InputEvent::MouseDown { x, y, .. } => {
                    self.places_view.row_hit(x, y).is_some_and(|(_, c)| c)
                }
                _ => false,
            };
            self.places_view.on_event(ev, inv);
            if chev {
                // 셰브론 = 펼침/접힘만(Explorer 탐색 창 관례) — 선택 변화로 이동하지 않게.
                self.last_place_row = self.places_view.selected_row();
            }
            self.lazy_load_places();
        }
        if route(self.grid.bounds(), self.grid.is_focused()) {
            match *ev {
                InputEvent::Key {
                    key: Key::Enter, ..
                } => {
                    let row = self.grid.selected_row();
                    self.activate_row(row);
                }
                InputEvent::Char { c: '\u{8}', .. } => self.go_up(),
                InputEvent::MouseDown {
                    x,
                    y,
                    shift,
                    primary,
                } => {
                    let hit = self.grid.row_hit(x, y);
                    self.grid.on_event(ev, inv);
                    self.lazy_load_grid();
                    if let Some((row, on_chev)) = hit {
                        if on_chev {
                            // 셰브론 = 인라인 펼침/접힘만(더블클릭 체인·파일명 갱신 없음).
                            self.last_row_click = None;
                        } else {
                            let now = Instant::now();
                            let dbl = matches!(self.last_row_click, Some((r, t)) if r == row && now.duration_since(t) <= DOUBLE_CLICK);
                            self.last_row_click = Some((row, now));
                            if let Some((path, is_dir)) = self.selected_item() {
                                // 폴더 고르기에서는 **클릭한 폴더**가 이름 상자에 들어간다(Windows의 "폴더:" 칸과 같다) — 확정 버튼은
                                // 이름 상자 → 지금 폴더 순으로 보므로, 열자마자 자동으로 잡힌 첫 줄이 골라지는 일이 없다.
                                let fill = !is_dir || self.mode == PickerMode::Folder;
                                if fill {
                                    // 펼친 하위 폴더 안의 항목이면 지금 폴더 기준 상대 경로로.
                                    let text = path
                                        .strip_prefix(&self.dir)
                                        .ok()
                                        .map(|r| r.to_string_lossy().into_owned())
                                        .filter(|r| !r.is_empty())
                                        .or_else(|| {
                                            path.file_name()
                                                .map(|n| n.to_string_lossy().into_owned())
                                        });
                                    if let Some(n) = text.filter(|_| is_dir) {
                                        self.name_box.set_text(&n);
                                    } else if let Some(n) = path.file_name() {
                                        self.name_box.set_text(&n.to_string_lossy());
                                    }
                                    self.pending_overwrite = None;
                                }
                            }
                            // ★ 다중 선택(열기 모드): 클릭 = 단일 · Ctrl = 토글 · Shift = 범위(dir2 규약).
                            if self.multi() {
                                if shift {
                                    self.mark_range(row);
                                } else if primary {
                                    self.mark_toggle(row);
                                    // 스윕 시작 — 누른 파일이 지금 선택돼 있으면 추가 모드 · 아니면 해제 모드.
                                    if let Some(p) = self.row_file(row) {
                                        self.drag_sweep = Some((row, self.marks.contains(&p)));
                                    }
                                } else {
                                    self.mark_single(row);
                                    // 행에서 시작하는 드래그 = 러버밴드(누른 파일을 밑바탕으로 · 클릭만 하면 단일 그대로 · 사용자 09-22).
                                    self.band_start(x, y, true);
                                }
                            }
                            if dbl {
                                self.last_row_click = None;
                                self.activate_row(row);
                            }
                        }
                    } else if self.multi() && !shift {
                        // 빈 공간(행 아래 · 열 밖) 좌클릭 = 러버밴드 시작(Ctrl = 기존 선택에 추가).
                        self.band_start(x, y, primary);
                    }
                }
                InputEvent::RightDown { x, y } => {
                    if let Some((row, _)) = self.grid.row_hit(x, y) {
                        self.grid.set_selected_row(row);
                        self.menu_row = Some(row);
                    } else {
                        self.menu_row = None;
                    }
                    self.open_menu(x, y);
                }
                InputEvent::MouseMove { x, y } if self.band.is_some() => {
                    self.band_to(x, y);
                    inv.push(self.base.bounds);
                }
                InputEvent::MouseMove { x, y } if self.drag_sweep.is_some() => {
                    self.grid.on_event(ev, inv);
                    if let Some((row, _)) = self.grid.row_hit(x, y) {
                        self.sweep_to(row);
                        self.grid.set_selected_row(row);
                    }
                }
                InputEvent::SelectAll if self.multi() => self.mark_all(),
                InputEvent::Key {
                    key: Key::Space, ..
                } if self.multi() && self.row_file(self.grid.selected_row()).is_some() => {
                    // 파일 행의 Space = 선택 토글(폴더 행은 그리드의 펼침/접힘 그대로).
                    let row = self.grid.selected_row();
                    self.mark_toggle(row);
                }
                InputEvent::Key {
                    key,
                    shift,
                    primary,
                } if self.multi()
                    && matches!(
                        key,
                        Key::Up | Key::Down | Key::PageUp | Key::PageDown | Key::Home | Key::End
                    ) =>
                {
                    self.grid.on_event(ev, inv);
                    self.lazy_load_grid();
                    let row = self.grid.selected_row();
                    if shift {
                        self.mark_range(row);
                    } else if primary {
                        // Ctrl+방향키 = 캐럿만(선택 유지 · 탐색기 규약).
                    } else {
                        self.mark_single(row);
                    }
                }
                _ => {
                    self.grid.on_event(ev, inv);
                    self.lazy_load_grid();
                }
            }
        }
        // 어디에도 포커스가 없을 때 Enter = 확정(OS 대화상자 관례).
        if !self.any_focused()
            && matches!(
                ev,
                InputEvent::Key {
                    key: Key::Enter,
                    ..
                }
            )
        {
            self.confirm();
        }
        self.collect();
        inv.push(self.base.bounds);
    }

    fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.base.bounds;
        ctx.fill_rect(b, theme.window_bg);
        ctx.select_font(FontSlot::Base, false);
        // 라벨
        let nb = self.name_box.bounds();
        let ty = ctx.text_center_y(nb.y, nb.h);
        let name_label = if self.mode == PickerMode::Folder {
            &self.labels.folder_name
        } else {
            &self.labels.file_name
        };
        ctx.text(b.x + self.s(PAD), ty, b, name_label, theme.text);
        let fb = self.filter_combo.bounds();
        // 필터 라벨은 콤보 왼쪽에 작게(자리가 있을 때만).
        let ft_w = ctx.text_width(&self.labels.file_type);
        if fb.x - self.s(GAP) - ft_w > nb.x + self.s(120) {
            let _ = ft_w;
        }
        self.home_btn.paint(ctx, theme);
        self.back_btn.paint(ctx, theme);
        self.fwd_btn.paint(ctx, theme);
        self.up_btn.paint(ctx, theme);
        self.new_folder_btn.paint(ctx, theme);
        if self.path_editing {
            self.path_box.paint(ctx, theme);
        } else {
            self.paint_crumbs(ctx, theme);
        }
        self.places_view.paint(ctx, theme);
        self.grid.paint(ctx, theme);
        if let Some((o, c, _)) = &self.band {
            let r = Rect::new(
                o.x.min(c.x),
                o.y.min(c.y),
                (o.x - c.x).abs().max(1),
                (o.y - c.y).abs().max(1),
            )
            .intersection(&self.grid.bounds());
            if !r.is_empty() {
                ctx.fill_rect_alpha(r, theme.accent, 0.15);
                ctx.stroke_round_rect(r, 0, theme.accent, 1.0);
            }
        }
        // 헤더 드래그 피드백 — 끄는 컬럼은 선택색 · 놓일 자리는 accent 세로선.
        if let Some((pos, _, x, true, _)) = self.hdr_drag {
            let cells = self.header_cells();
            let hh = self.s(HEADER_H);
            let g = self.grid_rect;
            if let Some(&(x0, x1)) = cells.get(pos) {
                let r = Rect::new(x0, g.y, x1 - x0, hh).intersection(&g);
                ctx.fill_rect_alpha(r, theme.sel_bg, 0.6);
            }
            let to = self.drop_pos_at(x);
            let lx = cells
                .get(to)
                .map_or(cells.last().map_or(g.x, |c| c.1), |c| c.0);
            ctx.fill_rect(
                Rect::new(lx - 1, g.y, 2, g.h).intersection(&g),
                theme.accent,
            );
        }
        self.name_box.paint(ctx, theme);
        self.hidden_chk.paint(ctx, theme);
        if DOT_TOGGLE {
            self.dot_chk.paint(ctx, theme);
        }
        self.ok_btn.paint(ctx, theme);
        self.cancel_btn.paint(ctx, theme);
        // 메시지(체크박스 오른쪽 · 버튼 왼쪽).
        if let Some((msg, err)) = &self.message {
            let cb = self.dot_chk.bounds();
            let x = cb.right() + self.s(GAP);
            let left_edge = self
                .extra
                .as_ref()
                .map_or(self.ok_btn.bounds().x, |(label, c)| {
                    c.bounds().x - self.s(GAP) * 2 - ctx.text_width(label)
                });
            let clip = Rect::new(x, cb.y, (left_edge - self.s(GAP) - x).max(0), cb.h);
            let ty = ctx.text_center_y(cb.y, cb.h);
            ctx.text(
                x,
                ty,
                clip,
                msg,
                if *err { theme.danger } else { theme.warn },
            );
        }
        if let Some((label, c)) = &self.extra {
            let cb = c.bounds();
            let lw = ctx.text_width(label);
            let ty = ctx.text_center_y(cb.y, cb.h);
            ctx.text(cb.x - self.s(GAP) - lw, ty, b, label, theme.text);
        }
        // 팝업은 맨 마지막(콤보 드롭다운 · 텍스트박스 편집 메뉴).
        if let Some((_, c)) = &self.extra {
            c.paint(ctx, theme);
        }
        self.filter_combo.paint(ctx, theme);
        if self.path_editing {
            self.path_box.paint_popup(ctx, theme);
        }
        self.menu.paint(ctx, theme);
        self.name_box.paint_popup(ctx, theme);
        // ★ 덮어쓰기 확인 카드(모달 · 맨 위) — 뒤를 살짝 어둡게 · 문구 + 파일 이름 + [취소] [덮어쓰기].
        if let Some(path) = &self.overwrite_ask {
            let sc = |v: f32| (v * self.base.scale).round() as i32;
            ctx.fill_rect_alpha(self.base.bounds, theme.text, 0.25);
            let card = self.overwrite_card();
            ctx.fill_round_rect(card, sc(8.0), theme.panel_bg);
            ctx.stroke_round_rect(card, sc(8.0), theme.border, 1.0);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let text = self.labels.overwrite_ask.replace("{0}", &name);
            let pad = sc(14.0);
            let clip = Rect::new(card.x + pad, card.y, card.w - pad * 2, card.h);
            ctx.text(card.x + pad, card.y + pad, clip, &text, theme.text);
            self.overwrite_no_btn.paint(ctx, theme);
            match &self.overwrite_arm {
                Some(tb) => tb.paint(ctx, theme),
                None => self.overwrite_yes_btn.paint(ctx, theme),
            }
        }
    }
}

/// 셸 아이콘이 없는 OS의 자체 그림(16px · 폴더 = 호박색 탭 폴더 · 파일 = 회색 종이 + 접힌 모서리).
fn fallback_icon(is_dir: bool) -> IconImage {
    const N: u32 = 16;
    let mut rgba = vec![0u8; (N * N * 4) as usize];
    let mut put = |x: u32, y: u32, c: [u8; 4]| {
        let i = ((y * N + x) * 4) as usize;
        rgba[i..i + 4].copy_from_slice(&c);
    };
    if is_dir {
        let body = [0xE8, 0xB8, 0x4A, 0xFF];
        let dark = [0xC9, 0x96, 0x2E, 0xFF];
        for y in 3..14 {
            for x in 1..15 {
                let tab = y < 5 && x > 7;
                if !tab {
                    put(
                        x,
                        y,
                        if y == 3 || y == 13 || x == 1 || x == 14 {
                            dark
                        } else {
                            body
                        },
                    );
                }
            }
        }
    } else {
        let paper = [0xF4, 0xF4, 0xF4, 0xFF];
        let edge = [0x9A, 0x9A, 0x9A, 0xFF];
        for y in 1..15 {
            for x in 3..13 {
                let fold = x > 9 && y < 4 && (x - 9) > (3 - y);
                if fold {
                    continue;
                }
                let border = y == 1
                    || y == 14
                    || x == 3
                    || x == 12
                    || (x > 9 && y < 5 && (x - 9) == (4 - y));
                put(x, y, if border { edge } else { paper });
            }
        }
    }
    IconImage::from_rgba(N, N, rgba)
}

/// 빈 폴더 셰브론 프로브(폴더마다 첫 일치 열거) 켜기/끄기 — 설정 `file.probe_chevrons`(끄면 모든 폴더에 셰브론 · nexa-sql 09-17 실행 속도 향상).
static PROBE_CHEVRONS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn set_probe_chevrons(on: bool) {
    PROBE_CHEVRONS.store(on, std::sync::atomic::Ordering::Relaxed);
}

#[must_use]
pub fn probe_chevrons_enabled() -> bool {
    PROBE_CHEVRONS.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> PickerLabels {
        PickerLabels {
            file_name: "File name:".into(),
            ok_open: "Open".into(),
            ok_save: "Save".into(),
            ok_folder: "Select Folder".into(),
            folder_name: "Folder:".into(),
            cancel: "Cancel".into(),
            new_folder: "New folder".into(),
            new_folder_name: "New folder".into(),
            kind_folder: "Folder".into(),
            kind_file: "File".into(),
            err_exists: "exists".into(),
            overwrite_ask: "{0} exists. Overwrite?".into(),
            overwrite_yes: "Overwrite".into(),
            err_not_found: "not found".into(),
            ..PickerLabels::default()
        }
    }

    /// 백그라운드 열거가 끝날 때까지 틱을 돌린다(테스트 전용 · 5초 상한).
    fn settle(p: &mut FilePicker) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            p.tick(0);
            if !p.loading() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "열거가 5초 안에 끝나야 한다"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        p.tick(0);
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nexa-dlg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sub")).unwrap_or(());
        std::fs::write(d.join("a.sql"), b"select 1").unwrap_or(());
        std::fs::write(d.join("b.txt"), b"x").unwrap_or(());
        d
    }

    #[test]
    fn filter_hides_other_extensions_and_keeps_folders() {
        let d = temp_dir("filter");
        let mut p = FilePicker::new(
            PickerMode::Open,
            Some(&d),
            vec![
                FileFilter::new("SQL", &["sql"]),
                FileFilter::new("All", &[]),
            ],
            labels(),
        );
        settle(&mut p);
        let names: Vec<String> = p.grid.rows().iter().map(|r| r.label.clone()).collect();
        assert_eq!(names, vec!["sub", "a.sql"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn save_confirms_twice_when_file_exists_and_appends_ext() {
        let d = temp_dir("save");
        let mut p = FilePicker::new(
            PickerMode::Save,
            Some(&d),
            vec![FileFilter::new("SQL", &["sql"])],
            labels(),
        );
        p.set_default_name("a");
        p.confirm();
        assert_eq!(
            p.take_action(),
            PickerAction::None,
            "첫 확정 = 덮어쓰기 안내"
        );
        assert!(p.pending_overwrite.is_some());
        assert!(p.overwrite_ask.is_some(), "확인 카드가 열린다");
        let mut inv = Invalidations::default();
        let enter = InputEvent::Key {
            key: Key::Enter,
            shift: false,
            primary: false,
        };
        // ★ 타임아웃 버튼(두 번 눌러야 저장): 첫 Enter = 무장만 · 시간이 지나면 풀린다 · 다시 두 번 = 확정.
        p.tick(1_000);
        p.on_event(&enter, &mut inv);
        assert!(p.overwrite_arm.is_some() && p.overwrite_ask.is_some());
        assert_eq!(
            p.take_action(),
            PickerAction::None,
            "한 번으로는 저장되지 않는다"
        );
        p.tick(1_000 + 6_000);
        assert!(p.overwrite_arm.is_none(), "만료 = 무장 해제(확정 아님)");
        assert_eq!(p.take_action(), PickerAction::None);
        p.on_event(&enter, &mut inv);
        p.on_event(&enter, &mut inv);
        assert_eq!(p.take_action(), PickerAction::Confirm(d.join("a.sql")));
        // 새 이름은 바로 확정 + 확장자 부여.
        p.set_default_name("new");
        p.confirm();
        assert_eq!(p.take_action(), PickerAction::Confirm(d.join("new.sql")));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 폴더 고르기(nexa-sql 09-21): 목록에 **폴더만** · 확정 = 고른 것이 없으면 지금 폴더 · 이름 상자에 적은 폴더 · 파일 이름이나
    /// 없는 경로는 거부 · Enter로 폴더 이름을 치면 그리로 들어간다(열기 모드와 같다).
    #[test]
    fn folder_mode_lists_only_folders_and_confirms_a_directory() {
        let d = temp_dir("folder");
        std::fs::create_dir_all(d.join("sub").join("deep")).unwrap_or(());
        let mut p = FilePicker::new(PickerMode::Folder, Some(&d), Vec::new(), labels());
        settle(&mut p);
        let names: Vec<String> = p.grid.rows().iter().map(|r| r.label.clone()).collect();
        assert_eq!(names, vec!["sub"], "파일(a.sql · b.txt)은 보이지 않는다");
        // 고른 것 없음 = 지금 폴더.
        p.confirm_folder();
        assert_eq!(p.take_action(), PickerAction::Confirm(d.clone()));
        // 이름 상자에 적은 하위 폴더.
        p.set_default_name("sub");
        p.confirm_folder();
        assert_eq!(p.take_action(), PickerAction::Confirm(d.join("sub")));
        // 파일 · 없는 경로 = 거부(오류 글).
        for bad in ["a.sql", "nope"] {
            p.set_default_name(bad);
            p.confirm_folder();
            assert_eq!(p.take_action(), PickerAction::None, "{bad}");
            assert!(matches!(p.message, Some((_, true))), "{bad}");
        }
        // Enter(confirm) = 폴더 이름이면 들어간다.
        p.set_default_name("sub");
        p.confirm();
        assert_eq!(p.current_dir(), d.join("sub"));
        settle(&mut p);
        let names: Vec<String> = p.grid.rows().iter().map(|r| r.label.clone()).collect();
        assert_eq!(names, vec!["deep"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 다중 선택(열기 모드 · 09-22): 단일 → Shift 범위 → Ctrl 토글 → 폴더 제외 → 확정 = 고른 순서 · 저장 모드는 단일.
    #[test]
    #[allow(clippy::unwrap_used)]
    fn open_mode_multi_select_rules() {
        let d = temp_dir("multi");
        for n in ["c1.sql", "c2.sql", "c3.sql"] {
            std::fs::write(d.join(n), "x").unwrap();
        }
        std::fs::write(d.join("sub").join("z.sql"), "z").unwrap();
        let mut p = FilePicker::new(PickerMode::Open, Some(&d), Vec::new(), labels());
        settle(&mut p);
        let names: Vec<String> = p.grid.rows().iter().map(|r| r.label.clone()).collect();
        let file_rows: Vec<usize> = (0..names.len())
            .filter(|&r| p.row_file(r).is_some())
            .collect();
        let dir_row = (0..names.len()).find(|&r| p.row_file(r).is_none()).unwrap();
        assert!(file_rows.len() >= 3, "{names:?}");
        // 단일.
        p.mark_single(file_rows[0]);
        assert_eq!(p.marks.len(), 1);
        assert!(p.grid.is_marked(file_rows[0]));
        // Shift 범위(폴더 행이 사이에 있어도 파일만).
        p.mark_range(*file_rows.last().unwrap());
        assert_eq!(p.marks.len(), file_rows.len());
        assert!(!p.grid.is_marked(dir_row));
        assert!(p.name_box.text().starts_with('"'));
        assert!(matches!(p.message, Some((_, false))));
        // Ctrl 토글 = 빼기 · 고른 순서 유지.
        p.mark_toggle(file_rows[1]);
        assert_eq!(p.marks.len(), file_rows.len() - 1);
        assert_eq!(p.marks[0], p.row_file(file_rows[0]).unwrap());
        // 폴더 토글 = 선택 변화 없음.
        let before = p.marks.clone();
        p.mark_toggle(dir_row);
        assert_eq!(p.marks, before);
        // ★ 폴더를 펼쳐 행이 끼어들어도 강조는 파일을 따라간다(가시 행 인덱스가 아니라 노드 경로 · 사용자 09-22 실기).
        let dir_path = p.grid.rows()[dir_row].path.clone();
        p.grid.model_mut().set_expanded(&dir_path, true);
        p.lazy_load_grid();
        settle(&mut p);
        let rows_now = p.grid.rows();
        assert!(rows_now.len() > names.len(), "자식 행이 들어왔다");
        for (i, r) in rows_now.iter().enumerate() {
            let is_file = FilePicker::row_item(&r.cells, &r.label).is_some_and(|(_, d)| !d);
            let in_marks = FilePicker::row_item(&r.cells, &r.label)
                .is_some_and(|(pp, _)| before.contains(&pp));
            assert_eq!(
                p.grid.is_marked(i),
                is_file && in_marks,
                "row {i} {}",
                r.label
            );
        }
        p.grid.model_mut().set_expanded(&dir_path, false);
        // ★ Ctrl+드래그 스윕: 누른 행이 선택돼 있으면 지나는 파일을 추가 · 해제돼 있으면 해제(사용자 09-22).
        p.drag_sweep = Some((file_rows[0], true));
        p.sweep_to(*file_rows.last().unwrap());
        assert_eq!(p.marks.len(), file_rows.len(), "전부 추가");
        p.drag_sweep = Some((file_rows[0], false));
        p.sweep_to(file_rows[1]);
        assert_eq!(p.marks.len(), file_rows.len() - 2, "두 행 해제");
        let mut inv2 = Invalidations::default();
        p.on_event(&InputEvent::MouseUp { x: 0, y: 0 }, &mut inv2);
        assert!(p.drag_sweep.is_none(), "MouseUp = 스윕 끝");
        // ★ 러버밴드: 빈 공간에서 끌어 세로 범위와 겹치는 행의 파일 선택 · Ctrl = 밑바탕 유지 · MouseUp = 끝.
        p.set_bounds(Rect::new(0, 0, 640, 480), &mut inv2);
        let vp = p.grid.rows_viewport();
        let rh = p.grid.content_size().1 / p.grid.rows().len() as i32;
        let y_of = |r: usize| vp.y + r as i32 * rh + rh / 2;
        p.band_start(vp.x + 5, y_of(file_rows[0]), false);
        assert!(p.marks.is_empty(), "수식키 없는 밴드 = 선택 비움");
        p.band_to(vp.x + 50, y_of(file_rows[1]));
        assert_eq!(p.marks.len(), 2, "두 행과 겹침: {:?}", p.marks);
        p.on_event(&InputEvent::MouseUp { x: 0, y: 0 }, &mut inv2);
        assert!(p.band.is_none());
        p.band_start(vp.x + 5, y_of(*file_rows.last().unwrap()), true);
        assert_eq!(p.marks.len(), 2, "Ctrl 밴드 = 기존 유지");
        p.band_to(vp.x + 5, y_of(*file_rows.last().unwrap()) + 1);
        assert_eq!(p.marks.len(), 3);
        p.on_event(&InputEvent::MouseUp { x: 0, y: 0 }, &mut inv2);
        // 행에서 시작한 일반 드래그: 누른 파일 + 지나는 행(밑바탕 = 그 파일 하나).
        p.mark_single(file_rows[0]);
        p.band_start(vp.x + 5, y_of(file_rows[0]), true);
        assert_eq!(p.marks.len(), 1, "클릭만 = 단일");
        p.band_to(vp.x + 5, y_of(file_rows[2]));
        assert_eq!(p.marks.len(), 3, "행에서 끌어 세 파일: {:?}", p.marks);
        p.band_to(vp.x + 5, y_of(file_rows[0]));
        assert_eq!(p.marks.len(), 1, "되돌리면 밑바탕만");
        p.on_event(&InputEvent::MouseUp { x: 0, y: 0 }, &mut inv2);
        p.marks = before.clone();
        p.sync_marks();
        // 확정 = 통째로(고른 순서).
        p.confirm();
        assert_eq!(p.take_action(), PickerAction::ConfirmMany(before));
        // 이름 상자를 고치면 단일로.
        p.name_box.set_text("c1.sql");
        p.marks.clear();
        p.grid.set_marked_paths(Vec::new());
        p.confirm();
        assert_eq!(p.take_action(), PickerAction::Confirm(d.join("c1.sql")));
        // Ctrl+A = 보이는 파일 전부 · 폴더 이동 = 해제.
        p.mark_all();
        assert_eq!(p.marks.len(), file_rows.len());
        p.go(&d.join("sub"));
        assert!(p.marks.is_empty());
        // 저장 모드는 다중을 쓰지 않는다.
        let mut sv = FilePicker::new(PickerMode::Save, Some(&d), Vec::new(), labels());
        settle(&mut sv);
        assert!(!sv.multi());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn open_rejects_missing_and_enters_folder_by_name() {
        let d = temp_dir("open");
        let mut p = FilePicker::new(PickerMode::Open, Some(&d), Vec::new(), labels());
        p.set_default_name("nope.sql");
        p.confirm();
        assert_eq!(p.take_action(), PickerAction::None);
        assert!(matches!(p.message, Some((_, true))));
        p.set_default_name("sub");
        p.confirm();
        assert_eq!(p.current_dir(), d.join("sub"));
        p.go_up();
        assert_eq!(p.current_dir(), d);
        p.set_default_name("b.txt");
        p.confirm();
        assert_eq!(p.take_action(), PickerAction::Confirm(d.join("b.txt")));
        let _ = std::fs::remove_dir_all(&d);
    }
}

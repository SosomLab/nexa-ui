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

use nexa_ctl::controls::LabelSide;
use nexa_ctl::IconImage;
use nexa_ctl::{
    Button, Checkbox, Combo, ComboControl, ComboItem, Control, ControlBase, DrawCtx, FontSlot,
    GridColumn, InputEvent, Invalidations, Key, Point, Rect, TextBox, Theme, TreeControl, TreeGrid,
    TreeModel, TreeNode, TreeView, Widget,
};
use nexa_fs::shell::{IconKey, IconService, Lookup};
use nexa_fs::{Entry, History, Place, PlaceKind, SortKey};
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
    /// 취소.
    pub cancel: String,
    /// 새 폴더 버튼.
    pub new_folder: String,
    /// 새 폴더 기본 이름.
    pub new_folder_name: String,
    /// 숨김 파일 표시.
    pub show_hidden: String,
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
    /// 장소 그룹.
    pub place_drives: String,
    /// 장소 그룹.
    pub place_recent: String,
    /// 경로 상자 placeholder.
    pub path_hint: String,
    /// 오류 — 파일 없음.
    pub err_not_found: String,
    /// 안내 — 같은 이름 존재(한 번 더 누르면 덮어씀).
    pub err_exists: String,
    /// 오류 — 파일명 규칙 위반.
    pub err_bad_name: String,
    /// 오류 — 폴더를 읽을 수 없음.
    pub err_list: String,
    /// 오류 — 새 폴더 실패.
    pub err_mkdir: String,
}

/// 선택기 결과(1회성 · [`FilePicker::take_action`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerAction {
    /// 없음.
    None,
    /// 확정(열기·저장 경로).
    Confirm(PathBuf),
    /// 취소.
    Cancel,
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
    /// 필터·정렬을 거친 표시 목록(그리드 행 ↔ 인덱스).
    shown: Vec<usize>,
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
    filter_combo: Combo,
    hidden_chk: Checkbox,
    /// 하단 부가 콤보(앱이 주입 · 예: 인코딩 — Golden/DBeaver 하단 줄 · 사용자 09-15) — (라벨, 콤보).
    extra: Option<(String, Combo)>,
    ok_btn: Button,
    cancel_btn: Button,
    // 상태
    cursor: (i32, i32),
    message: Option<(String, bool)>, // (본문, 오류인가)
    pending_overwrite: Option<PathBuf>,
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
        };
        let mut p = FilePicker {
            base: ControlBase::default(),
            mode,
            filters,
            dir: dir.clone(),
            entries: Vec::new(),
            shown: Vec::new(),
            sort_keys: Vec::new(),
            col_order: vec![2, 1, 3],
            hdr_drag: None,
            hdr_resize: None,
            show_hidden: false,
            places: nexa_fs::places(),
            recent: Vec::new(),
            home_btn: Button::new("⌂"),
            back_btn: Button::new("←"),
            fwd_btn: Button::new("→"),
            up_btn: Button::new("↑"),
            path_box: TextBox::new(labels.path_hint.clone()),
            path_editing: false,
            crumb_ranges: std::cell::RefCell::new(Vec::new()),
            crumb_hover: None,
            history: History::new(dir.clone()),
            new_folder_btn: Button::new(labels.new_folder.clone()),
            places_view: TreeView::new(TreeModel::new(Vec::new())),
            grid: TreeGrid::new(TreeModel::new(Vec::new()), Vec::new()),
            name_box: TextBox::new(String::new()),
            filter_combo: Combo::new(items, 0),
            hidden_chk: Checkbox::new(labels.show_hidden.clone(), false)
                .with_label_side(LabelSide::Right),
            extra: None,
            ok_btn: Button::new(ok_label),
            cancel_btn: Button::new(labels.cancel.clone()),
            cursor: (0, 0),
            message: None,
            pending_overwrite: None,
            last_row_click: None,
            last_place_row: usize::MAX,
            action: PickerAction::None,
            col_w: [320, 150, 90, 110],
            grid_rect: Rect::default(),
            icons: HashMap::new(),
            fallback_dir: Rc::new(fallback_icon(true)),
            fallback_file: Rc::new(fallback_icon(false)),
            icon_version: IconService::global().version(),
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
        self.filter_combo.is_open() || self.extra_open() || self.path_editing
    }

    /// 프레임 틱 — 다시 그려야 하면 true(아이콘/종류 이름 도착 포함).
    pub fn tick(&mut self, now_ms: u64) -> bool {
        self.apply_icon_updates()
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
        self.icons_pending()
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
            std_places.push(Self::folder_node(label_of(p, &self.labels), &p.path, img));
        }
        nodes.extend(std_places);
        let mut drives: Vec<TreeNode> = Vec::new();
        for p in places.iter().filter(|p| p.kind == PlaceKind::Drive) {
            let img = self.path_icon(&p.path);
            drives.push(Self::folder_node(p.name.clone(), &p.path, img));
        }
        if !drives.is_empty() {
            let mut b = TreeNode::branch(self.labels.place_drives.clone(), drives);
            b.expanded = true;
            nodes.push(b);
        }
        if !self.recent.is_empty() {
            let folder = self.kind_icon(true, "");
            let kids: Vec<TreeNode> = self
                .recent
                .iter()
                .map(|p| Self::folder_node(nexa_fs::path::display(p), p, folder.clone()))
                .collect();
            let mut b = TreeNode::branch(self.labels.place_recent.clone(), kids);
            b.expanded = true;
            nodes.push(b);
        }
        *self.places_view.model_mut() = TreeModel::new(nodes);
        self.places_view.set_selected_row(usize::MAX);
        self.last_place_row = usize::MAX;
    }

    /// 장소 가시 행 → 경로(그룹 행은 None).
    /// 폴더 노드 — `cells[0]` = 전체 경로 · 자리표시 자식 1개(펼치면 하위 폴더를 읽어 채운다 · 없으면 글리프 제거).
    fn folder_node(label: String, path: &Path, icon: Rc<IconImage>) -> TreeNode {
        let mut n = TreeNode::branch(
            label,
            vec![TreeNode::leaf(String::new()).with_cells(vec![PENDING.into()])],
        )
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
        let folder_icon = self.kind_icon(true, "");
        let mut todo: Vec<Vec<usize>> = Vec::new();
        for row in self.places_view.rows() {
            if !row.expanded {
                continue;
            }
            if let Some(n) = Self::node_at(&self.places_view.model().roots, &row.path) {
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
        for path in todo {
            let Some(dir) = Self::node_at(&self.places_view.model().roots, &path)
                .and_then(|n| n.cells.first().cloned())
            else {
                continue;
            };
            let mut subs: Vec<Entry> = nexa_fs::list(Path::new(&dir), show_hidden)
                .unwrap_or_default()
                .into_iter()
                .filter(|e| e.is_dir)
                .collect();
            nexa_fs::sort_by(&mut subs, &[]);
            let kids: Vec<TreeNode> = subs
                .iter()
                .map(|e| Self::folder_node(e.name.clone(), &e.path, folder_icon.clone()))
                .collect();
            if let Some(n) = Self::node_at_mut(&mut self.places_view.model_mut().roots, &path) {
                n.children = kids; // 비면 자식 0 = 글리프 없음(dir2 X-43)
            }
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
        let shown = self.shown.clone();
        for (row, &i) in shown.iter().enumerate() {
            let (is_dir, ext) = (self.entries[i].is_dir, self.entries[i].ext());
            let img = self.kind_icon(is_dir, &ext);
            let name = self.kind_name(is_dir, &ext);
            if let Some(node) = self.grid.model_mut().roots.get_mut(row) {
                if node.image.as_ref().map(Rc::as_ptr) != Some(Rc::as_ptr(&img)) {
                    node.image = Some(img);
                    changed = true;
                }
                if let Some(n) = name {
                    if node.cells.get(2) != Some(&n) {
                        if let Some(c) = node.cells.get_mut(2) {
                            *c = n;
                            changed = true;
                        }
                    }
                }
            }
        }
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
        let th = ctx.text_height();
        let ty = b.y + (b.h - th) / 2;
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
        if self.go_no_history(dir) {
            self.history.push(self.dir.clone());
            self.sync_nav_buttons();
        }
    }

    /// 히스토리에 넣지 않는 이동(뒤로/앞으로) — 성공하면 true.
    fn go_no_history(&mut self, dir: &Path) -> bool {
        match nexa_fs::list(dir, self.show_hidden) {
            Ok(entries) => {
                self.dir = dir.to_path_buf();
                self.entries = entries;
                self.message = None;
                self.pending_overwrite = None;
                self.path_box.set_text(&nexa_fs::path::display(&self.dir));
                self.end_path_edit();
                self.refresh_grid(0);
                true
            }
            Err(e) => {
                self.message = Some((format!("{} — {e}", self.labels.err_list), true));
                false
            }
        }
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
                let label = p
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| {
                        p.to_string_lossy()
                            .trim_end_matches(['\\', '/'])
                            .to_string()
                    });
                (label, p)
            })
            .collect()
    }

    /// 경로 편집 시작(우클릭 · 빈 곳 클릭) — 전체 경로를 상자에 넣고 전체 선택.
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
        let sel = self.grid.selected_row();
        if let Ok(entries) = nexa_fs::list(&self.dir, self.show_hidden) {
            self.entries = entries;
        }
        self.refresh_grid(sel);
    }

    /// 필터·정렬 적용 → 그리드 모델 재구성(정렬 표시는 헤더 제목에).
    fn refresh_grid(&mut self, select: usize) {
        nexa_fs::sort_by(&mut self.entries, &self.sort_keys);
        let exts = self.current_filter().exts.clone();
        self.shown = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_dir || exts.is_empty() || exts.contains(&e.ext()))
            .map(|(i, _)| i)
            .collect();
        // 아이콘·OS 종류 이름 — 표시 항목의 (폴더?, 확장자) 조합마다 1회(캐시 · 디스크 접근 없음).
        let mut icons: HashMap<(bool, String), Rc<IconImage>> = HashMap::new();
        let mut kind_names: HashMap<(bool, String), String> = HashMap::new();
        let combos: Vec<(bool, String)> = {
            let mut v: Vec<(bool, String)> = self
                .shown
                .iter()
                .map(|&i| (self.entries[i].is_dir, self.entries[i].ext()))
                .collect();
            v.sort();
            v.dedup();
            v
        };
        for (is_dir, ext) in combos {
            let img = self.kind_icon(is_dir, &ext);
            icons.insert((is_dir, ext.clone()), img);
            if let Some(k) = self.kind_name(is_dir, &ext) {
                kind_names.insert((is_dir, ext), k);
            }
        }
        let nodes: Vec<TreeNode> = self
            .shown
            .iter()
            .map(|&i| {
                let e = &self.entries[i];
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
                let kind = kind_names
                    .get(&(e.is_dir, ext.clone()))
                    .cloned()
                    .unwrap_or_else(|| {
                        if e.is_dir {
                            self.labels.kind_folder.clone()
                        } else if ext.is_empty() {
                            self.labels.kind_file.clone()
                        } else {
                            format!("{} {}", ext.to_uppercase(), self.labels.kind_file)
                        }
                    });
                let icon = icons.get(&(e.is_dir, ext)).cloned();
                let logical = [modified, size, kind];
                let cells: Vec<String> = self
                    .col_order
                    .iter()
                    .map(|&c| logical[c - 1].clone())
                    .collect();
                let node = TreeNode::leaf(e.name.clone()).with_cells(cells);
                match icon {
                    Some(img) => node.with_image(img),
                    None => node,
                }
            })
            .collect();
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
        grid.set_scale(self.base.scale);
        grid.set_focused(focused);
        grid.set_selected_row(select.min(self.shown.len().saturating_sub(1)));
        let mut inv = Invalidations::default();
        grid.set_bounds(self.grid_rect, &mut inv);
        self.grid = grid;
        self.last_row_click = None;
    }

    fn selected_entry(&self) -> Option<&Entry> {
        let row = self.grid.selected_row();
        self.shown.get(row).and_then(|&i| self.entries.get(i))
    }

    fn select_name(&mut self, name: &str) {
        if let Some(row) = self
            .shown
            .iter()
            .position(|&i| self.entries[i].name == name)
        {
            self.grid.set_selected_row(row);
        }
    }

    /// 행 활성화(더블클릭·Enter) — 폴더 진입 / 파일 확정.
    fn activate_row(&mut self, row: usize) {
        let Some(&i) = self.shown.get(row) else {
            return;
        };
        let e = self.entries[i].clone();
        if e.is_dir {
            self.go(&e.path);
        } else {
            self.name_box.set_text(&e.name);
            self.confirm();
        }
    }

    fn go_up(&mut self) {
        if let Some(parent) = self.dir.parent().map(Path::to_path_buf) {
            let child = self
                .dir
                .file_name()
                .map(|s| s.to_string_lossy().into_owned());
            self.go(&parent);
            if let Some(c) = child {
                self.select_name(&c);
            }
        }
    }

    /// 경로 상자 Enter — 폴더면 이동 · 파일이면 확정 · 없으면 오류.
    fn commit_path(&mut self, text: &str) {
        let p = nexa_fs::path::resolve(text, &self.dir);
        if p.is_dir() {
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

    /// 확정 — 파일명 상자 → 경로. 저장은 덮어쓰기 2단 확인 · 확장자 자동 부여 · 이름 검증.
    fn confirm(&mut self) {
        let mut name = self.name_box.text().trim().to_string();
        if name.is_empty() {
            if let Some(e) = self.selected_entry().cloned() {
                if e.is_dir {
                    self.go(&e.path);
                    return;
                }
                name = e.name;
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
                    self.pending_overwrite = Some(p);
                    self.message = Some((self.labels.err_exists.clone(), false));
                    return;
                }
                self.action = PickerAction::Confirm(p);
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
                self.select_name(&name);
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

    fn layout(&mut self) {
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
            .set_bounds(Rect::new(x0, y_btn, self.s(220), row), &mut inv);
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
            self.confirm();
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
            // 이름을 고치면 덮어쓰기 확인은 무효.
            self.pending_overwrite = None;
            if matches!(self.message, Some((_, false))) {
                self.message = None;
            }
        }
        if self.filter_combo.take_changed().is_some() {
            let sel = self.grid.selected_row();
            self.refresh_grid(sel);
        }
        if let Some(on) = self.hidden_chk.take_toggled() {
            self.show_hidden = on;
            self.reload();
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
        let is_mouse = matches!(
            ev,
            InputEvent::MouseDown { .. }
                | InputEvent::MouseUp { .. }
                | InputEvent::MouseMove { .. }
                | InputEvent::RightDown { .. }
        );
        let is_wheel = matches!(ev, InputEvent::Wheel { .. } | InputEvent::HWheel { .. });
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
            self.filter_combo.on_event(ev, inv);
            if let Some((_, c)) = &mut self.extra {
                c.on_event(ev, inv);
            }
        } else if self.extra.as_ref().is_some_and(|(_, c)| c.is_focused()) {
            if let Some((_, c)) = &mut self.extra {
                c.on_event(ev, inv);
            }
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
                        Some(_) => {}
                        None => self.begin_path_edit(inv),
                    }
                }
                InputEvent::RightDown { .. } => self.begin_path_edit(inv),
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
                InputEvent::MouseDown { x, y, .. } => {
                    self.grid.on_event(ev, inv);
                    if let Some((row, _)) = self.grid.row_hit(x, y) {
                        let now = Instant::now();
                        let dbl = matches!(self.last_row_click, Some((r, t)) if r == row && now.duration_since(t) <= DOUBLE_CLICK);
                        self.last_row_click = Some((row, now));
                        if let Some(e) = self.selected_entry().cloned() {
                            if !e.is_dir {
                                self.name_box.set_text(&e.name);
                                self.pending_overwrite = None;
                            }
                        }
                        if dbl {
                            self.last_row_click = None;
                            self.activate_row(row);
                        }
                    }
                }
                _ => self.grid.on_event(ev, inv),
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
        let th = ctx.text_height();
        // 라벨
        let nb = self.name_box.bounds();
        ctx.text(
            b.x + self.s(PAD),
            nb.y + (nb.h - th) / 2,
            b,
            &self.labels.file_name,
            theme.text,
        );
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
        self.ok_btn.paint(ctx, theme);
        self.cancel_btn.paint(ctx, theme);
        // 메시지(체크박스 오른쪽 · 버튼 왼쪽).
        if let Some((msg, err)) = &self.message {
            let cb = self.hidden_chk.bounds();
            let x = cb.right() + self.s(GAP);
            let left_edge = self
                .extra
                .as_ref()
                .map_or(self.ok_btn.bounds().x, |(label, c)| {
                    c.bounds().x - self.s(GAP) * 2 - ctx.text_width(label)
                });
            let clip = Rect::new(x, cb.y, (left_edge - self.s(GAP) - x).max(0), cb.h);
            ctx.text(
                x,
                cb.y + (cb.h - th) / 2,
                clip,
                msg,
                if *err { theme.danger } else { theme.warn },
            );
        }
        if let Some((label, c)) = &self.extra {
            let cb = c.bounds();
            let lw = ctx.text_width(label);
            ctx.text(
                cb.x - self.s(GAP) - lw,
                cb.y + (cb.h - th) / 2,
                b,
                label,
                theme.text,
            );
        }
        // 팝업은 맨 마지막(콤보 드롭다운 · 텍스트박스 편집 메뉴).
        if let Some((_, c)) = &self.extra {
            c.paint(ctx, theme);
        }
        self.filter_combo.paint(ctx, theme);
        if self.path_editing {
            self.path_box.paint_popup(ctx, theme);
        }
        self.name_box.paint_popup(ctx, theme);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn labels() -> PickerLabels {
        PickerLabels {
            file_name: "File name:".into(),
            ok_open: "Open".into(),
            ok_save: "Save".into(),
            cancel: "Cancel".into(),
            new_folder: "New folder".into(),
            new_folder_name: "New folder".into(),
            kind_folder: "Folder".into(),
            kind_file: "File".into(),
            err_exists: "exists".into(),
            err_not_found: "not found".into(),
            ..PickerLabels::default()
        }
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
        let p = FilePicker::new(
            PickerMode::Open,
            Some(&d),
            vec![
                FileFilter::new("SQL", &["sql"]),
                FileFilter::new("All", &[]),
            ],
            labels(),
        );
        let names: Vec<String> = p.shown.iter().map(|&i| p.entries[i].name.clone()).collect();
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
        p.confirm();
        assert_eq!(p.take_action(), PickerAction::Confirm(d.join("a.sql")));
        // 새 이름은 바로 확정 + 확장자 부여.
        p.set_default_name("new");
        p.confirm();
        assert_eq!(p.take_action(), PickerAction::Confirm(d.join("new.sql")));
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

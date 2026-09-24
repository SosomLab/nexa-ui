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
use crate::theme::{Color, IconImage, Theme};
use crate::FontSlot;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

/// 메뉴 항목 아이콘을 그릴지(09-17 nexa-sql "실행 속도 향상" — 우클릭마다 마스크 틴트/래스터 비용 0). 토글 도형은 유지.
static MENU_ICONS: AtomicBool = AtomicBool::new(true);

/// 메뉴 항목 아이콘 켜기/끄기(전역 · 설정 `ui.menu_icons`).
pub fn set_menu_icons(on: bool) {
    MENU_ICONS.store(on, Ordering::Relaxed);
}

/// 메뉴 항목 아이콘을 그리는가.
#[must_use]
pub fn menu_icons_enabled() -> bool {
    MENU_ICONS.load(Ordering::Relaxed)
}

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
    /// `w*h` 커버리지(틴트용 · 색 이미지도 알파는 여기 복사).
    pub alpha: Rc<[u8]>,
    /// `w*h*4` RGBA — 있으면 **색 그대로**(OS 폴더 아이콘 등 · 09-15) · 없으면 상태색 틴트.
    pub rgba: Option<Rc<[u8]>>,
}

impl MenuIcon {
    /// 마스크로 만든다(상태색 틴트).
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
            rgba: None,
        }
    }

    /// 색 이미지로 만든다(OS 아이콘 — 틴트하지 않는다).
    ///
    /// # Panics
    /// 길이가 `w*h*4`가 아니면 패닉(구성 오류).
    #[must_use]
    pub fn from_rgba(w: u32, h: u32, rgba: &[u8]) -> Self {
        assert_eq!(rgba.len(), (w * h * 4) as usize, "RGBA 길이 불일치");
        let alpha: Vec<u8> = rgba.chunks(4).map(|p| p[3]).collect();
        Self {
            w,
            h,
            alpha: Rc::from(alpha),
            rgba: Some(Rc::from(rgba)),
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
        /// 라벨 뒤 흐린 보조 글(완성 팝업 ` : 타입` · 사용자 09-24 "컬럼 이름이 더 잘 보이게").
        sub: Option<String>,
        /// 전체 일치 = 굵게 + 파랑(#0000FF · 완성 팝업 · 사용자 09-24).
        emph: bool,
        /// 라벨 안 일치 구간(바이트 범위 · 강조색으로 · 부분 일치).
        marks: Vec<std::ops::Range<usize>>,
        /// 하위 메뉴(비면 없음).
        children: Vec<CtxItem>,
        /// 토글 상태(`Some` = 켜짐/꺼짐 아이콘을 아이콘 칸에 · 라벨은 다른 행과 같은 열에 정렬 · 09-15).
        checked: Option<bool>,
        /// 현재 선택된 항목(라디오 의미) — 아이콘·라벨을 강조색으로(hover 행은 반전 그대로 · nexa-sql 보기 모드 09-16).
        active: bool,
        /// **체크 표시**(✓ · 아이콘 칸) — 목록 중 "지금 것". `Some(_)`인 항목이 하나라도 있으면(체크가 하나도 안 켜져 있어도)
        /// 전 행이 그 칸을 비워 두어 글자가 늘 같은 열에 놓인다 · 메뉴 아이콘 설정과 무관(nexa-sql 탭 연결 목록 09-18).
        mark: Option<bool>,
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
            sub: None,
            emph: false,
            marks: Vec::new(),
            children: Vec::new(),
            checked: None,
            active: false,
            mark: None,
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
            sub: None,
            emph: false,
            marks: Vec::new(),
            children: Vec::new(),
            checked: None,
            active: false,
            mark: None,
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
            sub: None,
            emph: false,
            marks: Vec::new(),
            children,
            checked: None,
            active: false,
            mark: None,
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
    /// 전체 일치 강조(굵게 + 파랑).
    #[must_use]
    pub fn with_emphasis(mut self, on: bool) -> Self {
        if let Self::Item { emph, .. } = &mut self {
            *emph = on;
        }
        self
    }

    /// 일치 구간(라벨 바이트 범위 · 오름차순 · 비겹침) — 강조색으로 그린다.
    #[must_use]
    pub fn with_marks(mut self, m: Vec<std::ops::Range<usize>>) -> Self {
        if let Self::Item { marks, .. } = &mut self {
            *marks = m;
        }
        self
    }

    /// 라벨 뒤 흐린 보조 글(` : 타입`).
    pub fn with_sub(mut self, s: impl Into<String>) -> Self {
        if let Self::Item { sub, .. } = &mut self {
            let v: String = s.into();
            *sub = (!v.is_empty()).then_some(v);
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
    /// 토글 항목(켜짐/꺼짐 아이콘 · 빌더).
    #[must_use]
    /// 현재 선택(라디오) 표시 — 아이콘·라벨 강조색.
    pub fn with_active(mut self, on: bool) -> Self {
        if let Self::Item { active, .. } = &mut self {
            *active = on;
        }
        self
    }

    /// ✓ 체크 표시(라디오 목록의 "지금 것").
    #[must_use]
    pub fn with_mark(mut self, on: bool) -> Self {
        if let Self::Item { mark, .. } = &mut self {
            *mark = Some(on);
        }
        self
    }

    pub fn with_checked(mut self, on: bool) -> Self {
        if let Self::Item { checked, .. } = &mut self {
            *checked = Some(on);
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
    /// ★ **기본 항목**(nexa-sql 사용자 09-22 "기본 메뉴 개념"): hover가 없을 때 Enter가 고르는 항목 · 그림은 hover(채움)와 달리
    /// accent **테두리 + accent 글자**. `open_at`마다 비워진다 — 호스트가 연 뒤 `set_default`.
    default_idx: Option<usize>,
    picked: Option<String>,
    scale: f32,
    /// 마지막으로 계산한 팝업 rect(히트 판정용). 열 때는 호스트가 준 근사 폭으로
    /// 잡고, **첫 paint가 실측 폭으로 보정**한다(셀 — paint는 `&self`).
    rect: std::cell::Cell<Rect>,
    /// 팝업이 넘어가면 안 되는 영역(열 때 저장 — 실측 보정 후 경계 재접기용).
    host: Rect,
    /// 가리면 안 되는 대상(행)과 누른 자리 — [`Self::open_beside`]로 열었을 때만. paint의 안전망도 이 규칙으로 다시 놓는다.
    avoid: Option<(Point, Rect)>,
    /// 마지막 paint가 본 그리기 표면 크기 — 호출자가 끝없는 `host`를 넘겨도 표면 안에 놓는다(하위 메뉴를 열 때도 쓴다).
    surface: std::cell::Cell<Option<(i32, i32)>>,
    /// 라벨 최대 폭(px) — 열 때는 호스트 근사, paint가 실측으로 올려친다(08-14
    /// 실기: 자당 근사가 글꼴 크기에 뒤처져 "소유자만 초대로 전환"이 잘렸다).
    fit_w: std::cell::Cell<i32>,
    /// 단축키 문구 최대 폭(px · paint 실측).
    sc_w: std::cell::Cell<i32>,
    /// 열린 하위 메뉴(+ 어느 항목의 것인가).
    child: Option<Box<ContextMenu>>,
    child_of: Option<usize>,
    /// 마지막 커서 위치(하위 메뉴로 **가는 중**인지 판정용 · 09-16).
    last_pos: Option<Point>,
    /// 하위 메뉴가 열린 채 다른 부모 행 위에 머문 시각 — 유예([`SUBMENU_GRACE_MS`]) 안이면 자식을 유지한다.
    pending_since: Option<std::time::Instant>,
    /// ★ 보이는 행 수 상한(nexa-sql 사용자 09-23 "최대 N개 미리보기 + 스크롤" — 검색어 이력 드롭다운 · 완성 팝업) · `None` = 전부.
    /// 넘치면 `first`부터 그 수만큼만 그리고 휠·↑/↓/PgUp/PgDn/Home/End로 스크롤한다(오른쪽에 가는 스크롤 표시).
    max_rows: Option<usize>,
    first: usize,
    /// ★ 폭 상한(논리 px · nexa-sql 완성 팝업 사용자 09-23 "긴 이름") — 라벨이 넘치면 가로 스크롤(`hscroll`) · 0일 때는 가운데 ….
    max_w: Option<i32>,
    /// 가로 스크롤 오프셋(px · 0 = 처음) · paint가 잰 라벨 넘침 폭.
    hscroll: std::cell::Cell<i32>,
    label_over: std::cell::Cell<i32>,
    /// ★ 클릭 = 선택(강조·카드) · 같은 행을 400 ms 안에 다시 클릭 = 확정 · Enter = 확정(nexa-sql 완성 팝업 · 사용자 09-24
    ///   "단일 클릭은 내용 표시 · 엔터/더블 클릭은 선택"). 기본(false) = 클릭이 곧 확정(우클릭 메뉴).
    click_selects: bool,
    last_click: Option<(usize, std::time::Instant)>,
    /// 트랙패드 세로 휠 누적(px · 40마다 1행 · nexa-sql 09-24 "끝까지 내렸는데 한 칸 위로" = delta 0 사건이 위로 1행이던 결함).
    wheel_acc: std::cell::Cell<i32>,
    /// ★ 최소 행 수(완성 팝업 · 사용자 09-24 "항목이 1개여도 10칸 높이"): 항목이 적어도 이 높이를 유지한다(빈 자리는 바탕).
    min_rows: Option<usize>,
}

/// 하위 메뉴 유예(ms) — 자식이 화면 안에 맞추느라 부모 행과 세로가 어긋나면 대각선 이동 중 다른 부모 행을 지난다.
/// 그 사이 자식이 닫히던 것(nexa-sql 사용자 09-16 "Copy SQL 메뉴 유실") → 자식 쪽으로 움직이는 동안은 유지.
const SUBMENU_GRACE_MS: u128 = 400;

impl ContextMenu {
    /// 새 메뉴(닫힌 상태).
    #[must_use]
    pub fn new() -> Self {
        Self {
            scale: 1.0,
            ..Self::default()
        }
    }

    /// 기본 항목 지정(활성 항목만 · Enter = 이것 · 열린 뒤에 부른다).
    pub fn set_default(&mut self, i: usize) {
        if matches!(self.items.get(i), Some(CtxItem::Item { enabled: true, .. })) {
            self.default_idx = Some(i);
        }
    }

    /// 폭 상한(논리 px · `None` = 내용대로) — 넘치는 라벨은 Shift+휠/틸트 휠로 가로 스크롤 · 스크롤 0이면 가운데 …
    /// (nexa-sql 완성 팝업 · 사용자 09-23 "세로·가로 스크롤 · 긴 이름").
    pub fn set_max_width(&mut self, w: Option<i32>) {
        self.max_w = w.filter(|&w| w > 0);
    }

    /// 최소 행 수(완성 팝업): 항목이 적어도 이 행 수만큼의 높이를 유지한다(옆 카드가 같은 높이를 쓴다 · 09-24).
    pub fn set_min_rows(&mut self, n: Option<usize>) {
        self.min_rows = n.filter(|&n| n > 0);
    }

    /// 클릭 = 선택 모드(완성 팝업): 단일 클릭은 강조만 · 더블 클릭/Enter = 확정.
    pub fn set_click_selects(&mut self, on: bool) {
        self.click_selects = on;
    }

    /// 가로로 넘친 라벨이 있는가(paint가 잰 값 · 호스트 안내용).
    #[must_use]
    pub fn label_overflow(&self) -> i32 {
        self.label_over.get()
    }

    /// 배율(고DPI).
    pub fn set_scale(&mut self, scale: f32) {
        self.scale = scale;
    }

    /// 보이는 행 수 상한(`None` = 전부 · `open_at` 뒤에도 유지) — 넘치면 스크롤(휠 · 키 · 오른쪽 표시).
    pub fn set_max_rows(&mut self, n: Option<usize>) {
        self.max_rows = n.filter(|&n| n > 0);
        self.first = 0;
    }

    /// 지금 hover(키보드 이동 포함) 행 — 호스트가 "Enter를 메뉴가 먹을지"를 가르는 데 쓴다(hover 없으면 Enter는 기본 항목/닫기).
    #[must_use]
    pub fn hovered(&self) -> Option<usize> {
        self.hover
    }

    /// 항목 id 목록(구분선 = 빈 문자열 · 열린 순서 그대로) — 호스트 시험이 "무엇이 떠 있나"를 확인하는 용도(09-24).
    #[must_use]
    pub fn item_ids(&self) -> Vec<&str> {
        self.items
            .iter()
            .map(|it| match it {
                CtxItem::Item { id, .. } => id.as_str(),
                _ => "",
            })
            .collect()
    }

    /// 보이는 행 수(상한 적용).
    fn vis_count(&self) -> usize {
        self.max_rows
            .map_or(self.items.len(), |m| m.min(self.items.len()))
    }

    fn scrollable(&self) -> bool {
        self.vis_count() < self.items.len()
    }

    /// `i`가 보이게 `first`를 옮긴다.
    fn ensure_visible(&mut self, i: usize) {
        let n = self.vis_count().max(1);
        if i < self.first {
            self.first = i;
        } else if i >= self.first + n {
            self.first = i + 1 - n;
        }
        self.first = self.first.min(self.items.len().saturating_sub(n));
    }

    /// 휠/키로 `rows`행 스크롤(음수 = 위).
    fn scroll_rows(&mut self, rows: i32) {
        let n = self.vis_count().max(1);
        let max_first = self.items.len().saturating_sub(n) as i32;
        self.first = (self.first as i32 + rows).clamp(0, max_first) as usize;
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

    /// 휠을 받을 자리인가 — 마지막 MouseMove 위치가 팝업(하위 포함) 안이면 참 · 위치를 모르면 참(종전 동작).
    fn wheel_inside(&self) -> bool {
        self.last_pos.is_none_or(|p| self.bounds().contains(p))
    }

    /// **바깥 클릭**인가 — 열린 동안 좌/우 MouseDown이 팝업(하위 메뉴 포함) 밖. `on_event`는 바깥 클릭을 *닫고 소비*하므로
    /// 호스트는 이 판정으로 "닫힌 클릭"과 "메뉴가 먹은 클릭"을 갈라 **바깥 클릭은 그대로 아래로 흘린다**(팝업 UX 규칙 —
    /// nexa-sql 09-22: 탭 메뉴가 열린 채 편집기·결과 탭을 우클릭하면 닫히기만 하고 다시 눌러야 했다).
    #[must_use]
    pub fn is_outside_click(&self, ev: &InputEvent) -> bool {
        if !self.is_open() {
            return false;
        }
        match *ev {
            InputEvent::MouseDown { x, y, .. } | InputEvent::RightDown { x, y } => {
                !self.bounds().contains(Point { x, y })
            }
            _ => false,
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
        self.first = 0;
        // 커서 위치는 이번 열림 동안 받은 MouseMove로만(이전 열림의 자리를 믿지 않는다 · 휠 안/밖 판정 · 09-24).
        self.last_pos = None;
        self.default_idx = None;
        self.fit_w.set(text_w);
        self.sc_w.set(0);
        self.host = host;
        self.hover = None;
        self.picked = None;
        self.child = None;
        self.child_of = None;
        self.avoid = None;
        self.hscroll.set(0);
        self.label_over.set(0);
        self.last_click = None;
        self.wheel_acc.set(0);
        let (w, h) = self.size_px();
        // 경계 접기 — 공용 배치 규칙([`crate::geom::place_popup`]: 정방향 → 반대쪽 → 밀어 넣기). 표면 크기를 이미 알면(앞선
        // paint) 그 안에서 · 모르면 첫 paint의 안전망이 맞춘다.
        let area = crate::geom::popup_host(host, self.surface.get());
        let p = crate::geom::place_popup(Point { x, y }, (w, h), area);
        self.at.set(Some(p));
        self.rect.set(Rect::new(p.x, p.y, w, h));
    }

    /// **대상을 가리지 않게** 연다(트리·목록의 행 우클릭 — nexa-sql 사용자 09-21): 메뉴를 `avoid`(대상 행) 바로 아래에,
    /// 자리가 없으면 바로 위에 둔다 → 누른 자리에 가깝고 대상 이름이 온전히 보인다([`crate::geom::place_popup_beside`]).
    pub fn open_beside(
        &mut self,
        x: i32,
        y: i32,
        avoid: Rect,
        items: Vec<CtxItem>,
        host: Rect,
        text_w: i32,
    ) {
        self.open_at(x, y, items, host, text_w);
        if !self.is_open() {
            return;
        }
        self.avoid = Some((Point { x, y }, avoid));
        let (w, h) = self.size_px();
        let area = crate::geom::popup_host(host, self.surface.get());
        let p = crate::geom::place_popup_beside(Point { x, y }, avoid, (w, h), area);
        self.at.set(Some(p));
        self.rect.set(Rect::new(p.x, p.y, w, h));
    }

    fn row_h(&self) -> i32 {
        // 글꼴 높이를 모르는 자리라 논리 px 기준으로 잡는다(paint에서 실제 글자를 세로 중앙에 둔다).
        self.s(16 + ROW_EXTRA)
    }

    fn has_icons(&self) -> bool {
        let any_mark = self
            .items
            .iter()
            .any(|it| matches!(it, CtxItem::Item { mark: Some(_), .. }));
        any_mark
            || (menu_icons_enabled()
                && self.items.iter().any(|it| {
                    matches!(
                        it,
                        CtxItem::Item { icon: Some(_), .. }
                            | CtxItem::Item {
                                checked: Some(_),
                                ..
                            }
                    )
                }))
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

    /// 보이는 항목 범위(`first..first+vis_count`).
    fn vis_range(&self) -> std::ops::Range<usize> {
        let n = self.vis_count();
        let first = self.first.min(self.items.len().saturating_sub(n));
        first..first + n
    }

    fn size_px(&self) -> (i32, i32) {
        let mut h = self.s(PAD_V) * 2;
        for it in &self.items[self.vis_range()] {
            h += match it {
                CtxItem::Item { .. } => self.row_h(),
                CtxItem::Separator => self.s(SEP_H),
            };
        }
        if let Some(n) = self.min_rows {
            h = h.max(self.s(PAD_V) * 2 + self.row_h() * n as i32);
        }
        let sc = self.sc_w.get();
        let extra = if sc > 0 { self.s(SC_GAP) + sc } else { 0 }
            + if self.has_arrows() {
                self.s(ARROW_W)
            } else {
                0
            };
        let mut w =
            (self.icon_col() + self.fit_w.get() + extra + self.s(PAD_H) * 2).max(self.s(MIN_W));
        if let Some(m) = self.max_w {
            w = w.min(self.s(m).max(self.s(MIN_W)));
        }
        (w, h)
    }

    /// 인덱스 → 그 행의 rect.
    fn row_rect(&self, idx: usize) -> Option<Rect> {
        let at = self.at.get()?;
        let mut y = at.y + self.s(PAD_V);
        let range = self.vis_range();
        if !range.contains(&idx) {
            return None; // 스크롤로 가려진 행
        }
        for (i, it) in self
            .items
            .iter()
            .enumerate()
            .take(range.end)
            .skip(range.start)
        {
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

    /// 어느 행이든(비활성·구분선 포함) 커서 아래 행 — 하위 메뉴 닫기 판정용(nexa-sql 사용자 09-22 "비활성 항목에선 상세 메뉴가 안 닫힘").
    fn row_at(&self, p: Point) -> Option<usize> {
        (0..self.items.len()).find(|&i| self.row_rect(i).is_some_and(|r| r.contains(p)))
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
        self.ensure_visible(sel[next]);
    }

    /// PgUp/PgDn(한 화면) · Home/End(처음/끝) — 활성 항목 기준 · 보이게 스크롤.
    fn move_hover_by(&mut self, step: i32, edge: bool) {
        let sel: Vec<usize> = (0..self.items.len())
            .filter(|&i| matches!(self.items[i], CtxItem::Item { enabled: true, .. }))
            .collect();
        if sel.is_empty() {
            return;
        }
        let cur = self
            .hover
            .and_then(|h| sel.iter().position(|&i| i == h))
            .map_or(if step < 0 { 0 } else { -1 }, |p| p as i32);
        let next = if edge {
            if step < 0 {
                0
            } else {
                sel.len() as i32 - 1
            }
        } else {
            (cur + step).clamp(0, sel.len() as i32 - 1)
        } as usize;
        self.hover = Some(sel[next]);
        self.ensure_visible(sel[next]);
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
        // 부모가 본 표면 크기를 물려준다 — 끝없는 `host`여도 하위 메뉴가 화면 안에서 접힌다.
        c.surface.set(self.surface.get());
        let area = crate::geom::popup_host(self.host, self.surface.get());
        c.open_at(x, y, children.clone(), self.host, self.s(approx));
        // 오른쪽에 자리가 없으면(open_at이 왼쪽으로 접었으면) 부모 왼쪽에 붙인다.
        let cw = c.rect.get().w;
        if x + cw > area.right() {
            let nx = (row.x - cw + self.s(4)).max(area.x);
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
        // 커서 직전 위치(하위 메뉴 쪽으로 가는 중인지 판정) — 자식 유무와 무관하게 기록.
        let last = if let InputEvent::MouseMove { x, y } = *ev {
            self.last_pos.replace(Point { x, y })
        } else {
            self.last_pos
        };
        // ── 하위 메뉴가 열려 있으면: 그 안의 사건은 자식이 · 부모 행 위 이동은 부모가(다른 항목 = 자식 교체).
        let child_open = self.child.as_ref().is_some_and(|c| c.is_open());
        if child_open {
            let child_rect = self
                .child
                .as_ref()
                .map_or(Rect::default(), |c| c.rect.get());
            // ★ 자식 영역 = 자식 + 그 아래 열린 손자까지(`bounds`) — `rect`만 보면 손자 항목 클릭이 "바깥"으로 오인돼
            //   메뉴가 통째로 닫히고 클릭이 아래 컨트롤로 흘렀다(nexa-sql 09-16: Advanced Copy ▸ Copy SQL ▸ MERGE 무반응).
            let child_area = self.child.as_ref().map_or(Rect::default(), |c| c.bounds());
            match *ev {
                InputEvent::MouseMove { x, y } => {
                    let p = Point { x, y };
                    if child_area.contains(p) {
                        self.pending_since = None;
                        return self.forward_child(ev);
                    }
                    // 비활성 행·구분선 위로 옮겨도 하위 메뉴는 닫힌다(포인터가 부모의 다른 행에 있으면 하위는 의미가 없다 · Windows 관례).
                    if let Some(i) = self.row_at(p) {
                        let enabled = matches!(self.items[i], CtxItem::Item { enabled: true, .. });
                        if Some(i) != self.child_of {
                            // ★ 자식 쪽으로 움직이는 중(가로로 자식에 가까워지고 · 부모 행~자식 사이 세로 띠 안)이면
                            //   유예 동안 자식을 유지 — 대각선 이동 중 다른 부모 행을 스쳐도 잃지 않는다.
                            let parent_row = self.child_of.and_then(|c| self.row_rect(c));
                            let toward = match (last, parent_row) {
                                (Some(l), Some(pr)) => {
                                    let child_right = child_rect.x >= pr.right() - self.s(8);
                                    let closer = if child_right { p.x > l.x } else { p.x < l.x };
                                    let top = pr.y.min(child_rect.y);
                                    let bottom = pr.bottom().max(child_rect.bottom());
                                    closer && p.y >= top && p.y <= bottom
                                }
                                _ => false,
                            };
                            let since = *self
                                .pending_since
                                .get_or_insert_with(std::time::Instant::now);
                            if toward && since.elapsed().as_millis() < SUBMENU_GRACE_MS {
                                return true;
                            }
                            self.pending_since = None;
                            self.close_child();
                            self.hover = enabled.then_some(i);
                            if enabled && self.items[i].has_children() {
                                self.open_child(i, false);
                            }
                        } else {
                            self.pending_since = None;
                        }
                        return true;
                    }
                    // 부모·자식 어디도 아님 — 자식 hover만 지운다.
                    return self.forward_child(ev);
                }
                InputEvent::MouseDown { x, y, .. } | InputEvent::RightDown { x, y } => {
                    let p = Point { x, y };
                    if child_area.contains(p) {
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
                    // ★ 클릭 = 선택 모드: 첫 클릭은 강조만(호스트가 카드를 그린다) · 같은 행 400 ms 안 재클릭 = 확정.
                    if self.click_selects {
                        let now = std::time::Instant::now();
                        let double = self.last_click.is_some_and(|(j, t)| {
                            j == i && now.duration_since(t).as_millis() < 400
                        });
                        self.last_click = Some((i, now));
                        self.hover = Some(i);
                        if !double {
                            return true;
                        }
                    }
                    if let CtxItem::Item { id, .. } = &self.items[i] {
                        self.picked = Some(id.clone());
                    }
                    self.close();
                } else if self.rect.get().contains(p) {
                    // 팝업 안 여백: 스크롤 트랙이면 한 쪽씩(세로 = 오른쪽 띠 · 가로 = 아래 띠 · 09-24) · 그 밖은 무시.
                    let r = self.rect.get();
                    if self.scrollable() && p.x >= r.right() - self.s(10) {
                        let rows = self.vis_count().max(1) as i32;
                        let mid = r.y + r.h / 2;
                        self.scroll_rows(if p.y < mid { -rows } else { rows });
                    } else if self.label_over.get() > 0 && p.y >= r.bottom() - self.s(10) {
                        let step = (r.w / 2).max(self.s(24));
                        let cur = self.hscroll.get();
                        let next = if p.x < r.x + r.w / 2 {
                            cur - step
                        } else {
                            cur + step
                        };
                        self.hscroll.set(next.clamp(0, self.label_over.get()));
                    }
                } else {
                    self.close();
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
            // 휠 = 스크롤(행 수 상한이 있을 때 · 120 = 한 칸 = 한 행 · 음수 = 아래) · 커서 아래 행을 다시 hover.
            InputEvent::Wheel { delta } => {
                // ★ 휠은 커서가 팝업 **안**에 있을 때만(마우스 라우팅 규칙 · 사용자 09-24 "영역 밖 스크롤이 안으로 전달") — 위치를
                //   알면(MouseMove를 받은 뒤) 밖이면 소비하지 않고 호스트로 흘린다 · 모르면 종전대로 팝업이.
                if !self.wheel_inside() {
                    return false;
                }
                // ★ 0 = 무시(트랙패드 제스처 끝의 빈 사건이 "위로 1행"이 되던 결함 · 09-24) · 노치(|delta| ≥ 120) = 노치당 1행 ·
                //   트랙패드(작은 px delta)는 누적해 40px마다 1행(사건마다 1행이면 너무 빠르다).
                if delta != 0 && self.scrollable() {
                    let rows = if delta.abs() >= 120 {
                        self.wheel_acc.set(0);
                        -(delta / 120)
                    } else {
                        let acc = self.wheel_acc.get() + delta;
                        let r = acc / 40;
                        self.wheel_acc.set(acc - r * 40);
                        -r
                    };
                    if rows != 0 {
                        self.scroll_rows(rows);
                        if let Some(p) = self.last_pos {
                            self.hover = self.hit(p);
                        }
                    }
                }
                true
            }
            // 가로 휠(Shift+휠 · 트랙패드 틸트) = 넘친 라벨을 가로로(폭 상한이 있을 때만 · 09-23).
            InputEvent::HWheel { delta } => {
                if !self.wheel_inside() {
                    return false;
                }
                let over = self.label_over.get();
                if over > 0 {
                    let step = self.s(24) * ((delta.abs() / 120).max(1));
                    let cur = self.hscroll.get();
                    let next = if delta < 0 { cur + step } else { cur - step };
                    self.hscroll.set(next.clamp(0, over));
                }
                true
            }
            InputEvent::MouseUp { .. } => true,
            // 키보드 — ↑/↓ 이동 · PgUp/PgDn/Home/End(스크롤 목록) · → 하위 열기 · Enter 선택(하위 있으면 열기) · 그 외(Esc 포함)는 메뉴만 닫는다.
            InputEvent::Key { key, .. } => {
                match key {
                    Key::Down => self.move_hover(true),
                    Key::Up => self.move_hover(false),
                    Key::PageDown => self.move_hover_by(self.vis_count().max(1) as i32, false),
                    Key::PageUp => self.move_hover_by(-(self.vis_count().max(1) as i32), false),
                    Key::Home => self.move_hover_by(-1, true),
                    Key::End => self.move_hover_by(1, true),
                    // ←/→ = 넘친 라벨 가로 스크롤(하위 메뉴가 없는 목록 · 09-24) · 하위 메뉴가 있으면 → 는 펼침.
                    Key::Right => {
                        if let Some(i) = self.hover {
                            if self.items[i].has_children() {
                                self.open_child(i, true);
                                return true;
                            }
                        }
                        if self.label_over.get() > 0 {
                            let n = (self.hscroll.get() + self.s(24)).min(self.label_over.get());
                            self.hscroll.set(n);
                        }
                    }
                    Key::Left => {
                        if self.label_over.get() > 0 {
                            self.hscroll.set((self.hscroll.get() - self.s(24)).max(0));
                        } else if !self.click_selects {
                            self.close();
                        }
                    }
                    Key::Enter => {
                        if let Some(i) = self.hover {
                            if self.items[i].has_children() {
                                self.open_child(i, true);
                                return true;
                            }
                        }
                        // hover가 없으면 **기본 항목**(사용자 09-22).
                        if let Some(CtxItem::Item {
                            id, enabled: true, ..
                        }) = self.hover.or(self.default_idx).map(|i| &self.items[i])
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
                label,
                shortcut,
                sub,
                ..
            } = it
            {
                let lw = ctx.text_width(label) + sub.as_deref().map_or(0, |s| ctx.text_width(s));
                real = real.max(lw);
                if let Some(sc) = shortcut {
                    sc_real = sc_real.max(ctx.text_width(sc));
                }
            }
        }
        if real > self.fit_w.get() || sc_real > self.sc_w.get() {
            self.fit_w.set(real.max(self.fit_w.get()));
            self.sc_w.set(sc_real.max(self.sc_w.get()));
            let (w, h) = self.size_px();
            self.rect.set(Rect::new(at.x, at.y, w, h));
        }
        // ★ 안전망(nexa-sql 사용자 09-21 "우클릭 메뉴가 잘린다"): 실측 폭 보정 뒤에도 · 호출자가 끝없는 `host`를 넘겼어도
        //   **그리기 표면 안**으로 옮긴다(히트 판정 rect·at 동기 갱신 · 크기는 그대로).
        self.surface.set(ctx.surface_size());
        let area = crate::geom::popup_host(self.host, self.surface.get());
        let fitted = match self.avoid {
            // 대상을 가리지 않게 연 메뉴 = 같은 규칙으로 다시(실측 폭·표면을 이제 안다).
            Some((anchor, avoid)) => {
                let r = self.rect.get();
                let p = crate::geom::place_popup_beside(anchor, avoid, (r.w, r.h), area);
                crate::geom::nudge_into(Rect::new(p.x, p.y, r.w, r.h), area)
            }
            None => crate::geom::nudge_into(self.rect.get(), area),
        };
        if fitted != self.rect.get() {
            self.rect.set(fitted);
            at = Point {
                x: fitted.x,
                y: fitted.y,
            };
            self.at.set(Some(at));
        }
        let r = self.rect.get();
        // 바탕 + 테두리(그림자 대신 테두리로 층을 만든다 — 렌더러에 블러가 없다).
        ctx.fill_round_rect(r, self.s(RADIUS), theme.panel_bg);
        ctx.stroke_round_rect(r, self.s(RADIUS), theme.border, 1.0);
        ctx.select_font(FontSlot::Base, false);
        let icon_col = self.icon_col();
        let arrows = self.has_arrows();
        let mut y = at.y + self.s(PAD_V);
        let range = self.vis_range();
        // ★ 라벨 열의 가용 폭과 넘침(폭 상한 때문에 실측보다 좁아진 만큼) — 가로 스크롤 범위.
        let label_x0 = r.x + self.s(PAD_H) + icon_col;
        let label_right = r.right()
            - self.s(PAD_H)
            - if arrows { self.s(ARROW_W) } else { 0 }
            - if self.sc_w.get() > 0 {
                self.sc_w.get() + self.s(PAD_H)
            } else {
                0
            }
            - if self.scrollable() { self.s(6) } else { 0 };
        let label_avail = (label_right - label_x0).max(self.s(24));
        let over = (real - label_avail).max(0);
        self.label_over.set(over);
        if self.hscroll.get() > over {
            self.hscroll.set(over);
        }
        let hscroll = self.hscroll.get();
        // 스크롤 표시(행 수 상한을 넘을 때) — 오른쪽 안쪽에 가는 트랙 + 썸(비율).
        if self.scrollable() {
            let tw = self.s(3);
            let track = Rect::new(
                r.right() - self.s(3) - tw,
                r.y + self.s(PAD_V),
                tw,
                r.h - self.s(PAD_V) * 2,
            );
            ctx.fill_rect_alpha(track, theme.text_dim, 0.15);
            let n = self.items.len().max(1) as f32;
            let th_h = ((range.len() as f32 / n) * track.h as f32).max(self.s(8) as f32) as i32;
            let off = ((range.start as f32 / n) * track.h as f32) as i32;
            let th_y = track.y + off.min(track.h - th_h).max(0);
            ctx.fill_round_rect(Rect::new(track.x, th_y, tw, th_h), tw / 2, theme.text_dim);
        }
        for (i, it) in self
            .items
            .iter()
            .enumerate()
            .take(range.end)
            .skip(range.start)
        {
            match it {
                CtxItem::Item {
                    label,
                    enabled,
                    icon,
                    shortcut,
                    sub,
                    emph,
                    marks,
                    children,
                    checked,
                    active,
                    mark,
                    ..
                } => {
                    let h = self.row_h();
                    let row = Rect::new(r.x, y, r.w, h);
                    let hot = *enabled && (self.hover == Some(i) || self.child_of == Some(i));
                    let is_default = self.default_idx == Some(i);
                    if hot {
                        ctx.fill_rect(
                            Rect::new(r.x + self.s(2), y, r.w - self.s(4), h),
                            theme.accent,
                        );
                    } else if is_default {
                        // 기본 항목 = 채움이 아니라 accent 테두리(hover·키보드 이동과 구분).
                        ctx.stroke_round_rect(
                            Rect::new(r.x + self.s(2), y, r.w - self.s(4), h),
                            self.s(3),
                            theme.accent,
                            1.0,
                        );
                    }
                    let fg = if !*enabled {
                        theme.text_dim
                    } else if hot {
                        theme.window_bg
                    } else if *active || is_default {
                        theme.accent
                    } else {
                        theme.text
                    };
                    let mut x = r.x + self.s(PAD_H);
                    // 아이콘(상태색 틴트 · 세로 중앙) — 없는 행도 칸은 비워 둔다(글자 세로 정렬).
                    // 토글 항목은 켜짐/꺼짐 도형이 아이콘 칸에(항목 아이콘이 따로 있으면 그것 우선).
                    let icon = icon.as_ref().filter(|_| menu_icons_enabled());
                    // 토글 항목(`checked`) = **일반 체크박스**(nexa-sql 로그 창 사용자 09-19 "스위치는 보기 불편") — 종전의
                    // 스위치 글리프(ToggleOn/Off)는 켜짐이 손잡이 위치로만 보여 알아보기 어려웠다. 아이콘이 있으면 아이콘이 우선.
                    if icon.is_none() {
                        if let Some(on) = checked {
                            let sz = self.s(ICON_PX);
                            let bx = sz * 3 / 4;
                            let area = Rect::new(x + (sz - bx) / 2, y + (h - bx) / 2, bx, bx);
                            super::draw_checkbox_glyph(ctx, theme, area, *on && *enabled, true);
                        }
                    }
                    if *mark == Some(true) {
                        // ✓ 체크 표시 — 로그인 목록의 체크 상자와 같은 글리프(부품 재사용).
                        let sz = self.s(ICON_PX);
                        let area = Rect::new(
                            x + sz / 6,
                            y + (h - sz) / 2 + sz / 6,
                            sz * 2 / 3,
                            sz * 2 / 3,
                        );
                        super::draw_check_mark(ctx, area, fg);
                    } else if let Some(ic) = icon {
                        let sz = self.s(ICON_PX);
                        let img = match &ic.rgba {
                            Some(rgba) if *enabled => {
                                IconImage::from_rgba(ic.w, ic.h, rgba.to_vec())
                            }
                            _ => {
                                let (cr, cg, cb) = fg.rgb();
                                IconImage::from_alpha_tinted(ic.w, ic.h, &ic.alpha, (cr, cg, cb))
                            }
                        };
                        ctx.image_scaled(Rect::new(x, y + (h - sz) / 2, sz, sz), &img, row);
                    }
                    x += icon_col;
                    // 세로 정확히 가운데 — 글자 높이를 재서 놓는다(눈대중 상수 금지 · 08-09).
                    let ty = ctx.text_center_y(y, h);
                    let right =
                        r.right() - self.s(PAD_H) - if arrows { self.s(ARROW_W) } else { 0 };
                    // 긴 라벨(경로)은 가운데 … — 단축키 자리를 남기고(Alt = 전체 · 사용자 09-22).
                    //   ★ 가로 스크롤 중(`hscroll` > 0)이면 자르지 않고 밀어서 라벨 열 안에 클립(09-23 "긴 이름").
                    let sc_room = shortcut
                        .as_deref()
                        .map_or(0, |sc| ctx.text_width(sc) + self.s(PAD_H));
                    let avail = right - sc_room - x;
                    // 보조 글(` : 타입`)은 라벨 뒤에 흐리게 — 라벨(이름)이 도드라진다(09-24).
                    let subfg = if hot { fg } else { theme.text_dim };
                    // 라벨 = 전체 일치면 굵게+파랑 · 부분 일치 구간은 강조색(hover 행은 본문색 그대로 · 09-24).
                    let blue = Color::from_rgb(0, 0, 255);
                    let draw_label = |ctx: &mut dyn DrawCtx, lx: i32, clip: Rect| -> i32 {
                        if *emph {
                            ctx.select_font(FontSlot::Base, true);
                            let c = if hot { fg } else { blue };
                            ctx.text(lx, ty, clip, label, c);
                            let w = ctx.text_width(label);
                            ctx.select_font(FontSlot::Base, false);
                            return w;
                        }
                        if marks.is_empty() || hot {
                            ctx.text(lx, ty, clip, label, fg);
                            return ctx.text_width(label);
                        }
                        let mut cx = lx;
                        let mut pos = 0usize;
                        for r in marks.iter() {
                            let (s, e) = (r.start.min(label.len()), r.end.min(label.len()));
                            if s > pos {
                                let seg = &label[pos..s];
                                ctx.text(cx, ty, clip, seg, fg);
                                cx += ctx.text_width(seg);
                            }
                            if e > s {
                                let seg = &label[s..e];
                                ctx.text(cx, ty, clip, seg, theme.accent);
                                cx += ctx.text_width(seg);
                            }
                            pos = pos.max(e);
                        }
                        if pos < label.len() {
                            let seg = &label[pos..];
                            ctx.text(cx, ty, clip, seg, fg);
                            cx += ctx.text_width(seg);
                        }
                        cx - lx
                    };
                    if hscroll > 0 {
                        let clip = Rect::new(x, y, avail.max(0), h);
                        let lw = draw_label(ctx, x - hscroll, clip);
                        if let Some(sb) = sub {
                            ctx.text(x - hscroll + lw, ty, clip, sb, subfg);
                        }
                    } else {
                        let lw = ctx.text_width(label);
                        match sub {
                            Some(sb) if lw < avail => {
                                let lw = draw_label(ctx, x, row);
                                let shown = crate::draw::ellipsize_middle(ctx, sb, avail - lw);
                                ctx.text(x + lw, ty, row, &shown, subfg);
                            }
                            _ if lw <= avail => {
                                draw_label(ctx, x, row);
                            }
                            _ => {
                                let shown = crate::draw::ellipsize_middle(ctx, label, avail);
                                ctx.text(x, ty, row, &shown, fg);
                            }
                        }
                    }
                    // 단축키 — 오른쪽 정렬 · 흐리게(hover면 본문색).
                    if let Some(sc) = shortcut {
                        let w = ctx.text_width(sc);
                        let scfg = if hot { fg } else { theme.text_dim };
                        ctx.text(right - w, ty, row, sc, scfg);
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
        // 가로 스크롤 표시(라벨이 넘칠 때) — 아래 안쪽에 가는 트랙 + 썸(비율 · 세로 표시와 같은 모양).
        if over > 0 {
            let th = self.s(3);
            let track = Rect::new(label_x0, r.bottom() - self.s(3) - th, label_avail, th);
            ctx.fill_rect_alpha(track, theme.text_dim, 0.15);
            let total = (label_avail + over).max(1) as f32;
            let th_w = ((label_avail as f32 / total) * track.w as f32).max(self.s(8) as f32) as i32;
            let off = ((hscroll as f32 / total) * track.w as f32) as i32;
            let th_x = track.x + off.min(track.w - th_w).max(0);
            ctx.fill_round_rect(Rect::new(th_x, track.y, th_w, th), th / 2, theme.text_dim);
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

    /// 바깥 클릭 판정: 안 = false · 밖(좌/우) = true · 이동/닫힘 = false — 호스트가 바깥 클릭을 아래로 흘리는 근거.
    #[test]
    fn outside_click_is_reported_for_host_passthrough() {
        let mut m = ContextMenu::new();
        m.open_at(10, 10, items(), host(), 60);
        let r = m.bounds();
        assert!(!m.is_outside_click(&down(r.x + 2, r.y + 2)));
        assert!(m.is_outside_click(&down(390, 290)));
        assert!(m.is_outside_click(&InputEvent::RightDown { x: 390, y: 290 }));
        assert!(!m.is_outside_click(&InputEvent::MouseMove { x: 390, y: 290 }));
        m.close();
        assert!(!m.is_outside_click(&down(390, 290)));
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

    /// 기본 항목: hover 없이 Enter = 기본 항목 · 키보드로 옮기면 hover가 우선 · 비활성은 기본이 될 수 없다.
    /// 최소 행 수(09-24): 항목 1개도 10행 높이 · 상한(max_rows)과 함께 · 없으면 종전(내용만큼).
    #[test]
    fn min_rows_keeps_height_with_few_items() {
        let mut m = ContextMenu::new();
        let one = vec![CtxItem::item("a", "only")];
        m.open_at(10, 10, one.clone(), Rect::new(0, 0, 800, 600), 80);
        let h1 = m.rect.get().h;
        m.set_min_rows(Some(10));
        m.open_at(10, 10, one, Rect::new(0, 0, 800, 600), 80);
        let h10 = m.rect.get().h;
        assert!(h10 > h1 * 5, "{h1} → {h10}");
        assert!(
            m.row_rect_of(0).is_some()
                && m.hit(Point {
                    x: 20,
                    y: m.rect.get().bottom() - 5
                })
                .is_none(),
            "빈 자리는 항목 아님"
        );
        m.set_min_rows(None);
    }

    /// 휠(09-24): delta 0은 아무것도 안 함(끝에서 한 칸 위로 튀던 결함) · 트랙패드 작은 delta는 40px 누적마다 1행 · 노치 120 = 1행.
    #[test]
    fn wheel_zero_is_noop_and_small_deltas_accumulate() {
        let mut m = ContextMenu::new();
        m.set_max_rows(Some(3));
        let items: Vec<CtxItem> = (0..8)
            .map(|i| CtxItem::item(format!("h{i}"), format!("item {i}")))
            .collect();
        m.open_at(10, 10, items, Rect::new(0, 0, 800, 600), 80);
        for _ in 0..20 {
            m.on_event(&InputEvent::Wheel { delta: -120 });
        }
        assert_eq!(m.vis_range(), 5..8, "끝");
        m.on_event(&InputEvent::Wheel { delta: 0 });
        assert_eq!(m.vis_range(), 5..8, "0 = 그대로");
        m.on_event(&InputEvent::Wheel { delta: 120 });
        assert_eq!(m.vis_range(), 4..7);
        for _ in 0..13 {
            m.on_event(&InputEvent::Wheel { delta: -3 });
        }
        assert_eq!(m.vis_range(), 4..7, "39px = 아직");
        m.on_event(&InputEvent::Wheel { delta: -3 });
        assert_eq!(m.vis_range(), 5..8, "42px = 1행");
        // ★ 커서가 팝업 밖(MouseMove로 알게 된 뒤)이면 휠·가로 휠을 소비하지 않고 목록도 그대로(사용자 09-24).
        m.on_event(&InputEvent::MouseMove { x: 700, y: 500 });
        assert!(!m.on_event(&InputEvent::Wheel { delta: 120 }));
        assert!(!m.on_event(&InputEvent::HWheel { delta: 120 }));
        assert_eq!(m.vis_range(), 5..8, "밖 = 그대로");
        let r = m.bounds();
        m.on_event(&InputEvent::MouseMove {
            x: r.x + 5,
            y: r.y + 5,
        });
        assert!(m.on_event(&InputEvent::Wheel { delta: 120 }));
        assert_eq!(m.vis_range(), 4..7, "안 = 스크롤");
    }

    /// 클릭 = 선택 모드(완성 팝업 · 09-24): 단일 클릭 = 강조만 · 같은 행 재클릭 = 확정 · ←/→ = 넘침이 있을 때만 가로 스크롤.
    #[test]
    fn click_selects_then_double_click_or_enter_picks() {
        let mut m = ContextMenu::new();
        m.set_click_selects(true);
        let items: Vec<CtxItem> = (0..3)
            .map(|i| CtxItem::item(format!("h{i}"), format!("item {i}")))
            .collect();
        m.open_at(10, 10, items, Rect::new(0, 0, 800, 600), 120);
        let r1 = m.row_rect_of(1).expect("row 1");
        let (x, y) = (r1.x + 5, r1.y + r1.h / 2);
        m.on_event(&InputEvent::MouseDown {
            x,
            y,
            shift: false,
            primary: false,
        });
        assert!(
            m.is_open() && m.hovered() == Some(1) && m.take_picked().is_none(),
            "첫 클릭 = 강조만"
        );
        m.on_event(&InputEvent::MouseDown {
            x,
            y,
            shift: false,
            primary: false,
        });
        assert_eq!(m.take_picked().as_deref(), Some("h1"), "재클릭 = 확정");
        assert!(!m.is_open());
        let items: Vec<CtxItem> = (0..3)
            .map(|i| CtxItem::item(format!("h{i}"), format!("item {i}")))
            .collect();
        m.open_at(10, 10, items, Rect::new(0, 0, 800, 600), 120);
        m.on_event(&InputEvent::MouseDown {
            x,
            y,
            shift: false,
            primary: false,
        });
        m.on_event(&key(Key::Enter));
        assert_eq!(
            m.take_picked().as_deref(),
            Some("h1"),
            "클릭 뒤 Enter = 확정"
        );
        let items: Vec<CtxItem> = vec![CtxItem::item("a", "LONG_NAME_0123456789_ABCDEFGHIJ")];
        m.set_max_width(Some(120));
        m.open_at(10, 10, items, Rect::new(0, 0, 800, 600), 400);
        m.label_over.set(60);
        m.on_event(&key(Key::Right));
        assert!(
            m.hscroll.get() > 0 && m.is_open(),
            "→ = 가로 스크롤(닫지 않음)"
        );
        m.on_event(&key(Key::Left));
        assert_eq!(m.hscroll.get(), 0);
        m.on_event(&key(Key::Left));
        assert!(m.is_open(), "선택 모드에서 ←는 닫지 않는다");
    }

    /// 폭 상한 + 가로 휠(nexa-sql 완성 팝업 사용자 09-23 "긴 이름"): 상한이 있으면 rect 폭이 잘리고 · 넘침은 paint가 재므로
    /// 그 전에는 휠이 무시되고 · 넘침을 알려 주면 Shift+휠로 0..over 사이를 오간다.
    #[test]
    fn max_width_caps_rect_and_hwheel_scrolls_labels() {
        let mut m = ContextMenu::new();
        let items = vec![
            CtxItem::item("a", "M4S_I002040_VERY_LONG_TABLE_NAME_FOR_TEST_0123456789"),
            CtxItem::item("b", "short"),
        ];
        m.open_at(10, 10, items.clone(), Rect::new(0, 0, 800, 600), 600);
        let wide = m.rect.get().w;
        m.set_max_width(Some(200));
        m.open_at(10, 10, items, Rect::new(0, 0, 800, 600), 600);
        let capped = m.rect.get().w;
        assert!(capped < wide && capped <= 200, "{capped} < {wide}");
        assert_eq!(m.hscroll.get(), 0);
        // 넘침을 아직 모르면(paint 전) 가로 휠은 무시.
        m.on_event(&InputEvent::HWheel { delta: -120 });
        assert_eq!(m.hscroll.get(), 0);
        m.label_over.set(100);
        m.on_event(&InputEvent::HWheel { delta: -120 });
        assert!(m.hscroll.get() > 0 && m.hscroll.get() <= 100);
        m.on_event(&InputEvent::HWheel { delta: -120 * 10 });
        assert_eq!(m.hscroll.get(), 100, "상한에서 멈춘다");
        m.on_event(&InputEvent::HWheel { delta: 120 * 10 });
        assert_eq!(m.hscroll.get(), 0);
        assert!(m.is_open());
    }

    #[test]
    fn scrolls_with_max_rows_keys_and_wheel() {
        // 행 수 상한 3 · 항목 8: 높이는 3행 · ↓로 넘어가면 first가 따라오고 · PgDn/End/Home · 휠 · 가려진 행은 rect 없음.
        let mut m = ContextMenu::new();
        m.set_max_rows(Some(3));
        let items: Vec<CtxItem> = (0..8)
            .map(|i| CtxItem::item(format!("h{i}"), format!("item {i}")))
            .collect();
        m.open_at(10, 10, items, Rect::new(0, 0, 800, 600), 80);
        let h3 = m.rect.get().h;
        m.set_max_rows(None);
        let items: Vec<CtxItem> = (0..8)
            .map(|i| CtxItem::item(format!("h{i}"), format!("item {i}")))
            .collect();
        m.open_at(10, 10, items, Rect::new(0, 0, 800, 600), 80);
        let h8 = m.rect.get().h;
        assert!(h3 < h8, "상한 3 = 3행 높이 {h3} < 전부 {h8}");
        m.set_max_rows(Some(3));
        let items: Vec<CtxItem> = (0..8)
            .map(|i| CtxItem::item(format!("h{i}"), format!("item {i}")))
            .collect();
        m.open_at(10, 10, items, Rect::new(0, 0, 800, 600), 80);
        assert!(m.row_rect_of(3).is_none(), "4번째는 가려짐");
        for _ in 0..4 {
            m.on_event(&key(Key::Down));
        }
        assert_eq!(m.hovered(), Some(3));
        assert_eq!(m.vis_range(), 1..4, "hover가 보이게 first가 1로");
        m.on_event(&key(Key::PageDown));
        assert_eq!(m.hovered(), Some(6));
        m.on_event(&key(Key::End));
        assert_eq!(m.hovered(), Some(7));
        assert_eq!(m.vis_range(), 5..8);
        m.on_event(&key(Key::Home));
        assert_eq!((m.hovered(), m.vis_range()), (Some(0), 0..3));
        // 휠 아래(음수) 2칸 → first 2 · 위로 한 칸 → 1.
        m.on_event(&InputEvent::Wheel { delta: -240 });
        assert_eq!(m.vis_range(), 2..5);
        m.on_event(&InputEvent::Wheel { delta: 120 });
        assert_eq!(m.vis_range(), 1..4);
        // 보이는 행 클릭 = 선택.
        let r = m.row_rect_of(2).expect("보이는 행");
        m.on_event(&InputEvent::MouseDown {
            x: r.x + 5,
            y: r.y + 5,
            shift: false,
            primary: false,
        });
        assert_eq!(m.take_picked().as_deref(), Some("h2"));
    }

    #[test]
    fn default_item_is_picked_by_enter_without_hover() {
        let mut m = ContextMenu::new();
        m.open_at(10, 10, items(), host(), 60);
        m.set_default(1); // cut = 비활성 → 무시
        assert!(m.default_idx.is_none());
        m.set_default(2); // 구분선 → 무시
        assert!(m.default_idx.is_none());
        m.set_default(3);
        m.on_event(&key(Key::Enter));
        assert_eq!(m.take_picked().as_deref(), Some("paste"));
        m.open_at(10, 10, items(), host(), 60);
        assert!(m.default_idx.is_none(), "다시 열면 비워진다");
    }

    /// 하위 메뉴가 열린 채 부모의 **비활성** 행으로 옮기면 하위가 닫힌다(활성 행과 같은 규칙).
    #[test]
    fn moving_onto_disabled_row_closes_submenu() {
        let mut m = ContextMenu::new();
        let items = vec![
            CtxItem::submenu("go", "이동", vec![CtxItem::item("a", "A")]),
            CtxItem::maybe("cut", "잘라내기", false),
            CtxItem::item("paste", "붙여넣기"),
        ];
        m.open_at(10, 10, items, host(), 60);
        m.set_scale(1.0);
        let r0 = m.row_rect(0).expect("row");
        m.on_event(&InputEvent::MouseMove {
            x: r0.x + 4,
            y: r0.y + 2,
        });
        assert!(m.child_for_test().is_some(), "하위 메뉴 열림");
        // 유예 시간을 지나 비활성 행으로.
        m.pending_since = Some(
            std::time::Instant::now()
                - std::time::Duration::from_millis(SUBMENU_GRACE_MS as u64 + 50),
        );
        let r1 = m.row_rect(1).expect("row");
        m.on_event(&InputEvent::MouseMove {
            x: r1.x + 4,
            y: r1.y + 2,
        });
        assert!(m.child_for_test().is_none(), "비활성 행 = 하위 닫힘");
        assert!(m.hover.is_none(), "비활성 행은 hover 없음");
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

    /// 자식 쪽으로 가는 대각선 이동(다른 부모 행을 스침)은 자식을 유지 · 자식에서 멀어지는 이동은 닫는다(09-16).
    #[test]
    fn grandchild_pick_reaches_root_and_closes_all() {
        // nexa-sql 09-16: Advanced Copy ▸ Copy SQL ▸ MERGE — 손자 항목 클릭이 최상위 pick으로 와야 한다.
        let sql = vec![CtxItem::item("sql_merge", "MERGE")];
        let adv = vec![CtxItem::submenu("copy_sql", "Copy SQL", sql)];
        let items = vec![
            CtxItem::submenu("adv", "Advanced Copy", adv),
            CtxItem::item("all", "Select All"),
        ];
        let mut m = ContextMenu::new();
        m.open_at(10, 10, items, host(), 100);
        let r0 = m.row_rect_of(0).unwrap();
        m.on_event(&InputEvent::MouseMove {
            x: r0.x + 5,
            y: r0.y + 2,
        });
        let c_row = m
            .child_for_test()
            .expect("자식 열림")
            .row_rect_of(0)
            .unwrap();
        m.on_event(&InputEvent::MouseMove {
            x: c_row.x + 5,
            y: c_row.y + 2,
        });
        let g_row = m
            .child_for_test()
            .and_then(|c| c.child_for_test())
            .expect("손자 열림")
            .row_rect_of(0)
            .unwrap();
        m.on_event(&InputEvent::MouseMove {
            x: g_row.x + 5,
            y: g_row.y + 2,
        });
        m.on_event(&InputEvent::MouseDown {
            x: g_row.x + 5,
            y: g_row.y + 2,
            shift: false,
            primary: false,
        });
        assert_eq!(m.take_picked().as_deref(), Some("sql_merge"));
        assert!(!m.is_open(), "선택 뒤 전부 닫힘");
    }

    #[test]
    fn moving_toward_submenu_keeps_it_open() {
        let mut m = ContextMenu::new();
        m.open_at(10, 10, nested(), host(), 100);
        let adv = m.row_rect(1).unwrap();
        m.on_event(&InputEvent::MouseMove {
            x: adv.x + 5,
            y: adv.y + 2,
        });
        assert!(m.child_for_test().is_some());
        let copy = m.row_rect(0).unwrap();
        // 실제 상황: 자식이 화면 안에 맞추느라 위로 밀려 부모 행 0까지 세로로 겹친다.
        let child = m.child_for_test().unwrap().rect.get();
        m.child_for_test().unwrap().rect.set(Rect::new(
            child.x,
            copy.y - 10,
            child.w,
            child.h + 40,
        ));
        // 자식 쪽(오른쪽)으로 x가 커지며 다른 부모 행(0) 위를 지난다 → 유지.
        m.on_event(&InputEvent::MouseMove {
            x: adv.x + 20,
            y: copy.y + 2,
        });
        assert!(m.child_for_test().is_some(), "자식 쪽으로 이동 중엔 유지");
        // 자식에서 멀어지며(x 감소) 다른 부모 행 위 → 닫힌다.
        m.on_event(&InputEvent::MouseMove {
            x: adv.x + 6,
            y: copy.y + 2,
        });
        assert!(m.child_for_test().is_none(), "멀어지면 닫힘");
    }

    /// 행 우클릭 메뉴 = 행 바로 아래(없으면 위) — 대상 행과 겹치지 않는다.
    #[test]
    fn open_beside_never_covers_the_target_row() {
        let mut m = ContextMenu::new();
        let row = Rect::new(0, 40, 300, 24);
        m.open_beside(60, 50, row, nested(), host(), 100);
        let r = m.bounds();
        assert_eq!((r.x, r.y), (60, row.bottom()));
        assert!(r.intersection(&row).is_empty());
        // 아래에 자리가 없는 행 → 위로.
        let h = host();
        let low = Rect::new(0, h.bottom() - 30, 300, 24);
        m.open_beside(60, low.y + 10, low, nested(), h, 100);
        let r = m.bounds();
        assert_eq!(r.bottom(), low.y);
        assert!(r.intersection(&low).is_empty());
        // 보통의 open_at은 규칙을 물려받지 않는다.
        m.open_at(60, 50, nested(), h, 100);
        assert_eq!((m.bounds().x, m.bounds().y), (60, 50));
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

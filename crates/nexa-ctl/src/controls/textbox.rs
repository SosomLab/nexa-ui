//! 텍스트 박스 — **placeholder** · char 단위 편집(캐럿·선택 [`EditState`]) · 포커스 링 · 도움말.
//!
//! 공통 기능은 [`Control`] 기본 메서드로 상속([`super`]).

use super::{image_fit_contain, Control, ControlBase};
use crate::draw::{DrawCtx, FontSlot};
use crate::edit::{EditCommand, EditKey, EditState};
use crate::event::{InputEvent, Key};
use crate::geom::{Point, Rect};
use crate::theme::{Color, IconImage, Theme};
use crate::widget::{Invalidations, Widget};
use std::rc::Rc;

/// 텍스트 박스 컨트롤.
/// 공백 표시 범위 — Sublime `draw_white_space`(none · selection · all).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WhitespaceMode {
    None,
    #[default]
    Selection,
    All,
}

/// 공백 표시 스타일 — 표시할 글자(공백·탭·줄끝 · `'\0'` = 표시 안 함) · 색(None = 테마 `text_dim`) · 불투명도(0.0~1.0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WhitespaceStyle {
    pub mode: WhitespaceMode,
    pub space: char,
    pub tab: char,
    pub eol: char,
    pub color: Option<Color>,
    pub alpha: f32,
}

impl Default for WhitespaceStyle {
    fn default() -> Self {
        WhitespaceStyle {
            mode: WhitespaceMode::Selection,
            space: '·',
            tab: '→',
            eol: '\0',
            color: None,
            alpha: 0.4,
        }
    }
}

/// 연속 클릭(더블·트리플) 최대 간격(ms · OS 기본 500).
pub const DOUBLE_CLICK_MS: u64 = 500;

#[derive(Debug)]
pub struct TextBox {
    base: ControlBase,
    /// 포커스 링 표시 여부(기본 켬). 편집기처럼 거의 항상 포커스인 상자는 끈다(nexa-sql 사용자 09-16 "매번 눈에 띄어 불편").
    focus_ring: bool,
    edit: EditState,
    placeholder: String,
    /// 선행 이미지 아이콘(옵션 · 투명 배경 RGBA). 있으면 placeholder·캐럿이 그 뒤로 밀린다.
    image: Option<Rc<IconImage>>,
    /// Enter 확정 1회성 보고.
    committed: bool,
    /// 내용 변경 1회성 보고.
    changed: bool,
    /// 값이 있으면 우측에 ×(지우기) 버튼 표시(클릭 = 초기화 · 사용자 요청 08-09).
    clearable: bool,
    /// 텍스트 시작 x(페인트가 기록 — 클릭 좌표를 글자 위치로 바꾸는 근거).
    text_x: std::cell::Cell<i32>,
    /// 각 문자 경계의 누적 폭(페인트가 실측해 기록 · 폰트를 모르는 이벤트 경로가 쓴다).
    caret_xs: std::cell::RefCell<Vec<i32>>,
    /// 드래그 선택 중.
    dragging: bool,
    /// 열(블록) 선택 모드 — 호스트가 Alt+Shift 상태를 밀어 준다(`set_column_mode`).
    column_mode: bool,
    /// 열 선택 드래그 시작점(위젯 좌표) — 드래그 동안 줄마다 같은 x 구간을 선택한다.
    col_anchor: Option<(i32, i32)>,
    /// 마지막 클릭 (캐럿 인덱스, 연속 횟수) — 더블·트리플 판정.
    /// `MouseDown`에는 시각이 없어 **같은 위치 + 무개입**(사이에 키 입력 없음)으로
    /// 연속을 판정한다(시각 주입은 M3-1e). 위치가 다르면 새 체인 = 캐럿 이동만.
    last_click: (usize, u8),
    /// 마지막 MouseDown 시각 — 연속 클릭은 [`DOUBLE_CLICK_MS`] 안에서만 잇는다(nexa-sql 09-15:
    /// 같은 자리를 한참 뒤에 다시 누르면 더블클릭(단어 선택)이 돼 드래그 선택이 시작되지 않던 결함).
    last_click_at: Option<std::time::Instant>,
    /// 가로 스크롤(px · ① 08-13) — 텍스트가 폭을 넘으면 **캐럿이 항상 보이게**
    /// 페인트가 조정한다(셀: 페인트는 &self).
    hscroll: std::cell::Cell<i32>,
    /// IME 조합 중 문자열(08-13) — 캐럿 자리에 밑줄로 끼워 그린다. 확정 전에도
    /// 지금 치는 글자가 보여야 한다(대화 입력과 동일한 경험 — 호스트가 배선).
    /// 우클릭 편집 메뉴(08-13 전수 검사 — 대화 입력에만 있고 일반 필드엔 없었다).
    ctx_menu: super::EditMenu,
    /// 메뉴에서 고른 클립보드 행동(1회성) — OS 클립보드는 호스트 몫이라 요청만 남긴다.
    edit_ctx: Option<EditCtxAction>,
    /// 붙여넣기 항목 활성 근거(호스트가 우클릭 시점에 1회 주입 — 대화 입력과 동일).
    clip_has_text: bool,
    /// 허용 문자 필터(08-22 공용 — Combo 직접 입력 위임의 재료): Some(f)면 f가
    /// 거짓인 문자를 **타이핑·붙여넣기 모두**에서 버린다(경로가 달라도 규칙은 하나).
    char_filter: Option<fn(char) -> bool>,
    /// 최대 문자 수(0 = 무제한 · 기본) — 타이핑·붙여넣기 공통 상한.
    max_chars: usize,
    /// 멀티라인(소개글) 모드(08-17) — Enter가 확정 대신 개행, 세로 여러 줄 렌더.
    /// 단일 라인 경로는 이 플래그가 꺼져 있어 종전 그대로다.
    multiline: bool,
    /// ★ 마스킹(●) — 비밀 값 표시용(09-03 동기화 패스프레이즈). 값·편집은 불변,
    ///   **표시 문자열만** 바꾼다(캐럿·히트테스트는 같은 마스킹 문자열을 재서 일관).
    masked: bool,
    /// ★ 멀티라인 **줄 바꿈**(09-02 사용자 요청 — 편집 시트 Alt+Z 토글). 표시 레이아웃만
    ///   접는다 — 세로 이동(↑↓)·Home/End는 **논리 줄** 기준 유지(1단 · 시각 줄 nav는 후속).
    wrap: bool,
    /// 멀티라인 세로 스크롤(첫 보이는 논리 줄 인덱스 · 캐럿을 따라간다).
    vscroll: std::cell::Cell<usize>,
    /// 멀티라인 가로 스크롤(px · 캐럿 열을 따라간다 · 08-17 드래그 자동 스크롤).
    mhscroll: std::cell::Cell<i32>,
    /// 사용자가 휠/바로 스크롤했다(08-18) — 참이면 paint가 캐럿을 따라가지 않고
    /// vscroll/mhscroll을 그대로 존중(자유 스크롤). 편집(캐럿 이동) 시 거짓으로 리셋.
    ml_user_scrolled: bool,
    /// 휠의 줄 단위 반올림에서 남은 px(다음 휠 사건에 이월 · 트랙패드 느린 스크롤 · 09-16).
    ml_wheel_rem: std::cell::Cell<i32>,
    /// 줄 경계 스냅(행 단위 스크롤 모드).
    scroll_snap: bool,
    /// 줄 주석 접두(`--` · `#` · `//` — 문법이 준다 · 주석 토글).
    line_comment: Option<String>,
    /// 찾기 일치 구간(전부 · 호스트가 갱신) — 반투명 채움으로 표시(nexa-sql T-73 · 09-16).
    find_marks: Vec<(usize, usize)>,
    /// 줄번호 거터(멀티라인 · 09-14 nexa-sql 편집기). 폭은 페인트가 재서 캐시한다.
    line_numbers: bool,
    /// 줄번호 오른쪽 **표시 띠**(Golden식 · 3px 색 막대 자리 4px + 첫 글자 앞 2px 여백 · nexa-sql 09-16).
    gutter_marks: bool,
    /// 첫 글자 앞 추가 여백(논리 px · 기본 0 · nexa-sql `editor.text_pad_left` 3).
    text_inset: i32,
    /// 논리 줄(0부터)별 표시 색 — 북마크·오류·변경 등 호스트가 정한다.
    line_marks: Vec<(usize, Color)>,
    /// 들여쓰기(nexa-sql 09-15 · docs/31): 탭 폭(칸) · Tab 키 = 공백(다음 탭 정지까지) 여부.
    tab_size: u8,
    indent_spaces: bool,
    /// 탭/공백 들여쓰기 = 정지점(다음 탭 폭 배수 열까지 · 기본) 또는 절대(늘 탭 폭) — nexa-sql `editor.tab_stops`(사용자 09-16).
    tab_stops: bool,
    gutter_px: std::cell::Cell<i32>,
    /// 멀티라인 스크롤바(08-18 · 대화 입력창과 동일 컨트롤 · 상하+좌우 · 자동 숨김).
    ml_bars: super::ScrollBars,
    /// 멀티라인 콘텐츠 크기 (content_w, content_h) px — paint가 실측해 캐시하고
    /// on_event(폰트 못 재는 경로)가 스크롤바 계산에 쓴다.
    ml_content: std::cell::Cell<(i32, i32)>,
    /// 멀티라인 클릭→캐럿 변환용 줄 배치(페인트가 남긴다).
    line_lay: std::cell::RefCell<Vec<MlLine>>,
    /// ★ 단일 행 hover 페이드(회색 계열 · `Slow` 1초 · nexa-sql 사용자 09-14) — 멀티라인(편집기)은 제외.
    hover: crate::tokens::Fade,
    /// 구문 강조(멀티라인 · 옵션) — 줄 단위 스팬을 색으로 그린다.
    highlighter: Option<Rc<dyn crate::highlight::Highlighter>>,
    /// 세로 안내선(글자 열 · 예 `[80]` · 여러 개) — 멀티라인 고정폭에서 그린다.
    rulers: Vec<usize>,
    /// 안내선 표시 여부(설정 `editor.rulers_show`).
    rulers_show: bool,
    /// 안내선 색(None = 테마 경계선) · 투명도(0~1 · 기본 0.25 — 종전 불투명 경계선은 너무 진했다 · nexa-sql 09-16).
    ruler_color: Option<Color>,
    ruler_alpha: f32,
    /// ★ 선택한 글과 같은 다른 출현을 외곽선으로(Sublime · nexa-sql 09-16) — 선택된 출현은 채움 그대로.
    occurrence_hl: bool,
    /// 공백 문자 표시(설정 `editor.whitespace*`).
    whitespace: WhitespaceStyle,
}

/// 멀티라인 한 줄의 화면 배치(클릭 매핑용 · 페인트가 채운다).
#[derive(Clone, Debug)]
struct MlLine {
    /// 줄 상단 y.
    top: i32,
    /// 이 줄 첫 글자의 **버퍼 char 인덱스**.
    start_idx: usize,
    /// 글자 경계 절대 x(len = 줄 글자수 + 1).
    xs: Vec<i32>,
}

/// 우클릭 편집 메뉴에서 고른 행동 — 실행(클립보드 접근)은 호스트가 한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditCtxAction {
    /// 복사(⌘/Ctrl+C와 같은 경로).
    Copy,
    /// 잘라내기.
    Cut,
    /// 붙여넣기.
    Paste,
}

impl TextBox {
    /// placeholder로 만든다(빈 값 시작).
    #[must_use]
    pub fn new(placeholder: impl Into<String>) -> Self {
        Self {
            focus_ring: true,
            base: ControlBase::default(),
            edit: EditState::new(),
            placeholder: placeholder.into(),
            image: None,
            committed: false,
            changed: false,
            clearable: false,
            text_x: std::cell::Cell::new(0),
            caret_xs: std::cell::RefCell::new(Vec::new()),
            dragging: false,
            column_mode: false,
            col_anchor: None,
            last_click: (0, 0),
            last_click_at: None,
            hscroll: std::cell::Cell::new(0),
            ctx_menu: super::EditMenu::new(),
            edit_ctx: None,
            clip_has_text: true,
            char_filter: None,
            max_chars: 0,
            multiline: false,
            masked: false,
            wrap: false,
            vscroll: std::cell::Cell::new(0),
            mhscroll: std::cell::Cell::new(0),
            ml_user_scrolled: false,
            ml_wheel_rem: std::cell::Cell::new(0),
            scroll_snap: false,
            line_comment: None,
            find_marks: Vec::new(),
            line_numbers: false,
            gutter_marks: false,
            text_inset: 0,
            line_marks: Vec::new(),
            tab_size: 4,
            indent_spaces: false,
            tab_stops: true,
            gutter_px: std::cell::Cell::new(0),
            ml_bars: super::ScrollBars::new(),
            ml_content: std::cell::Cell::new((0, 0)),
            line_lay: std::cell::RefCell::new(Vec::new()),
            hover: crate::tokens::Fade::at(crate::tokens::FadeSpeed::Slow),
            highlighter: None,
            rulers: Vec::new(),
            rulers_show: true,
            ruler_color: None,
            ruler_alpha: 0.25,
            occurrence_hl: true,
            whitespace: WhitespaceStyle::default(),
        }
    }

    /// 멀티라인 스크롤바 시간 틱(08-18 · 자동 숨김) — 호스트(프로필)가 부른다.
    /// `true` = 다시 그려야 한다.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        let a = self.ml_bars.tick(now_ms);
        let b = self.hover.tick(now_ms);
        a || b
    }

    /// hover 페이드가 움직이는 중인가(호스트가 프레임을 예약할지).
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.hover.is_animating()
    }

    /// 줄번호 거터 켜기/끄기(멀티라인에서만 그려진다 · 기본 끔).
    /// 구문 강조 규격 지정(None = 강조 없음). 멀티라인에서만 그린다.
    pub fn set_highlighter(&mut self, h: Option<Rc<dyn crate::highlight::Highlighter>>) {
        self.highlighter = h;
    }

    #[must_use]
    pub fn highlighter(&self) -> Option<&Rc<dyn crate::highlight::Highlighter>> {
        self.highlighter.as_ref()
    }

    /// 공백 표시 방식(어떤 공백을 · 어떤 글자로 · 무슨 색/투명도로).
    pub fn set_whitespace(&mut self, ws: WhitespaceStyle) {
        self.whitespace = ws;
    }

    /// 안내선 표시 여부.
    pub fn set_rulers_visible(&mut self, on: bool) {
        self.rulers_show = on;
    }

    /// 안내선 색(None = 테마 경계선)과 투명도(0~1).
    pub fn set_ruler_style(&mut self, color: Option<Color>, alpha: f32) {
        self.ruler_color = color;
        self.ruler_alpha = alpha.clamp(0.0, 1.0);
    }

    /// 선택한 글과 같은 다른 출현 외곽선 켬/끔.
    pub fn set_occurrence_highlight(&mut self, on: bool) {
        self.occurrence_hl = on;
    }

    /// 세로 안내선 열 목록(0 = 없음) — 설정 `editor.rulers`(기본 80).
    pub fn set_rulers(&mut self, cols: Vec<usize>) {
        self.rulers = cols;
    }

    /// 들여쓰기 설정 — 탭 폭(칸 · 1~8) · Tab 키가 공백을 넣는지(Sublime `translate_tabs_to_spaces`).
    pub fn set_indent(&mut self, tab_size: u8, spaces: bool) {
        self.tab_size = tab_size.clamp(1, 8);
        self.indent_spaces = spaces;
    }

    /// 탭 폭(칸).
    #[must_use]
    pub fn tab_size(&self) -> u8 {
        self.tab_size
    }

    /// Tab 키·붙여넣기가 공백을 넣는가.
    #[must_use]
    pub fn indent_spaces(&self) -> bool {
        self.indent_spaces
    }

    /// 탭/공백 적용 방식 — `true` = 정지점(앞 글자 수를 고려해 1~탭 폭 칸 · Golden/Sublime 관례) · `false` = 절대(늘 탭 폭).
    pub fn set_tab_stops(&mut self, on: bool) {
        self.tab_stops = on;
    }

    /// 휠 스크롤을 **줄 경계에 맞춰** 그리는가(`true` = 행 단위 · `false` = 픽셀 단위 기본). 잔여 px는 두 모드 모두
    /// 누적되므로 느린 트랙패드 이동도 잃지 않는다(nexa-sql `editor.scroll` · 09-16).
    pub fn set_scroll_snap(&mut self, on: bool) {
        self.scroll_snap = on;
    }

    /// 줄 주석 접두(문법별 · `None` = 주석 토글 없음).
    pub fn set_line_comment(&mut self, prefix: Option<String>) {
        self.line_comment = prefix;
    }

    /// 찾기 일치 구간 전부(문자 인덱스 · 빈 목록 = 표시 없음).
    pub fn set_find_marks(&mut self, marks: Vec<(usize, usize)>) {
        self.find_marks = marks;
    }

    /// ★ Sublime식 편집 명령(줄 복제/삭제/합치기/이동 · 주석 토글 · 들여쓰기 ± · 줄 선택/나누기 · 캐럿 추가 · 대소문자).
    /// 들여쓰기 단위·주석 접두는 이 상자의 설정을 쓴다. 바뀌었으면 `true`(호스트가 다시 그린다).
    pub fn edit_command(&mut self, cmd: EditCommand) -> bool {
        let unit = if self.indent_spaces {
            " ".repeat(usize::from(self.tab_size.max(1)))
        } else {
            "\t".to_string()
        };
        let changed = self.edit.command(
            cmd,
            &unit,
            usize::from(self.tab_size.max(1)),
            self.line_comment.as_deref(),
        );
        if changed {
            self.changed = true;
            self.last_click.1 = 0;
            self.ml_user_scrolled = false;
        }
        changed
    }

    /// `n`번째 줄(1 기준)로 캐럿 이동(Goto line · 넘치면 마지막 줄) — 캐럿을 따라 스크롤한다.
    pub fn goto_line(&mut self, n: usize) {
        let i = self.edit.line_start_index(n);
        self.edit.set_caret(i, false);
        self.last_click.1 = 0;
        self.ml_user_scrolled = false;
    }

    /// 탭 정지점 방식인가.
    #[must_use]
    pub fn tab_stops(&self) -> bool {
        self.tab_stops
    }

    /// 열 `col`에서 탭 하나가 옮기는 칸 수(정지점 또는 절대).
    fn tab_cells(ts: usize, col: usize, stops: bool) -> usize {
        if stops {
            ts - col % ts
        } else {
            ts
        }
    }

    /// 연속 클릭 판정 — 직전 MouseDown이 [`DOUBLE_CLICK_MS`] 안이면 체인을 잇고, 지금 시각을 기록한다.
    fn click_chain_alive(&mut self) -> bool {
        let now = std::time::Instant::now();
        let alive = self
            .last_click_at
            .is_some_and(|t| now.duration_since(t).as_millis() <= u128::from(DOUBLE_CLICK_MS));
        self.last_click_at = Some(now);
        alive
    }

    /// 탭 문자를 **공백**으로 편다 — 정지점이면 다음 정지까지 · 절대면 탭 폭만큼(줄마다 열을 다시 센다 · 첫 줄은 `start_col`부터).
    fn expand_tabs(text: &str, tab_size: usize, start_col: usize, stops: bool) -> String {
        let ts = tab_size.max(1);
        let mut out = String::with_capacity(text.len());
        let mut col = start_col;
        for c in text.chars() {
            match c {
                '\t' => {
                    let n = Self::tab_cells(ts, col, stops);
                    out.push_str(&" ".repeat(n));
                    col += n;
                }
                '\n' => {
                    out.push(c);
                    col = 0;
                }
                _ => {
                    out.push(c);
                    col += 1;
                }
            }
        }
        out
    }

    /// 캐럿이 있는 줄의 열(0 기준 · 탭은 다음 정지까지).
    fn caret_column(&self) -> usize {
        let text = self.edit.text();
        let chars: Vec<char> = text.chars().collect();
        let caret = self.edit.caret().min(chars.len());
        let line_start = chars[..caret]
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(0, |p| p + 1);
        Self::col_of(
            &chars[line_start..caret],
            usize::from(self.tab_size.max(1)),
            self.tab_stops,
        )
    }

    /// 줄 앞부분 문자열의 열(탭은 정지점 또는 절대 폭).
    fn col_of(chars: &[char], ts: usize, stops: bool) -> usize {
        let mut col = 0usize;
        for &c in chars {
            col += if c == '\t' {
                Self::tab_cells(ts, col, stops)
            } else {
                1
            };
        }
        col
    }

    /// 줄머리 들여쓰기 변환(공백 ↔ 탭 · 본문 전체 · 되돌리기 히스토리는 새로 시작).
    pub fn convert_indent(&mut self, to_spaces: bool) {
        let ts = usize::from(self.tab_size.max(1));
        let mut out = String::new();
        for (i, line) in self.edit.text().split('\n').enumerate() {
            if i > 0 {
                out.push('\n');
            }
            let body_at = line
                .char_indices()
                .find(|(_, c)| *c != ' ' && *c != '\t')
                .map_or(line.len(), |(b, _)| b);
            let (lead, body) = line.split_at(body_at);
            let mut col = 0usize;
            for c in lead.chars() {
                col += if c == '\t' {
                    Self::tab_cells(ts, col, self.tab_stops)
                } else {
                    1
                };
            }
            if to_spaces {
                out.push_str(&" ".repeat(col));
            } else {
                out.push_str(&"\t".repeat(col / ts));
                out.push_str(&" ".repeat(col % ts));
            }
            out.push_str(body);
        }
        self.edit.set_text(&out);
        self.changed = true;
    }

    pub fn set_line_numbers(&mut self, on: bool) {
        self.line_numbers = on;
    }

    /// 포커스 링 표시 여부 — 끄면 포커스여도 헤일로를 그리지 않는다(캐럿·선택은 그대로).
    pub fn set_focus_ring(&mut self, on: bool) {
        self.focus_ring = on;
    }

    /// 첫 글자 앞 추가 여백(논리 px) — 텍스트 원점만 옮긴다(거터·히트 테스트는 같은 원점을 쓴다).
    pub fn set_text_inset(&mut self, px: i32) {
        self.text_inset = px.clamp(0, 64);
    }

    /// 줄번호 오른쪽 표시 띠(4px) + 첫 글자 앞 여백(2px) 켬/끔 — 줄번호 거터가 있을 때만 그려진다.
    pub fn set_gutter_marks(&mut self, on: bool) {
        self.gutter_marks = on;
    }

    /// 표시 띠의 색 막대(논리 줄 0부터 · 색) — 전체 교체.
    pub fn set_line_marks(&mut self, marks: Vec<(usize, Color)>) {
        self.line_marks = marks;
    }

    /// 줄번호 거터 폭(마지막 페인트 실측 · 0 = 없음).
    #[must_use]
    pub fn gutter_width(&self) -> i32 {
        self.gutter_px.get()
    }

    /// 멀티라인 오버레이 스크롤바가 지금 보이는가 — 호스트가 페이드 타이머(≈30ms)를 돌릴지 정하는 근거(09-14).
    #[must_use]
    pub fn scrollbars_visible(&self) -> bool {
        self.multiline && self.ml_bars.is_visible()
    }

    /// 허용 문자 필터 지정(08-22) — 타이핑·붙여넣기 공통. None = 전부 허용(기본).
    pub fn set_char_filter(&mut self, f: Option<fn(char) -> bool>) {
        self.char_filter = f;
    }

    /// 최대 문자 수 지정(08-22) — 0 = 무제한(기본). 타이핑·붙여넣기 공통 상한.
    pub fn set_max_chars(&mut self, n: usize) {
        self.max_chars = n;
    }

    /// 이 문자를 받는가(필터 판정 — 한 곳).
    fn accepts(&self, c: char) -> bool {
        self.char_filter.is_none_or(|f| f(c))
    }

    /// 남은 자리 수(상한 없음 = usize::MAX).
    fn room(&self) -> usize {
        if self.max_chars == 0 {
            usize::MAX
        } else {
            self.max_chars
                .saturating_sub(self.edit.text().chars().count())
        }
    }

    /// 멀티라인(소개글) 모드로 만든다(체이닝 · 08-17) — Enter = 개행. 보이는 줄
    /// 수는 상자 높이가 정한다(호스트가 relayout에서 높이를 준다).
    #[must_use]
    pub fn with_multiline(mut self) -> Self {
        self.multiline = true;
        self
    }

    /// ★ 마스킹 토글(단일행) — 비밀 값을 ●로 가린다(09-03).
    pub fn set_masked(&mut self, on: bool) {
        self.masked = on;
    }

    /// 현재 마스킹 상태.
    #[must_use]
    pub fn masked(&self) -> bool {
        self.masked
    }

    /// ★ 줄 바꿈 토글(멀티라인 전용) — 켜면 가로 스크롤 대신 폭에 맞춰 접는다.
    pub fn set_wrap(&mut self, on: bool) {
        self.wrap = on;
        self.mhscroll.set(0);
    }

    /// 현재 줄 바꿈 상태.
    #[must_use]
    pub fn wrap(&self) -> bool {
        self.wrap
    }

    /// 논리 줄 분해 — `(첫 글자 char 인덱스, 줄 문자열)`. `'\n'`은 줄에 안 담고
    /// 다음 줄의 start를 그 뒤로 민다. 빈 텍스트도 한 줄(빈 줄)로 본다.
    fn logical_lines(text: &str) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        let mut line_start = 0usize; // 이 줄 첫 글자의 char 인덱스
        let mut pos = 0usize; // 지금까지 훑은 char 수
        let mut cur = String::new();
        for ch in text.chars() {
            if ch == '\n' {
                out.push((line_start, std::mem::take(&mut cur)));
                pos += 1;
                line_start = pos; // 다음 줄 시작 = '\n' 바로 뒤
            } else {
                cur.push(ch);
                pos += 1;
            }
        }
        out.push((line_start, cur)); // 마지막 줄(개행으로 안 끝난 부분)
        out
    }

    /// 멀티라인 세로 이동(위/아래) — 같은 열을 목표로, 짧은 줄이면 줄 끝으로.
    fn ml_move_vert(&mut self, down: bool, shift: bool) {
        let text = self.edit.text();
        let chars: Vec<char> = text.chars().collect();
        let caret = self.edit.caret().min(chars.len());
        let line_start = chars[..caret]
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(0, |p| p + 1);
        let col = caret - line_start;
        if down {
            let rel = chars[line_start..].iter().position(|&c| c == '\n');
            let Some(nl) = rel else { return }; // 마지막 줄 — 아래 없음
            let next_start = line_start + nl + 1;
            let next_end = next_start
                + chars[next_start..]
                    .iter()
                    .position(|&c| c == '\n')
                    .unwrap_or(chars.len() - next_start);
            self.edit.set_caret((next_start + col).min(next_end), shift);
        } else {
            if line_start == 0 {
                return; // 첫 줄 — 위 없음
            }
            let prev_end = line_start - 1; // '\n' 위치
            let prev_start = chars[..prev_end]
                .iter()
                .rposition(|&c| c == '\n')
                .map_or(0, |p| p + 1);
            self.edit.set_caret((prev_start + col).min(prev_end), shift);
        }
    }

    /// 멀티라인 줄 처음/끝 인덱스(Home/End).
    fn ml_line_edge(&self, end: bool) -> usize {
        let text = self.edit.text();
        let chars: Vec<char> = text.chars().collect();
        let caret = self.edit.caret().min(chars.len());
        let start = chars[..caret]
            .iter()
            .rposition(|&c| c == '\n')
            .map_or(0, |p| p + 1);
        if end {
            start
                + chars[start..]
                    .iter()
                    .position(|&c| c == '\n')
                    .unwrap_or(chars.len() - start)
        } else {
            start
        }
    }

    /// 멀티라인 클릭 → 캐럿 인덱스(페인트가 남긴 줄 배치에서 가장 가까운 경계).
    fn ml_caret_at(&self, x: i32, y: i32) -> usize {
        let lay = self.line_lay.borrow();
        if lay.is_empty() {
            return 0;
        }
        // y로 줄 선택(위/아래 밖은 처음/끝 줄로 클램프).
        let li = lay
            .iter()
            .position(|l| y < l.top + self.line_h())
            .unwrap_or(lay.len() - 1);
        let line = &lay[li];
        let mut best = 0usize;
        let mut best_d = i32::MAX;
        for (i, cx) in line.xs.iter().enumerate() {
            let d = (x - cx).abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        line.start_idx + best
    }

    /// 멀티라인 한 줄 높이(글꼴 실측 + 여백은 페인트와 같은 값).
    fn line_h(&self) -> i32 {
        self.s(20)
    }

    /// 메뉴에서 고른 클립보드 행동(1회성) — 호스트가 ⌘C/X/V와 같은 경로로 잇는다.
    pub fn take_edit_ctx(&mut self) -> Option<EditCtxAction> {
        self.edit_ctx.take()
    }

    /// 클립보드에 텍스트가 있는가(호스트가 우클릭 시점에 1회 주입 — 붙여넣기 활성 근거).
    pub fn set_clipboard_has_text(&mut self, yes: bool) {
        self.clip_has_text = yes;
    }

    /// IME 조합 중 문자열 갱신(빈 문자열 = 소거). 포커스 없는 박스는 무시한다 —
    /// 호스트가 창 단위로 보내므로 초점 필드만 받아야 이중 표시가 없다.
    pub fn set_preedit(&mut self, text: &str, inv: &mut Invalidations) {
        if !self.base.focused && !text.is_empty() {
            return; // 포커스 가드는 위젯 몫(H-25 선택 삭제·저장은 EditState 공용)
        }
        let changed = self.edit.preedit() != text;
        // H-25(조합 시작 = 선택 삭제)는 EditState::set_preedit이 공용으로 처리하고,
        // 버퍼를 바꿨으면 true를 준다 — dirty 플래그 갱신 근거(M3-1e ①).
        if self.edit.set_preedit(text) {
            self.changed = true;
        }
        if changed {
            inv.push(self.base.bounds);
        }
    }

    /// ×(지우기) 버튼 사용(체이닝) — 값이 있을 때만 표시, 클릭 = 즉시 초기화.
    #[must_use]
    pub fn with_clearable(mut self) -> Self {
        self.clearable = true;
        self
    }

    /// 클릭 x → 캐럿 인덱스(페인트가 남긴 실측 폭을 쓴다 — 가장 가까운 경계).
    fn caret_at_x(&self, x: i32) -> usize {
        let xs = self.caret_xs.borrow();
        if xs.is_empty() {
            return 0;
        }
        let mut best = 0usize;
        let mut best_d = i32::MAX;
        for (i, cx) in xs.iter().enumerate() {
            let d = (x - cx).abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        best
    }

    /// 단어 경계로 선택(더블클릭).
    fn select_word_at(&mut self, idx: usize) {
        let chars: Vec<char> = self.edit.text().chars().collect();
        if chars.is_empty() {
            return;
        }
        let i = idx.min(chars.len().saturating_sub(1));
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let mut a = i;
        while a > 0 && is_word(chars[a - 1]) {
            a -= 1;
        }
        let mut b = i;
        while b < chars.len() && is_word(chars[b]) {
            b += 1;
        }
        self.edit.set_selection(a, b);
    }

    /// 열(블록) 선택 모드 — Sublime의 Alt+Shift 드래그. 호스트가 수식키 상태를 밀어 준다.
    pub fn set_column_mode(&mut self, on: bool) {
        self.column_mode = on;
    }

    /// 다중 선택/캐럿을 하나로 접는다(Esc) — 접었으면 `true`.
    pub fn clear_multi(&mut self) -> bool {
        self.edit.clear_multi()
    }

    /// 다중 선택 중인가(상태줄 표시).
    #[must_use]
    pub fn has_multi(&self) -> bool {
        self.edit.has_multi()
    }

    /// 선택 구간 수(주 선택 포함).
    #[must_use]
    pub fn selection_count(&self) -> usize {
        self.edit.regions().len()
    }

    /// ★ Sublime `find_under_expand`(Ctrl+D) — 선택이 없으면 캐럿 밑 **단어**를 선택하고,
    /// 이미 선택이 있으면 같은 문자열의 **다음 출현**을 추가 선택한다(끝까지 가면 처음으로 되돌아온다).
    /// 더 찾을 것이 없으면 `false`.
    pub fn select_next_occurrence(&mut self) -> bool {
        let chars: Vec<char> = self.edit.text().chars().collect();
        if chars.is_empty() {
            return false;
        }
        let Some((a, b)) = self.edit.selection() else {
            let i = self.edit.caret().min(chars.len());
            self.select_word_at(i);
            return self.edit.selection().is_some();
        };
        let needle: Vec<char> = chars[a..b].to_vec();
        if needle.is_empty() {
            return false;
        }
        let taken = self.edit.regions();
        let n = chars.len();
        let m = needle.len();
        if m > n {
            return false;
        }
        // 주 선택 끝 다음부터 앞으로 훑고, 끝까지 가면 처음으로 되돌아온다(Sublime).
        let start = b;
        for k in 0..=(n - m) {
            let i = (start + k) % (n - m + 1);
            if chars[i..i + m] != needle[..] {
                continue;
            }
            if taken.contains(&(i, i + m)) {
                continue;
            }
            return self.edit.add_selection(i, i + m);
        }
        false
    }

    /// 열 선택 드래그 중인가(테스트·호스트 판정).
    #[must_use]
    pub fn column_dragging(&self) -> bool {
        self.col_anchor.is_some()
    }

    /// 두 점(위젯 좌표) 사이의 **열 블록** 선택 — 줄마다 같은 x 구간을 구간 하나로 만든다.
    /// 가로 폭이 0이면 줄마다 캐럿만 남는다(Sublime의 다중 커서).
    fn column_regions(&self, ax: i32, ay: i32, bx: i32, by: i32) -> Vec<(usize, usize)> {
        let lay = self.line_lay.borrow();
        if lay.is_empty() {
            return Vec::new();
        }
        let lh = self.line_h();
        let row_at = |y: i32| -> usize {
            lay.iter()
                .position(|l| y < l.top + lh)
                .unwrap_or(lay.len() - 1)
        };
        let (r0, r1) = (row_at(ay), row_at(by));
        let (lo, hi) = (r0.min(r1), r0.max(r1));
        let mut out = Vec::with_capacity(hi - lo + 1);
        for li in lo..=hi {
            let y = lay[li].top;
            let i0 = self.ml_caret_at(ax, y);
            let i1 = self.ml_caret_at(bx, y);
            out.push((i0, i1));
        }
        // 드래그 방향에 따라 마지막(= 주 선택)을 커서 쪽 줄로.
        if r1 < r0 {
            out.reverse();
        }
        out
    }

    /// ×(지우기) 버튼 영역(값이 있을 때만 유효) — 호스트 테스트용 공개.
    #[must_use]
    pub fn clear_rect(&self) -> Rect {
        let b = self.base.bounds;
        let d = self.s(16);
        Rect::new(b.right() - d - self.s(6), b.y + (b.h - d) / 2, d, d)
    }

    /// 선행 이미지 아이콘 지정(체이닝) — placeholder·캐럿이 아이콘 뒤로 배치된다.
    #[must_use]
    pub fn with_image(mut self, image: Rc<IconImage>) -> Self {
        self.image = Some(image);
        self
    }

    /// 초기 텍스트 지정.
    #[must_use]
    pub fn with_text(mut self, text: &str) -> Self {
        self.edit = EditState::with_text(text, false);
        self
    }

    /// 현재 텍스트.
    #[must_use]
    pub fn text(&self) -> String {
        self.edit.text()
    }

    /// 텍스트 지정(보고 없음).
    pub fn set_text(&mut self, text: &str) {
        self.edit.set_text(text);
    }

    /// 내용이 바뀌었으면 새 텍스트를 꺼낸다(1회성).
    pub fn take_changed(&mut self) -> Option<String> {
        std::mem::take(&mut self.changed).then(|| self.edit.text())
    }

    /// 캐럿의 **문자 인덱스**(호스트가 "캐럿 위치의 문장" 같은 것을 계산 — nexa-sql Ctrl+Enter 한 문장 실행 · 09-14).
    /// 선택이 있으면 head(움직이는 쪽)다. 바이트 오프셋이 필요하면 호스트가 `text().char_indices()`로 바꾼다.
    #[must_use]
    pub fn caret(&self) -> usize {
        self.edit.caret()
    }

    /// 선택 범위(문자 인덱스 · 정렬됨). 없으면 `None`.
    #[must_use]
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.edit.selection()
    }

    /// 프로그램 선택(찾기 결과 강조 · nexa-sql 09-15) — 문자 인덱스 `from..to` · 캐럿은 `to` · 캐럿 추종 스크롤 재개.
    pub fn select_range(&mut self, from: usize, to: usize, inv: &mut Invalidations) {
        self.edit.set_selection(from, to);
        self.ml_user_scrolled = false;
        inv.push(self.base.bounds);
    }

    /// 범위 교체(바꾸기) — 되돌리기 히스토리에 남는다(`set_text`와 달리).
    pub fn replace_range(&mut self, from: usize, to: usize, text: &str, inv: &mut Invalidations) {
        self.edit.set_selection(from, to);
        self.edit.insert_str(text);
        self.changed = true;
        self.ml_user_scrolled = false;
        inv.push(self.base.bounds);
    }

    /// 선택 텍스트(복사 — ① 08-13). 위젯은 OS 클립보드를 모른다 — 호스트가 잇는다.
    #[must_use]
    pub fn copy_selection(&self) -> Option<String> {
        self.base.focused.then(|| self.edit.selected_text_multi())?
    }

    /// 선택 텍스트를 잘라낸다(① — 반환 텍스트를 호스트가 클립보드에 쓴다).
    pub fn cut_selection(&mut self, inv: &mut Invalidations) -> Option<String> {
        if !self.base.focused {
            return None;
        }
        let t = self.edit.cut()?;
        self.changed = true;
        inv.push(self.base.bounds);
        Some(t)
    }

    /// 붙여넣기(① — 호스트가 읽은 클립보드 텍스트). 단일 행 컨트롤이라
    /// 개행·제어문자는 공백 하나로 접는다(주소·이름·검색 어디서든 안전).
    pub fn paste(&mut self, text: &str, inv: &mut Invalidations) {
        if !self.base.focused || text.is_empty() {
            return;
        }
        // 멀티라인은 개행을 **보존**(08-18 사용자 실기 — 여러 줄 붙여넣기가 한 줄이
        // 됐다): `\r\n`/`\r`을 `\n`으로 정규화하고 그 외 제어문자만 공백으로 접는다.
        // 단일 행은 종전대로 개행·제어를 공백 하나로 접는다.
        let cleaned = if self.multiline {
            let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            let mut out = String::with_capacity(normalized.len());
            for c in normalized.chars() {
                // 탭은 코드(SQL·스크립트)에서 의미 있는 공백 — 제어문자지만 보존(09-14 nexa-sql 편집기).
                if c == '\n' || c == '\t' || !c.is_control() {
                    out.push(c);
                }
            }
            // ★ 공백 들여쓰기 탭(파일)이면 붙여넣는 탭 문자를 **탭 폭만큼 공백**으로 편다(탭 들여쓰기 탭은
            //   그대로 · 열은 캐럿 열부터 · nexa-sql 09-15 사용자 요청).
            if self.indent_spaces && out.contains('\t') {
                let col = if self.edit.selection().is_some() {
                    // 선택 대체 = 선택 시작 열(캐럿이 선택 끝일 수 있다).
                    let (a, _) = self.edit.selection().unwrap_or((0, 0));
                    let text = self.edit.text();
                    let chars: Vec<char> = text.chars().collect();
                    let a = a.min(chars.len());
                    let ls = chars[..a]
                        .iter()
                        .rposition(|&c| c == '\n')
                        .map_or(0, |p| p + 1);
                    Self::col_of(
                        &chars[ls..a],
                        usize::from(self.tab_size.max(1)),
                        self.tab_stops,
                    )
                } else {
                    self.caret_column()
                };
                Self::expand_tabs(&out, usize::from(self.tab_size.max(1)), col, self.tab_stops)
            } else {
                out
            }
        } else {
            let mut out = String::with_capacity(text.len());
            let mut ws = false;
            for c in text.chars() {
                if c.is_control() {
                    ws = true;
                    continue;
                }
                if ws {
                    out.push(' ');
                    ws = false;
                }
                out.push(c);
            }
            out
        };
        // 필터·상한은 붙여넣기에도 동일 적용(08-22 — 경로가 달라도 규칙은 하나).
        let mut cleaned = cleaned;
        if let Some(f) = self.char_filter {
            cleaned.retain(f);
        }
        let room = self.room();
        if cleaned.chars().count() > room {
            cleaned = cleaned.chars().take(room).collect();
        }
        if cleaned.is_empty() {
            return;
        }
        self.edit.insert_str(&cleaned);
        self.changed = true;
        inv.push(self.base.bounds);
    }

    /// Enter 확정되었으면 텍스트를 꺼낸다(1회성).
    pub fn take_committed(&mut self) -> Option<String> {
        std::mem::take(&mut self.committed).then(|| self.edit.text())
    }

    /// 우클릭 편집 메뉴가 열려 있는가 — 컨테이너의 Esc 가드용(08-13 실기:
    /// 메뉴가 열려 있는데 Esc가 창 닫기로 새면 메뉴를 키보드로 못 닫는다).
    #[must_use]
    pub fn popup_open(&self) -> bool {
        self.ctx_menu.is_open()
    }

    /// 조합 중(preedit) 문자열까지 캐럿 자리에 끼운 **표시용** 텍스트(편집 상태 불변).
    /// 아바타 이니셜 미리보기 등 "지금 화면에 보이는 그대로"가 필요한 곳이 쓴다
    /// (08-13 실기: 필드엔 "나다"가 보이는데 아바타는 "나"라 미입력처럼 보였다).
    #[must_use]
    pub fn display_text(&self) -> String {
        self.edit.display_text()
    }

    /// 우클릭 메뉴를 **최상위 레이어로** 다시 그린다(08-13 실기: 프로필에서 아래
    /// 필드가 메뉴를 덮었다 — z순서는 그리는 순서가 전부다). `paint`도 그리지만
    /// (단독 사용 안전망), 컨테이너는 **모든 자식을 그린 뒤** 이걸 한 번 더 불러
    /// 팝업을 맨 위로 올린다.
    pub fn paint_popup(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        if self.ctx_menu.is_open() {
            self.ctx_menu.paint(ctx, theme);
        }
    }

    /// 멀티라인(소개글) 페인트(08-17) — 논리 줄을 위에서부터 여러 줄로 그린다.
    /// 캐럿 줄이 보이도록 세로 스크롤을 따라가고, 클릭→캐럿 변환용 줄 배치를 남긴다.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    fn paint_multiline(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.base.bounds;
        ctx.fill_round_rect(b, self.s(6), theme.field_bg);
        ctx.stroke_round_rect(b, self.s(6), theme.border, 1.0);
        if self.focus_ring {
            self.draw_focus_ring(ctx, theme, b);
        }
        ctx.select_font(FontSlot::Base, false);
        let th = ctx.text_height();
        let lh = self.line_h();
        let text = self.edit.text();
        // 줄번호 거터 폭 — 논리 줄 수의 자릿수 × 숫자 폭 + 여백(줄 수가 변해도 자릿수가 같으면 폭 불변).
        let logical_count = text.split('\n').count().max(1);
        // 표시 띠(4px) + 첫 글자 앞 여백(2px) = 거터에 6px 더(줄번호는 그만큼 왼쪽에 머문다).
        let mark_extra = if self.gutter_marks && self.line_numbers && self.multiline {
            self.s(6)
        } else {
            0
        };
        let gw = if self.line_numbers && self.multiline {
            let digits = logical_count.to_string().len().max(2) as i32;
            digits * ctx.text_width("0") + self.s(14) + mark_extra
        } else {
            0
        };
        self.gutter_px.set(gw);
        let tx = b.x + self.s(10) + gw + self.s(self.text_inset);
        let top0 = b.y + self.s(8);
        let avail = (b.right() - self.s(10) - tx).max(self.s(20));

        // 표시 텍스트 — 조합 중이면 캐럿 자리에 preedit를 끼워 그린다(편집 불변).
        let chars: Vec<char> = text.chars().collect();
        let caret_i = self.edit.caret().min(chars.len());
        let preedit_n = self.edit.preedit().chars().count();
        let display = if preedit_n == 0 {
            text
        } else {
            let before: String = chars[..caret_i].iter().collect();
            let after: String = chars[caret_i..].iter().collect();
            format!("{before}{}{after}", self.edit.preedit())
        };
        let disp_caret = caret_i + preedit_n;
        // ★ wrap = 논리 줄을 폭(avail)에 맞춰 소프트 행으로 접는다(공백 경계 선호).
        //   행이 (시작 char 인덱스, 문자열) 계약을 지키므로 선택 반전·히트테스트(line_lay)가
        //   그대로 따라온다.
        let lines: Vec<(usize, String)> = if self.wrap {
            let mut rows: Vec<(usize, String)> = Vec::new();
            for (lstart, lstr) in Self::logical_lines(&display) {
                let chars: Vec<char> = lstr.chars().collect();
                if chars.is_empty() {
                    rows.push((lstart, String::new()));
                    continue;
                }
                let mut wpx = Vec::new();
                ctx.text_prefix_widths(&lstr, &mut wpx);
                let mut row_start = 0usize;
                while row_start < chars.len() {
                    let base = wpx[row_start];
                    let mut end = row_start + 1;
                    while end < chars.len() && wpx[end + 1] - base <= avail {
                        end += 1;
                    }
                    let mut brk = end;
                    if end < chars.len() {
                        if let Some(sp) = chars[row_start..end].iter().rposition(|c| *c == ' ') {
                            if sp > 0 {
                                brk = row_start + sp + 1;
                            }
                        }
                    }
                    rows.push((lstart + row_start, chars[row_start..brk].iter().collect()));
                    row_start = brk;
                }
            }
            rows
        } else {
            Self::logical_lines(&display)
        };
        let caret_line = if self.wrap {
            // 캐럿이 속한 소프트 행 — 시작이 캐럿 이하인 마지막 행.
            lines
                .iter()
                .rposition(|(st, _)| *st <= disp_caret)
                .unwrap_or(0)
        } else {
            display
                .chars()
                .take(disp_caret)
                .filter(|&c| c == '\n')
                .count()
        };

        // 보이는 줄 수 + 세로 스크롤. 08-18: 사용자가 휠로 스크롤 중이면 vscroll을
        // 그대로 존중(자유 스크롤 · 캐럿 안 따라감). 아니면 캐럿을 따라간다.
        let rows = (((b.h - self.s(12)) / lh).max(1)) as usize;
        let max_top = lines.len().saturating_sub(rows);
        let mut top = self.vscroll.get();
        if self.ml_user_scrolled {
            top = top.min(max_top);
        } else {
            if caret_line < top {
                top = caret_line;
            } else if caret_line >= top + rows {
                top = caret_line + 1 - rows;
            }
            top = top.min(lines.len().saturating_sub(1));
        }
        self.vscroll.set(top);

        // 가로 스크롤(08-17) — 캐럿 열이 보이도록 따라간다(긴 줄·드래그 자동 스크롤).
        let (caret_start, caret_str) = &lines[caret_line.min(lines.len() - 1)];
        let caret_col = disp_caret.saturating_sub(*caret_start);
        let mut cw = Vec::new();
        ctx.text_prefix_widths(caret_str, &mut cw);
        let caret_px = cw.get(caret_col).copied().unwrap_or(0);
        // 콘텐츠 크기(스크롤바·클램프용) — 모든 줄의 최대 폭 + 총 높이. on_event가
        // 폰트를 못 재므로 여기서 실측해 캐시한다.
        let content_w = lines
            .iter()
            .map(|(_, s)| ctx.text_width(s))
            .max()
            .unwrap_or(0);
        let content_h = lines.len() as i32 * lh + self.s(16);
        self.ml_content.set((content_w, content_h));
        let max_hs = if self.wrap {
            0
        } else {
            (content_w - avail).max(0)
        };
        let mut hs = self.mhscroll.get();
        if self.ml_user_scrolled {
            // 사용자 스크롤(바/휠) — 캐럿 안 따라감. 콘텐츠 범위로만 클램프.
            hs = hs.clamp(0, max_hs);
        } else {
            // 캐럿 열이 보이도록 따라간다(편집 중).
            if caret_px - hs > avail {
                hs = caret_px - avail;
            }
            if caret_px - hs < 0 {
                hs = caret_px;
            }
            hs = hs.clamp(0, max_hs);
        }
        self.mhscroll.set(hs);

        // 빈 값 = placeholder(첫 줄).
        let empty = display.is_empty();
        let sel = if preedit_n == 0 {
            self.edit.selection()
        } else {
            None
        };
        // 다중 선택(Ctrl+D · 열 선택) — 그릴 구간 전부(빈 구간은 캐럿으로만 그린다).
        let sels: Vec<(usize, usize)> = if preedit_n == 0 {
            self.edit
                .regions()
                .into_iter()
                .filter(|(a, e)| e > a)
                .collect()
        } else {
            Vec::new()
        };
        // ★ 동일 출현 외곽선의 바늘: 주 선택(마지막 구간)의 글 — 한 줄 · 공백만이 아님 · 200자 이하.
        let needle: Option<Vec<char>> = if self.occurrence_hl && preedit_n == 0 {
            self.edit.selection().and_then(|(a, e)| {
                let n: Vec<char> = chars.get(a..e)?.to_vec();
                (n.len() <= 200
                    && !n.is_empty()
                    && !n.contains(&'\n')
                    && n.iter().any(|c| !c.is_whitespace()))
                .then_some(n)
            })
        } else {
            None
        };
        let carets: Vec<usize> = if preedit_n == 0 {
            self.edit.carets()
        } else {
            vec![disp_caret]
        };

        // 거터 배경·구분선(텍스트보다 먼저 · 가로 스크롤 무관).
        if gw > 0 {
            let gr = Rect::new(b.x + 1, b.y + 1, self.s(10) + gw - self.s(4), b.h - 2);
            ctx.fill_rect(gr, theme.panel_bg_alt);
            ctx.fill_rect(Rect::new(gr.right(), b.y + 1, 1, b.h - 2), theme.border);
        }
        // 소프트 행 → 논리 줄 번호(행 시작이 논리 줄 시작이면 번호 · 접힌 나머지 행은 빈칸).
        let logical_starts: Vec<usize> = Self::logical_lines(&display)
            .into_iter()
            .map(|(st, _)| st)
            .collect();
        let mut lay = self.line_lay.borrow_mut();
        lay.clear();
        let dx = tx - hs; // 가로 스크롤 반영 시작 x
                          // ★ 탭 원점 = 줄 시작 x — 한 줄을 색 구간마다 따로 그려도 탭 정지점이 줄 기준으로 맞는다(사용자 09-16 Golden 방식).
        ctx.set_tab_origin(Some(dx));
        let (vx0, vx1) = (tx, tx + avail); // 뷰포트(선택 반전 클립 범위)
                                           // 줄끝 표시용 전체 글자(eol 표시가 켜진 때만 수집 — 꺼진 기본은 비용 0).
        let eol_chars: Vec<char> =
            if self.whitespace.eol != '\0' && self.whitespace.mode != WhitespaceMode::None {
                display.chars().collect()
            } else {
                Vec::new()
            };
        // 세로 안내선(열 × 'M' 폭 · 뷰포트 안에서만).
        if self.rulers_show && !self.rulers.is_empty() {
            let cw = ctx.text_width("M").max(1);
            let rc = self.ruler_color.unwrap_or(theme.border);
            for &col in &self.rulers {
                let rx = dx + cw * col as i32;
                if rx >= vx0 && rx < vx1 {
                    ctx.fill_rect_alpha(Rect::new(rx, b.y + 1, 1, b.h - 2), rc, self.ruler_alpha);
                }
            }
        }
        // 구문 강조 상태(블록 주석)를 첫 표시 행까지 이어 온다.
        let mut hl_state = 0u32;
        let mut hl_spans: Vec<(usize, crate::highlight::TokenKind)> = Vec::new();
        if let Some(h) = &self.highlighter {
            for (_, l) in lines.iter().take(top) {
                hl_spans.clear();
                h.line_spans(l, &mut hl_state, &mut hl_spans);
            }
        }
        // 캐럿이 속한 표시 행(다중 캐럿 포함) — 시작이 캐럿 이하인 마지막 행.
        let caret_rows: Vec<(usize, usize)> = carets
            .iter()
            .map(|&c| (lines.iter().rposition(|(st, _)| *st <= c).unwrap_or(0), c))
            .collect();
        // ★ 픽셀 스크롤(nexa-sql 사용자 09-16): 휠 잔여 px(`ml_wheel_rem`)만큼 행을 위로 밀어 그린다 — 줄 단위 반올림은
        //   위/아래 반응이 비대칭이었다(내림이면 위로는 1px에 한 줄, 아래로는 한 줄 높이를 채워야). 캐럿 추종 중이거나
        //   맨 아래면 잔여를 버린다. 밀린 만큼 아래에 한 행을 더 그리고, 채움·캐럿·텍스트는 본문 영역으로 세로 클립.
        let rem = if self.ml_user_scrolled && top < max_top {
            self.ml_wheel_rem.get().clamp(0, lh - 1)
        } else {
            self.ml_wheel_rem.set(0);
            0
        };
        // 행 단위 모드: 잔여는 누적만 하고 그리기는 줄 경계에.
        let rem = if self.scroll_snap { 0 } else { rem };
        let (vy0, vy1) = (top0, b.y + b.h - self.s(4));
        let clipv = |r: Rect| -> Option<Rect> {
            let y0 = r.y.max(vy0);
            let y1 = (r.y + r.h).min(vy1);
            (y1 > y0).then(|| Rect::new(r.x, y0, r.w, y1 - y0))
        };
        let extra_row = usize::from(rem > 0);
        for (vi, li) in (top..lines.len().min(top + rows + extra_row)).enumerate() {
            let (start_idx, line_str) = &lines[li];
            let y = top0 + (vi as i32) * lh - rem;
            let line_len = line_str.chars().count();
            // 이 행이 선택에 걸리는가 — 줄번호를 선택 색으로 표시한다(여러 행 선택이 한눈에 · 사용자 09-15).
            let row_selected = sels.iter().any(|&(a, e)| {
                let (ls, le) = (*start_idx, *start_idx + line_len);
                a <= le && e >= ls
            });
            if gw > 0 {
                if row_selected {
                    // 선택 행 표시 — 거터 배경을 선택색으로(텍스트 선택 블록과 같은 색 계열).
                    if let Some(gr) = clipv(Rect::new(b.x + 1, y, self.s(10) + gw - self.s(4), lh))
                    {
                        ctx.fill_rect(
                            gr,
                            if self.base.focused {
                                theme.sel_bg
                            } else {
                                theme.sel_bg_inactive
                            },
                        );
                    }
                }
                if let Ok(n) = logical_starts.binary_search(start_idx) {
                    let num = (n + 1).to_string();
                    let nw = ctx.text_width(&num);
                    let gx = b.x + self.s(10) + gw - self.s(8) - mark_extra - nw;
                    // 표시 띠의 색 막대(줄번호와 경계선 사이 · 3px).
                    if mark_extra > 0 {
                        if let Some((_, c)) = self.line_marks.iter().find(|(l, _)| *l == n) {
                            let mx = b.x + self.s(10) + gw - self.s(4) - mark_extra + self.s(1);
                            if let Some(r) = clipv(Rect::new(mx, y + 1, self.s(3), lh - 2)) {
                                ctx.fill_rect(r, *c);
                            }
                        }
                    }
                    let is_caret_line = li == caret_line || row_selected;
                    ctx.text(
                        gx,
                        y,
                        Rect::new(b.x, vy0, self.s(10) + gw, vy1 - vy0),
                        &num,
                        if is_caret_line {
                            theme.text
                        } else {
                            theme.text_dim
                        },
                    );
                }
            }
            let view = clipv(Rect::new(tx, y, avail, lh)).unwrap_or(Rect::new(tx, y, avail, 0));
            let mut w = Vec::new();
            ctx.text_prefix_widths(line_str, &mut w);
            // 선택 반전(줄 범위와 겹치는 부분만 · 뷰포트로 클립 · 텍스트 아래 먼저 · 구간마다).
            for (a, e) in sels.iter().copied() {
                let (ls, le) = (*start_idx, *start_idx + line_len);
                let s0 = a.max(ls);
                let s1 = e.min(le);
                // 줄 넘김까지 선택에 들면(다음 행으로 이어짐) **글자 끝 + 한 칸**(줄바꿈 자리)까지만 채운다 —
                // 전폭이 아니라 텍스트 범위만 반전(Golden식 · 사용자 09-15 "끝의 공백이 더 선명히 보인다").
                // 높이는 행 피치(lh) 전체 — 행끼리 붙은 한 블록.
                let spans_next = e > le && li + 1 < lines.len() && a <= le;
                if s1 > s0 || spans_next {
                    let x0 = (dx + w.get(s0 - ls).copied().unwrap_or(0)).max(vx0);
                    let x1 = if spans_next {
                        let end = dx + w.get(line_len).copied().unwrap_or(0);
                        (end + ctx.text_width(" ")).min(vx1)
                    } else {
                        (dx + w.get(s1 - ls).copied().unwrap_or(0)).min(vx1)
                    };
                    if let (true, Some(r)) = (x1 > x0, clipv(Rect::new(x0, y, x1 - x0, lh))) {
                        ctx.fill_rect(
                            r,
                            if self.base.focused {
                                theme.sel_bg
                            } else {
                                theme.sel_bg_inactive
                            },
                        );
                    }
                }
            }
            // 찾기 일치 전부(반투명 채움 · 선택 아래 · 이 행과 겹치는 구간만 · T-73).
            if !self.find_marks.is_empty() {
                let (ls, le) = (*start_idx, *start_idx + line_len);
                for &(a, e) in &self.find_marks {
                    if e <= ls || a >= le {
                        continue;
                    }
                    let (s0, s1) = (a.max(ls) - ls, e.min(le) - ls);
                    if s1 <= s0 {
                        continue;
                    }
                    let x0 = (dx + w.get(s0).copied().unwrap_or(0)).max(vx0);
                    let x1 = (dx + w.get(s1).copied().unwrap_or(0)).min(vx1);
                    if let (true, Some(r)) = (x1 > x0, clipv(Rect::new(x0, y + 1, x1 - x0, lh - 2)))
                    {
                        ctx.fill_round_rect_alpha(r, self.s(2), theme.warn, 0.28);
                    }
                }
            }
            // 동일 출현 외곽선(선택에 든 출현은 채움이 이미 그려졌으니 건너뜀).
            if let Some(nd) = &needle {
                let lchars: Vec<char> = line_str.chars().collect();
                let n = nd.len();
                let ls = *start_idx;
                let mut i = 0usize;
                while i + n <= lchars.len() {
                    if lchars[i..i + n] == nd[..] {
                        let (a0, a1) = (ls + i, ls + i + n);
                        let in_sel = sels.iter().any(|(a, e)| *a < a1 && a0 < *e);
                        if !in_sel {
                            let x0 = dx + w.get(i).copied().unwrap_or(0);
                            let x1 = dx + w.get(i + n).copied().unwrap_or(0);
                            if x1 > x0 && x1 > vx0 && x0 < vx1 && y >= vy0 && y + lh <= vy1 {
                                ctx.stroke_round_rect_alpha(
                                    Rect::new(
                                        x0.max(vx0),
                                        y + 1,
                                        (x1.min(vx1) - x0.max(vx0)).max(1),
                                        lh - 2,
                                    ),
                                    self.s(2),
                                    theme.accent,
                                    1.0,
                                    0.7,
                                );
                            }
                        }
                        i += n;
                    } else {
                        i += 1;
                    }
                }
            }
            if empty && li == 0 {
                ctx.text(dx, y, view, &self.placeholder, theme.text_dim);
            } else if let Some(h) = &self.highlighter {
                hl_spans.clear();
                h.line_spans(line_str, &mut hl_state, &mut hl_spans);
                let lchars: Vec<char> = line_str.chars().collect();
                let mut ci = 0usize;
                for (n, k) in &hl_spans {
                    let end = (ci + n).min(lchars.len());
                    let sx = dx + w.get(ci).copied().unwrap_or(0);
                    if sx < vx1 && end > ci {
                        let seg: String = lchars[ci..end].iter().collect();
                        ctx.text(sx, y, view, &seg, k.color(theme));
                    }
                    ci = end;
                }
                if ci < lchars.len() {
                    let seg: String = lchars[ci..].iter().collect();
                    ctx.text(
                        dx + w.get(ci).copied().unwrap_or(0),
                        y,
                        view,
                        &seg,
                        theme.text,
                    );
                }
            } else {
                ctx.text(dx, y, view, line_str, theme.text);
            }
            // 공백 표시(·/→/¶ · 선택 안 또는 전체 · 반투명 = 배경과 섞은 색).
            if self.whitespace.mode != WhitespaceMode::None && !empty {
                let ws = &self.whitespace;
                let base = ws.color.unwrap_or(theme.text_dim);
                let col = base.lerp(theme.field_bg, 1.0 - ws.alpha.clamp(0.0, 1.0));
                let (ls, le) = (*start_idx, *start_idx + line_len);
                let (sa, se) = match (ws.mode, sel) {
                    (WhitespaceMode::All, _) => (ls, le + 1),
                    (WhitespaceMode::Selection, Some((a, e))) => (a, e),
                    _ => (0, 0),
                };
                let mut buf = [0u8; 4];
                for (ci, ch) in line_str.chars().enumerate() {
                    let idx = ls + ci;
                    if idx < sa || idx >= se {
                        continue;
                    }
                    let mark = match ch {
                        ' ' => ws.space,
                        '\t' => ws.tab,
                        _ => continue,
                    };
                    if mark == '\0' {
                        continue;
                    }
                    let mx = dx + w.get(ci).copied().unwrap_or(0);
                    if mx >= vx0 && mx < vx1 {
                        ctx.text(mx, y, view, mark.encode_utf8(&mut buf), col);
                    }
                }
                // 줄끝 표시 — 이 행이 논리 줄의 끝(다음 글자가 '\n')일 때.
                if ws.eol != '\0' && le >= sa && le < se && eol_chars.get(le) == Some(&'\n') {
                    let mx = dx + w.get(line_len).copied().unwrap_or(0);
                    if mx >= vx0 && mx < vx1 {
                        ctx.text(mx, y, view, ws.eol.encode_utf8(&mut buf), col);
                    }
                }
            }
            // 캐럿 — 이 줄에 있는 캐럿 전부(다중 커서 · 포커스·깜빡임 위상).
            if self.base.focused && ctx.caret_on() {
                for &(cl, c) in &caret_rows {
                    if cl != li {
                        continue;
                    }
                    let col = c.saturating_sub(*start_idx);
                    let cx = dx + w.get(col).copied().unwrap_or(0);
                    if cx >= vx0 && cx <= vx1 {
                        if let Some(r) = clipv(Rect::new(cx, y, self.s(2).max(2), th)) {
                            ctx.fill_rect(r, theme.text);
                        }
                    }
                }
            }
            lay.push(MlLine {
                top: y,
                start_idx: *start_idx,
                xs: w.iter().map(|px| dx + px).collect(),
            });
        }
        drop(lay);
        // 스크롤바 오버레이(08-18 · 대화 입력창과 동일) — 상하+좌우 · 자동 숨김.
        // content_w에 좌우 여백 s(20)을 더해 스크롤 범위를 max_hs와 맞춘다(끝 글자
        // 가림 수정 · on_event와 같은 값).
        self.ml_bars.paint(
            ctx,
            theme,
            b,
            (content_w + self.s(20) + gw).max(b.w),
            content_h.max(b.h),
            hs,
            (top as i32) * lh + rem,
            self.base.scale,
        );
        self.paint_popup(ctx, theme);
        ctx.set_tab_origin(None);
    }
}

impl Control for TextBox {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
}

impl Widget for TextBox {
    fn bounds(&self) -> Rect {
        self.base.bounds
    }

    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        self.base.bounds = bounds;
        inv.push(bounds);
    }

    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        // hover 목표(단일 행만) — 밝기는 `tick`이 옮긴다.
        if let InputEvent::MouseMove { x, y } = *ev {
            self.hover
                .set(!self.multiline && self.base.bounds.contains(Point { x, y }));
        }
        // 우클릭 편집 메뉴가 열려 있으면 가장 먼저 먹는다(팝업 최상위).
        if self.ctx_menu.is_open() {
            let menu_rect = self.ctx_menu.bounds();
            if self.ctx_menu.on_event(ev) {
                inv.push(menu_rect);
                inv.push(self.base.bounds);
                if let Some(a) = self.ctx_menu.take_action() {
                    use super::EditMenuAction as A;
                    match a {
                        // 전체 선택은 위젯 내부 상태 — 즉시 실행.
                        A::SelectAll => self.edit.key(EditKey::SelectAll, false),
                        // 클립보드는 호스트 몫 — 요청만 남긴다(⌘C/X/V와 같은 경로).
                        A::Copy => self.edit_ctx = Some(EditCtxAction::Copy),
                        A::Cut => self.edit_ctx = Some(EditCtxAction::Cut),
                        A::Paste => self.edit_ctx = Some(EditCtxAction::Paste),
                        A::Extra(_) => {}
                    }
                }
                return;
            }
        }
        if let InputEvent::RightDown { x, y } = *ev {
            if self.base.bounds.contains(Point { x, y }) {
                // 항목 구성·게이트·순서는 EditMenu 한 벌(M3-1e ① 1슬라이스).
                self.base.focused = true; // 우클릭도 포커스(메뉴 행동의 대상이 된다)
                let caps = super::EditMenuCaps {
                    has_sel: self.edit.selected_text().is_some(),
                    has_text: !self.edit.text().is_empty(),
                    clip_has_text: self.clip_has_text,
                };
                // 팝업이 박스 밖(아래)으로 펼쳐질 공간 — 박스 사각형만 주면 안에 구겨진다.
                let host = Rect::new(
                    self.base.bounds.x,
                    self.base.bounds.y,
                    self.base.bounds.w.max(self.s(200)),
                    self.base.bounds.h + self.s(140),
                );
                self.ctx_menu
                    .open_at(x, y, self.base.scale, host, caps, Vec::new());
                inv.push(self.base.bounds);
                inv.push(self.ctx_menu.bounds());
            }
            return;
        }
        // 멀티라인 스크롤바(08-18 · 대화 입력창과 동일) — 휠·HWheel·썸 드래그를
        // 먼저 먹는다. vp/콘텐츠 크기는 paint가 캐시한 값. 소비되면 텍스트 처리로
        // 흘리지 않는다(썸 드래그가 캐럿 이동으로 새지 않게).
        if self.multiline {
            let vp = self.base.bounds;
            let (cw, ch) = self.ml_content.get();
            let line_h = self.line_h();
            // ★ 스크롤바는 뷰포트를 vp.w로 보지만 실제 텍스트 뷰포트는 좌우 여백
            //   s(20)을 뺀 값이다(08-18 실기: 끝 ~2글자가 여백만큼 안 보였다).
            //   content_w에 그 여백을 더해 스크롤 범위를 paint의 max_hs와 맞춘다.
            let cw_bars = cw + self.s(20);
            // ★ 세로 오프셋은 줄 단위(`vscroll`)지만 휠은 px로 온다 — 줄로 반올림하고 남은 px를 버리면 트랙패드의
            //   느린 이동(사건당 1~3px)이 영원히 한 줄을 못 넘는다(nexa-sql 사용자 09-16). 잔여 px를 `ml_wheel_rem`에
            //   보관해 다음 휠 사건에 더한다 · 휠이 아닌 사건(썸 드래그·클릭)은 잔여를 버리고 가장 가까운 줄로.
            let is_wheel = matches!(ev, InputEvent::Wheel { .. } | InputEvent::HWheel { .. });
            let rem = if is_wheel { self.ml_wheel_rem.get() } else { 0 };
            let (nx, ny, consumed) = self.ml_bars.on_event(
                ev,
                vp,
                cw_bars.max(vp.w),
                ch.max(vp.h),
                self.mhscroll.get(),
                (self.vscroll.get() as i32) * line_h + rem,
                self.base.scale,
            );
            let mut moved = false;
            if nx != self.mhscroll.get() {
                self.mhscroll.set(nx.max(0));
                moved = true;
            }
            let lh = line_h.max(1);
            let nl = if is_wheel {
                let nl = ny.div_euclid(lh).max(0);
                let new_rem = ny - nl * lh;
                // 잔여 px만 바뀌어도 "스크롤했다"(픽셀 스크롤 · 캐럿 추종 해제 + 다시 그리기).
                if new_rem != rem {
                    moved = true;
                }
                self.ml_wheel_rem.set(new_rem);
                nl as usize
            } else {
                self.ml_wheel_rem.set(0);
                ((ny + lh / 2) / lh).max(0) as usize
            };
            if nl != self.vscroll.get() {
                self.vscroll.set(nl);
                moved = true;
            }
            if moved {
                self.ml_user_scrolled = true;
                inv.push(vp);
            }
            if consumed {
                return;
            }
        }
        match *ev {
            InputEvent::MouseDown { x, y, shift, .. } => {
                let badge = self.help_badge_rect(self.base.bounds);
                if self.handle_help_click(x, y, badge) {
                    inv.push(self.base.bounds);
                    return;
                }
                // ×(지우기) — 값이 있을 때만. 클릭 = 초기화 + 변경 보고.
                if self.clearable
                    && !self.edit.text().is_empty()
                    && self.clear_rect().contains(Point { x, y })
                {
                    self.edit.set_text("");
                    self.changed = true;
                    inv.push(self.base.bounds);
                    return;
                }
                // 멀티라인(08-17) — (x,y)로 줄·열을 함께 잡는다(세로 매핑).
                // 조합 중(preedit)엔 배치 인덱스가 표시 텍스트 기준이라 클릭 재배치는
                // 건너뛰고 포커스만(조합은 곧 확정된다).
                if self.multiline && self.base.bounds.contains(Point { x, y }) {
                    self.base.focused = true;
                    self.ml_user_scrolled = false; // 클릭 = 캐럿 이동 → 캐럿 추종 재개
                                                   // ★ Alt+Shift 드래그 = 열(블록) 선택 · 다중 커서(Sublime · 사용자 09-15).
                    if self.column_mode {
                        self.col_anchor = Some((x, y));
                        self.dragging = true;
                        self.edit.set_caret(self.ml_caret_at(x, y), false);
                        self.last_click = (0, 0);
                        inv.push(self.base.bounds);
                        return;
                    }
                    if self.edit.preedit().is_empty() {
                        let idx = self.ml_caret_at(x, y);
                        // ★ 더블 = 단어 · 트리플 = **논리 줄**(09-02 사용자 요청 — 단일 줄과
                        //   같은 체인 규약: 같은 위치 연속 클릭만 잇고 ⇧는 제외).
                        let chain = self.click_chain_alive();
                        self.last_click.1 = if shift {
                            0
                        } else if chain && self.last_click.0 == idx && self.last_click.1 > 0 {
                            if self.last_click.1 >= 3 {
                                1
                            } else {
                                self.last_click.1 + 1
                            }
                        } else {
                            1
                        };
                        self.last_click.0 = idx;
                        match self.last_click.1 {
                            2 => self.select_word_at(idx),
                            3 => {
                                let text = self.edit.text();
                                let chars: Vec<char> = text.chars().collect();
                                let i = idx.min(chars.len());
                                let start = chars[..i]
                                    .iter()
                                    .rposition(|&c| c == '\n')
                                    .map_or(0, |p| p + 1);
                                let end = start
                                    + chars[start..]
                                        .iter()
                                        .position(|&c| c == '\n')
                                        .unwrap_or(chars.len() - start);
                                self.edit.set_caret(start, false);
                                self.edit.set_caret(end, true);
                            }
                            _ => {
                                self.edit.set_caret(idx, shift);
                                self.dragging = true;
                            }
                        }
                    }
                    inv.push(self.base.bounds);
                    return;
                }
                if self.base.bounds.contains(Point { x, y }) {
                    self.base.focused = true;
                    // 클릭 지점으로 캐럿 이동 + 드래그 선택 시작(기본 텍스트 동작).
                    let idx = self.caret_at_x(x);
                    // 연속 클릭은 **같은 캐럿 위치일 때만** 잇는다(08-13 실기 — 위치 무관
                    // 누적이라 두 번째 단일 클릭이 단어 선택이 돼 캐럿 재배치가 불가능했다).
                    // Shift+클릭은 언제나 선택 확장 — 더블클릭 체인에 넣지 않는다.
                    let chain = self.click_chain_alive();
                    self.last_click.1 = if shift {
                        0
                    } else if chain && self.last_click.0 == idx && self.last_click.1 > 0 {
                        if self.last_click.1 >= 3 {
                            1
                        } else {
                            self.last_click.1 + 1
                        }
                    } else {
                        1
                    };
                    self.last_click.0 = idx;
                    match self.last_click.1 {
                        2 => self.select_word_at(idx),                 // 더블 = 단어
                        3 => self.edit.key(EditKey::SelectAll, false), // 트리플 = 전체
                        _ => {
                            self.edit.set_caret(idx, shift);
                            self.dragging = true;
                        }
                    }
                    inv.push(self.base.bounds);
                }
            }
            // 열 선택 드래그 — 줄마다 같은 x 구간(폭 0이면 캐럿만).
            InputEvent::MouseMove { x, y } if self.dragging && self.col_anchor.is_some() => {
                if let Some((ax, ay)) = self.col_anchor {
                    let regions = self.column_regions(ax, ay, x, y);
                    if !regions.is_empty() {
                        self.edit.set_regions(&regions);
                    }
                }
                inv.push(self.base.bounds);
            }
            InputEvent::MouseMove { x, y } if self.dragging && self.multiline => {
                // 멀티라인 드래그 자동 스크롤(08-17) — 상/하 밖 = 줄 단위 세로 이동
                // (vscroll이 따라온다), 좌/우 밖 = 한 글자 가로 이동(mhscroll이 따라온다).
                let b = self.base.bounds;
                // 첫/마지막 줄에서 위/아래로 끌면 **본문 처음/끝까지**(Sublime·VS Code 관례 · nexa-sql 09-15:
                // 첫 줄 앞 글자 몇 개가 빠진 채 선택되던 문제 — 줄 이동만으로는 열이 유지됐다).
                let text = self.edit.text();
                let caret = self.edit.caret();
                let on_first = !text.chars().take(caret).any(|c| c == '\n');
                let on_last = !text.chars().skip(caret).any(|c| c == '\n');
                if y < b.y {
                    if on_first {
                        self.edit.set_caret(0, true);
                    } else {
                        self.ml_move_vert(false, true);
                    }
                } else if y > b.bottom() {
                    if on_last {
                        let n = text.chars().count();
                        self.edit.set_caret(n, true);
                    } else {
                        self.ml_move_vert(true, true);
                    }
                } else if x > b.right() - self.s(8) {
                    self.edit.key(EditKey::Right, true);
                } else if x < b.x + self.s(8) {
                    self.edit.key(EditKey::Left, true);
                } else {
                    self.edit.set_caret(self.ml_caret_at(x, y), true);
                }
                inv.push(b);
            }
            InputEvent::MouseMove { x, .. } if self.dragging => {
                // 영역 밖으로 끌면 **한 글자씩 자동 진행**(① 08-13) — 페인트가 캐럿을
                // 따라 스크롤하므로, 마우스를 밖에 둔 채 움직이면 계속 밀린다.
                let idx = if x > self.base.bounds.right() {
                    self.edit.caret().saturating_add(1)
                } else if x < self.base.bounds.x {
                    self.edit.caret().saturating_sub(1)
                } else {
                    self.caret_at_x(x)
                };
                self.edit.set_caret(idx, true); // 앵커 유지 = 범위 확장
                inv.push(self.base.bounds);
            }
            InputEvent::MouseUp { .. } => {
                self.dragging = false;
                self.col_anchor = None;
            }
            InputEvent::Char { c, .. } if self.base.focused => {
                self.last_click.1 = 0; // 타이핑 = 클릭 체인 끊김(클릭-타이핑-클릭 ≠ 더블클릭)
                self.ml_user_scrolled = false; // 타이핑 = 캐럿 이동 → 캐럿 추종 재개
                if c == '\u{8}' {
                    self.edit.backspace();
                } else if c == '\t' && self.multiline && self.edit.selection_spans_lines() {
                    // 여러 줄 선택 + Tab = 블록 들여쓰기(Sublime · 09-16). Shift+Tab(내어쓰기)은 호스트 키맵이 명령으로 보낸다.
                    self.edit_command(EditCommand::Indent);
                } else if c == '\t' && self.multiline && self.room() > 0 {
                    // 멀티라인(편집기)은 Tab = 탭 문자 또는 다음 탭 정지까지 공백(`set_indent` · 09-15) — 단일 행은 호스트가 포커스 이동에 쓴다.
                    if self.indent_spaces {
                        let ts = usize::from(self.tab_size.max(1));
                        let n = Self::tab_cells(ts, self.caret_column(), self.tab_stops);
                        self.edit.insert_str(&" ".repeat(n));
                    } else {
                        self.edit.insert('\t');
                    }
                } else if !c.is_control() && self.accepts(c) && self.room() > 0 {
                    self.edit.insert(c);
                }
                self.changed = true;
                inv.push(self.base.bounds);
            }
            InputEvent::Key {
                key,
                shift,
                primary,
            } if self.base.focused => {
                self.last_click.1 = 0; // 키 개입 = 클릭 체인 끊김
                self.dragging = false; // 키 입력 = 드래그 끝(MouseUp을 못 받은 경우 방어 · nexa-sql 09-15)
                self.ml_user_scrolled = false; // 키 이동/편집 = 캐럿 이동 → 캐럿 추종 재개
                match key {
                    Key::Enter => {
                        // 멀티라인은 Enter = 개행(확정은 상위의 적용 버튼 몫).
                        if self.multiline {
                            self.edit.insert('\n');
                            self.changed = true;
                        } else {
                            self.committed = true;
                        }
                        inv.push(self.base.bounds);
                    }
                    // 멀티라인 세로 이동(08-17) — 같은 열을 목표로 위/아래 줄.
                    Key::Up if self.multiline => {
                        self.ml_move_vert(false, shift);
                        inv.push(self.base.bounds);
                    }
                    Key::Down if self.multiline => {
                        self.ml_move_vert(true, shift);
                        inv.push(self.base.bounds);
                    }
                    // ⌘/Ctrl+←/→ = 줄 처음/끝(mac 관례 · DR-16 — 08-13 전수 검사).
                    // 멀티라인은 **현재 줄**의 처음/끝(단일 라인은 버퍼 전체).
                    // 이동·선택 키는 **다시 그리기를 요청해야** 캐럿·선택 반전이 보인다
                    // (08-13 실기 — 프로필 필드에서 ←/→·Shift+←·⌘A가 무반응으로 보였다).
                    Key::Left if primary => {
                        if self.multiline {
                            self.edit.set_caret(self.ml_line_edge(false), shift);
                        } else {
                            self.edit.key(EditKey::Home, shift);
                        }
                        inv.push(self.base.bounds);
                    }
                    Key::Right if primary => {
                        if self.multiline {
                            self.edit.set_caret(self.ml_line_edge(true), shift);
                        } else {
                            self.edit.key(EditKey::End, shift);
                        }
                        inv.push(self.base.bounds);
                    }
                    Key::Left => {
                        self.edit.key(EditKey::Left, shift);
                        inv.push(self.base.bounds);
                    }
                    Key::Right => {
                        self.edit.key(EditKey::Right, shift);
                        inv.push(self.base.bounds);
                    }
                    Key::Home => {
                        if self.multiline {
                            self.edit.set_caret(self.ml_line_edge(false), shift);
                        } else {
                            self.edit.key(EditKey::Home, shift);
                        }
                        inv.push(self.base.bounds);
                    }
                    Key::End => {
                        if self.multiline {
                            self.edit.set_caret(self.ml_line_edge(true), shift);
                        } else {
                            self.edit.key(EditKey::End, shift);
                        }
                        inv.push(self.base.bounds);
                    }
                    Key::Delete => {
                        self.edit.key(EditKey::DeleteForward, false);
                        self.changed = true;
                        inv.push(self.base.bounds);
                    }
                    // Esc = 다중 선택 접기(Sublime).
                    Key::Escape if self.edit.has_multi() => {
                        self.edit.clear_multi();
                        inv.push(self.base.bounds);
                    }
                    _ => {}
                }
            }
            InputEvent::SelectAll if self.base.focused => {
                self.edit.key(EditKey::SelectAll, false);
                inv.push(self.base.bounds); // 선택 반전이 즉시 보여야 한다
            }
            InputEvent::Undo if self.base.focused && self.edit.undo() => {
                self.changed = true;
                self.ml_user_scrolled = false;
                inv.push(self.base.bounds);
            }
            InputEvent::Redo if self.base.focused && self.edit.redo() => {
                self.changed = true;
                self.ml_user_scrolled = false;
                inv.push(self.base.bounds);
            }
            _ => {}
        }
    }

    fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        // ★ 탭 폭은 **이 상자의 설정**으로 그린다(탭마다 다를 수 있다 · nexa-sql 09-15) —
        //   측정·그리기·캐럿이 같은 값을 보도록 페인트 진입에서 주입한다.
        nexa_gfx::text::set_tab_cols(u32::from(self.tab_size.max(1)));
        nexa_gfx::text::set_tab_stops(self.tab_stops);
        if self.multiline {
            self.paint_multiline(ctx, theme);
            return;
        }
        let b = self.base.bounds;
        ctx.fill_round_rect(b, self.s(6), theme.field_bg);
        // hover — 전경색(회색 계열)을 진행도만큼 얹는다(서서히 진해짐 · 색을 새로 만들지 않는다).
        let hov = crate::tokens::hover_alpha(false, self.hover.value());
        if hov > 0.0 {
            ctx.fill_round_rect_alpha(b, self.s(6), theme.text, hov);
        }
        ctx.stroke_round_rect(b, self.s(6), theme.border, 1.0);
        if self.focus_ring {
            self.draw_focus_ring(ctx, theme, b);
        }

        let cy = b.y + b.h / 2;
        let s16 = self.s(16);
        ctx.select_font(FontSlot::Base, false);
        let ty = cy - ctx.text_height() / 2;
        // 선행 이미지(있으면) — placeholder·텍스트·캐럿의 시작 x를 그 뒤로 민다.
        let mut tx = b.x + self.s(10);
        if let Some(img) = self.image.as_deref() {
            let boxr = Rect::new(tx, cy - s16 / 2, s16, s16);
            let fit = image_fit_contain(boxr, img.w as i32, img.h as i32);
            ctx.image_scaled(fit, img, b);
            tx += s16 + self.s(6);
        }

        // 텍스트/placeholder는 고정 시작점(tx)에서 그리되, 폭을 넘으면 **가로 스크롤**로
        // 캐럿을 따라간다(① 08-13 — 그전엔 긴 텍스트에서 캐럿이 화면 밖으로 사라졌다).
        ctx.select_font(FontSlot::Base, false);
        // ★ 마스킹(09-03) — 값·preedit을 ●로 치환한 **표시 문자열**로 재고 그린다:
        //   폭 측정·캐럿·히트테스트가 전부 같은 문자열을 쓰므로 좌표가 어긋나지 않는다.
        let mask = |n: usize| "•".repeat(n);
        let raw_text = self.edit.text();
        let text = if self.masked {
            mask(raw_text.chars().count())
        } else {
            raw_text
        };
        let chars: Vec<char> = text.chars().collect();
        let caret_i = self.edit.caret().min(chars.len());
        let before: String = chars[..caret_i].iter().collect();
        let preedit_disp = if self.masked {
            mask(self.edit.preedit().chars().count())
        } else {
            self.edit.preedit().to_string()
        };
        // 조합 중 문자열(preedit)은 캐럿 자리에 끼워 **표시만** 한다(편집 상태 불변).
        let shown = if preedit_disp.is_empty() {
            text.clone()
        } else {
            let after: String = chars[caret_i..].iter().collect();
            format!("{before}{preedit_disp}{after}")
        };
        // 문자 경계 누적 폭 — 단일 패스(08-14 성능 · 값은 접두사 재측정과 동일 계약.
        // 캐럿 깜빡임이 포커스 창을 상시 리페인트해 매 프레임 O(n²) 측정이 비쌌다).
        let mut w = Vec::new();
        ctx.text_prefix_widths(&text, &mut w);
        let pre_start_px = w.get(caret_i).copied().unwrap_or(0);
        let caret_px = pre_start_px + ctx.text_width(&preedit_disp); // 조합 뒤가 캐럿
        let total_px = if preedit_disp.is_empty() {
            w.last().copied().unwrap_or(0) // shown == text — 같은 값(계약)
        } else {
            ctx.text_width(&shown) // 조합 중 한정 — 종전 그대로
        };
        // 가용 폭 — 우측 여백(×·도움말 배지 자리)을 뺀다.
        let avail = (b.right() - self.s(24) - tx).max(self.s(20));
        let mut hs = self.hscroll.get();
        if total_px <= avail {
            hs = 0; // 다 들어가면 스크롤 없음
        } else {
            hs = hs.clamp(0, total_px - avail); // 텍스트가 줄면 빈 공간이 남지 않게
            if caret_px - hs > avail {
                hs = caret_px - avail; // 캐럿이 오른쪽 밖 → 따라간다
            }
            if caret_px - hs < 0 {
                hs = caret_px; // 캐럿이 왼쪽 밖
            }
        }
        self.hscroll.set(hs);
        // 텍스트 뷰포트(스크롤 전 시작 ~ 가용 폭 끝) — fill_rect는 클립을 모르므로
        // 선택 반전·밑줄은 이 범위로 **직접 잘라** 그린다(08-13 실기: 하이라이트가
        // 좌우로 삐져나왔다). ★ 글자도 **같은 뷰포트로** 잘라야 한다(08-14 실기:
        // 글자는 상자 전체(b)로 잘라 우측 여백(×·배지 자리) 아래까지 보이는데
        // 하이라이트만 뷰포트에서 멈춰 "오른쪽 2글자 반전 누락"으로 보였다 —
        // 둘의 클립이 다르면 어느 쪽이 맞아도 어긋나 보인다).
        let (view_x0, view_x1) = (tx, tx + avail);
        let view = Rect::new(view_x0, b.y, avail, b.h);
        let tx = tx - hs;
        // 문자 경계 x(화면 좌표 — 스크롤 반영)를 남긴다 — 클릭→캐럿 변환 근거.
        {
            self.text_x.set(tx);
            let mut xs = self.caret_xs.borrow_mut();
            xs.clear();
            xs.extend(w.iter().map(|px| tx + px));
        }
        // 선택 반전(08-13 전수 검사: 선택은 되는데 하이라이트가 안 보였다) —
        // 텍스트보다 먼저 채워야 글자가 위에 얹힌다. preedit 중엔 선택이 없다.
        if let Some((a, b_end)) = self.edit.selection() {
            let mid: String = chars[a.min(chars.len())..b_end.min(chars.len())]
                .iter()
                .collect();
            let wp = w.get(a.min(chars.len())).copied().unwrap_or(0); // 접두사 폭(누적 재사용)
            let we = w.get(b_end.min(chars.len())).copied().unwrap_or(wp);
            let _ = &mid;
            let x0 = (tx + wp).max(view_x0);
            let x1 = (tx + we).min(view_x1);
            let th = ctx.text_height();
            if x1 > x0 {
                ctx.fill_rect(
                    Rect::new(x0, ty, x1 - x0, th),
                    if self.base.focused {
                        theme.sel_bg
                    } else {
                        theme.sel_bg_inactive
                    },
                );
            }
        }
        if shown.is_empty() {
            ctx.text(tx, ty, view, &self.placeholder, theme.text_dim);
        } else {
            ctx.text(tx, ty, view, &shown, theme.text);
        }
        // 조합 구간 밑줄 — "여기가 아직 확정 전"임을 대화 입력과 같은 문법으로 표시.
        if !self.edit.preedit().is_empty() {
            let th = ctx.text_height();
            let ux0 = (tx + pre_start_px).max(view_x0);
            let ux1 = (tx + caret_px).min(view_x1).max(ux0 + self.s(4));
            ctx.fill_rect(
                Rect::new(ux0, ty + th, ux1 - ux0, self.s(1).max(1)),
                theme.text_dim,
            );
        }

        // 캐럿은 **별도 세로 막대**로 그린다(문자열에 '|'를 끼워 넣지 않음 → 위치 고정).
        // 깜빡임 위상(caret_on)은 호스트가 주입한다(08-13 — 포커스 창에서만 점멸).
        if self.base.focused && ctx.caret_on() {
            let cx = tx + caret_px;
            // 캐럿 높이 = 실측 텍스트 높이(고정 16 근사는 고배율에서 반토막으로 보였다).
            let th = ctx.text_height();
            ctx.fill_rect(Rect::new(cx, ty, self.s(2).max(2), th), theme.text);
        }

        // ×(지우기) — 값이 있을 때만(원 배경 없이 × 두 획 · text_dim).
        if self.clearable && !text.is_empty() {
            let r = self.clear_rect();
            let m = self.s(4);
            let (x0, y0, x1, y1) = (r.x + m, r.y + m, r.right() - m, r.bottom() - m);
            ctx.polyline(
                &[(x0, y0), (x1, y1)],
                theme.text_dim,
                self.s(1).max(1) as f32 + 0.5,
            );
            ctx.polyline(
                &[(x0, y1), (x1, y0)],
                theme.text_dim,
                self.s(1).max(1) as f32 + 0.5,
            );
        }

        let badge = self.help_badge_rect(b);
        self.draw_help_badge(ctx, theme, badge);
        self.draw_help_tip(ctx, theme, badge);

        // 우클릭 편집 메뉴 — 이 위젯 안에서는 최상위. 형제 위젯이 뒤에 그려지는
        // 컨테이너에선 부족하다 — 컨테이너가 `paint_popup`을 끝에 한 번 더 부른다.
        self.paint_popup(ctx, theme);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tb() -> (TextBox, Invalidations) {
        let mut t = TextBox::new("Run command");
        let mut inv = Invalidations::default();
        t.set_bounds(Rect::new(0, 0, 260, 30), &mut inv);
        (t, inv)
    }
    fn ch(c: char) -> InputEvent {
        InputEvent::Char { c, now_ms: 0 }
    }
    fn click(x: i32, y: i32) -> InputEvent {
        InputEvent::MouseDown {
            x,
            y,
            shift: false,
            primary: false,
        }
    }

    /// 멀티라인(08-17) — Enter가 개행, 세로 이동이 같은 열을 목표로 한다.
    #[test]
    fn multiline_enter_inserts_newline_and_vertical_move() {
        let mut t = TextBox::new("bio").with_multiline();
        let mut inv = Invalidations::default();
        t.set_bounds(Rect::new(0, 0, 200, 80), &mut inv);
        t.set_focused(true);
        let key = |k| InputEvent::Key {
            key: k,
            shift: false,
            primary: false,
        };
        for c in "ab".chars() {
            t.on_event(&ch(c), &mut inv);
        }
        t.on_event(&key(Key::Enter), &mut inv); // 개행(확정 아님)
        for c in "cd".chars() {
            t.on_event(&ch(c), &mut inv);
        }
        assert_eq!(t.text(), "ab\ncd", "Enter = 개행");
        assert!(
            t.take_committed().is_none(),
            "멀티라인은 Enter로 확정하지 않는다"
        );
        // 캐럿은 2번째 줄 끝(열 2) — Up이면 1번째 줄 같은 열(끝, 열 2).
        t.on_event(&key(Key::Up), &mut inv);
        for c in "X".chars() {
            t.on_event(&ch(c), &mut inv);
        }
        assert_eq!(t.text(), "abX\ncd", "Up = 윗줄 같은 열");
    }

    /// 논리 줄 분해 — 첫 글자 char 인덱스가 정확해야 클릭 매핑이 맞는다.
    #[test]
    fn logical_lines_indices() {
        let v = TextBox::logical_lines("ab\ncde\n");
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], (0, "ab".to_string()));
        assert_eq!(v[1], (3, "cde".to_string()));
        assert_eq!(v[2], (7, String::new()));
    }

    #[test]
    fn primary_arrows_jump_line_edges() {
        let (mut t, mut inv) = tb();
        t.on_event(&click(5, 15), &mut inv);
        for c in "abc".chars() {
            t.on_event(&ch(c), &mut inv);
        }
        let key = |key, shift, primary| InputEvent::Key {
            key,
            shift,
            primary,
        };
        t.on_event(&key(Key::Left, false, true), &mut inv); // ⌘← = 줄 처음
        t.on_event(&ch('x'), &mut inv);
        assert_eq!(t.text(), "xabc", "⌘/Ctrl+← = 줄 처음(삽입 위치로 검증)");
        t.on_event(&key(Key::Right, false, true), &mut inv); // ⌘→ = 줄 끝
        t.on_event(&ch('z'), &mut inv);
        assert_eq!(t.text(), "xabcz", "⌘/Ctrl+→ = 줄 끝");
        t.on_event(&key(Key::Left, false, true), &mut inv);
        t.on_event(&key(Key::Right, true, true), &mut inv); // ⌘⇧→ = 끝까지 선택
        assert_eq!(
            t.copy_selection().as_deref(),
            Some("xabcz"),
            "⇧ 조합 = 범위 선택"
        );
    }

    /// H-25 — 조합 시작(첫 프리에딧)이 선택을 대체한다(선택 삭제 → 확정 합류).
    #[test]
    fn preedit_start_replaces_selection() {
        let (mut t, mut inv) = tb();
        t.on_event(&click(5, 15), &mut inv);
        for c in "abc".chars() {
            t.on_event(&ch(c), &mut inv);
        }
        t.on_event(&InputEvent::SelectAll, &mut inv);
        t.set_preedit("나", &mut inv); // 조합 시작 = 선택 즉시 삭제
        assert_eq!(t.text(), "", "선택분 제거(표시엔 조합 밑줄만)");
        t.set_preedit("", &mut inv);
        t.on_event(&ch('나'), &mut inv); // 확정 문자 합류(호스트 라우팅 모사)
        assert_eq!(t.text(), "나", "선택이 조합으로 대체됐다");
    }

    #[test]
    fn typing_requires_focus_and_reports_change() {
        let (mut t, mut inv) = tb();
        t.on_event(&ch('a'), &mut inv);
        assert_eq!(t.text(), "", "비포커스 = 무입력");
        t.on_event(&click(5, 15), &mut inv);
        assert!(t.is_focused());
        for c in "git".chars() {
            t.on_event(&ch(c), &mut inv);
        }
        assert_eq!(t.text(), "git");
        assert_eq!(t.take_changed().as_deref(), Some("git"));
    }

    #[test]
    fn enter_commits_once() {
        let (mut t, mut inv) = tb();
        t.on_event(&click(5, 15), &mut inv);
        for c in "hi".chars() {
            t.on_event(&ch(c), &mut inv);
        }
        t.on_event(
            &InputEvent::Key {
                key: Key::Enter,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(t.take_committed().as_deref(), Some("hi"));
        assert!(t.take_committed().is_none(), "1회성");
    }

    #[test]
    fn placeholder_present_until_typed() {
        let (t, _) = tb();
        assert_eq!(t.text(), "");
        // placeholder는 렌더 전용 — 텍스트 값에는 포함되지 않는다.
    }

    /// 페인트를 한 번 태워 문자 경계 실측을 채운다(클릭→캐럿 변환의 전제).
    fn measure(t: &TextBox) {
        use crate::controls::ProbeCtx;
        let mut probe = ProbeCtx;
        let theme = crate::theme::Theme::dark();
        t.paint(&mut probe, &theme);
    }

    #[test]
    fn select_all_selects_everything() {
        let (mut t, mut inv) = tb();
        t.set_text("hello world");
        t.base.focused = true;
        t.on_event(&InputEvent::SelectAll, &mut inv);
        assert_eq!(t.edit.selected_text().as_deref(), Some("hello world"));
    }

    #[test]
    fn double_click_selects_word_triple_selects_all() {
        let (mut t, mut inv) = tb();
        t.set_text("alpha beta");
        measure(&t);
        // 같은 자리 두 번 = 단어.
        t.on_event(&click(5, 15), &mut inv);
        t.on_event(&click(5, 15), &mut inv);
        assert_eq!(t.edit.selected_text().as_deref(), Some("alpha"));
        // 세 번째 = 전체.
        t.on_event(&click(5, 15), &mut inv);
        assert_eq!(t.edit.selected_text().as_deref(), Some("alpha beta"));
    }

    #[test]
    fn second_click_elsewhere_moves_caret_without_selecting() {
        // 08-13 실기 — 위치 무관 클릭 누적이라 두 번째 단일 클릭이 단어 선택이 됐다.
        let (mut t, mut inv) = tb();
        t.set_text("alpha beta");
        measure(&t);
        t.on_event(&click(5, 15), &mut inv); // 앞쪽
        let far = *t.caret_xs.borrow().last().unwrap(); // 맨 끝 경계
        t.on_event(&click(far, 15), &mut inv); // 다른 위치 = 새 체인
        assert!(
            t.edit.selected_text().is_none(),
            "다른 위치의 두 번째 클릭은 캐럿 이동이지 단어 선택이 아니다"
        );
        assert_eq!(t.edit.caret(), 10, "캐럿은 클릭 지점으로");
    }

    #[test]
    fn shift_click_extends_selection_not_word_select() {
        let (mut t, mut inv) = tb();
        t.set_text("alpha beta");
        measure(&t);
        t.on_event(&click(5, 15), &mut inv); // 캐럿 0
        let far = *t.caret_xs.borrow().last().unwrap();
        t.on_event(
            &InputEvent::MouseDown {
                x: far,
                y: 15,
                shift: true,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(
            t.edit.selected_text().as_deref(),
            Some("alpha beta"),
            "Shift+클릭 = 앵커에서 클릭 지점까지 확장"
        );
    }

    #[test]
    fn typing_breaks_click_chain() {
        let (mut t, mut inv) = tb();
        t.set_text("alpha");
        measure(&t);
        t.on_event(&click(5, 15), &mut inv);
        t.on_event(&ch('x'), &mut inv); // 키 개입 = 체인 끊김
        measure(&t);
        t.on_event(&click(5, 15), &mut inv);
        assert!(
            t.edit.selected_text().is_none(),
            "클릭-타이핑-클릭은 더블클릭이 아니다"
        );
    }

    #[test]
    fn movement_and_select_keys_request_repaint() {
        // 08-13 실기 — 이동·선택 키가 무효화를 안 밀어 프로필 필드에서
        // ←/→·Shift+←·⌘A가 무반응(화면 불변)으로 보였다.
        let (mut t, _) = tb();
        t.set_text("abc");
        t.base.focused = true;
        let key = |key, shift, primary| InputEvent::Key {
            key,
            shift,
            primary,
        };
        let mut inv = Invalidations::default();
        t.on_event(&key(Key::Left, true, false), &mut inv);
        assert!(!inv.is_empty(), "Shift+← = 다시 그리기 요청");
        let mut inv = Invalidations::default();
        t.on_event(&InputEvent::SelectAll, &mut inv);
        assert!(!inv.is_empty(), "⌘/Ctrl+A = 다시 그리기 요청");
        assert_eq!(t.edit.selected_text().as_deref(), Some("abc"));
    }

    #[test]
    fn display_text_includes_preedit_at_caret() {
        // 아바타 미리보기 등은 "화면에 보이는 그대로"를 써야 한다(08-13 실기).
        let (mut t, mut inv) = tb();
        t.set_text("나");
        t.base.focused = true;
        t.set_preedit("다", &mut inv);
        assert_eq!(t.display_text(), "나다", "조합 중 글자 포함");
        assert_eq!(t.text(), "나", "편집 상태(버퍼)는 불변");
        t.set_preedit("", &mut inv);
        assert_eq!(t.display_text(), "나", "소거 후엔 버퍼 그대로");
    }

    #[test]
    fn delete_key_removes_forward() {
        let (mut t, mut inv) = tb();
        t.set_text("abc");
        t.base.focused = true;
        t.edit.set_caret(0, false);
        t.on_event(
            &InputEvent::Key {
                key: Key::Delete,
                shift: false,
                primary: false,
            },
            &mut inv,
        );
        assert_eq!(t.text(), "bc");
    }

    #[test]
    fn clear_button_resets_and_reports() {
        let mut t = TextBox::new("Search").with_clearable();
        let mut inv = Invalidations::default();
        t.set_bounds(Rect::new(0, 0, 260, 30), &mut inv);
        t.set_text("abc");
        let r = t.clear_rect();
        t.on_event(&click(r.x + 3, r.y + 3), &mut inv);
        assert_eq!(t.text(), "", "× = 초기화");
        assert_eq!(t.take_changed().as_deref(), Some(""), "변경 보고");
        // 값이 없으면 × 영역 클릭은 일반 포커스 클릭.
        t.on_event(&click(r.x + 3, r.y + 3), &mut inv);
        assert_eq!(t.text(), "");
        assert!(t.is_focused());
    }

    #[test]
    fn backspace_edits() {
        let (mut t, mut inv) = tb();
        t.set_text("abc");
        t.base.focused = true;
        t.on_event(&ch('\u{8}'), &mut inv);
        assert_eq!(t.text(), "ab");
    }

    #[test]
    fn clipboard_copy_cut_paste_roundtrip() {
        // ① 08-13 — 모든 텍스트 컨트롤의 기본 에디터 기능(클립보드는 호스트가 잇는다).
        let (mut t, mut inv) = tb();
        t.set_text("hello world");
        t.base.focused = true;
        t.edit.set_selection(0, 5);
        assert_eq!(t.copy_selection().as_deref(), Some("hello"), "복사");
        assert_eq!(t.text(), "hello world", "복사는 내용 불변");
        assert_eq!(
            t.cut_selection(&mut inv).as_deref(),
            Some("hello"),
            "잘라내기"
        );
        assert_eq!(t.text(), " world");
        // 붙여넣기 — 개행·제어문자는 공백 하나로 접는다(단일 행 컨트롤).
        t.edit.set_caret(0, false);
        t.paste("multi\nline\ttext", &mut inv);
        assert_eq!(t.text(), "multi line text world");
        // 비포커스면 전부 무시(다른 컨트롤의 단축키를 삼키지 않는다).
        t.base.focused = false;
        assert!(t.copy_selection().is_none());
        assert!(t.cut_selection(&mut inv).is_none());
        let before = t.text();
        t.paste("x", &mut inv);
        assert_eq!(t.text(), before);
    }

    /// 멀티라인 붙여넣기(08-18) — 개행을 **보존**한다(단일 행은 공백으로 접음).
    /// `\r\n`/`\r`은 `\n`으로 정규화하고 그 외 제어문자(탭)만 접는다.
    #[test]
    fn multiline_paste_preserves_newlines() {
        let mut t = TextBox::new("bio").with_multiline();
        let mut inv = Invalidations::default();
        t.set_bounds(Rect::new(0, 0, 200, 80), &mut inv);
        t.set_focused(true);
        t.paste("가나다\r\n라마바\nAB\tCD\u{7}EF", &mut inv);
        // 개행 정규화 · 탭 보존(09-14 — SQL·코드 들여쓰기) · 그 외 제어문자(BEL)는 버린다.
        assert_eq!(t.text(), "가나다\n라마바\nAB\tCDEF");
    }

    #[test]
    fn drag_beyond_edges_advances_selection() {
        // ① — 영역 밖 드래그 = 한 글자씩 자동 진행(페인트가 캐럿을 따라 스크롤).
        let (mut t, mut inv) = tb();
        t.set_text("abcdef");
        measure(&t);
        t.on_event(&click(5, 15), &mut inv); // 앞쪽 클릭 → 드래그 시작
        let start = t.edit.caret();
        let b = t.bounds();
        for _ in 0..3 {
            t.on_event(
                &InputEvent::MouseMove {
                    x: b.right() + 20,
                    y: 15,
                },
                &mut inv,
            );
        }
        assert_eq!(t.edit.caret(), (start + 3).min(6), "오른쪽 밖 = +1씩");
        assert!(t.edit.selected_text().is_some(), "앵커 유지 = 선택 확장");
        for _ in 0..10 {
            t.on_event(&InputEvent::MouseMove { x: b.x - 20, y: 15 }, &mut inv);
        }
        assert_eq!(t.edit.caret(), 0, "왼쪽 밖 = -1씩(0에서 멈춤)");
    }

    /// fill_rect만 기록하는 캔버스 — 선택 반전·밑줄·캐럿이 상자를 넘는지 검증.
    struct RecCtx(Vec<Rect>);
    impl crate::draw::DrawCtx for RecCtx {
        fn fill_rect(&mut self, r: Rect, _c: crate::theme::Color) {
            self.0.push(r);
        }
        fn text_opaque(
            &mut self,
            _x: i32,
            _y: i32,
            _clip: Rect,
            _t: &str,
            _f: crate::theme::Color,
            _b: crate::theme::Color,
        ) {
        }
        fn text(&mut self, _x: i32, _y: i32, _clip: Rect, _t: &str, _f: crate::theme::Color) {}
        fn text_width(&mut self, text: &str) -> i32 {
            text.chars().count() as i32 * 7
        }
    }

    #[test]
    fn selection_highlight_clipped_to_box() {
        // 08-13 실기 — 가로 스크롤 상태의 전체 선택 하이라이트가 컨트롤 좌우로
        // 삐져나왔다(fill_rect는 클립을 모른다 — 뷰포트로 직접 잘라야 한다).
        let (mut t, _inv) = tb();
        t.set_text(&"가나다라마바사아자차".repeat(10)); // 260px 상자를 확실히 넘긴다
        t.base.focused = true;
        t.edit.key(EditKey::SelectAll, false); // 선택 끝(=캐럿)이 오른쪽 밖
        let mut rec = RecCtx(Vec::new());
        let theme = crate::theme::Theme::dark();
        t.paint(&mut rec, &theme);
        let b = t.bounds();
        assert!(!rec.0.is_empty(), "선택 반전이 그려져야 한다");
        for r in &rec.0 {
            assert!(
                r.x >= b.x && r.right() <= b.right(),
                "채움이 상자 밖으로 나갔다: {r:?} vs 상자 {b:?}"
            );
        }
        // ★ 08-14 실기 — 하이라이트가 **뷰포트를 정확히 채워야** 한다(글자 클립과
        // 동일 범위). 좌 10px·우 24px 어긋남이 "좌우 글자 반전 누락"으로 보였다.
        let (vx0, vx1) = (b.x + 10, b.right() - 24); // scale 1.0 기준 s(10)·s(24)
        assert!(
            rec.0.iter().any(|r| r.x == vx0 && r.right() == vx1),
            "전체 선택(스크롤 중) 하이라이트가 뷰포트 전폭이어야 한다: {:?}",
            rec.0
        );
        // 왼쪽 밖 케이스 — 캐럿을 앞으로 옮겨 스크롤을 왼쪽 끝으로 되돌린 뒤
        // 다시 전체 선택(앵커 끝 유지 → 선택이 왼쪽 밖까지 걸치는 상태).
        t.edit.set_caret(0, true);
        let mut rec = RecCtx(Vec::new());
        t.paint(&mut rec, &theme);
        for r in &rec.0 {
            assert!(
                r.x >= b.x && r.right() <= b.right(),
                "왼쪽 케이스 — 채움이 상자 밖: {r:?} vs {b:?}"
            );
        }
    }

    #[test]
    fn hscroll_follows_caret_and_resets_when_fits() {
        // ① — 긴 텍스트에서 캐럿이 항상 보인다(그전엔 오른쪽 밖으로 사라졌다).
        let (mut t, _inv) = tb();
        t.set_text(&"m".repeat(200)); // 260px 상자를 확실히 넘긴다
        t.base.focused = true;
        t.edit.set_caret(200, false);
        measure(&t); // 페인트가 스크롤을 조정한다
        assert!(t.hscroll.get() > 0, "캐럿(끝)이 보이려면 스크롤돼야 한다");
        t.set_text("short");
        measure(&t);
        assert_eq!(t.hscroll.get(), 0, "다 들어가면 스크롤 없음");
    }

    #[test]
    fn tab_key_and_paste_follow_stops_or_fixed() {
        // 사용자 09-16 — 정지점(Golden): "AND" 뒤 Tab = 1칸 · 절대: 늘 4칸.
        let mut inv = Invalidations::default();
        for (stops, want_key, want_paste) in
            [(true, "AND ", "AND x"), (false, "AND    ", "AND    x")]
        {
            let mut t = TextBox::new("p").with_multiline();
            t.set_bounds(Rect::new(0, 0, 400, 200), &mut inv);
            t.set_focused(true);
            t.set_indent(4, true);
            t.set_tab_stops(stops);
            t.set_text("AND");
            t.on_event(&InputEvent::Char { c: '\t', now_ms: 0 }, &mut inv);
            assert_eq!(t.text(), want_key, "stops={stops}");
            t.set_text("AND");
            t.paste("\tx", &mut inv);
            assert_eq!(t.text(), want_paste, "stops={stops} 붙여넣기");
        }
    }

    #[test]
    fn paste_expands_tabs_when_indent_spaces() {
        // 사용자 09-15 — 공백 들여쓰기 탭에 붙여넣는 탭 문자는 **탭 폭만큼 공백**이 된다.
        let mut t = TextBox::new("p").with_multiline();
        let mut inv = Invalidations::default();
        t.set_bounds(Rect::new(0, 0, 400, 200), &mut inv);
        t.set_focused(true);
        t.set_indent(4, true);
        t.paste(
            "a	b
	c", &mut inv,
        );
        assert_eq!(
            t.text(),
            "a   b
    c",
            "탭 정지까지 채운다(a 뒤는 3칸)"
        );
        // 탭 들여쓰기 탭은 그대로 둔다.
        let mut t2 = TextBox::new("p").with_multiline();
        t2.set_bounds(Rect::new(0, 0, 400, 200), &mut inv);
        t2.set_focused(true);
        t2.set_indent(4, false);
        t2.paste("a	b", &mut inv);
        assert_eq!(t2.text(), "a	b");
        // 폭 2도 설정대로.
        let mut t3 = TextBox::new("p").with_multiline();
        t3.set_bounds(Rect::new(0, 0, 400, 200), &mut inv);
        t3.set_focused(true);
        t3.set_indent(2, true);
        t3.paste("	x", &mut inv);
        assert_eq!(t3.text(), "  x");
    }

    #[test]
    fn slow_second_click_starts_drag_not_word_select() {
        // 사용자 09-15 — 같은 자리를 한참 뒤에 다시 누르면 더블클릭이 아니라 새 클릭이다
        // (그전엔 단어가 선택되고 드래그가 시작되지 않았다).
        let (mut t, mut inv) = tb();
        t.set_text("select x;");
        t.on_event(&click(200, 15), &mut inv);
        assert!(t.dragging, "첫 클릭 = 드래그 시작");
        t.on_event(&InputEvent::MouseUp { x: 200, y: 15 }, &mut inv);
        // 더블클릭 간격을 넘긴 두 번째 클릭(시각을 과거로 돌려 흉내).
        t.last_click_at = Some(
            std::time::Instant::now() - std::time::Duration::from_millis(DOUBLE_CLICK_MS + 200),
        );
        t.on_event(&click(200, 15), &mut inv);
        assert!(t.dragging, "간격을 넘긴 클릭은 드래그 시작이어야 한다");
        assert!(t.edit.selection().is_none(), "단어 선택이 되면 안 된다");
    }

    #[test]
    fn ctrl_d_selects_word_then_next_occurrence() {
        // 사용자 09-15 — Sublime Ctrl+D.
        let mut t = TextBox::new("p").with_multiline();
        let mut inv = Invalidations::default();
        t.set_bounds(Rect::new(0, 0, 400, 200), &mut inv);
        t.set_focused(true);
        t.set_text("sum a sum b sum");
        t.edit.set_caret(1, false); // 첫 "sum" 안
        assert!(t.select_next_occurrence(), "첫 누름 = 단어 선택");
        assert_eq!(t.edit.selection(), Some((0, 3)));
        assert!(t.select_next_occurrence(), "두 번째 = 다음 출현 추가");
        assert_eq!(t.edit.regions(), vec![(0, 3), (6, 9)]);
        assert!(t.select_next_occurrence());
        assert_eq!(t.selection_count(), 3);
        assert!(!t.select_next_occurrence(), "더 없으면 false");
        // 타이핑은 세 곳 모두에.
        t.on_event(&ch('X'), &mut inv);
        assert_eq!(t.text(), "X a X b X");
        // Esc = 접기.
        assert!(t.clear_multi());
        assert!(!t.has_multi());
    }

    #[test]
    fn alt_shift_drag_makes_column_selection() {
        let mut t = TextBox::new("p").with_multiline();
        let mut inv = Invalidations::default();
        t.set_bounds(Rect::new(0, 0, 400, 200), &mut inv);
        t.set_focused(true);
        t.set_text("abcd\nefgh\nijkl");
        measure(&t); // 줄 배치 기록(히트테스트 근거)
        let lay: Vec<(i32, usize)> = t
            .line_lay
            .borrow()
            .iter()
            .map(|l| (l.top, l.start_idx))
            .collect();
        assert_eq!(lay.len(), 3, "세 줄이 배치돼야 한다");
        let x1 = t.line_lay.borrow()[0].xs[1]; // 첫 줄 1열 경계 x
        let x3 = t.line_lay.borrow()[0].xs[3]; // 첫 줄 3열 경계 x
        t.set_column_mode(true);
        t.on_event(
            &InputEvent::MouseDown {
                x: x1,
                y: lay[0].0 + 2,
                shift: true,
                primary: false,
            },
            &mut inv,
        );
        assert!(t.column_dragging());
        t.on_event(
            &InputEvent::MouseMove {
                x: x3,
                y: lay[2].0 + 2,
            },
            &mut inv,
        );
        assert_eq!(t.selection_count(), 3, "세 줄에 걸친 블록");
        t.on_event(
            &InputEvent::MouseUp {
                x: x3,
                y: lay[2].0 + 2,
            },
            &mut inv,
        );
        assert!(!t.column_dragging());
        t.on_event(&ch('.'), &mut inv);
        assert_eq!(t.text(), "a.d\ne.h\ni.l", "블록이 한 번에 대체된다");
    }
}

#[cfg(test)]
mod scroll_sim_tests {
    use super::*;
    use crate::controls::ProbeCtx;

    /// 트랙패드 느린 스크롤(사건당 ±3 = 1px) 양방향 시뮬레이션 — 매 사건 뒤 paint(실제와 같은 순서)에서
    /// 표시 오프셋(줄×높이 + 잔여)이 정확히 1px씩 단조롭게 움직여야 한다(nexa-sql 사용자 09-16 "위로는 흔들리거나 안 움직임").
    #[test]
    fn slow_wheel_is_monotonic_both_directions_with_paint_between() {
        let mut t = TextBox::new("").with_multiline();
        let mut inv = Invalidations::default();
        t.set_bounds(Rect::new(0, 0, 400, 300), &mut inv);
        let text: String = (1..=200).map(|i| format!("line {i}\n")).collect();
        t.set_text(&text);
        t.base.focused = true;
        t.edit.set_caret(0, false);
        let theme = crate::theme::Theme::dark();
        let mut probe = ProbeCtx;
        t.paint(&mut probe, &theme);
        let lh = t.line_h();
        let pos = |t: &TextBox| t.vscroll.get() as i32 * lh + t.ml_wheel_rem.get();
        assert_eq!(pos(&t), 0);
        // 아래로(본문이 위로 = delta 음수) 1px씩 100번.
        for i in 1..=100 {
            t.on_event(&InputEvent::Wheel { delta: -3 }, &mut inv);
            t.paint(&mut probe, &theme);
            assert_eq!(pos(&t), i, "아래로 {i}번째");
        }
        // 위로 1px씩 60번 — 되돌아온다.
        for i in 1..=60 {
            t.on_event(&InputEvent::Wheel { delta: 3 }, &mut inv);
            t.paint(&mut probe, &theme);
            assert_eq!(pos(&t), 100 - i, "위로 {i}번째");
        }
        // 다시 아래로 — 방향 전환 뒤에도 1px.
        for i in 1..=5 {
            t.on_event(&InputEvent::Wheel { delta: -3 }, &mut inv);
            t.paint(&mut probe, &theme);
            assert_eq!(pos(&t), 40 + i, "재전환 {i}번째");
        }
    }
}

//! 살아 있는 셀 편집기 — 편집 중인 셀 **한 곳**에만 실제 `TextBox`를 둔다(21 §3-2 "one live editor").
//!
//! 호스트: 셀 사각형·현재 글·명세로 [`LiveEditor::begin`] → 사건을 먼저 여기로([`LiveEditor::on_event`]) → `Commit`이면 검증된 정규형을
//! 변경 집합에 놓고 `mv`로 이동 · `Invalid`면 상자에 붉은 띠가 남고 안내를 띄운다 · 스크롤·크기 변화는 [`LiveEditor::set_rect`].

use crate::controls::{Control, TextBox};
use crate::draw::DrawCtx;
use crate::event::{InputEvent, Key};
use crate::geom::Rect;
use crate::theme::Theme;
use crate::widget::{Invalidations, Widget};

use super::changeset::RowRef;
use super::keymap::Move;
use super::spec::{CellSpec, EditError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveEvent {
    None,
    /// 검증을 지난 값(`None` = NULL) + 이동 방향.
    Commit {
        value: Option<String>,
        mv: Move,
    },
    Cancel,
    /// 검증 실패 — 편집기는 열린 채(붉은 띠) · 호스트는 안내만.
    Invalid(EditError),
}

/// [`LiveEditor::begin`] 인자 — `replace` = 타이핑으로 진입한 첫 글자(값을 대체) · 없으면 현재 글(`select_all`이면 전체 선택 · 아니면 캐럿 끝).
#[derive(Clone, Debug)]
pub struct EditStart<'a> {
    pub cell: (RowRef, usize),
    pub spec: CellSpec,
    pub text: &'a str,
    pub rect: Rect,
    pub scale: f32,
    pub replace: Option<char>,
    pub select_all: bool,
    /// 셀 편집 모드 여백(물리 px · 그리드 셀 글자 여백과 같게) — `None` = 보통 상자 모양.
    pub pad: Option<i32>,
}

pub struct LiveEditor {
    tb: TextBox,
    cell: Option<(RowRef, usize)>,
    spec: CellSpec,
    error: Option<EditError>,
    /// 빈 글 커밋 = NULL(true) / 빈 문자열(false) — 호스트 정책(nexa-sql `grid.edit_empty`).
    pub empty_as_null: bool,
    rect: Rect,
}

impl std::fmt::Debug for LiveEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveEditor")
            .field("cell", &self.cell)
            .field("spec", &self.spec)
            .field("error", &self.error)
            .field("rect", &self.rect)
            .finish_non_exhaustive()
    }
}

impl Default for LiveEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl LiveEditor {
    #[must_use]
    pub fn new() -> Self {
        let mut tb = TextBox::new("");
        tb.set_focus_ring(false);
        // 우클릭 메뉴는 호스트의 팝업 층에서(`paint_popup`) — 편집 테두리·다른 셀이 덮지 않는 최상위(UX 규칙).
        tb.set_popup_deferred(true);
        LiveEditor {
            tb,
            cell: None,
            spec: CellSpec::default(),
            error: None,
            empty_as_null: true,
            rect: Rect::default(),
        }
    }

    /// 편집 시작(`EditStart` 한 벌).
    pub fn begin(&mut self, start: EditStart<'_>) {
        let EditStart {
            cell,
            spec,
            text,
            rect,
            scale,
            replace,
            select_all,
            pad,
        } = start;
        let mut inv = Invalidations::default();
        self.cell = Some(cell);
        self.spec = spec;
        self.error = None;
        self.tb.set_warning(false);
        self.tb.set_scale(scale);
        self.tb.set_bounds(rect, &mut inv);
        self.rect = rect;
        self.tb.set_read_only(false);
        self.tb.set_cell_pad(pad);
        self.tb.set_focused(true);
        match replace {
            Some(c) => {
                self.tb.set_text("");
                self.tb
                    .on_event(&InputEvent::Char { c, now_ms: 0 }, &mut inv);
            }
            None => {
                self.tb.set_text(text);
                if select_all {
                    // ★ 전체 선택하되 캐럿은 **앞**에(끝 → Shift+Home): 폭보다 긴 글도 첫머리부터 보인다(사용자 09-26).
                    self.tb.on_event(
                        &InputEvent::Key {
                            key: Key::End,
                            shift: false,
                            primary: false,
                        },
                        &mut inv,
                    );
                    self.tb.on_event(
                        &InputEvent::Key {
                            key: Key::Home,
                            shift: true,
                            primary: false,
                        },
                        &mut inv,
                    );
                } else {
                    self.tb.on_event(
                        &InputEvent::Key {
                            key: Key::End,
                            shift: false,
                            primary: false,
                        },
                        &mut inv,
                    );
                }
            }
        }
    }

    #[must_use]
    pub fn is_open(&self) -> bool {
        self.cell.is_some()
    }
    #[must_use]
    pub fn cell(&self) -> Option<(RowRef, usize)> {
        self.cell
    }
    #[must_use]
    pub fn spec(&self) -> &CellSpec {
        &self.spec
    }
    #[must_use]
    pub fn error(&self) -> Option<&EditError> {
        self.error.as_ref()
    }
    #[must_use]
    pub fn text(&self) -> String {
        self.tb.text()
    }
    #[must_use]
    pub fn rect(&self) -> Rect {
        self.rect
    }
    /// 글자 수 안내(`12/50`) — 상한이 있을 때.
    #[must_use]
    pub fn length_hint(&self) -> Option<(usize, usize)> {
        self.spec
            .max_len
            .map(|m| (self.tb.text().chars().count(), m))
    }

    pub fn close(&mut self) {
        self.cell = None;
        self.error = None;
        self.tb.set_focused(false);
        self.tb.set_warning(false);
    }

    /// 셀 사각형이 움직였다(스크롤·열 폭).
    pub fn set_rect(&mut self, rect: Rect) {
        let mut inv = Invalidations::default();
        self.rect = rect;
        self.tb.set_bounds(rect, &mut inv);
    }

    /// 사건 처리. `shift`는 Tab(`Char('\t')`)의 방향용(Key 사건은 자체 shift를 가진다).
    pub fn on_event(&mut self, ev: &InputEvent, shift: bool, inv: &mut Invalidations) -> LiveEvent {
        if self.cell.is_none() {
            return LiveEvent::None;
        }
        match *ev {
            InputEvent::Key {
                key: Key::Escape, ..
            } => {
                self.close();
                LiveEvent::Cancel
            }
            InputEvent::Char { c: '\t', .. } => {
                self.try_commit(if shift { Move::Left } else { Move::Right })
            }
            InputEvent::Key {
                key: Key::Enter,
                shift: s,
                ..
            } => {
                // 상자가 스스로 `committed`를 세우지만 여기서 바로 커밋한다(단일행).
                self.try_commit(if s { Move::Up } else { Move::Down })
            }
            InputEvent::Key { key: Key::Down, .. } => self.try_commit(Move::Down),
            InputEvent::Key { key: Key::Up, .. } => self.try_commit(Move::Up),
            InputEvent::Key {
                key: Key::PageUp | Key::PageDown,
                ..
            } => self.try_commit(Move::None),
            _ => {
                self.tb.on_event(ev, inv);
                if self.error.is_some() {
                    // 고치기 시작하면 붉은 띠를 걷는다.
                    self.error = None;
                    self.tb.set_warning(false);
                }
                let _ = self.tb.take_committed();
                LiveEvent::None
            }
        }
    }

    /// 지금 글로 커밋 시도(마우스로 다른 셀을 클릭했을 때 호스트가 부른다).
    pub fn try_commit(&mut self, mv: Move) -> LiveEvent {
        let text = self.tb.text();
        let input: Option<&str> = if text.is_empty() && self.empty_as_null {
            None
        } else {
            Some(text.as_str())
        };
        match self.spec.validate(input) {
            Ok(value) => {
                self.close();
                LiveEvent::Commit { value, mv }
            }
            Err(e) => {
                self.error = Some(e.clone());
                self.tb.set_warning(true);
                LiveEvent::Invalid(e)
            }
        }
    }

    pub fn paint(&self, dc: &mut dyn DrawCtx, th: &Theme) {
        if self.cell.is_none() {
            return;
        }
        self.tb.paint(dc, th);
        if self.error.is_some() {
            dc.stroke_round_rect(self.rect, 0, th.danger, 2.0);
        } else {
            dc.stroke_round_rect(self.rect, 0, th.accent, 2.0);
        }
    }

    /// 우클릭 메뉴(팝업 층 · 호스트가 모든 층을 그린 뒤 부른다).
    pub fn paint_popup(&self, dc: &mut dyn DrawCtx, th: &Theme) {
        if self.cell.is_some() {
            self.tb.paint_popup(dc, th);
        }
    }

    /// 상자 자체(우클릭 메뉴 열림 판정 등 호스트가 필요할 때).
    #[must_use]
    pub fn textbox(&self) -> &TextBox {
        &self.tb
    }
    pub fn textbox_mut(&mut self) -> &mut TextBox {
        &mut self.tb
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gridedit::spec::CellKind;

    fn key(k: Key, shift: bool) -> InputEvent {
        InputEvent::Key {
            key: k,
            shift,
            primary: false,
        }
    }
    fn open(spec: CellSpec, text: &str, replace: Option<char>) -> LiveEditor {
        let mut le = LiveEditor::new();
        le.begin(EditStart {
            cell: (RowRef::Existing(0), 1),
            spec,
            text,
            rect: Rect::new(0, 0, 100, 20),
            scale: 1.0,
            replace,
            select_all: true,
            pad: Some(6),
        });
        le
    }

    #[test]
    fn type_and_commit_moves_down() {
        let mut le = open(CellSpec::text("n"), "ab", None);
        let mut inv = Invalidations::default();
        assert!(le.is_open());
        // 전체 선택 상태에서 타이핑 = 대체.
        le.on_event(&InputEvent::Char { c: 'z', now_ms: 0 }, false, &mut inv);
        assert_eq!(le.text(), "z");
        let ev = le.on_event(&key(Key::Enter, false), false, &mut inv);
        assert_eq!(
            ev,
            LiveEvent::Commit {
                value: Some("z".into()),
                mv: Move::Down
            }
        );
        assert!(!le.is_open());
    }

    #[test]
    fn replace_char_tab_and_escape() {
        let mut le = open(CellSpec::text("n"), "old", Some('q'));
        let mut inv = Invalidations::default();
        assert_eq!(le.text(), "q");
        let ev = le.on_event(&InputEvent::Char { c: '\t', now_ms: 0 }, true, &mut inv);
        assert_eq!(
            ev,
            LiveEvent::Commit {
                value: Some("q".into()),
                mv: Move::Left
            }
        );
        let mut le = open(CellSpec::text("n"), "old", None);
        assert_eq!(
            le.on_event(&key(Key::Escape, false), false, &mut inv),
            LiveEvent::Cancel
        );
        assert!(!le.is_open());
    }

    #[test]
    fn invalid_keeps_editor_open_then_fix() {
        let mut le = open(CellSpec::new("n", CellKind::Number), "12", Some('x'));
        let mut inv = Invalidations::default();
        assert_eq!(
            le.on_event(&key(Key::Enter, false), false, &mut inv),
            LiveEvent::Invalid(EditError::NotNumber)
        );
        assert!(le.is_open());
        assert!(le.error().is_some());
        // 고치면 띠가 걷히고 커밋된다.
        le.on_event(
            &InputEvent::Char {
                c: '\u{8}',
                now_ms: 0,
            },
            false,
            &mut inv,
        );
        assert!(le.error().is_none());
        le.on_event(&InputEvent::Char { c: '7', now_ms: 0 }, false, &mut inv);
        assert_eq!(
            le.on_event(&key(Key::Up, false), false, &mut inv),
            LiveEvent::Commit {
                value: Some("7".into()),
                mv: Move::Up
            }
        );
    }

    #[test]
    fn empty_commits_null_or_empty_by_policy() {
        let mut le = open(CellSpec::text("n"), "", None);
        let mut inv = Invalidations::default();
        assert_eq!(
            le.on_event(&key(Key::Enter, false), false, &mut inv),
            LiveEvent::Commit {
                value: None,
                mv: Move::Down
            }
        );
        let mut le = open(CellSpec::text("n").not_null(), "", None);
        le.empty_as_null = false;
        assert_eq!(
            le.on_event(&key(Key::Enter, false), false, &mut inv),
            LiveEvent::Commit {
                value: Some(String::new()),
                mv: Move::Down
            }
        );
        assert_eq!(le.length_hint(), None);
    }
}

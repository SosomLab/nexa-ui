//! 엑셀식 편집 키 · 명령 id → 편집 동작(순수). OS 수식키 관례는 호스트가 `primary`/`shift`로 넘긴다.
//! F2·⌘D·⌘0 같은 키는 `InputEvent`에 없으므로 호스트 키맵의 **명령 id**([`action_for_command`])로 온다.

use crate::event::{InputEvent, Key};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Move {
    #[default]
    None,
    Down,
    Up,
    Right,
    Left,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditAction {
    /// 편집 진입(현재 값 그대로 · 캐럿 끝).
    BeginEdit,
    /// 타이핑으로 진입(값을 이 글자로 대체 · 엑셀).
    BeginEditWith(char),
    /// 선택 셀 비움(Delete = NULL 또는 빈 문자열 · 호스트 정책).
    ClearCells,
    SetNull,
    DuplicateRow,
    InsertRow,
    /// 행 삭제 표식 토글.
    DeleteRow,
    Copy,
    Paste,
    Undo,
    Redo,
    /// 변경 집합 적용(✓).
    Apply,
    /// 변경 집합 전부 되돌림(✗).
    Revert,
    ViewValue,
    PreviewSql,
    /// 적용 대기 변경 목록(행 키 · 열 · 원본 → 새 값).
    ShowChanges,
}

/// 편집기가 **열려 있지 않을 때**의 키 → 동작. 열려 있으면 [`super::LiveEditor`]가 먹는다.
#[must_use]
pub fn action_for_event(ev: &InputEvent, editing: bool) -> Option<EditAction> {
    if editing {
        return None;
    }
    match *ev {
        InputEvent::Key {
            key: Key::Enter, ..
        } => Some(EditAction::BeginEdit),
        InputEvent::Key {
            key: Key::Delete, ..
        } => Some(EditAction::ClearCells),
        // Backspace(`\u{8}`) = 엑셀처럼 내용 지움.
        InputEvent::Char { c: '\u{8}', .. } => Some(EditAction::ClearCells),
        InputEvent::Char { c, .. } if !c.is_control() && c != '\t' => {
            Some(EditAction::BeginEditWith(c))
        }
        InputEvent::Undo => Some(EditAction::Undo),
        InputEvent::Redo => Some(EditAction::Redo),
        _ => None,
    }
}

/// 명령 id(팔레트·메뉴·키맵 공통 · nexa-sql 77 §3 `grid.*` 규약) → 동작.
#[must_use]
pub fn action_for_command(id: &str) -> Option<EditAction> {
    Some(match id {
        "grid.edit.begin" | "f2" => EditAction::BeginEdit,
        "grid.edit.set_null" => EditAction::SetNull,
        "grid.edit.clear" => EditAction::ClearCells,
        "grid.edit.dup_row" | "row.dup" => EditAction::DuplicateRow,
        "grid.edit.insert_row" | "row.add" => EditAction::InsertRow,
        "grid.edit.delete_row" | "row.del" => EditAction::DeleteRow,
        "grid.edit.apply" | "row.save" => EditAction::Apply,
        "grid.edit.revert" | "row.cancel" => EditAction::Revert,
        "grid.edit.paste" | "edit.paste" => EditAction::Paste,
        "grid.edit.copy" | "edit.copy" => EditAction::Copy,
        "grid.edit.undo" => EditAction::Undo,
        "grid.edit.redo" => EditAction::Redo,
        "grid.edit.view_value" => EditAction::ViewValue,
        "grid.edit.preview_sql" => EditAction::PreviewSql,
        "grid.edit.changes" => EditAction::ShowChanges,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: Key) -> InputEvent {
        InputEvent::Key {
            key: k,
            shift: false,
            primary: false,
        }
    }

    #[test]
    fn events_map_when_not_editing() {
        assert_eq!(
            action_for_event(&key(Key::Enter), false),
            Some(EditAction::BeginEdit)
        );
        assert_eq!(action_for_event(&key(Key::Enter), true), None);
        assert_eq!(
            action_for_event(&key(Key::Delete), false),
            Some(EditAction::ClearCells)
        );
        assert_eq!(
            action_for_event(&InputEvent::Char { c: 'x', now_ms: 0 }, false),
            Some(EditAction::BeginEditWith('x'))
        );
        assert_eq!(
            action_for_event(&InputEvent::Char { c: '\t', now_ms: 0 }, false),
            None
        );
        assert_eq!(
            action_for_event(
                &InputEvent::Char {
                    c: '\u{8}',
                    now_ms: 0
                },
                false
            ),
            Some(EditAction::ClearCells)
        );
        assert_eq!(
            action_for_event(&InputEvent::Undo, false),
            Some(EditAction::Undo)
        );
        assert_eq!(action_for_event(&key(Key::Down), false), None);
    }

    #[test]
    fn command_ids() {
        assert_eq!(
            action_for_command("grid.edit.dup_row"),
            Some(EditAction::DuplicateRow)
        );
        assert_eq!(action_for_command("row.save"), Some(EditAction::Apply));
        assert_eq!(action_for_command("nope"), None);
    }
}

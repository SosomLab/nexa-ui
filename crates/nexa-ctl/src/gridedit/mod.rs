//! ★ 편집 가능한 그리드의 **핵심 부품**(nexa-sql 87 · 사용자 09-26 "다른 프로그램에서도 사용할 수 있게") — DBMS·SQL·그리기를 모른다.
//!
//! - [`spec`] 셀 명세(`CellKind`·`CellSpec` · 타입/길이/NULL/기본값 · 검증 → 정규형 문자열)
//! - [`datetime`] 날짜·시각 입력 형식 여럿 → ISO 정규형
//! - [`changeset`] 변경 집합(셀 수정 · 행 추가/삭제/복제 · 되돌리기 · 표시 순서) — 원본 세트는 건드리지 않고 **덧그린다**
//! - [`paste`] 클립보드 행렬 해석(TSV · 따옴표 셀) + 앵커부터 채우기(자동 확장)
//! - [`keymap`] 엑셀식 키·명령 id → 편집 동작
//! - [`live`] 편집 중인 셀 **한 곳**의 실제 `TextBox`(살아 있는 편집기 · 21 §3-2)
//!
//! 값은 전부 `Option<String>`(None = NULL). 호스트 그리드는 ① 자기 좌표를 `RowRef`/열 인덱스로 바꿔 부품을 부르고
//! ② `ChangeSet::cell`이 `Some`이면 그 값을 그려(색·띠·취선) ③ 커밋된 변경 집합을 자기 방식(SQL · 파일 · 설정)으로 적용한다.

pub mod changeset;
pub mod datetime;
pub mod keymap;
pub mod live;
pub mod paste;
pub mod spec;

pub use changeset::{ChangeSet, InsertedRow, RowRef, RowStatus};
pub use keymap::{action_for_command, action_for_event, EditAction, Move};
pub use live::{EditStart, LiveEditor, LiveEvent};
pub use paste::{apply as paste_apply, parse_matrix, PasteAnchor, PasteOpts, PasteReport};
pub use spec::{CellKind, CellSpec, EditError};

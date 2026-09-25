//! 변경 집합 — 원본 세트는 그대로 두고 **덧그린다**(nexa-sql DR-33 · 87 §3).
//!
//! 기존 행의 셀 수정 `(row, col) → Option<String>` · 삭제 표식 · 추가/복제 행(원본 아래 표시) · 자체 되돌리기(묶음 지원) ·
//! 표시 순서 `layout(n)`(삽입이 없으면 비용 0). 값의 "원본과 같아지면 Clean" 판정은 호스트가 원본을 넘겨 준다.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

/// 행을 가리키는 값 — 기존 행(원본 인덱스) 또는 추가 행(변경 집합 안 인덱스 · 안정적).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RowRef {
    Existing(usize),
    Inserted(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowStatus {
    Clean,
    Modified,
    Deleted,
    Inserted,
}

/// 추가(복제) 행 — 셀 전부 + 어느 기존 행 아래 보일지(`None` = 맨 끝).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsertedRow {
    pub cells: Vec<Option<String>>,
    pub after: Option<usize>,
    /// 사용자가 지운 추가 행(인덱스 안정성을 위해 자리만 남긴다 · 되돌리기로 되살아난다).
    pub removed: bool,
}

/// 되돌리기 원소 — 적용하면 **역연산**을 돌려준다.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Op {
    /// 기존 행 셀: `before` = 이전 덧그림(`None` = 없었음 = 원본).
    SetExisting {
        row: usize,
        col: usize,
        before: Option<Option<String>>,
        after: Option<Option<String>>,
    },
    SetInserted {
        k: usize,
        col: usize,
        before: Option<String>,
        after: Option<String>,
    },
    Delete {
        row: usize,
        on: bool,
    },
    /// 추가 행 자리 만들기(`k` = 끝) — 역 = `RemoveSlot`(맨 끝일 때만 진짜로 뺀다).
    Insert {
        k: usize,
        row: InsertedRow,
    },
    RemoveSlot {
        k: usize,
    },
    Removed {
        k: usize,
        on: bool,
    },
    Group(Vec<Op>),
}

/// (세대, 기존 행 수, 표시 순서).
type LayoutCache = (u64, usize, Rc<Vec<RowRef>>);

#[derive(Debug, Default)]
pub struct ChangeSet {
    ncols: usize,
    edits: BTreeMap<(usize, usize), Option<String>>,
    deleted: BTreeSet<usize>,
    inserted: Vec<InsertedRow>,
    undo: Vec<Op>,
    redo: Vec<Op>,
    group: Option<Vec<Op>>,
    gen: u64,
    layout_cache: RefCell<Option<LayoutCache>>,
}

impl ChangeSet {
    #[must_use]
    pub fn new(ncols: usize) -> Self {
        ChangeSet {
            ncols,
            ..Default::default()
        }
    }

    #[must_use]
    pub fn ncols(&self) -> usize {
        self.ncols
    }

    /// 세대(바뀔 때마다 +1 · 호스트 캐시 열쇠).
    #[must_use]
    pub fn generation(&self) -> u64 {
        self.gen
    }

    #[must_use]
    pub fn is_dirty(&self) -> bool {
        !self.edits.is_empty()
            || !self.deleted.is_empty()
            || self.inserted.iter().any(|r| !r.removed)
    }

    /// (수정된 기존 행 수, 살아 있는 추가 행 수, 삭제 표식 수).
    #[must_use]
    pub fn counts(&self) -> (usize, usize, usize) {
        let rows: BTreeSet<usize> = self.edits.keys().map(|(r, _)| *r).collect();
        (
            rows.len(),
            self.inserted.iter().filter(|r| !r.removed).count(),
            self.deleted.len(),
        )
    }

    #[must_use]
    pub fn has_inserts(&self) -> bool {
        self.inserted.iter().any(|r| !r.removed)
    }

    /// 덧그린 값 — `Some(&v)`면 그 값을 그린다(NULL 포함) · `None`이면 원본.
    #[must_use]
    pub fn cell(&self, row: RowRef, col: usize) -> Option<&Option<String>> {
        match row {
            RowRef::Existing(r) => self.edits.get(&(r, col)),
            RowRef::Inserted(k) => self.inserted.get(k).and_then(|ir| ir.cells.get(col)),
        }
    }

    #[must_use]
    pub fn is_modified_cell(&self, row: RowRef, col: usize) -> bool {
        matches!(row, RowRef::Existing(r) if self.edits.contains_key(&(r, col)))
    }

    #[must_use]
    pub fn status(&self, row: RowRef) -> RowStatus {
        match row {
            RowRef::Inserted(_) => RowStatus::Inserted,
            RowRef::Existing(r) => {
                if self.deleted.contains(&r) {
                    RowStatus::Deleted
                } else if self.edits.range((r, 0)..(r + 1, 0)).next().is_some() {
                    RowStatus::Modified
                } else {
                    RowStatus::Clean
                }
            }
        }
    }

    #[must_use]
    pub fn is_deleted(&self, row: RowRef) -> bool {
        match row {
            RowRef::Existing(r) => self.deleted.contains(&r),
            RowRef::Inserted(k) => self.inserted.get(k).is_none_or(|r| r.removed),
        }
    }

    #[must_use]
    pub fn inserted(&self, k: usize) -> Option<&InsertedRow> {
        self.inserted.get(k).filter(|r| !r.removed)
    }

    /// 수정된 기존 행 셀 전부 `((row, col), 값)`.
    pub fn edits(&self) -> impl Iterator<Item = (&(usize, usize), &Option<String>)> {
        self.edits.iter()
    }
    pub fn deleted_rows(&self) -> impl Iterator<Item = usize> + '_ {
        self.deleted.iter().copied()
    }
    /// 살아 있는 추가 행 `(k, 행)`.
    pub fn inserted_rows(&self) -> impl Iterator<Item = (usize, &InsertedRow)> {
        self.inserted.iter().enumerate().filter(|(_, r)| !r.removed)
    }

    // ---- 변경(전부 되돌리기 기록) ----

    /// 셀 값을 놓는다. `original`은 기존 행의 원본 값(같으면 덧그림을 지워 Clean으로) — 추가 행은 무시.
    /// 삭제 표식 행은 거부(false).
    pub fn set_cell(
        &mut self,
        row: RowRef,
        col: usize,
        value: Option<String>,
        original: Option<&Option<String>>,
    ) -> bool {
        if col >= self.ncols || self.is_deleted(row) {
            return false;
        }
        let op = match row {
            RowRef::Existing(r) => {
                let before = self.edits.get(&(r, col)).cloned();
                let after = if original.is_some_and(|o| *o == value) {
                    None
                } else {
                    Some(value)
                };
                if before == after {
                    return false;
                }
                Op::SetExisting {
                    row: r,
                    col,
                    before,
                    after,
                }
            }
            RowRef::Inserted(k) => {
                let before = self.inserted[k].cells[col].clone();
                if before == value {
                    return false;
                }
                Op::SetInserted {
                    k,
                    col,
                    before,
                    after: value,
                }
            }
        };
        self.commit(op);
        true
    }

    /// 삭제 표식 토글(기존 행) · 추가 행이면 그 행을 지운다(되돌리기 가능). 되돌아온 상태를 준다.
    pub fn toggle_delete(&mut self, row: RowRef) -> bool {
        match row {
            RowRef::Existing(r) => {
                let on = !self.deleted.contains(&r);
                self.commit(Op::Delete { row: r, on });
                on
            }
            RowRef::Inserted(k) => {
                if k >= self.inserted.len() {
                    return false;
                }
                let on = !self.inserted[k].removed;
                self.commit(Op::Removed { k, on });
                on
            }
        }
    }

    /// 새 행(전부 NULL/빈 값은 호스트가 채움) — `after` = 그 기존 행 아래 · `None` = 맨 끝. 추가 행 인덱스 k.
    pub fn insert_row(&mut self, after: Option<usize>, cells: Vec<Option<String>>) -> usize {
        let mut cells = cells;
        cells.resize(self.ncols, None);
        let k = self.inserted.len();
        self.commit(Op::Insert {
            k,
            row: InsertedRow {
                cells,
                after,
                removed: false,
            },
        });
        k
    }

    /// 복제 = 원본 셀을 받아 새 행으로(키·읽기 전용 열은 호스트가 `None`으로 비워서 넘긴다). 표시는 원본 바로 아래.
    pub fn duplicate_row(&mut self, source: RowRef, cells: Vec<Option<String>>) -> usize {
        let after = match source {
            RowRef::Existing(r) => Some(r),
            RowRef::Inserted(k) => self.inserted.get(k).and_then(|r| r.after),
        };
        self.insert_row(after, cells)
    }

    /// 묶음 시작(붙여넣기 등) — `end_group`까지의 변경이 한 번의 되돌리기.
    pub fn begin_group(&mut self) {
        if self.group.is_none() {
            self.group = Some(Vec::new());
        }
    }
    pub fn end_group(&mut self) {
        if let Some(ops) = self.group.take() {
            if !ops.is_empty() {
                self.undo.push(Op::Group(ops));
                self.redo.clear();
            }
        }
    }

    /// 전부 버린다(되돌리기 기록까지).
    pub fn clear(&mut self) {
        *self = ChangeSet::new(self.ncols);
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo(&mut self) -> bool {
        let Some(op) = self.undo.pop() else {
            return false;
        };
        let inv = self.apply(op);
        self.redo.push(inv);
        true
    }
    pub fn redo(&mut self) -> bool {
        let Some(op) = self.redo.pop() else {
            return false;
        };
        let inv = self.apply(op);
        self.undo.push(inv);
        true
    }

    fn commit(&mut self, op: Op) {
        let inv = self.apply(op);
        if let Some(g) = self.group.as_mut() {
            g.push(inv);
        } else {
            self.undo.push(inv);
            self.redo.clear();
        }
    }

    /// 적용하고 역연산을 돌려준다.
    fn apply(&mut self, op: Op) -> Op {
        self.gen += 1;
        match op {
            Op::SetExisting {
                row,
                col,
                before,
                after,
            } => {
                match &after {
                    Some(v) => {
                        self.edits.insert((row, col), v.clone());
                    }
                    None => {
                        self.edits.remove(&(row, col));
                    }
                }
                Op::SetExisting {
                    row,
                    col,
                    before: after,
                    after: before,
                }
            }
            Op::SetInserted {
                k,
                col,
                before,
                after,
            } => {
                self.inserted[k].cells[col] = after.clone();
                Op::SetInserted {
                    k,
                    col,
                    before: after,
                    after: before,
                }
            }
            Op::Delete { row, on } => {
                if on {
                    self.deleted.insert(row);
                } else {
                    self.deleted.remove(&row);
                }
                Op::Delete { row, on: !on }
            }
            Op::Insert { k, row } => {
                if k == self.inserted.len() {
                    self.inserted.push(row);
                } else {
                    self.inserted[k] = row;
                }
                Op::RemoveSlot { k }
            }
            Op::RemoveSlot { k } => {
                let row = if k + 1 == self.inserted.len() {
                    self.inserted.pop().unwrap_or(InsertedRow {
                        cells: vec![None; self.ncols],
                        after: None,
                        removed: true,
                    })
                } else {
                    let r = self.inserted[k].clone();
                    self.inserted[k].removed = true;
                    r
                };
                Op::Insert { k, row }
            }
            Op::Removed { k, on } => {
                self.inserted[k].removed = on;
                Op::Removed { k, on: !on }
            }
            Op::Group(ops) => {
                // 역순으로 적용하고 역연산도 역순으로 쌓아 다시 묶는다.
                let mut inv = Vec::with_capacity(ops.len());
                for op in ops.into_iter().rev() {
                    inv.push(self.apply(op));
                }
                inv.reverse();
                Op::Group(inv)
            }
        }
    }

    // ---- 표시 순서 ----

    /// 표시 순서(기존 행 사이에 추가 행을 끼운다). 삽입이 없으면 호스트가 `RowRef::Existing(i)`로 직접 매핑하는 편이 싸다([`Self::has_inserts`]).
    /// 결과는 세대·행 수로 캐시된다.
    #[must_use]
    pub fn layout(&self, n_existing: usize) -> Rc<Vec<RowRef>> {
        if let Some((g, n, v)) = self.layout_cache.borrow().as_ref() {
            if *g == self.gen && *n == n_existing {
                return Rc::clone(v);
            }
        }
        let mut by_after: BTreeMap<Option<usize>, Vec<usize>> = BTreeMap::new();
        for (k, r) in self.inserted.iter().enumerate() {
            if !r.removed {
                let key = r.after.filter(|a| *a < n_existing);
                by_after.entry(key).or_default().push(k);
            }
        }
        let mut out = Vec::with_capacity(n_existing + self.inserted.len());
        for i in 0..n_existing {
            out.push(RowRef::Existing(i));
            if let Some(ks) = by_after.get(&Some(i)) {
                out.extend(ks.iter().map(|k| RowRef::Inserted(*k)));
            }
        }
        if let Some(ks) = by_after.get(&None) {
            out.extend(ks.iter().map(|k| RowRef::Inserted(*k)));
        }
        let rc = Rc::new(out);
        *self.layout_cache.borrow_mut() = Some((self.gen, n_existing, Rc::clone(&rc)));
        rc
    }

    /// 표시 행 수(기존 + 살아 있는 추가).
    #[must_use]
    pub fn display_len(&self, n_existing: usize) -> usize {
        n_existing + self.inserted.iter().filter(|r| !r.removed).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    #[test]
    fn set_cell_tracks_original_and_undo() {
        let mut cs = ChangeSet::new(3);
        let orig = s("a");
        assert!(cs.set_cell(RowRef::Existing(0), 1, s("b"), Some(&orig)));
        assert_eq!(cs.cell(RowRef::Existing(0), 1), Some(&s("b")));
        assert_eq!(cs.status(RowRef::Existing(0)), RowStatus::Modified);
        // 원본과 같아지면 Clean.
        assert!(cs.set_cell(RowRef::Existing(0), 1, s("a"), Some(&orig)));
        assert_eq!(cs.cell(RowRef::Existing(0), 1), None);
        assert_eq!(cs.status(RowRef::Existing(0)), RowStatus::Clean);
        assert!(cs.undo());
        assert_eq!(cs.cell(RowRef::Existing(0), 1), Some(&s("b")));
        assert!(cs.redo());
        assert_eq!(cs.cell(RowRef::Existing(0), 1), None);
        // NULL 덧그림.
        assert!(cs.set_cell(RowRef::Existing(2), 0, None, Some(&s("x"))));
        assert_eq!(cs.cell(RowRef::Existing(2), 0), Some(&None));
        assert!(
            !cs.set_cell(RowRef::Existing(2), 5, s("z"), None),
            "열 범위 밖"
        );
        assert_eq!(cs.counts(), (1, 0, 0));
    }

    #[test]
    fn delete_insert_duplicate_layout() {
        let mut cs = ChangeSet::new(2);
        assert!(cs.toggle_delete(RowRef::Existing(1)));
        assert!(cs.is_deleted(RowRef::Existing(1)));
        assert!(
            !cs.set_cell(RowRef::Existing(1), 0, s("q"), None),
            "삭제 행은 편집 거부"
        );
        let k = cs.duplicate_row(RowRef::Existing(0), vec![None, s("v")]);
        assert_eq!(k, 0);
        assert_eq!(cs.cell(RowRef::Inserted(0), 1), Some(&s("v")));
        let k2 = cs.insert_row(None, vec![]);
        assert_eq!(cs.cell(RowRef::Inserted(k2), 0), Some(&None));
        let lay = cs.layout(3);
        assert_eq!(
            *lay,
            vec![
                RowRef::Existing(0),
                RowRef::Inserted(0),
                RowRef::Existing(1),
                RowRef::Existing(2),
                RowRef::Inserted(1)
            ]
        );
        assert_eq!(cs.display_len(3), 5);
        assert_eq!(cs.counts(), (0, 2, 1));
        // 추가 행 지우기 → 레이아웃에서 빠짐 · 되돌리기로 복귀 · 인덱스 안정.
        assert!(cs.toggle_delete(RowRef::Inserted(0)));
        assert_eq!(cs.layout(3).len(), 4);
        assert!(cs.undo());
        assert_eq!(cs.layout(3).len(), 5);
        assert_eq!(cs.cell(RowRef::Inserted(0), 1), Some(&s("v")));
        // 전부 되돌리면 깨끗.
        while cs.undo() {}
        assert!(!cs.is_dirty());
        assert_eq!(cs.layout(3).len(), 3);
        // 다시 하기는 "추가 행 0 지우기"까지 되돌려 놓는다(그 전에 한 번 되돌린 것도 redo 스택에 있었다).
        while cs.redo() {}
        assert_eq!(cs.layout(3).len(), 4);
        assert_eq!(cs.counts(), (0, 1, 1));
    }

    #[test]
    fn group_is_one_undo_step() {
        let mut cs = ChangeSet::new(2);
        cs.begin_group();
        cs.set_cell(RowRef::Existing(0), 0, s("1"), None);
        cs.set_cell(RowRef::Existing(0), 1, s("2"), None);
        let k = cs.insert_row(None, vec![s("3"), s("4")]);
        cs.end_group();
        assert_eq!(cs.counts(), (1, 1, 0));
        assert!(cs.undo());
        assert!(!cs.is_dirty());
        assert!(!cs.undo());
        assert!(cs.redo());
        assert_eq!(cs.cell(RowRef::Inserted(k), 1), Some(&s("4")));
        assert_eq!(cs.counts(), (1, 1, 0));
    }

    #[test]
    fn layout_cache_follows_generation() {
        let mut cs = ChangeSet::new(1);
        let a = cs.layout(2);
        let b = cs.layout(2);
        assert!(Rc::ptr_eq(&a, &b));
        cs.insert_row(Some(0), vec![]);
        let c = cs.layout(2);
        assert!(!Rc::ptr_eq(&a, &c));
        assert_eq!(c[1], RowRef::Inserted(0));
        // 범위 밖 `after`는 끝으로.
        cs.insert_row(Some(99), vec![]);
        assert_eq!(*cs.layout(2).last().unwrap(), RowRef::Inserted(1));
    }
}

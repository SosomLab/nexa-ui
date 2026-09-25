//! 클립보드 행렬 붙여넣기 — 해석(TSV · 따옴표 셀 · 셀 안 줄바꿈 · CRLF)과 앵커부터 채우기(아래로 부족하면 행 **자동 확장**).

use super::changeset::{ChangeSet, RowRef};
use super::spec::{CellSpec, EditError};

/// 텍스트 → 행렬. 탭 = 열 · 줄바꿈 = 행 · `"…"` 셀은 탭·줄바꿈을 품을 수 있고 `""`는 따옴표 하나(엑셀/Numbers 규칙).
/// 마지막 빈 줄은 버린다. 탭이 하나도 없고 줄이 하나면 셀 하나.
#[must_use]
pub fn parse_matrix(text: &str) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut chars = text.chars().peekable();
    let mut in_q = false;
    let mut cell_started = false;
    while let Some(c) = chars.next() {
        if in_q {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    in_q = false;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        match c {
            '"' if !cell_started => {
                in_q = true;
                cell_started = true;
            }
            '\t' => {
                row.push(std::mem::take(&mut cell));
                cell_started = false;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
                cell_started = false;
            }
            '\n' => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
                cell_started = false;
            }
            _ => {
                cell.push(c);
                cell_started = true;
            }
        }
    }
    if !cell.is_empty() || !row.is_empty() || cell_started {
        row.push(cell);
        rows.push(row);
    }
    rows
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasteOpts {
    /// 이 글자와 같은 셀은 NULL(예: 그리드의 NULL 표기 `(null)`) · `None` = 없음.
    pub null_token: Option<String>,
    /// 빈 셀 = NULL(true) / 빈 문자열(false).
    pub empty_as_null: bool,
    /// 아래로 부족하면 행을 추가한다.
    pub auto_extend: bool,
    /// 한 번에 붙일 최대 행(초과분은 `clipped_rows`).
    pub max_rows: usize,
}

impl Default for PasteOpts {
    fn default() -> Self {
        PasteOpts {
            null_token: None,
            empty_as_null: true,
            auto_extend: true,
            max_rows: 10_000,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PasteReport {
    pub set: usize,
    pub added_rows: usize,
    /// 검증에 걸려 건너뛴 셀 `(행렬 행, 행렬 열, 이유)`.
    pub rejected: Vec<(usize, usize, EditError)>,
    pub clipped_cols: usize,
    pub clipped_rows: usize,
}

/// 붙여넣을 자리 — 지금 표시 순서와 앵커(표시 행 인덱스 · 열).
#[derive(Clone, Copy, Debug)]
pub struct PasteAnchor<'a> {
    pub layout: &'a [RowRef],
    pub row: usize,
    pub col: usize,
}

/// 앵커부터 행렬을 채운다 — 한 묶음(되돌리기 1회).
/// `original(row, col)` = 기존 행의 원본 값(Clean 판정 · 복제 없음) · `specs` = 열 명세.
pub fn apply(
    cs: &mut ChangeSet,
    at: PasteAnchor<'_>,
    matrix: &[Vec<String>],
    specs: &[CellSpec],
    original: &dyn Fn(usize, usize) -> Option<String>,
    opts: &PasteOpts,
) -> PasteReport {
    let (layout, anchor_row, anchor_col) = (at.layout, at.row, at.col);
    let mut rep = PasteReport::default();
    let ncols = cs.ncols();
    let rows_to_paste = matrix.len().min(opts.max_rows);
    rep.clipped_rows = matrix.len().saturating_sub(rows_to_paste);
    cs.begin_group();
    let mut targets: Vec<RowRef> = layout
        .iter()
        .skip(anchor_row)
        .take(rows_to_paste)
        .copied()
        .collect();
    while targets.len() < rows_to_paste {
        if !opts.auto_extend {
            rep.clipped_rows += rows_to_paste - targets.len();
            break;
        }
        let k = cs.insert_row(None, vec![]);
        rep.added_rows += 1;
        targets.push(RowRef::Inserted(k));
    }
    for (ri, target) in targets.iter().enumerate() {
        let line = &matrix[ri];
        for (ci, raw) in line.iter().enumerate() {
            let col = anchor_col + ci;
            if col >= ncols {
                rep.clipped_cols = rep.clipped_cols.max(line.len() - ci);
                break;
            }
            let input: Option<&str> = if opts.null_token.as_deref().is_some_and(|t| t == raw)
                || (opts.empty_as_null && raw.is_empty())
            {
                None
            } else {
                Some(raw.as_str())
            };
            let value = match specs.get(col) {
                Some(spec) => match spec.validate(input) {
                    Ok(v) => v,
                    Err(e) => {
                        rep.rejected.push((ri, ci, e));
                        continue;
                    }
                },
                None => input.map(str::to_string),
            };
            let orig = match target {
                RowRef::Existing(r) => Some(original(*r, col)),
                RowRef::Inserted(_) => None,
            };
            if cs.set_cell(*target, col, value, orig.as_ref()) {
                rep.set += 1;
            }
        }
    }
    cs.end_group();
    rep
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gridedit::spec::CellKind;

    #[test]
    fn parse_tsv_quotes_and_crlf() {
        assert_eq!(
            parse_matrix("a\tb\r\nc\td\n"),
            vec![vec!["a", "b"], vec!["c", "d"]]
        );
        assert_eq!(parse_matrix("x"), vec![vec!["x"]]);
        assert_eq!(
            parse_matrix("\"a\tb\"\t\"line1\nline2\"\t\"say \"\"hi\"\"\"\n"),
            vec![vec!["a\tb", "line1\nline2", "say \"hi\""]]
        );
        assert_eq!(parse_matrix("1\t\t3\n"), vec![vec!["1", "", "3"]]);
        assert_eq!(parse_matrix(""), Vec::<Vec<String>>::new());
        assert_eq!(parse_matrix("\t"), vec![vec!["", ""]]);
    }

    #[test]
    fn apply_fills_and_extends() {
        let mut cs = ChangeSet::new(3);
        let specs = vec![
            CellSpec::new("id", CellKind::Number),
            CellSpec::text("name").max_len(4),
            CellSpec::new("d", CellKind::Date),
        ];
        let layout = vec![RowRef::Existing(0), RowRef::Existing(1)];
        let m = parse_matrix("1\tab\t2026-01-02\n2\ttoolong\t\n3\tzz\t20260304\n");
        let orig = |r: usize, c: usize| -> Option<String> { Some(format!("{r}{c}")) };
        let rep = apply(
            &mut cs,
            PasteAnchor {
                layout: &layout,
                row: 1,
                col: 0,
            },
            &m,
            &specs,
            &orig,
            &PasteOpts::default(),
        );
        assert_eq!(rep.added_rows, 2, "행 2개 부족 → 추가");
        assert_eq!(rep.rejected.len(), 1);
        assert!(matches!(rep.rejected[0], (1, 1, EditError::TooLong { .. })));
        // 9칸 − 거부 1 − 추가 행의 빈 셀(이미 NULL이라 변화 없음) 1 = 7.
        assert_eq!(rep.set, 7);
        assert_eq!(
            cs.cell(RowRef::Existing(1), 2),
            Some(&Some("2026-01-02".into()))
        );
        assert_eq!(cs.cell(RowRef::Inserted(0), 2), Some(&None), "빈 셀 = NULL");
        assert_eq!(cs.cell(RowRef::Inserted(1), 1), Some(&Some("zz".into())));
        // 한 묶음 되돌리기.
        assert!(cs.undo());
        assert!(!cs.is_dirty());
    }

    #[test]
    fn apply_clips_columns_and_respects_null_token() {
        let mut cs = ChangeSet::new(2);
        let specs = vec![CellSpec::text("a"), CellSpec::text("b").not_null()];
        let layout = vec![RowRef::Existing(0)];
        let m = parse_matrix("(null)\t(null)\tx\ty\n");
        let opts = PasteOpts {
            null_token: Some("(null)".into()),
            ..PasteOpts::default()
        };
        let rep = apply(
            &mut cs,
            PasteAnchor {
                layout: &layout,
                row: 0,
                col: 0,
            },
            &m,
            &specs,
            &|_, _| Some("o".into()),
            &opts,
        );
        assert_eq!(rep.clipped_cols, 2);
        assert_eq!(cs.cell(RowRef::Existing(0), 0), Some(&None));
        assert_eq!(rep.rejected, vec![(0, 1, EditError::NotNull)]);
        // 원본과 같은 값은 덧그리지 않는다.
        let mut cs2 = ChangeSet::new(1);
        let rep2 = apply(
            &mut cs2,
            PasteAnchor {
                layout: &[RowRef::Existing(0)],
                row: 0,
                col: 0,
            },
            &parse_matrix("o"),
            &[CellSpec::text("a")],
            &|_, _| Some("o".into()),
            &PasteOpts::default(),
        );
        assert_eq!(rep2.set, 0);
        assert!(!cs2.is_dirty());
    }
}

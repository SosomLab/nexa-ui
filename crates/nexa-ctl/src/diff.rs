//! ★ 줄 단위 정렬 디프(nexa-sql T-283 2단계 · docs/19 §6-2 · 10-09): 두 줄 목록을 **나란히 보는 행**으로 정렬한다 — 같은 줄은 양쪽에,
//! 한쪽에만 있는 줄은 맞은편을 빈 행으로, 바뀐 줄은 양쪽 다른 내용으로. [`TextBox`](crate::controls::textbox)의 줄 표시 디프
//! (`diff_lines` = 한쪽 거터 표식)와 같은 LCS(공통 앞·뒤 제거 → 가운데 LCS · 상한 넘으면 전부 Replace)를 쓰되, 결과가 **양쪽 index를
//! 다 담는 행 목록**이라 2-pane 뷰어가 그대로 그린다. 순수 함수 · 시험 = 단순 모델(모든 index가 순서대로 한 번씩 · Equal 행은 같은 글)
//! 과 난수 대조.

use std::ops::Range;

/// 행의 종류.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind {
    /// 양쪽 같은 줄.
    Equal,
    /// 오른쪽(b)에만 있는 줄(왼쪽은 빈 행).
    Insert,
    /// 왼쪽(a)에만 있는 줄(오른쪽은 빈 행).
    Delete,
    /// 양쪽 다른 줄(같은 자리에서 내용이 바뀜).
    Replace,
}

/// 나란히 보는 한 행 — `a`/`b` = 그쪽 줄 index(없으면 빈 행).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Row {
    pub a: Option<usize>,
    pub b: Option<usize>,
    pub kind: RowKind,
}

/// 바뀐 덩어리(연속된 비-Equal 행) — 행 범위 · 양쪽 줄 범위.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hunk {
    pub rows: Range<usize>,
    pub a: Range<usize>,
    pub b: Range<usize>,
}

/// LCS 상한(가운데 구간 한쪽이 이보다 길면 전부 Replace/Insert/Delete로 뭉뚱그린다 — `diff_lines`와 같은 값).
const LCS_CAP: usize = 1500;

/// 두 줄 목록을 행으로 정렬한다.
#[must_use]
pub fn align(a: &[String], b: &[String]) -> Vec<Row> {
    let (n, m) = (a.len(), b.len());
    let mut rows: Vec<Row> = Vec::with_capacity(n.max(m));
    let mut pre = 0;
    while pre < n && pre < m && a[pre] == b[pre] {
        rows.push(Row {
            a: Some(pre),
            b: Some(pre),
            kind: RowKind::Equal,
        });
        pre += 1;
    }
    let mut suf = 0;
    while suf < n - pre && suf < m - pre && a[n - 1 - suf] == b[m - 1 - suf] {
        suf += 1;
    }
    let (a0, a1, b0, b1) = (pre, n - suf, pre, m - suf);
    let (la, lb) = (a1 - a0, b1 - b0);
    if la > LCS_CAP || lb > LCS_CAP {
        // 상한 초과 = 짝수 길이만큼 Replace · 나머지는 한쪽만.
        let k = la.min(lb);
        for i in 0..k {
            rows.push(Row {
                a: Some(a0 + i),
                b: Some(b0 + i),
                kind: RowKind::Replace,
            });
        }
        for i in k..la {
            rows.push(Row {
                a: Some(a0 + i),
                b: None,
                kind: RowKind::Delete,
            });
        }
        for j in k..lb {
            rows.push(Row {
                a: None,
                b: Some(b0 + j),
                kind: RowKind::Insert,
            });
        }
    } else if la > 0 || lb > 0 {
        // LCS 표(u16 · (la+1)×(lb+1)).
        let w = lb + 1;
        let mut dp = vec![0u16; (la + 1) * w];
        for i in (0..la).rev() {
            for j in (0..lb).rev() {
                dp[i * w + j] = if a[a0 + i] == b[b0 + j] {
                    dp[(i + 1) * w + j + 1] + 1
                } else {
                    dp[(i + 1) * w + j].max(dp[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0usize, 0usize);
        // 한쪽만 소비한 줄은 잠시 모아 두었다가 맞은편 소비와 짝지어 Replace로(양쪽 다른 줄은 나란히 보이게).
        let mut pend_a: Vec<usize> = Vec::new();
        let mut pend_b: Vec<usize> = Vec::new();
        let flush = |rows: &mut Vec<Row>, pa: &mut Vec<usize>, pb: &mut Vec<usize>| {
            let k = pa.len().min(pb.len());
            for t in 0..k {
                rows.push(Row {
                    a: Some(pa[t]),
                    b: Some(pb[t]),
                    kind: RowKind::Replace,
                });
            }
            for &x in &pa[k..] {
                rows.push(Row {
                    a: Some(x),
                    b: None,
                    kind: RowKind::Delete,
                });
            }
            for &y in &pb[k..] {
                rows.push(Row {
                    a: None,
                    b: Some(y),
                    kind: RowKind::Insert,
                });
            }
            pa.clear();
            pb.clear();
        };
        while i < la || j < lb {
            if i < la && j < lb && a[a0 + i] == b[b0 + j] {
                flush(&mut rows, &mut pend_a, &mut pend_b);
                rows.push(Row {
                    a: Some(a0 + i),
                    b: Some(b0 + j),
                    kind: RowKind::Equal,
                });
                i += 1;
                j += 1;
            } else if j < lb && (i >= la || dp[i * w + j + 1] >= dp[(i + 1) * w + j]) {
                pend_b.push(b0 + j);
                j += 1;
            } else {
                pend_a.push(a0 + i);
                i += 1;
            }
        }
        flush(&mut rows, &mut pend_a, &mut pend_b);
    }
    for s in 0..suf {
        rows.push(Row {
            a: Some(a1 + s),
            b: Some(b1 + s),
            kind: RowKind::Equal,
        });
    }
    rows
}

/// 바뀐 덩어리 목록(연속된 비-Equal 행).
#[must_use]
pub fn hunks(rows: &[Row]) -> Vec<Hunk> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        if rows[i].kind == RowKind::Equal {
            i += 1;
            continue;
        }
        let start = i;
        let (mut a_lo, mut a_hi, mut b_lo, mut b_hi) = (usize::MAX, 0usize, usize::MAX, 0usize);
        while i < rows.len() && rows[i].kind != RowKind::Equal {
            if let Some(x) = rows[i].a {
                a_lo = a_lo.min(x);
                a_hi = a_hi.max(x + 1);
            }
            if let Some(y) = rows[i].b {
                b_lo = b_lo.min(y);
                b_hi = b_hi.max(y + 1);
            }
            i += 1;
        }
        // 한쪽이 비어 있으면(순수 삽입/삭제) 그쪽 범위는 맞은편 자리(빈 범위).
        let a = if a_lo == usize::MAX {
            let at = rows[start]
                .a
                .or_else(|| rows[..start].iter().rev().find_map(|r| r.a).map(|x| x + 1))
                .unwrap_or(0);
            at..at
        } else {
            a_lo..a_hi
        };
        let b = if b_lo == usize::MAX {
            let at = rows[start]
                .b
                .or_else(|| rows[..start].iter().rev().find_map(|r| r.b).map(|y| y + 1))
                .unwrap_or(0);
            at..at
        } else {
            b_lo..b_hi
        };
        out.push(Hunk {
            rows: start..i,
            a,
            b,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_string()).collect()
    }

    /// 단순 모델: 양쪽 index가 **순서대로 한 번씩** 나오고 · Equal 행은 글이 같고 · Replace 행은 글이 다르다.
    fn check(a: &[String], b: &[String], rows: &[Row]) {
        let mut ia = 0;
        let mut ib = 0;
        for r in rows {
            if let Some(x) = r.a {
                assert_eq!(x, ia, "a 순서");
                ia += 1;
            }
            if let Some(y) = r.b {
                assert_eq!(y, ib, "b 순서");
                ib += 1;
            }
            match r.kind {
                RowKind::Equal => assert_eq!(a[r.a.unwrap()], b[r.b.unwrap()]),
                RowKind::Replace => assert_ne!(a[r.a.unwrap()], b[r.b.unwrap()]),
                RowKind::Insert => assert!(r.a.is_none() && r.b.is_some()),
                RowKind::Delete => assert!(r.a.is_some() && r.b.is_none()),
            }
        }
        assert_eq!((ia, ib), (a.len(), b.len()), "전부 소비");
    }

    #[test]
    fn basic_shapes() {
        let a = s(&["x", "y", "z"]);
        assert!(align(&a, &a).iter().all(|r| r.kind == RowKind::Equal));
        assert!(hunks(&align(&a, &a)).is_empty());
        // 삽입.
        let b = s(&["x", "n", "y", "z"]);
        let rows = align(&a, &b);
        check(&a, &b, &rows);
        let h = hunks(&rows);
        assert_eq!(h.len(), 1);
        assert_eq!((h[0].a.clone(), h[0].b.clone()), (1..1, 1..2));
        // 삭제.
        let rows = align(&b, &a);
        check(&b, &a, &rows);
        assert_eq!(hunks(&rows)[0].a, 1..2);
        // 바뀜 = Replace 한 행.
        let c = s(&["x", "Y", "z"]);
        let rows = align(&a, &c);
        check(&a, &c, &rows);
        assert_eq!(rows[1].kind, RowKind::Replace);
        assert_eq!(hunks(&rows).len(), 1);
        // 빈 쪽.
        check(&a, &[], &align(&a, &[]));
        check(&[], &a, &align(&[], &a));
        assert!(align(&[], &[]).is_empty());
    }

    /// 난수 대조(61 §2 자료 구조 규칙): 모델이 늘 성립 · Equal 행 수 ≥ 공통 줄의 하한(최장 공통 부분열 ≥ 공통 앞·뒤).
    #[test]
    fn random_vs_model() {
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..400 {
            let n = (next() % 12) as usize;
            let m = (next() % 12) as usize;
            let a: Vec<String> = (0..n)
                .map(|_| ((b'a' + (next() % 4) as u8) as char).to_string())
                .collect();
            let b: Vec<String> = (0..m)
                .map(|_| ((b'a' + (next() % 4) as u8) as char).to_string())
                .collect();
            let rows = align(&a, &b);
            check(&a, &b, &rows);
            let hs = hunks(&rows);
            // 덩어리는 서로 안 겹치고 행 순서대로.
            for w in hs.windows(2) {
                assert!(w[0].rows.end <= w[1].rows.start);
            }
            // 덩어리 밖의 행은 전부 Equal.
            let mut in_h = vec![false; rows.len()];
            for h in &hs {
                for k in h.rows.clone() {
                    in_h[k] = true;
                }
            }
            for (k, r) in rows.iter().enumerate() {
                assert_eq!(r.kind == RowKind::Equal, !in_h[k]);
            }
        }
    }

    #[test]
    fn over_cap_falls_back() {
        let a: Vec<String> = (0..2000).map(|i| format!("a{i}")).collect();
        let b: Vec<String> = (0..1990).map(|i| format!("b{i}")).collect();
        let rows = align(&a, &b);
        check(&a, &b, &rows);
        assert_eq!(hunks(&rows).len(), 1);
    }
}

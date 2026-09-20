//! **줄 단위 3-way 병합**(diff3 · nexa-sql docs/58 · 사용자 09-19 "외부에서 바뀐 파일을 병합해 문제없으면 바로 반영").
//!
//! `base`(마지막으로 읽거나 저장한 내용) · `ours`(편집기 버퍼) · `theirs`(디스크)를 받아, 양쪽이 **서로 다른 곳**을 고쳤으면
//! 합친 결과를 주고 같은 곳(또는 맞닿은 곳)을 고쳤으면 충돌로 센다 — 충돌이 하나라도 있으면 호출자는 결과를 쓰지 않고
//! 사용자에게 묻는다. git보다 보수적이다: 안정된 줄(세 쪽이 모두 같은 줄)이 사이에 없으면 한 덩어리로 본다.
//!
//! 정합은 Myers O(ND) — 차이가 `MAX_D`를 넘으면 포기(`None`)한다(그때는 병합하지 말고 물어야 한다). 외부 crate 0.

/// 편집 거리 상한 — 넘으면 정합을 포기한다(거대한 재작성 = 자동 병합 대상이 아니다 · 되짚기 기록 = D² 칸 ≈ 18 MB 상한).
const MAX_D: usize = 1500;

/// 병합 결과.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Merge3 {
    /// 합친 본문(충돌 덩어리는 `ours` 쪽을 넣어 둔다 — `conflicts > 0`이면 쓰지 말 것).
    pub text: String,
    /// 같은 곳을 양쪽이 다르게 고친 덩어리 수.
    pub conflicts: usize,
    /// `theirs`에서 가져온 덩어리 수(0 = 디스크 변경이 버퍼에 이미 들어 있다).
    pub applied: usize,
    /// 결과에서 `theirs`가 들어간 줄 범위(0-기준 · `[from, to)`) — 거터 표시용.
    pub applied_lines: Vec<(usize, usize)>,
}

/// `a`의 줄 i가 `b`의 줄 j와 같다 = `out[i] = Some(j)`(LCS 정합 · 단조 증가). 차이가 너무 크면 `None`.
fn match_lines(a: &[&str], b: &[&str]) -> Option<Vec<Option<usize>>> {
    let (n, m) = (a.len(), b.len());
    let mut out = vec![None; n];
    // 공통 앞·뒤를 먼저 걷는다(대부분의 편집은 국소적이다).
    let mut pre = 0;
    while pre < n && pre < m && a[pre] == b[pre] {
        out[pre] = Some(pre);
        pre += 1;
    }
    let mut suf = 0;
    while suf < n - pre && suf < m - pre && a[n - 1 - suf] == b[m - 1 - suf] {
        out[n - 1 - suf] = Some(m - 1 - suf);
        suf += 1;
    }
    let (a, b) = (&a[pre..n - suf], &b[pre..m - suf]);
    let (n, m) = (a.len(), b.len());
    if n == 0 || m == 0 {
        return Some(out);
    }
    // Myers: v[k] = 대각선 k에서 가장 멀리 간 x. 단계마다 v를 남겨 되짚는다.
    let max = (n + m).min(MAX_D);
    let off = max as isize + 1;
    let mut v = vec![0isize; 2 * max + 3];
    let mut trace: Vec<Vec<isize>> = Vec::new();
    let mut found = None;
    'outer: for d in 0..=max as isize {
        // 되짚기에 필요한 창(k = -d-1 ..= d+1)만 남긴다 — 전체 v를 복사하면 D × (n+m)이 된다.
        let (w0, w1) = ((off - d - 1) as usize, (off + d + 1) as usize);
        trace.push(v[w0..=w1].to_vec());
        let mut k = -d;
        while k <= d {
            let idx = (k + off) as usize;
            let mut x = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) {
                v[idx + 1]
            } else {
                v[idx - 1] + 1
            };
            let mut y = x - k;
            while (x as usize) < n && (y as usize) < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[idx] = x;
            if x as usize >= n && y as usize >= m {
                found = Some(d);
                break 'outer;
            }
            k += 2;
        }
    }
    let d_end = found?;
    // 되짚기 — 대각선 구간(= 같은 줄)을 정합으로 적는다.
    let (mut x, mut y) = (n as isize, m as isize);
    for d in (0..=d_end).rev() {
        let v = &trace[d as usize];
        let k = x - y;
        // 창의 색인: k = -d-1이 0번.
        let idx = (k + d + 1) as usize;
        let prev_k = if k == -d || (k != d && v[idx - 1] < v[idx + 1]) {
            k + 1
        } else {
            k - 1
        };
        let prev_x = v[(prev_k + d + 1) as usize];
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y && x > 0 && y > 0 {
            x -= 1;
            y -= 1;
            out[pre + x as usize] = Some(pre + y as usize);
        }
        if d > 0 {
            x = prev_x;
            y = prev_y;
        }
    }
    Some(out)
}

/// **최소 줄 편집**(nexa-sql docs/60 D-132 · VS Code `_computeEdits`와 같은 생각): `old`를 `new`로 만드는 편집 목록 —
/// `old`의 **글자 인덱스** `(from, to, 넣을 글)` · 오름차순 · 비겹침. 같은 줄은 건드리지 않으므로 외부 변경을 받아들여도
/// 되돌리기 기록이 "바뀐 줄들"만 든다(종전 = 첫 차이부터 끝 차이까지 한 덩이 — 위아래 한 줄씩만 달라도 본문 전체).
/// 차이가 너무 커서 정합을 포기했으면 `None`(호출자는 한 덩이 교체로 물러난다). 같으면 빈 목록.
#[must_use]
pub fn line_edits(old: &str, new: &str) -> Option<Vec<(usize, usize, String)>> {
    let a: Vec<&str> = old.split('\n').collect();
    let b: Vec<&str> = new.split('\n').collect();
    let map = match_lines(&a, &b)?;
    // 줄 i의 시작 글자 인덱스(끝에 본문 길이 + 1을 하나 더 — "마지막 줄 다음 줄의 시작").
    let mut starts: Vec<usize> = Vec::with_capacity(a.len() + 1);
    let mut at = 0usize;
    for l in &a {
        starts.push(at);
        at += l.chars().count() + 1;
    }
    let total = at - 1;
    let (n, nb) = (a.len(), b.len());
    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n || j < nb {
        if i < n && map[i] == Some(j) {
            i += 1;
            j += 1;
            continue;
        }
        // 어긋난 덩이: old[i0..i1) → new[j0..j1) — 다음 정합(또는 끝)까지.
        let (i0, j0) = (i, j);
        while i < n && map[i].is_none() {
            i += 1;
        }
        let j1 = if i < n { map[i].unwrap_or(nb) } else { nb };
        j = j1;
        let (i1, at_end) = (i, i == n);
        if !at_end {
            // 가운데 덩이: 줄들을 개행째로 바꾼다.
            let mut ins = String::new();
            for l in &b[j0..j1] {
                ins.push_str(l);
                ins.push('\n');
            }
            out.push((starts[i0], starts[i1], ins));
        } else if i0 < i1 && j0 < j1 {
            // 끝 덩이(양쪽에 줄이 있다): 마지막 줄에는 개행이 없다.
            out.push((starts[i0], total, b[j0..j1].join("\n")));
        } else if j0 < j1 {
            // 끝에 덧붙임: 앞 줄의 끝에 개행부터.
            out.push((total, total, format!("\n{}", b[j0..j1].join("\n"))));
        } else if i0 < i1 {
            // 끝의 줄들을 지움: 앞 줄의 개행까지 함께(앞 줄이 없으면 본문 전체가 빈 글이 된다).
            out.push((starts[i0].saturating_sub(1), total, String::new()));
        }
    }
    Some(out)
}

/// 3-way 병합. 정합을 포기했으면(차이가 너무 큼) `None` — 호출자는 병합하지 말고 물어야 한다.
#[must_use]
pub fn merge3(base: &str, ours: &str, theirs: &str) -> Option<Merge3> {
    let o: Vec<&str> = base.split('\n').collect();
    let a: Vec<&str> = ours.split('\n').collect();
    let b: Vec<&str> = theirs.split('\n').collect();
    let ma = match_lines(&o, &a)?;
    let mb = match_lines(&o, &b)?;
    let mut out: Vec<&str> = Vec::with_capacity(a.len().max(b.len()));
    let (mut conflicts, mut applied) = (0usize, 0usize);
    let mut applied_lines = Vec::new();
    let (mut lo, mut la, mut lb) = (0usize, 0usize, 0usize);
    loop {
        // 안정 구간: 세 쪽이 나란히 같은 줄.
        let mut i = 0;
        while lo + i < o.len() && ma[lo + i] == Some(la + i) && mb[lo + i] == Some(lb + i) {
            i += 1;
        }
        if i > 0 {
            out.extend_from_slice(&o[lo..lo + i]);
            lo += i;
            la += i;
            lb += i;
            continue;
        }
        // 불안정 덩어리: 다음으로 양쪽 모두에 정합된 base 줄까지.
        let next = (lo..o.len())
            .find(|&k| matches!((ma[k], mb[k]), (Some(x), Some(y)) if x >= la && y >= lb));
        let (eo, ea, eb) = match next {
            Some(k) => (k, ma[k].unwrap_or(a.len()), mb[k].unwrap_or(b.len())),
            None => (o.len(), a.len(), b.len()),
        };
        if lo == eo && la == ea && lb == eb {
            break;
        }
        let (co, ca, cb) = (&o[lo..eo], &a[la..ea], &b[lb..eb]);
        if cb == co || ca == cb {
            // 디스크는 그대로(또는 양쪽이 같은 수정) → 우리 것.
            out.extend_from_slice(ca);
        } else if ca == co {
            // 우리는 그대로 → 디스크 것을 가져온다.
            let from = out.len();
            out.extend_from_slice(cb);
            applied += 1;
            applied_lines.push((from, out.len()));
        } else {
            conflicts += 1;
            out.extend_from_slice(ca);
        }
        lo = eo;
        la = ea;
        lb = eb;
        if next.is_none() {
            break;
        }
    }
    Some(Merge3 {
        text: out.join("\n"),
        conflicts,
        applied,
        applied_lines,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// 최소 줄 편집: 뒤에서부터 적용하면 `new`가 된다(빈 글 · 끝 개행 유무 · 끝에 덧붙임/지움 · 가운데 · 여러 덩이 · 한글) ·
    /// 같은 줄은 편집에 들지 않는다.
    #[test]
    fn line_edits_rebuild_new_text() {
        fn apply(old: &str, edits: &[(usize, usize, String)]) -> String {
            let mut c: Vec<char> = old.chars().collect();
            for (a, b, s) in edits.iter().rev() {
                c.splice(*a..*b, s.chars());
            }
            c.into_iter().collect()
        }
        let texts = [
            "",
            "a",
            "a\n",
            "\n",
            "a\nb\nc",
            "a\nb\nc\n",
            "a\nB\nc",
            "x\na\nb\nc",
            "a\nb\nc\nd\ne",
            "a\nc",
            "한글\n줄\n셋",
            "한글\n줄 바뀜\n셋\n넷\n",
            "top\na\nb\nc\nbottom",
            "TOP\na\nb\nc\nBOTTOM",
        ];
        for old in texts {
            for new in texts {
                let edits = line_edits(old, new).expect("small diff");
                assert_eq!(apply(old, &edits), new, "{old:?} → {new:?} via {edits:?}");
                assert!(
                    edits.windows(2).all(|w| w[0].1 <= w[1].0),
                    "오름차순·비겹침"
                );
                if old == new {
                    assert!(edits.is_empty());
                }
            }
        }
        // 위아래 한 줄씩만 다르면 가운데 줄들은 편집에 없다(덩이 둘 · 넣는 글이 짧다).
        let e = line_edits("top\na\nb\nc\nbottom", "TOP\na\nb\nc\nBOTTOM").expect("diff");
        assert_eq!(e.len(), 2);
        assert!(e.iter().all(|x| x.2.len() <= 7));
    }

    const BASE: &str = "select a,\n       b,\n       c\n  from t\n where x = 1\n order by a;\n";

    #[test]
    fn disjoint_edits_merge_cleanly() {
        let ours = BASE.replace("       b,", "       b2,");
        let theirs = BASE.replace(" where x = 1", " where x = 2\n   and y = 3");
        let m = merge3(BASE, &ours, &theirs).unwrap();
        assert_eq!(m.conflicts, 0);
        assert_eq!(m.applied, 1);
        assert_eq!(
            m.text,
            "select a,\n       b2,\n       c\n  from t\n where x = 2\n   and y = 3\n order by a;\n"
        );
        assert_eq!(m.applied_lines, vec![(4, 6)]);
    }

    #[test]
    fn same_region_is_a_conflict_and_keeps_ours() {
        let ours = BASE.replace("x = 1", "x = 10");
        let theirs = BASE.replace("x = 1", "x = 20");
        let m = merge3(BASE, &ours, &theirs).unwrap();
        assert_eq!((m.conflicts, m.applied), (1, 0));
        assert_eq!(m.text, ours);
        // 맞닿은 줄(사이에 안정 줄 없음)도 충돌 — git보다 보수적.
        let ours = BASE.replace("       b,", "       B,");
        let theirs = BASE.replace("       c\n", "       C\n");
        assert_eq!(merge3(BASE, &ours, &theirs).unwrap().conflicts, 1);
    }

    #[test]
    fn trivial_sides() {
        let theirs = BASE.replace("order by a", "order by b");
        // 우리가 안 고쳤으면 = 디스크 그대로.
        let m = merge3(BASE, BASE, &theirs).unwrap();
        assert_eq!(
            (m.conflicts, m.applied, m.text.as_str()),
            (0, 1, theirs.as_str())
        );
        // 디스크가 그대로면 = 우리 것 · 가져온 것 0.
        let m = merge3(BASE, &theirs, BASE).unwrap();
        assert_eq!(
            (m.conflicts, m.applied, m.text.as_str()),
            (0, 0, theirs.as_str())
        );
        // 양쪽이 같은 수정 = 충돌 아님.
        let m = merge3(BASE, &theirs, &theirs).unwrap();
        assert_eq!(
            (m.conflicts, m.applied, m.text.as_str()),
            (0, 0, theirs.as_str())
        );
        // 빈 글 · 추가/삭제만.
        let m = merge3("", "a\n", "").unwrap();
        assert_eq!((m.conflicts, m.text.as_str()), (0, "a\n"));
        let m = merge3("a\nb\nc\nd\ne\n", "a\nc\nd\ne\n", "a\nb\nc\nd\ne\nf\n").unwrap();
        assert_eq!((m.conflicts, m.text.as_str()), (0, "a\nc\nd\ne\nf\n"));
    }

    #[test]
    fn insertions_at_both_ends_and_large_input() {
        let base: String = (0..3000).map(|i| format!("line {i}\n")).collect();
        let ours = format!("-- mine\n{base}");
        let theirs = format!("{base}-- theirs\n");
        let m = merge3(&base, &ours, &theirs).unwrap();
        assert_eq!(m.conflicts, 0);
        assert!(
            m.text.starts_with("-- mine\nline 0\n") && m.text.ends_with("line 2999\n-- theirs\n")
        );
        // 전혀 다른 글 = 한 덩어리 충돌(또는 포기) — 어느 쪽이든 자동 반영은 안 된다.
        let other: String = (0..50).map(|i| format!("x{i}\n")).collect();
        assert!(merge3(&base, &ours, &other).is_none_or(|m| m.conflicts > 0));
    }

    #[test]
    fn match_lines_is_monotonic() {
        let a: Vec<&str> = "a b c d e f".split(' ').collect();
        let b: Vec<&str> = "a x c d y f g".split(' ').collect();
        let m = match_lines(&a, &b).unwrap();
        assert_eq!(m, vec![Some(0), None, Some(2), Some(3), None, Some(5)]);
    }
}

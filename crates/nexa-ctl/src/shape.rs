//! 점-도형 판정 원시 함수(아이콘 래스터용) — "점 `(x, y)`가 이 도형 안인가"만 답한다.
//!
//! 글리프(`controls::glyphs`)와 앱의 아이콘 모음(nexa-sql 탐색기·툴바 아이콘)이 같은 식을 각자 복사해 쓰던 것을
//! 한 곳으로(nexa-sql docs/93 §4). 좌표계 = 아이콘 단위 격자(보통 0..24) · 안티에일리어싱은 호출측이 샘플을 여러 번 찍어 낸다.

/// 점 `(x, y)`에서 선분 `a–b`까지의 거리.
#[must_use]
#[inline]
pub fn seg_dist(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (vx, vy) = (bx - ax, by - ay);
    let (wx, wy) = (x - ax, y - ay);
    let t = ((wx * vx + wy * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    let (px, py) = (ax + t * vx, ay + t * vy);
    ((x - px) * (x - px) + (y - py) * (y - py)).sqrt()
}

/// 폭 `w`인 획(선분 `a–b`) 안인가.
#[must_use]
#[inline]
pub fn stroke(x: f32, y: f32, a: (f32, f32), b: (f32, f32), w: f32) -> bool {
    seg_dist(x, y, a.0, a.1, b.0, b.1) <= w / 2.0
}

/// 축 정렬 사각형 `[x0, x1] × [y0, y1]` 안인가.
#[must_use]
#[inline]
pub fn rect(x: f32, y: f32, x0: f32, x1: f32, y0: f32, y1: f32) -> bool {
    (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
}

/// 모서리 반지름 `r`인 둥근 사각형(`x0, y0, w, h`) 안인가.
#[must_use]
#[inline]
pub fn rrect(x: f32, y: f32, x0: f32, y0: f32, w: f32, h: f32, r: f32) -> bool {
    if x < x0 || y < y0 || x > x0 + w || y > y0 + h {
        return false;
    }
    let cx = x.clamp(x0 + r, x0 + w - r);
    let cy = y.clamp(y0 + r, y0 + h - r);
    (x - cx) * (x - cx) + (y - cy) * (y - cy) <= r * r
}

/// 원판(중심 `cx, cy` · 반지름 `r`) 안인가.
#[must_use]
#[inline]
pub fn disc(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> bool {
    (x - cx) * (x - cx) + (y - cy) * (y - cy) <= r * r
}

/// 고리(안 반지름 `r_in` ~ 바깥 `r_out`) 안인가.
#[must_use]
#[inline]
pub fn ring(x: f32, y: f32, cx: f32, cy: f32, r_in: f32, r_out: f32) -> bool {
    let d = ((x - cx) * (x - cx) + (y - cy) * (y - cy)).sqrt();
    (r_in..=r_out).contains(&d)
}

/// 타원(중심 · 반지름 `rx, ry`) 안인가.
#[must_use]
#[inline]
pub fn ellipse(x: f32, y: f32, cx: f32, cy: f32, rx: f32, ry: f32) -> bool {
    let (dx, dy) = ((x - cx) / rx, (y - cy) / ry);
    dx * dx + dy * dy <= 1.0
}

/// 삼각형 `a b c` 안인가(방향 무관).
#[must_use]
#[inline]
pub fn tri(x: f32, y: f32, a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let s = |p: (f32, f32), q: (f32, f32)| (x - q.0) * (p.1 - q.1) - (p.0 - q.0) * (y - q.1);
    let (d1, d2, d3) = (s(a, b), s(b, c), s(c, a));
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// 다각형(짝홀 규칙) 안인가.
#[must_use]
#[inline]
pub fn poly(x: f32, y: f32, pts: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let n = pts.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = pts[i];
        let (xj, yj) = pts[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// 여러 다각형 — 짝홀 규칙(구멍 = 겹친 짝수 번).
#[must_use]
#[inline]
pub fn polys_evenodd(x: f32, y: f32, polys: &[Vec<(f32, f32)>]) -> bool {
    let mut inside = false;
    for p in polys {
        let n = p.len();
        if n < 3 {
            continue;
        }
        for i in 0..n {
            let (x0, y0) = p[i];
            let (x1, y1) = p[(i + 1) % n];
            if (y0 > y) != (y1 > y) && x < x0 + (y - y0) * (x1 - x0) / (y1 - y0) {
                inside = !inside;
            }
        }
    }
    inside
}

/// 여러 다각형 — 0 아닌 감김 수 규칙(SVG `nonzero`).
#[must_use]
#[inline]
pub fn polys_nonzero(x: f32, y: f32, polys: &[Vec<(f32, f32)>]) -> bool {
    let mut wn = 0i32;
    for p in polys {
        let n = p.len();
        if n < 3 {
            continue;
        }
        for i in 0..n {
            let (x0, y0) = p[i];
            let (x1, y1) = p[(i + 1) % n];
            if y0 <= y {
                if y1 > y && (x1 - x0) * (y - y0) - (x - x0) * (y1 - y0) > 0.0 {
                    wn += 1;
                }
            } else if y1 <= y && (x1 - x0) * (y - y0) - (x - x0) * (y1 - y0) < 0.0 {
                wn -= 1;
            }
        }
    }
    wn != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_answer_inside_outside() {
        assert!((seg_dist(0.0, 1.0, -1.0, 0.0, 1.0, 0.0) - 1.0).abs() < 1e-6);
        assert!(stroke(0.0, 0.4, (-1.0, 0.0), (1.0, 0.0), 1.0));
        assert!(!stroke(0.0, 0.6, (-1.0, 0.0), (1.0, 0.0), 1.0));
        assert!(rect(1.0, 1.0, 0.0, 2.0, 0.0, 2.0) && !rect(3.0, 1.0, 0.0, 2.0, 0.0, 2.0));
        assert!(rrect(5.0, 5.0, 0.0, 0.0, 10.0, 10.0, 2.0));
        assert!(
            !rrect(0.1, 0.1, 0.0, 0.0, 10.0, 10.0, 2.0),
            "둥근 모서리 바깥"
        );
        assert!(disc(1.0, 0.0, 0.0, 0.0, 1.5) && !disc(2.0, 0.0, 0.0, 0.0, 1.5));
        assert!(ring(2.0, 0.0, 0.0, 0.0, 1.0, 3.0) && !ring(0.5, 0.0, 0.0, 0.0, 1.0, 3.0));
        assert!(ellipse(2.5, 0.0, 0.0, 0.0, 3.0, 1.0) && !ellipse(0.0, 1.5, 0.0, 0.0, 3.0, 1.0));
        assert!(tri(1.0, 1.0, (0.0, 0.0), (4.0, 0.0), (0.0, 4.0)));
        assert!(
            tri(1.0, 1.0, (0.0, 0.0), (0.0, 4.0), (4.0, 0.0)),
            "방향 무관"
        );
        assert!(!tri(3.0, 3.0, (0.0, 0.0), (4.0, 0.0), (0.0, 4.0)));
        let sq = [(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)];
        assert!(poly(2.0, 2.0, &sq) && !poly(5.0, 2.0, &sq));
        let hole = vec![
            sq.to_vec(),
            vec![(1.0, 1.0), (3.0, 1.0), (3.0, 3.0), (1.0, 3.0)],
        ];
        assert!(!polys_evenodd(2.0, 2.0, &hole), "짝홀 = 겹친 곳은 구멍");
        assert!(polys_evenodd(0.5, 0.5, &hole));
    }
}

//! 정수 픽셀 기하 — 창 클라이언트 좌표계(좌상 원점, px).
//!
//! `nexa-dir2/crates/nexa-gui/src/geom.rs` 이식([docs/12 §A] — 플랫폼 참조 0의 검증 자산).

/// 점(px).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Point {
    /// x(px).
    pub x: i32,
    /// y(px).
    pub y: i32,
}

/// 크기(px).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Size {
    /// 폭(px).
    pub w: i32,
    /// 높이(px).
    pub h: i32,
}

/// 사각형(px) — `contains`는 반열린 구간(`[x, x+w)`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rect {
    /// 왼쪽.
    pub x: i32,
    /// 위.
    pub y: i32,
    /// 폭.
    pub w: i32,
    /// 높이.
    pub h: i32,
}

impl Rect {
    /// 생성.
    #[must_use]
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }

    /// 오른쪽 경계(exclusive).
    #[must_use]
    pub const fn right(&self) -> i32 {
        self.x + self.w
    }

    /// 아래 경계(exclusive).
    #[must_use]
    pub const fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// 크기.
    #[must_use]
    pub const fn size(&self) -> Size {
        Size {
            w: self.w,
            h: self.h,
        }
    }

    /// 넓이 0 여부.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    /// 점 포함(반열린).
    #[must_use]
    pub const fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// 교차 여부(변 접촉은 비교차).
    #[must_use]
    pub fn intersects(&self, other: &Rect) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// 두 rect를 덮는 최소 rect. 빈 rect는 항등원.
    #[must_use]
    /// 교집합(겹치지 않으면 빈 rect) — 클립 영역 계산용(nexa-sql 그리드 · 09-12).
    pub fn intersection(&self, other: &Rect) -> Rect {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = self.right().min(other.right());
        let y2 = self.bottom().min(other.bottom());
        if x2 <= x1 || y2 <= y1 {
            return Rect::new(x1, y1, 0, 0);
        }
        Rect::new(x1, y1, x2 - x1, y2 - y1)
    }

    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect {
            x,
            y,
            w: self.right().max(other.right()) - x,
            h: self.bottom().max(other.bottom()) - y,
        }
    }
}

/// ★ **팝업 배치 규칙**(우클릭 메뉴 · 드롭다운 · 툴팁 공용 · nexa-sql 사용자 09-21 "화면에서 잘리지 않게"): 축마다
/// ① 기준점에서 **정방향**(오른쪽·아래)으로 놓아 `host` 안에 들어가면 그대로 ② 아니면 **반대쪽**(왼쪽·위 — 끝이 기준점에 닿게)이
/// 들어가면 그쪽 ③ 어느 쪽도 안 되면 `host` 안으로 **밀어 넣는다**(끝을 `host` 끝에 맞추고, 그래도 크면 시작을 `host` 시작에).
/// 팝업이 `host`보다 클 때만 잘린다(그때는 스크롤이 필요하다 — 이 함수의 일이 아니다).
#[must_use]
pub fn place_popup(anchor: Point, size: (i32, i32), host: Rect) -> Point {
    let axis = |at: i32, len: i32, lo: i32, hi: i32| -> i32 {
        if at >= lo && at + len <= hi {
            at
        } else if at - len >= lo && at <= hi {
            at - len
        } else {
            // 가장 가까운 자리로 밀어 넣는다(넘친 쪽의 끝을 맞춘다 · host보다 크면 시작을 맞춘다).
            at.min(hi - len).max(lo)
        }
    };
    Point {
        x: axis(anchor.x, size.0, host.x, host.right()),
        y: axis(anchor.y, size.1, host.y, host.bottom()),
    }
}

/// ★ **대상을 가리지 않는 배치**(nexa-sql 사용자 09-21 — 트리 우클릭 메뉴: "누른 자리에 가깝게 · 대상 이름은 온전히 보이게"):
/// 세로 = `avoid`(대상 행) **바로 아래** → 안 들어가면 **바로 위** → 둘 다 안 되면 [`place_popup`]의 일반 규칙(그때만 가린다).
/// 가로 = 누른 x에서 일반 규칙(정방향 → 반대쪽 → 밀어 넣기). 메뉴가 행에 붙어 있으므로 누른 자리에서 한 행 높이 이상 멀어지지 않는다.
#[must_use]
pub fn place_popup_beside(anchor: Point, avoid: Rect, size: (i32, i32), host: Rect) -> Point {
    let free = place_popup(anchor, size, host);
    let below = avoid.bottom();
    let above = avoid.y - size.1;
    let y = if below >= host.y && below + size.1 <= host.bottom() {
        below
    } else if above >= host.y && avoid.y <= host.bottom() {
        above
    } else {
        free.y
    };
    Point { x: free.x, y }
}

/// `r`을 `host` 안으로 **옮긴다**(크기는 그대로 · 넘친 쪽의 끝을 맞춘다 · `host`보다 크면 시작을 맞춘다) — 이미 놓인 팝업의 안전망.
#[must_use]
pub fn nudge_into(r: Rect, host: Rect) -> Rect {
    let axis = |at: i32, len: i32, lo: i32, hi: i32| at.min(hi - len).max(lo);
    Rect::new(
        axis(r.x, r.w, host.x, host.right()),
        axis(r.y, r.h, host.y, host.bottom()),
        r.w,
        r.h,
    )
}

/// 호출자가 준 `host`와 그리기 표면(있으면)의 **겹침** — 호출자가 "끝없는 영역"을 넘겨도 표면 밖으로는 나가지 않게.
#[must_use]
pub fn popup_host(host: Rect, surface: Option<(i32, i32)>) -> Rect {
    match surface {
        Some((w, h)) if w > 0 && h > 0 => {
            let both = host.intersection(&Rect::new(0, 0, w, h));
            if both.is_empty() {
                Rect::new(0, 0, w, h)
            } else {
                both
            }
        }
        _ => host,
    }
}

#[cfg(test)]
mod popup_tests {
    use super::*;

    /// 배치 규칙: 정방향 → 반대쪽 → 밀어 넣기 · 축은 서로 독립 · 끝없는 host는 표면으로 줄인다.
    #[test]
    fn popups_stay_inside_the_host() {
        let host = Rect::new(0, 0, 800, 600);
        let at = |x, y| Point { x, y };
        assert_eq!(place_popup(at(100, 100), (200, 300), host), at(100, 100));
        // 아래로 넘침 → 위로(끝이 기준점에 닿게).
        assert_eq!(place_popup(at(100, 500), (200, 300), host), at(100, 200));
        // 오른쪽으로 넘침 → 왼쪽으로.
        assert_eq!(place_popup(at(700, 100), (200, 300), host), at(500, 100));
        // 위·아래 어느 쪽도 안 들어감(기준점이 가운데 · 팝업이 큼) → 밀어 넣는다.
        assert_eq!(place_popup(at(100, 250), (200, 400), host), at(100, 200));
        // host보다 크면 시작을 맞춘다.
        assert_eq!(place_popup(at(100, 250), (200, 900), host), at(100, 0));
        // 기준점이 host 밖이어도 안으로.
        assert_eq!(place_popup(at(-50, 700), (100, 100), host), at(0, 500));
        // 대상을 가리지 않는 배치: 행 바로 아래 → 바로 위 → (둘 다 안 되면) 일반 규칙.
        let row = Rect::new(0, 100, 300, 24);
        assert_eq!(
            place_popup_beside(at(80, 110), row, (200, 300), host),
            at(80, 124)
        );
        let low = Rect::new(0, 500, 300, 24);
        assert_eq!(
            place_popup_beside(at(80, 510), low, (200, 300), host),
            at(80, 200),
            "아래에 자리가 없으면 행 바로 위"
        );
        let mid = Rect::new(0, 290, 300, 24);
        assert_eq!(
            place_popup_beside(at(80, 300), mid, (200, 400), host),
            place_popup(at(80, 300), (200, 400), host),
            "위·아래 모두 안 되면 일반 규칙"
        );
        assert_eq!(
            place_popup_beside(at(700, 110), row, (200, 300), host),
            at(500, 124),
            "가로는 일반 규칙"
        );
        // host가 (0,0)에서 시작하지 않을 때.
        let sub = Rect::new(200, 100, 300, 200);
        assert_eq!(place_popup(at(450, 250), (100, 100), sub), at(350, 150));

        assert_eq!(
            nudge_into(Rect::new(750, 580, 100, 50), host),
            Rect::new(700, 550, 100, 50)
        );
        assert_eq!(
            nudge_into(Rect::new(10, 10, 100, 50), host),
            Rect::new(10, 10, 100, 50)
        );
        assert_eq!(
            nudge_into(Rect::new(-5, -5, 900, 50), host),
            Rect::new(0, 0, 900, 50)
        );

        let endless = Rect::new(0, 0, i32::MAX / 2, i32::MAX / 2);
        assert_eq!(popup_host(endless, Some((800, 600))), host);
        assert_eq!(popup_host(endless, None), endless);
        assert_eq!(popup_host(sub, Some((800, 600))), sub);
        assert_eq!(
            popup_host(Rect::new(900, 900, 10, 10), Some((800, 600))),
            host
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_is_half_open() {
        let r = Rect::new(0, 0, 10, 10);
        assert!(r.contains(Point { x: 0, y: 0 }));
        assert!(r.contains(Point { x: 9, y: 9 }));
        assert!(!r.contains(Point { x: 10, y: 9 }));
    }

    #[test]
    fn union_with_empty_is_identity() {
        let r = Rect::new(5, 5, 10, 10);
        assert_eq!(r.union(&Rect::default()), r);
        assert_eq!(Rect::default().union(&r), r);
    }

    #[test]
    fn union_covers_both() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(20, 5, 10, 10);
        assert_eq!(a.union(&b), Rect::new(0, 0, 30, 15));
    }

    #[test]
    fn intersects_excludes_touching_edges() {
        let a = Rect::new(0, 0, 10, 10);
        assert!(a.intersects(&Rect::new(9, 9, 5, 5)));
        assert!(!a.intersects(&Rect::new(10, 0, 5, 5)));
        assert!(!a.intersects(&Rect::new(0, 0, 0, 5)));
    }
}

#[cfg(test)]
mod intersection_tests {
    use super::*;

    #[test]
    fn intersection_clips() {
        let a = Rect::new(0, 0, 10, 10);
        assert_eq!(
            a.intersection(&Rect::new(5, 5, 10, 10)),
            Rect::new(5, 5, 5, 5)
        );
        assert!(a.intersection(&Rect::new(20, 20, 5, 5)).is_empty());
    }
}

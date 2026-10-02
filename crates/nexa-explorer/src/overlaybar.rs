//! 오버레이 스크롤바 **공용 상태·기하**(10-02 — 하단 도크 Info/Preview 스크롤 도입).
//! 규약은 09-04 X-47 파일 목록(rows.rs)·설정 창 컨테이너와 동일: 얇은 바 6px·호버/드래그
//! 10px·알파 120(표시)/210(hot)·유지 22틱(≈900ms @40ms)·틱당 페이드 24. 축별 독립
//! (가로가 보이는 중 세로 스크롤 = 둘 다 — 사용자 확정 09-04).
//!
//! 위젯은 자기 상태(내용 길이·뷰포트·오프셋)를 [`AxisGeom`]으로 매 호출마다 공급하고,
//! 이 모듈은 썸 rect·히트·드래그·페이드만 계산한다(위젯 필드 복제 0). rows.rs의
//! 내장 구현을 이 모듈로 옮기는 것은 후속 리팩토링(TODO 참조).

use crate::draw::DrawCtx;
use crate::geom::Rect;
use crate::theme::Theme;
use crate::widget::Invalidations;

pub const BAR_THIN: i32 = 6;
pub const BAR_WIDE: i32 = 10;
pub const THUMB_MIN: i32 = 24;
pub const BAR_ALPHA: u8 = 120;
pub const BAR_ALPHA_HOT: u8 = 210;
pub const BAR_HOLD_TICKS: u8 = 22;
pub const BAR_FADE_STEP: u8 = 24;

/// 바 축(배열 인덱스 겸용).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    V = 0,
    H = 1,
}

/// 한 축의 기하 — 세로 = 행 단위·가로 = px 단위(혼용 가능 — 비례만 쓴다).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AxisGeom {
    /// 바가 놓이는 뷰포트(세로 바 = 우측 가장자리, 가로 바 = 하단 가장자리).
    pub view: Rect,
    /// 내용 전체 길이.
    pub content: i64,
    /// 뷰포트에 들어가는 길이.
    pub visible: i64,
    /// 현재 오프셋(0 ~ `max_offset`).
    pub offset: i64,
}

impl AxisGeom {
    pub fn max_offset(&self) -> i64 {
        (self.content - self.visible).max(0)
    }

    /// 내용이 뷰포트를 넘는가(= 바를 그릴 조건).
    pub fn scrollable(&self) -> bool {
        self.visible > 0 && self.content > self.visible && self.view.w > 0 && self.view.h > 0
    }
}

/// 트랙 클릭 판정 결과.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BarHit {
    /// 썸 프레스 = 드래그 시작(이후 `mouse_move`가 오프셋을 돌려준다).
    Drag,
    /// 썸 앞쪽 트랙 = 한 페이지 뒤로.
    PageBack,
    /// 썸 뒤쪽 트랙 = 한 페이지 앞으로.
    PageFwd,
}

/// 두 축의 오버레이 바 상태(표시 알파·유지·호버·드래그).
#[derive(Default, Debug)]
pub struct OverlayBars {
    alpha: [u8; 2],
    hold: [u8; 2],
    hover: Option<Axis>,
    /// (축, 프레스 좌표, 프레스 시점 오프셋).
    drag: Option<(Axis, i32, i64)>,
}

impl OverlayBars {
    /// 썸 rect — 스크롤 불가면 None. `wide` = 호버/드래그 두께.
    pub fn thumb(&self, axis: Axis, g: &AxisGeom, wide: bool) -> Option<Rect> {
        if !g.scrollable() {
            return None;
        }
        let t = if wide { BAR_WIDE } else { BAR_THIN };
        let max = g.max_offset().max(1);
        let off = g.offset.clamp(0, max);
        Some(match axis {
            Axis::V => {
                let len = g.view.h;
                let th = ((len as i64 * g.visible / g.content) as i32)
                    .max(THUMB_MIN)
                    .min(len);
                let ty = g.view.y + ((len - th) as i64 * off / max) as i32;
                Rect::new(g.view.right() - t - 2, ty, t, th)
            }
            Axis::H => {
                let len = g.view.w;
                let tw = ((len as i64 * g.visible / g.content) as i32)
                    .max(THUMB_MIN)
                    .min(len);
                let tx = g.view.x + ((len - tw) as i64 * off / max) as i32;
                Rect::new(tx, g.view.bottom() - t - 2, tw, t)
            }
        })
    }

    /// 축 트랙 스트립(무효화 영역) — 세로 = 우측·가로 = 하단.
    pub fn strip(axis: Axis, view: Rect) -> Rect {
        match axis {
            Axis::V => Rect::new(view.right() - BAR_WIDE - 4, view.y, BAR_WIDE + 4, view.h),
            Axis::H => Rect::new(view.x, view.bottom() - BAR_WIDE - 4, view.w, BAR_WIDE + 4),
        }
    }

    /// 호버/드래그 중(두꺼운 바·페이드 보류).
    pub fn hot(&self, axis: Axis) -> bool {
        self.hover == Some(axis) || matches!(self.drag, Some((a, ..)) if a == axis)
    }

    /// 바가 그려지는가(알파 > 0 또는 hot).
    pub fn visible(&self, axis: Axis) -> bool {
        self.alpha[axis as usize] > 0 || self.hot(axis)
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// 스크롤 직후 **그 축만** 표시 + 유지 → 페이드.
    pub fn flash(&mut self, axis: Axis, view: Rect, inv: &mut Invalidations) {
        self.alpha[axis as usize] = BAR_ALPHA;
        self.hold[axis as usize] = BAR_HOLD_TICKS;
        inv.push(Self::strip(axis, view));
        inv.request_tick();
    }

    /// 주기 틱(호스트 40ms) — 유지 소진 후 페이드. 더 할 일이 있으면 틱 재요청.
    pub fn tick(&mut self, view: Rect, inv: &mut Invalidations) {
        for axis in [Axis::V, Axis::H] {
            if self.hot(axis) {
                continue; // 호버/드래그 중인 축은 페이드 보류(이탈 시 flash가 재개)
            }
            let i = axis as usize;
            if self.hold[i] > 0 {
                self.hold[i] -= 1;
                inv.request_tick();
            } else if self.alpha[i] > 0 {
                self.alpha[i] = self.alpha[i].saturating_sub(BAR_FADE_STEP);
                inv.push(Self::strip(axis, view));
                if self.alpha[i] > 0 {
                    inv.request_tick();
                }
            }
        }
    }

    fn in_thumb(&self, axis: Axis, g: &AxisGeom, x: i32, y: i32) -> bool {
        let Some(t) = self.thumb(axis, g, true) else {
            return false;
        };
        match axis {
            Axis::V => x >= t.x - 2 && x < t.right() + 2 && y >= t.y && y < t.bottom(),
            Axis::H => x >= t.x && x < t.right() && y >= t.y - 2 && y < t.bottom() + 2,
        }
    }

    /// MouseMove — 드래그 중이면 `Some((축, 새 오프셋))`(호출자가 클램프·적용), 아니면
    /// 호버 갱신 후 None. `g` = [세로, 가로].
    pub fn mouse_move(
        &mut self,
        x: i32,
        y: i32,
        g: [AxisGeom; 2],
        inv: &mut Invalidations,
    ) -> Option<(Axis, i64)> {
        if let Some((axis, start, off0)) = self.drag {
            let geom = &g[axis as usize];
            let t = self.thumb(axis, geom, true)?;
            let (delta, denom) = match axis {
                Axis::V => ((y - start) as i64, (geom.view.h - t.h).max(1) as i64),
                Axis::H => ((x - start) as i64, (geom.view.w - t.w).max(1) as i64),
            };
            let next = (off0 + delta * geom.max_offset() / denom).clamp(0, geom.max_offset());
            return Some((axis, next));
        }
        // 호버는 **보이는** 축의 썸에만(숨겨진 바 자리에 올려도 드러나지 않음)
        let over = [Axis::V, Axis::H]
            .into_iter()
            .find(|&a| self.visible(a) && self.in_thumb(a, &g[a as usize], x, y));
        if over != self.hover {
            let prev = self.hover;
            self.hover = over;
            if let Some(a) = prev {
                self.flash(a, g[a as usize].view, inv); // 이탈 = 유지 → 페이드 재개
            }
            if let Some(a) = over {
                inv.push(Self::strip(a, g[a as usize].view));
            }
        }
        None
    }

    /// MouseDown — 보이는 축의 썸 = 드래그 시작, 트랙 = 페이지 이동. None = 바 밖.
    pub fn mouse_down(
        &mut self,
        x: i32,
        y: i32,
        g: [AxisGeom; 2],
        inv: &mut Invalidations,
    ) -> Option<(Axis, BarHit)> {
        for axis in [Axis::V, Axis::H] {
            let geom = &g[axis as usize];
            if !self.visible(axis) || !geom.view.contains(crate::geom::Point { x, y }) {
                continue;
            }
            if self.in_thumb(axis, geom, x, y) {
                let start = if axis == Axis::V { y } else { x };
                self.drag = Some((axis, start, geom.offset));
                inv.push(Self::strip(axis, geom.view));
                return Some((axis, BarHit::Drag));
            }
            let Some(t) = self.thumb(axis, geom, true) else {
                continue;
            };
            let (on_track, before) = match axis {
                Axis::V => (x >= t.x - 2, y < t.y),
                Axis::H => (y >= t.y - 2, x < t.x),
            };
            if on_track {
                return Some((
                    axis,
                    if before {
                        BarHit::PageBack
                    } else {
                        BarHit::PageFwd
                    },
                ));
            }
        }
        None
    }

    /// MouseUp — 드래그 종료(true = 소비). 종료 축은 유지 → 페이드.
    pub fn mouse_up(&mut self, view: Rect, inv: &mut Invalidations) -> bool {
        match self.drag.take() {
            Some((axis, ..)) => {
                self.flash(axis, view, inv);
                true
            }
            None => false,
        }
    }

    /// 썸 페인트(내용 위 마지막 — 알파 합성). 드래그 중인 축은 트랙 음영도.
    pub fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme, g: [AxisGeom; 2]) {
        for axis in [Axis::V, Axis::H] {
            if !self.visible(axis) {
                continue;
            }
            let geom = &g[axis as usize];
            let hot = self.hot(axis);
            let Some(t) = self.thumb(axis, geom, hot) else {
                continue;
            };
            if matches!(self.drag, Some((a, ..)) if a == axis) {
                let track = match axis {
                    Axis::V => Rect::new(t.x - 1, geom.view.y, t.w + 2, geom.view.h),
                    Axis::H => Rect::new(geom.view.x, t.y - 1, geom.view.w, t.h + 2),
                };
                ctx.fill_round_rect_alpha(track, 0, theme.text, 28);
            }
            let alpha = if hot {
                BAR_ALPHA_HOT
            } else {
                self.alpha[axis as usize]
            };
            let r = if axis == Axis::V { t.w / 2 } else { t.h / 2 };
            ctx.fill_round_rect_alpha(t, r, theme.text, alpha);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geom_v(content: i64, visible: i64, offset: i64) -> AxisGeom {
        AxisGeom {
            view: Rect::new(0, 0, 200, 100),
            content,
            visible,
            offset,
        }
    }

    fn geom_h(content: i64, visible: i64, offset: i64) -> AxisGeom {
        AxisGeom {
            view: Rect::new(0, 0, 200, 100),
            content,
            visible,
            offset,
        }
    }

    #[test]
    fn thumb_none_when_content_fits() {
        let b = OverlayBars::default();
        assert_eq!(b.thumb(Axis::V, &geom_v(5, 10, 0), false), None);
        assert_eq!(b.thumb(Axis::H, &geom_h(200, 200, 0), false), None);
    }

    #[test]
    fn thumb_geometry_is_proportional_and_clamped() {
        let b = OverlayBars::default();
        // 세로: 100px 트랙 · 20행 중 10행 가시 → 썸 50px · 오프셋 10(최대) = 하단 끝
        let t = b.thumb(Axis::V, &geom_v(20, 10, 10), false).unwrap();
        assert_eq!((t.x, t.y, t.w, t.h), (200 - BAR_THIN - 2, 50, BAR_THIN, 50));
        // 가로: 200px 트랙 · 내용 2000px → 비례 20px < THUMB_MIN → 24px로 상향
        let t = b.thumb(Axis::H, &geom_h(2000, 200, 0), true).unwrap();
        assert_eq!(
            (t.x, t.y, t.w, t.h),
            (0, 100 - BAR_WIDE - 2, THUMB_MIN, BAR_WIDE)
        );
        // 오프셋 초과값은 클램프(스크롤 상한 밖 좌표 안전)
        let t = b.thumb(Axis::V, &geom_v(20, 10, 999), false).unwrap();
        assert_eq!(t.y, 50);
    }

    #[test]
    fn flash_then_tick_fades_out_and_stops_requesting() {
        let mut inv = Invalidations::default();
        let mut b = OverlayBars::default();
        let view = Rect::new(0, 0, 200, 100);
        assert!(!b.visible(Axis::V));
        b.flash(Axis::V, view, &mut inv);
        assert!(b.visible(Axis::V) && inv.tick_requested());
        // 유지 22틱 동안은 알파 유지
        for _ in 0..BAR_HOLD_TICKS {
            let mut i2 = Invalidations::default();
            b.tick(view, &mut i2);
            assert!(i2.tick_requested());
        }
        assert_eq!(b.alpha[0], BAR_ALPHA);
        // 페이드 = 5틱(120/24) 후 0 → 틱 재요청 없음
        let mut ticks = 0;
        loop {
            let mut i2 = Invalidations::default();
            b.tick(view, &mut i2);
            ticks += 1;
            if !i2.tick_requested() {
                break;
            }
            assert!(ticks < 20, "페이드가 끝나지 않음");
        }
        assert!(!b.visible(Axis::V));
        assert_eq!(ticks, 5);
    }

    #[test]
    fn hover_only_on_visible_bar_and_drag_maps_offset() {
        let mut inv = Invalidations::default();
        let mut b = OverlayBars::default();
        let g = [geom_v(20, 10, 0), geom_h(200, 200, 0)];
        let t = b.thumb(Axis::V, &g[0], true).unwrap();
        // 숨은 바 위 호버 = 무반응
        assert_eq!(b.mouse_move(t.x + 1, t.y + 1, g, &mut inv), None);
        assert!(!b.hot(Axis::V));
        b.flash(Axis::V, g[0].view, &mut inv);
        assert_eq!(b.mouse_move(t.x + 1, t.y + 1, g, &mut inv), None);
        assert!(b.hot(Axis::V), "보이는 바 위 = 호버");
        // 썸 프레스 → 50px 아래로 드래그 = 트랙 여유 50px 전부 = 최대 오프셋 10
        assert_eq!(
            b.mouse_down(t.x + 1, t.y + 1, g, &mut inv),
            Some((Axis::V, BarHit::Drag))
        );
        assert!(b.dragging());
        assert_eq!(
            b.mouse_move(t.x + 1, t.y + 1 + 25, g, &mut inv),
            Some((Axis::V, 5))
        );
        assert_eq!(
            b.mouse_move(t.x + 1, t.y + 1 + 500, g, &mut inv),
            Some((Axis::V, 10)),
            "상한 클램프"
        );
        assert!(b.mouse_up(g[0].view, &mut inv));
        assert!(!b.dragging());
        assert!(!b.mouse_up(g[0].view, &mut inv), "재호출 = 무소비");
    }

    #[test]
    fn track_click_pages_in_the_right_direction() {
        let mut inv = Invalidations::default();
        let mut b = OverlayBars::default();
        let g = [geom_v(20, 10, 5), geom_h(200, 200, 0)];
        b.flash(Axis::V, g[0].view, &mut inv);
        let t = b.thumb(Axis::V, &g[0], true).unwrap();
        assert_eq!(
            b.mouse_down(t.x, t.y - 5, g, &mut inv),
            Some((Axis::V, BarHit::PageBack))
        );
        assert_eq!(
            b.mouse_down(t.x, t.bottom() + 5, g, &mut inv),
            Some((Axis::V, BarHit::PageFwd))
        );
        // 트랙 밖(왼쪽 내용 영역) = 바 아님
        assert_eq!(b.mouse_down(10, 50, g, &mut inv), None);
    }
}

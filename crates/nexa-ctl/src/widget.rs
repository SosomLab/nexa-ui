//! 위젯 trait · 무효화 수집 — 창 1개 + 논리 위젯 모델([docs/14 §2] — OS 자식 창 없음).
//!
//! `nexa-dir2/crates/nexa-gui/src/widget.rs` 이식([docs/12 §A]). 위젯은 상태 변화 시 더러워진
//! 영역만 [`Invalidations`]에 밀어 넣고, 루프 소유자가 그 사각형만 다시 그린다(FR-U-13 —
//! 상태 변화는 해당 컨트롤만 무효화).

use crate::draw::DrawCtx;
use crate::event::InputEvent;
use crate::geom::Rect;
use crate::theme::Theme;

/// 프레임당 무효화 rect 수집기. 교차 rect는 union으로 병합해 과분할을 막는다.
#[derive(Default, Debug)]
pub struct Invalidations {
    rects: Vec<Rect>,
    /// ★ 틱 요청(dir2 `widget.rs` 이식 · nexa-dir3 105차 10-03): 애니메이션·지연 판정이 있는 위젯이 "다음 프레임에 `tick`을 불러 달라"고
    /// 신고한다 — 호스트가 자체 타이머 없이 `take_tick()`으로 프레임을 예약한다(위젯은 시계가 없다).
    tick: bool,
}

impl Invalidations {
    /// 다음 프레임 틱 요청(멱등).
    pub fn request_tick(&mut self) {
        self.tick = true;
    }

    /// 틱 요청이 있는가(읽기만).
    #[must_use]
    pub fn tick_requested(&self) -> bool {
        self.tick
    }

    /// 틱 요청을 꺼내며 지운다(호스트 프레임 예약).
    pub fn take_tick(&mut self) -> bool {
        std::mem::take(&mut self.tick)
    }

    /// 더러워진 영역 추가(빈 rect 무시 · 교차분 병합).
    pub fn push(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }
        if let Some(hit) = self.rects.iter_mut().find(|r| r.intersects(&rect)) {
            *hit = hit.union(&rect);
            return;
        }
        self.rects.push(rect);
    }

    /// 무효화 없음 = 다시 그릴 것 없음(프레임 요청 안 함 — FR-U-13).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// 수집된 rect를 비우며 순회.
    pub fn drain(&mut self) -> impl Iterator<Item = Rect> + '_ {
        self.rects.drain(..)
    }
}

/// 논리 위젯 — OS 창 없는 그리기·입력 단위. 좌표는 전부 창 클라이언트 좌표계.
pub trait Widget {
    /// 현재 경계.
    fn bounds(&self) -> Rect;

    /// 레이아웃 반영 — 필요한 무효화를 스스로 push한다.
    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations);

    /// 입력 라우팅 — 상태가 바뀌면 더러워진 영역을 push.
    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations);

    /// 자신의 bounds 안을 그린다(가시 영역만).
    fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_merges_intersecting_rects() {
        let mut inv = Invalidations::default();
        inv.push(Rect::new(0, 0, 10, 10));
        inv.push(Rect::new(5, 5, 10, 10));
        inv.push(Rect::new(100, 0, 5, 5));
        let rects: Vec<_> = inv.drain().collect();
        assert_eq!(
            rects,
            vec![Rect::new(0, 0, 15, 15), Rect::new(100, 0, 5, 5)]
        );
    }

    #[test]
    fn empty_rect_is_ignored() {
        let mut inv = Invalidations::default();
        inv.push(Rect::new(0, 0, 0, 10));
        assert!(inv.is_empty());
    }

    #[test]
    fn tick_request_is_idempotent_and_taken_once() {
        let mut inv = Invalidations::default();
        assert!(!inv.tick_requested() && !inv.take_tick());
        inv.request_tick();
        inv.request_tick();
        assert!(inv.tick_requested());
        assert!(inv.take_tick());
        assert!(!inv.take_tick(), "꺼내면 지워진다");
        assert!(inv.is_empty(), "틱은 rect가 아니다");
    }
}

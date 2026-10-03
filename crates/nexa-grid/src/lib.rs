//! # nexa-grid — 가상 행 그리드 엔진(docs/21 G-1 · nexa-dir3 105차 10-03)
//!
//! `nexa-dir2/crates/nexa-gui/src/{widgets/rows.rs, columns.rs, edit.rs, fastscroll.rs, typeahead.rs}`를 **그대로** 옮겼다(dir3 DR-11 —
//! dir2 테스트 52개가 패리티 증거). 바뀐 것은 어휘 어댑터뿐:
//!
//! - 기하·이벤트·테마·위젯 계약은 nexa-ctl 것을 **재노출**(`geom`·`event`·`theme`·`widget` 모듈) — dir2 `InputEvent.ctrl`은 `primary`.
//! - 그리기 어휘는 dir2 세대([`draw::DrawCtx`] — `select_font(slot, bold, italic)` · `fill_round_rect_alpha(u8)` · `glyph_opaque` ·
//!   `draw_icon`)를 유지하고, [`draw::Adapt`]가 nexa-ctl `DrawCtx` 위에 얹는다(italic은 `select_font_styled`로 전달 · 113차).
//! - dir2 `Theme.header_bg`는 nexa-ctl에 없다 → [`theme::header_bg`] = `chrome_bg`(dir2 L-09 "header_bg는 chrome_bg와 명도 차 없음").
//! - 타입어헤드·고속 스크롤은 dir2 판을 **크레이트 안에** 두었다(nexa-ctl 판과의 통일은 G-2 — 동작 패리티 우선).
//!
//! 특화(파일·결과·접속 그리드)는 소비자가 [`RowSource`]를 구현한다(docs/21 §1).

#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod columns;
pub mod draw;
pub mod edit;
pub mod fastscroll;
pub mod rows;
pub mod typeahead;

/// 기하 — nexa-ctl 재노출.
pub mod geom {
    pub use nexa_ctl::geom::{Point, Rect};
}

/// 입력 사건 — nexa-ctl 재노출(+ 휠 줄 수 전역).
pub mod event {
    pub use nexa_ctl::event::{
        set_wheel_lines, wheel_lines, InputEvent, Key, WheelAccum, WHEEL_DELTA,
    };
}

/// 테마 — nexa-ctl 재노출 + dir2에만 있던 토큰의 대응.
pub mod theme {
    pub use nexa_ctl::theme::{Color, Theme};

    /// dir2 `Theme.header_bg`(컬럼 헤더 배경) — nexa-ctl에는 없다. dir2 교훈 L-09: `header_bg`는 `chrome_bg`와 명도 차가 없었다 → 그 값.
    #[must_use]
    pub fn header_bg(t: &Theme) -> Color {
        t.chrome_bg
    }
}

/// 위젯 계약 — nexa-ctl 재노출(`Invalidations`의 틱 요청은 105차에 nexa-ctl에 추가).
pub mod widget {
    pub use nexa_ctl::widget::{Invalidations, Widget};
}

pub use columns::{order_badge, Align, Column};
pub use draw::{Adapt, DrawCtx, FontSlot};
pub use edit::{EditKey, EditState};
pub use fastscroll::{
    fast_scroll, fast_scroll_grid, grid_extra_of, set_fast_scroll, set_fast_scroll_grid,
    FastScroll, FastScroller,
};
pub use rows::{Marker, RowItem, RowSource, ScrollAlign, SelectOp, ViewMode, VirtualRows};
pub use typeahead::{Query, TypeAhead, TYPEAHEAD_TIMEOUT_MS};

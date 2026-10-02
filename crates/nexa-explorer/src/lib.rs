//! # nexa-explorer — 파일 탐색기 위젯 묶음(nexa-dir3 106차 10-03)
//!
//! `nexa-dir2/crates/nexa-gui/src/widgets/{pathbar.rs, dock.rs, overlaybar.rs}`를 **그대로** 옮겼다(dir3 DR-11 · dir2 테스트 25).
//! 어휘 어댑터는 `nexa-grid`와 같은 원칙(nexa-grid `lib.rs` 머리말): 기하·사건·테마·위젯 계약 = nexa-ctl 재노출 · 그리기 = dir2 세대
//! [`nexa_grid::draw::DrawCtx`] + [`nexa_grid::draw::Adapt`] · 편집 필드·고속 스크롤 = `nexa_grid::{edit, fastscroll}`.
//!
//! - [`PathBar`] — 브레드크럼(세그먼트 클릭 = 이동) · 우클릭/더블클릭 편집 모드(한 줄 편집 필드) · 자동완성 팝업(호스트가 `set_suggestions`).
//! - [`InfoDock`] — 하단 도크: 종류 스트립(정보·미리보기·터미널 · `set_kinds`) · 텍스트 줄/이미지 내용(`set_content`·`set_image`) ·
//!   세로+가로 오버레이 바 · 드래그 선택·복사 · ↗ 팝아웃 · 터미널 영역은 [`InfoDock::content_rect`]에 호스트가 그린다.
//! - [`OverlayBars`] — 두 축 오버레이 스크롤바(hover 굵기 · 페이드 · 드래그) — 도크·목록·F3 창 공용.

#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod dock;
pub mod overlaybar;
pub mod pathbar;

/// 그리기 어휘 — nexa-grid(dir2 세대) 재노출.
pub mod draw {
    pub use nexa_grid::draw::{Adapt, DrawCtx, FontSlot};
}

/// 기하 — nexa-ctl 재노출.
pub mod geom {
    pub use nexa_ctl::geom::{Point, Rect};
}

/// 입력 사건 — nexa-ctl 재노출.
pub mod event {
    pub use nexa_ctl::event::{wheel_lines, InputEvent, Key, WheelAccum, WHEEL_DELTA};
}

/// 테마 — nexa-ctl 재노출 + dir2 토큰 대응(`header_bg`).
pub mod theme {
    pub use nexa_ctl::theme::{Color, Theme};
    pub use nexa_grid::theme::header_bg;
}

/// 위젯 계약 — nexa-ctl 재노출.
pub mod widget {
    pub use nexa_ctl::widget::{Invalidations, Widget};
}

/// 한 줄 편집 필드 — nexa-grid 재노출(경로 편집 · 이름 바꾸기 공용).
pub mod edit {
    pub use nexa_grid::edit::{EditKey, EditState};
}

/// 고속 스크롤 — nexa-grid 재노출.
pub mod fastscroll {
    pub use nexa_grid::fastscroll::FastScroller;
}

pub use dock::{InfoDock, IMG_MARKER, IMG_PAD};
pub use overlaybar::{Axis, AxisGeom, BarHit, OverlayBars};
pub use pathbar::{split_path, PathBar, Segment};

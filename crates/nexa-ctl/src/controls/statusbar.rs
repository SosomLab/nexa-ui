//! **상태 바** — 창 맨 아래 한 줄(출처: `nexa-dir2/crates/nexa-gui/src/widgets/chrome.rs` `StatusBar` · dir2 GUI-080 · nexa-dir3 104차 10-03).
//!
//! 왼쪽 글(본문색) + 오른쪽 글(흐린 색 · 오른쪽 정렬 — 왼쪽 글과 겹치면 오른쪽 글이 `x + pad`까지만 온다). 배경 `chrome_bg` ·
//! **위** 1px `border` · [`FontSlot::Status`]. [`StatusBar::set_text`]는 **바뀔 때만** 무효화한다(상태줄은 매 입력마다 갱신되므로
//! 같은 글이면 그리지 않는 것이 dir2 교훈).
//!
//! **칸(세그먼트 · 133차 nexa-dir3 상태줄 구성)**: [`StatusBar::set_segments`]로 칸을 주면 오른쪽 글 대신 칸들을 그린다 —
//! 기본은 오른쪽 끝에 붙여(첫 칸이 맨 왼쪽) · [`StatusBar::set_segments_leading`]이면 왼쪽부터(패널 아래 "탭 상태바").
//! 칸은 hover 배경 · 좌클릭/우클릭 통지([`StatusBar::take_click`])를 가진다. 칸이 없으면 종전 그대로(입력 무시).

use super::{Control, ControlBase};
use crate::draw::{DrawCtx, FontSlot};
use crate::event::InputEvent;
use crate::geom::Rect;
use crate::theme::{Color, Theme};
use crate::widget::{Invalidations, Widget};

/// 기본 높이(논리 px · dir2 상태바 22).
pub const DEFAULT_H: i32 = 22;
/// 좌우 여백(논리 px).
const PAD_X: i32 = 8;

/// 칸 안쪽 좌우 여백(논리 px).
const SEG_PAD: i32 = 7;

/// 칸 안 조각 사이 간격(논리 px).
const PART_GAP: i32 = 4;

/// 칸 안의 조각 하나(137차) — 색을 따로 주거나(예: 업로드 빨강 · 다운로드 파랑) **폭을 고정**할 수 있다.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusPart {
    /// 표시 글.
    pub text: String,
    /// 글 색(`None` = 칸 기본색).
    pub color: Option<Color>,
    /// 폭 견본들 — 조각 폭 = 글과 견본 중 **가장 넓은 것**(값이 바뀌어도 칸 폭이 흔들리지 않게 · 견본이 있으면 글은 오른쪽 정렬).
    pub hints: Vec<String>,
    /// 이 조각만의 글꼴 크기 증분(논리 px × 100 · 141차 — `None` = 칸의 크기). 값보다 단위를 더 작게 그릴 때.
    pub font_delta_c: Option<i32>,
    /// **표식**(147차 · 쌓는 줄에서만 그린다) — 글 왼쪽 **고정 자리**의 작은 삼각형(글 폭이 바뀌어도 움직이지 않는다).
    pub marker: Option<StatusMarker>,
    /// 표식 깜빡임 단계(0 = 깜빡이지 않음 · 흐리게 / 1 = 느리게 … 9 = 빠르게 — [`StatusBar::blink_half_ms`]). 호스트가
    /// [`StatusBar::tick`]을 불러야 움직인다.
    pub blink: u8,
}

/// 줄 표식 모양(147차).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusMarker {
    /// 위 삼각형(▲ — 올리기 · 읽기).
    Up,
    /// 아래 삼각형(▼ — 내려받기 · 쓰기).
    Down,
}

/// 표식 삼각형의 폭 · 높이 · 글과의 간격(논리 px).
const MARK_W: i32 = 6;
const MARK_H: i32 = 4;
const MARK_GAP: i32 = 4;
/// 표식 농도 — 깜빡임의 어두운 위상 · 단계 0(움직임 없음).
const MARK_DIM: f32 = 0.3;

/// 두 색을 섞는다(`t` = `a`의 몫 0..=1).
fn mix(a: Color, b: Color, t: f32) -> Color {
    let ((ar, ag, ab), (br, bg, bb)) = (a.rgb(), b.rgb());
    let f = |x: u8, y: u8| (f32::from(x) * t + f32::from(y) * (1.0 - t)).round() as u8;
    Color::from_rgb(f(ar, br), f(ag, bg), f(ab, bb))
}

impl StatusPart {
    /// 글만.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    /// 색 지정(체이닝).
    #[must_use]
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// 폭 견본 지정(체이닝).
    #[must_use]
    pub fn hints(mut self, hints: Vec<String>) -> Self {
        self.hints = hints;
        self
    }

    /// 이 조각만 글꼴 크기를 바꾼다(체이닝 · 논리 px 증분).
    #[must_use]
    pub fn font_delta(mut self, px: f32) -> Self {
        self.font_delta_c = Some((px * 100.0).round() as i32);
        self
    }

    /// 표식(작은 삼각형)과 깜빡임 단계(0~9 · 넘으면 9)를 붙인다(체이닝 · 쌓는 줄에서만 그려진다).
    #[must_use]
    pub fn marker(mut self, marker: StatusMarker, blink: u8) -> Self {
        self.marker = Some(marker);
        self.blink = blink.min(9);
        self
    }
}

/// 상태 바의 칸 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusSeg {
    /// 호스트가 붙이는 식별자(클릭 통지에 그대로 돌아온다).
    pub id: String,
    /// 표시 글(조각이 있으면 덤프 · 비교용 — 그리기는 조각으로).
    pub text: String,
    /// 클릭할 수 있는가(false = hover·클릭 없음 · 흐린 글).
    pub clickable: bool,
    /// 조각(137차) — 비어 있으면 `text` 한 덩어리(종전).
    pub parts: Vec<StatusPart>,
    /// 이 칸의 글꼴 크기 증분(논리 px × 100 · 141차 — 0 = 상태줄 글꼴 그대로). 칸 폭도 그 크기로 잰다.
    pub font_delta_c: i32,
    /// **세로로 쌓는 줄**(142차) — 조각들(`parts`) 오른쪽에 위에서 아래로 한 줄씩(예: `D` 옆에 ↑ 읽기 / ↓ 쓰기 두 줄).
    /// 칸 높이를 줄 수로 고르게 나눈다 · 줄마다 색 · 폭 견본 · 글꼴 크기(작게 줘야 두 줄이 들어간다). 비어 있으면 종전.
    pub rows: Vec<StatusPart>,
}

impl StatusSeg {
    /// 클릭 가능한 칸.
    #[must_use]
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            clickable: true,
            parts: Vec::new(),
            font_delta_c: 0,
            rows: Vec::new(),
        }
    }

    /// 세로로 쌓는 줄을 붙인다(체이닝 · 142차) — `text`에는 줄 글을 빈칸으로 이어 덧붙인다(덤프 · 비교용).
    #[must_use]
    pub fn rows(mut self, rows: Vec<StatusPart>) -> Self {
        for r in &rows {
            if !self.text.is_empty() {
                self.text.push(' ');
            }
            // 표식은 덤프 글에 ▲/▼로 남긴다(그리기는 도형).
            match r.marker {
                Some(StatusMarker::Up) => self.text.push_str("▲ "),
                Some(StatusMarker::Down) => self.text.push_str("▼ "),
                None => {}
            }
            self.text.push_str(&r.text);
        }
        self.rows = rows;
        self
    }

    /// 이 칸만 글꼴 크기를 바꾼다(체이닝 · 논리 px 증분 — 음수 = 작게).
    #[must_use]
    pub fn font_delta(mut self, px: f32) -> Self {
        self.font_delta_c = (px * 100.0).round() as i32;
        self
    }

    /// 조각으로 이루어진 클릭 가능한 칸(137차) — `text`는 조각 글을 빈칸으로 이은 것.
    #[must_use]
    pub fn with_parts(id: impl Into<String>, parts: Vec<StatusPart>) -> Self {
        let text = parts
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        Self {
            id: id.into(),
            text,
            clickable: true,
            parts,
            font_delta_c: 0,
            rows: Vec::new(),
        }
    }

    /// 표시 전용 칸.
    #[must_use]
    pub fn label(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            clickable: false,
            ..Self::new(id, text)
        }
    }
}

/// 상태 바 컨트롤.
#[derive(Debug, Default)]
pub struct StatusBar {
    base: ControlBase,
    left: String,
    right: String,
    segs: Vec<StatusSeg>,
    /// 칸을 왼쪽부터 놓는가(기본 = 오른쪽 끝에 붙임).
    leading: bool,
    /// 마지막으로 그린 칸 자리(글 폭은 그릴 때만 알 수 있다 — 입력 판정은 이 자리를 쓴다).
    rects: std::cell::RefCell<Vec<Rect>>,
    hover: Option<usize>,
    pressed: Option<usize>,
    click: Option<(String, bool)>,
    /// 표식 깜빡임의 시계(ms · [`StatusBar::tick`]이 넣는다) · 마지막으로 그리게 한 위상(표식마다 1비트).
    now_ms: u64,
    blink_mask: u64,
}

impl StatusBar {
    /// 빈 상태 바.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// 왼쪽·오른쪽 글 — **바뀔 때만** 무효화(dir2 `statusbar_set_text_invalidates_only_on_change`). 바뀌었으면 true.
    pub fn set_text(&mut self, left: &str, right: &str, inv: &mut Invalidations) -> bool {
        if self.left == left && self.right == right {
            return false;
        }
        self.left = left.to_string();
        self.right = right.to_string();
        inv.push(self.base.bounds);
        true
    }

    /// 왼쪽 글만(오른쪽 유지).
    pub fn set_left(&mut self, left: &str, inv: &mut Invalidations) -> bool {
        let right = self.right.clone();
        self.set_text(left, &right, inv)
    }

    /// 오른쪽 글만(왼쪽 유지).
    pub fn set_right(&mut self, right: &str, inv: &mut Invalidations) -> bool {
        let left = self.left.clone();
        self.set_text(&left, right, inv)
    }

    /// 왼쪽 글.
    #[must_use]
    pub fn left(&self) -> &str {
        &self.left
    }

    /// 오른쪽 글.
    #[must_use]
    pub fn right(&self) -> &str {
        &self.right
    }

    /// 칸 목록 — **바뀔 때만** 무효화. 칸이 하나라도 있으면 오른쪽 글 대신 칸을 그린다. 바뀌었으면 true.
    pub fn set_segments(&mut self, segs: Vec<StatusSeg>, inv: &mut Invalidations) -> bool {
        if self.segs == segs {
            return false;
        }
        if self.segs.len() != segs.len() {
            self.hover = None;
            self.pressed = None;
        }
        self.segs = segs;
        inv.push(self.base.bounds);
        true
    }

    /// 깜빡임 단계의 반주기(ms · 밝음 ↔ 어두움이 바뀌는 간격): 1 = 500 … 9 = 60. 0 = 깜빡이지 않음(`None`).
    #[must_use]
    pub fn blink_half_ms(level: u8) -> Option<u64> {
        (level > 0).then(|| 555 - 55 * u64::from(level.min(9)))
    }

    /// 지금 밝은 위상인가(단계 0 = 늘 어둡게 — 움직임이 없다는 표시).
    fn blink_on(level: u8, now_ms: u64) -> bool {
        Self::blink_half_ms(level).is_some_and(|half| (now_ms / half) % 2 == 0)
    }

    /// 깜빡이는 표식들의 단계(칸 · 줄 순서).
    fn blink_levels(&self) -> impl Iterator<Item = u8> + '_ {
        self.segs
            .iter()
            .flat_map(|s| s.rows.iter())
            .filter(|r| r.marker.is_some() && r.blink > 0)
            .map(|r| r.blink)
    }

    /// 시간 처리(147차) — 표식 깜빡임의 위상이 바뀌었으면 `true`(다시 그리기). 깜빡이는 표식이 없으면 늘 `false`.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        self.now_ms = now_ms;
        let mask = self
            .blink_levels()
            .take(64)
            .enumerate()
            .fold(0u64, |m, (i, l)| {
                m | (u64::from(Self::blink_on(l, now_ms)) << i)
            });
        let changed = mask != self.blink_mask;
        self.blink_mask = mask;
        changed
    }

    /// 다음 위상 전환까지 남은 시간(ms · 깜빡이는 표식이 없으면 `None` = 깨울 필요 없음).
    #[must_use]
    pub fn next_blink_ms(&self, now_ms: u64) -> Option<u64> {
        self.blink_levels()
            .filter_map(Self::blink_half_ms)
            .map(|half| half - now_ms % half)
            .min()
    }

    /// 칸을 왼쪽부터 놓는다(패널 아래 상태바) — 이때 왼쪽 글은 칸들 뒤(오른쪽)에 온다.
    pub fn set_segments_leading(&mut self, on: bool, inv: &mut Invalidations) {
        if self.leading != on {
            self.leading = on;
            inv.push(self.base.bounds);
        }
    }

    /// 칸 목록.
    #[must_use]
    pub fn segments(&self) -> &[StatusSeg] {
        &self.segs
    }

    /// 마지막으로 그린 그 칸의 자리(아직 그리지 않았거나 없는 칸 = `None`).
    #[must_use]
    pub fn seg_rect(&self, id: &str) -> Option<Rect> {
        let i = self.segs.iter().position(|s| s.id == id)?;
        self.rects.borrow().get(i).copied().filter(|r| r.w > 0)
    }

    /// 칸 클릭 통지 `(id, 우클릭인가)` — 한 번만 돌려준다.
    pub fn take_click(&mut self) -> Option<(String, bool)> {
        self.click.take()
    }

    fn seg_at(&self, x: i32, y: i32) -> Option<usize> {
        let rects = self.rects.borrow();
        self.segs
            .iter()
            .zip(rects.iter())
            .position(|(s, r)| s.clickable && r.contains(crate::geom::Point { x, y }))
    }

    /// 권장 높이(물리 px · 배율 반영).
    #[must_use]
    pub fn preferred_height(&self) -> i32 {
        self.s(DEFAULT_H)
    }

    /// 덤프(하네스 `status.dump`): `left\tright`.
    #[must_use]
    pub fn dump(&self) -> String {
        format!("{}\t{}", self.left, self.right)
    }
}

impl Control for StatusBar {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
}

impl Widget for StatusBar {
    fn bounds(&self) -> Rect {
        self.base.bounds
    }

    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        if self.base.bounds != bounds {
            self.base.bounds = bounds;
            inv.push(bounds);
        }
    }

    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        if self.segs.is_empty() {
            return;
        }
        match *ev {
            InputEvent::MouseMove { x, y } => {
                let h = self.seg_at(x, y);
                if h != self.hover {
                    self.hover = h;
                    inv.push(self.base.bounds);
                }
            }
            InputEvent::MouseDown { x, y, .. } | InputEvent::DoubleClick { x, y, .. } => {
                self.pressed = self.seg_at(x, y);
                if self.pressed.is_some() {
                    inv.push(self.base.bounds);
                }
            }
            InputEvent::MouseUp { x, y } => {
                if let Some(p) = self.pressed.take() {
                    if self.seg_at(x, y) == Some(p) {
                        self.click = Some((self.segs[p].id.clone(), false));
                    }
                    inv.push(self.base.bounds);
                }
            }
            InputEvent::RightDown { x, y } => {
                if let Some(i) = self.seg_at(x, y) {
                    self.click = Some((self.segs[i].id.clone(), true));
                }
            }
            _ => {}
        }
    }

    fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.base.bounds;
        if b.is_empty() {
            return;
        }
        ctx.fill_rect(b, theme.chrome_bg);
        ctx.fill_rect(Rect::new(b.x, b.y, b.w, 1), theme.border);
        ctx.select_font(FontSlot::Status, false);
        let pad = self.s(PAD_X);
        let ty = ctx.text_center_y(b.y + 1, b.h - 1);
        let clip = Rect::new(b.x, b.y + 1, b.w, b.h - 1);
        if self.segs.is_empty() {
            self.rects.borrow_mut().clear();
            if !self.right.is_empty() {
                let rw = ctx.text_width(&self.right);
                let rx = (b.right() - pad - rw).max(b.x + pad);
                ctx.text(rx, ty, clip, &self.right, theme.text_dim);
            }
            if !self.left.is_empty() {
                ctx.text(b.x + pad, ty, clip, &self.left, theme.text);
            }
            return;
        }
        // 칸: 폭 = 글 + 좌우 여백 · 칸 사이 1px 세로 선. 오른쪽 정렬이면 자리가 모자랄 때 **왼쪽 칸부터** 빠진다(맨 오른쪽 우선).
        let sp = self.s(SEG_PAD);
        let gap = self.s(PART_GAP);
        // 조각 폭 = 글과 견본 중 가장 넓은 것(값이 바뀌어도 칸이 흔들리지 않는다).
        // 칸마다 글꼴 크기가 다를 수 있다(141차) — 잴 때 · 그릴 때 그 칸의 크기로 고른다.
        let pick_c = |ctx: &mut dyn DrawCtx, delta_c: i32| {
            if delta_c == 0 {
                ctx.select_font(FontSlot::Status, false);
            } else {
                ctx.select_font_sized(FontSlot::Status, false, delta_c as f32 / 100.0);
            }
        };
        let pick = |ctx: &mut dyn DrawCtx, seg: &StatusSeg| pick_c(ctx, seg.font_delta_c);
        let part_ws: Vec<Vec<i32>> = self
            .segs
            .iter()
            .map(|s| {
                pick(ctx, s);
                s.parts
                    .iter()
                    .map(|p| {
                        pick_c(ctx, p.font_delta_c.unwrap_or(s.font_delta_c));
                        let mut w = ctx.text_width(&p.text);
                        for h in &p.hints {
                            w = w.max(ctx.text_width(h));
                        }
                        w
                    })
                    .collect()
            })
            .collect();
        // 쌓는 줄의 폭 = 가장 넓은 줄(견본 포함).
        let rows_w: Vec<i32> = self
            .segs
            .iter()
            .map(|s| {
                let mut w = 0;
                for r in &s.rows {
                    pick_c(ctx, r.font_delta_c.unwrap_or(s.font_delta_c));
                    w = w.max(ctx.text_width(&r.text));
                    for h in &r.hints {
                        w = w.max(ctx.text_width(h));
                    }
                }
                w
            })
            .collect();
        // 표식 자리(줄에 표식이 하나라도 있으면 그 칸의 줄 왼쪽에 고정 폭으로 비워 둔다).
        let mark_w = |s: &StatusSeg| {
            if s.rows.iter().any(|r| r.marker.is_some()) {
                self.s(MARK_W) + self.s(MARK_GAP)
            } else {
                0
            }
        };
        let widths: Vec<i32> = self
            .segs
            .iter()
            .zip(&part_ws)
            .zip(&rows_w)
            .map(|((s, pw), rw)| {
                if *rw > 0 {
                    let lead = pw.iter().sum::<i32>() + gap * pw.len() as i32;
                    lead + mark_w(s) + rw + 2 * sp
                } else if !pw.is_empty() {
                    pw.iter().sum::<i32>() + gap * (pw.len() as i32 - 1) + 2 * sp
                } else if s.text.is_empty() {
                    0
                } else {
                    pick(ctx, s);
                    ctx.text_width(&s.text) + 2 * sp
                }
            })
            .collect();
        ctx.select_font(FontSlot::Status, false);
        let mut rects = vec![Rect::new(0, 0, 0, 0); self.segs.len()];
        let left_w = if self.left.is_empty() {
            0
        } else {
            ctx.text_width(&self.left) + 2 * pad
        };
        let mut left_x = b.x + pad;
        let left_clip;
        if self.leading {
            let mut x = b.x;
            for (i, w) in widths.iter().enumerate() {
                if *w == 0 || x + w > b.right() {
                    continue;
                }
                rects[i] = Rect::new(x, b.y + 1, *w, b.h - 1);
                x += w + 1;
            }
            left_x = x + pad;
            left_clip = Rect::new(x, b.y + 1, (b.right() - x).max(0), b.h - 1);
        } else {
            let mut x = b.right();
            let min_x = b.x + left_w.min(b.w / 2);
            for (i, w) in widths.iter().enumerate().rev() {
                if *w == 0 || x - w < min_x {
                    continue;
                }
                x -= w;
                rects[i] = Rect::new(x, b.y + 1, *w, b.h - 1);
                x -= 1;
            }
            left_clip = Rect::new(b.x, b.y + 1, (x - b.x).max(0), b.h - 1);
        }
        for (i, (seg, r)) in self.segs.iter().zip(&rects).enumerate() {
            if r.w == 0 {
                continue;
            }
            let hot = seg.clickable && (self.hover == Some(i) || self.pressed == Some(i));
            if hot {
                ctx.fill_rect(*r, theme.sel_bg_inactive);
            }
            // 칸 경계(오른쪽 정렬 = 칸 왼쪽 · 왼쪽 정렬 = 칸 오른쪽).
            let line_x = if self.leading { r.right() } else { r.x - 1 };
            ctx.fill_rect(
                Rect::new(line_x, b.y + 5, 1, (b.h - 9).max(1)),
                theme.border,
            );
            let color = if hot || self.leading {
                theme.text
            } else {
                theme.text_dim
            };
            // 크기가 다른 칸은 그 글꼴로 세로 가운데를 다시 잡는다.
            pick(ctx, seg);
            let ty = if seg.font_delta_c == 0 {
                ty
            } else {
                ctx.text_center_y(b.y + 1, b.h - 1)
            };
            if seg.parts.is_empty() && seg.rows.is_empty() {
                ctx.text(r.x + sp, ty, *r, &seg.text, color);
            } else {
                let mut x = r.x + sp;
                // 조각마다 크기가 다르면 글 아래쪽(밑줄 근처)을 맞춘다 — 작은 단위가 값 옆에 내려앉게.
                let seg_h = ctx.text_height();
                for (p, w) in seg.parts.iter().zip(&part_ws[i]) {
                    let dc = p.font_delta_c.unwrap_or(seg.font_delta_c);
                    pick_c(ctx, dc);
                    let ty = if dc == seg.font_delta_c {
                        ty
                    } else {
                        ty + ((seg_h - ctx.text_height()) as f32 * 0.75).round() as i32
                    };
                    // 견본이 있는 조각(숫자) = 오른쪽 정렬 · 없으면 왼쪽.
                    let tx = if p.hints.is_empty() {
                        x
                    } else {
                        x + w - ctx.text_width(&p.text)
                    };
                    ctx.text(tx, ty, *r, &p.text, p.color.unwrap_or(color));
                    x += w + gap;
                }
                // 쌓는 줄: 칸 높이를 줄 수로 나눠 한 줄씩(오른쪽 정렬 — 숫자 자리가 위아래로 맞는다).
                let n = seg.rows.len() as i32;
                for (k, row) in seg.rows.iter().enumerate() {
                    pick_c(ctx, row.font_delta_c.unwrap_or(seg.font_delta_c));
                    // 띠 = 칸 높이를 줄 수로 **남김없이** 나눈 것(143차) · 글은 숫자 높이 기준으로 띠 가운데에 —
                    // 줄 글꼴을 띠에 꽉 차게 주면 위아래 · 줄 사이 여백이 최소가 된다(호스트가 크기를 정한다).
                    let avail = b.h - 1;
                    let by = b.y + 1 + avail * k as i32 / n.max(1);
                    let band_h = b.y + 1 + avail * (k as i32 + 1) / n.max(1) - by;
                    let ry = ctx.text_center_y(by, band_h);
                    let mw = mark_w(seg);
                    let row_color = row.color.unwrap_or(color);
                    // 표식 = 줄 왼쪽 고정 자리의 작은 삼각형(글 폭과 무관) · 밝은 위상 = 줄 색 · 어두운 위상/단계 0 = 바탕 쪽으로 흐리게.
                    if let Some(m) = row.marker {
                        let c = if Self::blink_on(row.blink, self.now_ms) {
                            row_color
                        } else {
                            mix(row_color, theme.chrome_bg, MARK_DIM)
                        };
                        let (w, h) = (self.s(MARK_W), self.s(MARK_H));
                        let top = by + (band_h - h) / 2;
                        let (base_y, tip_y) = match m {
                            StatusMarker::Up => (top + h, top),
                            StatusMarker::Down => (top, top + h),
                        };
                        ctx.fill_triangle((x, base_y), (x + w, base_y), (x + w / 2, tip_y), c);
                    }
                    let tx = x + mw + rows_w[i] - ctx.text_width(&row.text);
                    ctx.text(tx, ry, *r, &row.text, row_color);
                }
            }
        }
        ctx.select_font(FontSlot::Status, false);
        *self.rects.borrow_mut() = rects;
        if !self.left.is_empty() {
            ctx.text(left_x, ty, left_clip, &self.left, theme.text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::RecordCtx;

    /// dir2 GUI-080 계승: 같은 글 재설정 = 무효화 없음 · 바뀌면 bounds 무효화 · 오른쪽 글은 오른쪽 정렬(`x = right − pad − 폭`) ·
    /// 위 1px 경계 · 그리기는 bounds 안.
    #[test]
    fn set_text_invalidates_only_on_change_and_right_aligns() {
        let mut sb = StatusBar::new();
        let mut inv = Invalidations::default();
        sb.set_bounds(Rect::new(0, 100, 400, 22), &mut inv);
        assert!(!inv.is_empty());
        let mut inv = Invalidations::default();
        assert!(sb.set_text("3 items", "Tab 1/2", &mut inv));
        assert!(!inv.is_empty());
        let mut inv2 = Invalidations::default();
        assert!(!sb.set_text("3 items", "Tab 1/2", &mut inv2));
        assert!(inv2.is_empty(), "같은 글 = 무효화 없음");
        assert!(sb.set_right("Tab 2/2", &mut inv2) && !inv2.is_empty());
        assert_eq!(sb.dump(), "3 items\tTab 2/2");
        let mut rec = RecordCtx::with_surface(400, 200);
        sb.paint(&mut rec, &Theme::dark());
        assert!(rec.drew_text("3 items") && rec.drew_text("Tab 2/2"));
        let lx = rec
            .texts
            .iter()
            .find(|(_, _, _, s)| s == "3 items")
            .map(|t| t.0)
            .unwrap();
        let rx = rec
            .texts
            .iter()
            .find(|(_, _, _, s)| s == "Tab 2/2")
            .map(|t| t.0)
            .unwrap();
        assert_eq!(lx, 8, "왼쪽 여백");
        assert_eq!(rx, 400 - 8 - 7 * 7, "오른쪽 정렬 = right − pad − 글자수×7");
        assert!(
            rec.fills
                .iter()
                .any(|(r, _)| r == &Rect::new(0, 100, 400, 1)),
            "위 1px 경계"
        );
        assert!(rec.all_inside(Rect::new(0, 100, 400, 22)));
        assert_eq!(sb.preferred_height(), 22);
    }

    /// 칸(133차): 오른쪽 끝에 붙고(첫 칸이 맨 왼쪽) · 좌클릭/우클릭 통지 · 표시 전용 칸은 클릭 없음 · 좁으면 왼쪽 칸부터 빠진다 ·
    /// 왼쪽부터 놓기(탭 상태바) · 칸이 없으면 입력 무시.
    #[test]
    fn segments_layout_and_clicks() {
        let mut sb = StatusBar::new();
        let mut inv = Invalidations::default();
        sb.set_bounds(Rect::new(0, 100, 400, 22), &mut inv);
        sb.set_text("left", "ignored", &mut inv);
        let segs = vec![
            StatusSeg::label("tab", "Tab 1/2"),
            StatusSeg::new("mem", "12 MB"),
            StatusSeg::new("lic", "Free"),
        ];
        assert!(sb.set_segments(segs.clone(), &mut inv));
        let mut inv2 = Invalidations::default();
        assert!(!sb.set_segments(segs, &mut inv2) && inv2.is_empty());
        assert_eq!(sb.seg_rect("lic"), None, "그리기 전");
        let mut rec = RecordCtx::with_surface(400, 200);
        sb.paint(&mut rec, &Theme::dark());
        assert!(rec.drew_text("Free") && rec.drew_text("12 MB") && rec.drew_text("left"));
        assert!(
            !rec.drew_text("ignored"),
            "칸이 있으면 오른쪽 글은 안 그린다"
        );
        let (tab, mem, lic) = (
            sb.seg_rect("tab").unwrap(),
            sb.seg_rect("mem").unwrap(),
            sb.seg_rect("lic").unwrap(),
        );
        assert_eq!(lic.right(), 400);
        assert_eq!(lic.w, 4 * 7 + 14);
        assert!(tab.right() < mem.x && mem.right() < lic.x);
        let click = |sb: &mut StatusBar, r: Rect| {
            let mut inv = Invalidations::default();
            let (x, y) = (r.x + 3, r.y + 3);
            sb.on_event(
                &InputEvent::MouseDown {
                    x,
                    y,
                    shift: false,
                    primary: false,
                },
                &mut inv,
            );
            sb.on_event(&InputEvent::MouseUp { x, y }, &mut inv);
        };
        click(&mut sb, mem);
        assert_eq!(sb.take_click(), Some(("mem".to_string(), false)));
        assert_eq!(sb.take_click(), None);
        click(&mut sb, tab);
        assert_eq!(sb.take_click(), None, "표시 전용 칸");
        sb.on_event(
            &InputEvent::RightDown {
                x: lic.x + 2,
                y: lic.y + 2,
            },
            &mut inv,
        );
        assert_eq!(sb.take_click(), Some(("lic".to_string(), true)));
        // 좁으면 왼쪽 칸부터 빠진다(맨 오른쪽 = 라이선스가 남는다).
        sb.set_bounds(Rect::new(0, 100, 90, 22), &mut inv);
        let mut rec = RecordCtx::with_surface(400, 200);
        sb.paint(&mut rec, &Theme::dark());
        assert!(sb.seg_rect("lic").is_some() && sb.seg_rect("tab").is_none());
        // 왼쪽부터.
        sb.set_bounds(Rect::new(0, 100, 400, 22), &mut inv);
        sb.set_segments_leading(true, &mut inv);
        let mut rec = RecordCtx::with_surface(400, 200);
        sb.paint(&mut rec, &Theme::dark());
        assert_eq!(sb.seg_rect("tab").unwrap().x, 0);
        assert!(sb.seg_rect("mem").unwrap().x > sb.seg_rect("tab").unwrap().right());
        // 칸 없음 = 종전(입력 무시 · 오른쪽 글).
        sb.set_segments(Vec::new(), &mut inv);
        click(&mut sb, lic);
        assert_eq!(sb.take_click(), None);
    }

    /// 조각(137차): 색을 따로 · 폭 견본이 있으면 값이 바뀌어도 칸 폭이 같다 · 견본 조각은 오른쪽 정렬.
    #[test]
    fn parts_keep_width_and_colors() {
        let red = Color(0x00FF_0000);
        let seg = |v: &str| {
            StatusSeg::with_parts(
                "net",
                vec![
                    StatusPart::new("N"),
                    StatusPart::new(v)
                        .color(red)
                        .hints(vec!["1023.9 MB/s".into(), "999 B/s".into()]),
                ],
            )
        };
        let mut sb = StatusBar::new();
        let mut inv = Invalidations::default();
        sb.set_bounds(Rect::new(0, 0, 400, 22), &mut inv);
        let width_of = |sb: &mut StatusBar, v: &str| {
            let mut inv = Invalidations::default();
            sb.set_segments(vec![seg(v)], &mut inv);
            let mut rec = RecordCtx::with_surface(400, 60);
            sb.paint(&mut rec, &Theme::dark());
            assert!(rec.drew_text(v) && rec.drew_text("N"));
            (sb.seg_rect("net").unwrap().w, rec)
        };
        let (w1, rec1) = width_of(&mut sb, "0 B/s");
        let (w2, _) = width_of(&mut sb, "512.3 KB/s");
        assert_eq!(w1, w2, "값이 바뀌어도 폭 불변");
        // 폭 = N(1자) + 간격 4 + 견본(11자) + 좌우 여백 14 — RecordCtx 글자 폭 7.
        assert_eq!(w1, 7 + 4 + 11 * 7 + 14);
        let r = sb.seg_rect("net").unwrap();
        let vx = rec1
            .texts
            .iter()
            .find(|t| t.3 == "0 B/s")
            .map(|t| t.0)
            .unwrap();
        assert_eq!(vx, r.right() - 7 - 5 * 7, "견본 조각 = 오른쪽 정렬");
        assert_eq!(seg("x").text, "N x");
        // 칸별 글꼴 크기(141차): 증분이 있는 칸은 크기 지정 글꼴을 고른다 · 기본 = 0.
        assert_eq!(seg("x").font_delta_c, 0);
        let small = seg("x").font_delta(-1.25);
        assert_eq!(small.font_delta_c, -125);
        let mut inv = Invalidations::default();
        sb.set_segments(vec![small], &mut inv);
        let mut rec = RecordCtx::with_surface(400, 60);
        sb.paint(&mut rec, &Theme::dark());
        assert!(rec.drew_text("N") && sb.seg_rect("net").is_some());
        // 쌓는 줄(142차): 줄이 위에서 아래로 · 폭 = 앞 조각 + 간격 + 가장 넓은 줄(견본) + 여백 · 오른쪽 정렬.
        let stacked = StatusSeg::with_parts("disk", vec![StatusPart::new("D")]).rows(vec![
            StatusPart::new("↑ 1 KB/s").hints(vec!["↑ 999.9 MB/s".into()]),
            StatusPart::new("↓ 20 KB/s").color(red),
        ]);
        assert_eq!(stacked.text, "D ↑ 1 KB/s ↓ 20 KB/s");
        let mut inv = Invalidations::default();
        sb.set_segments(vec![stacked], &mut inv);
        let mut rec = RecordCtx::with_surface(400, 60);
        sb.paint(&mut rec, &Theme::dark());
        let r = sb.seg_rect("disk").unwrap();
        assert_eq!(r.w, 7 + 4 + 12 * 7 + 14);
        let pos = |t: &str| {
            rec.texts
                .iter()
                .find(|x| x.3 == t)
                .map(|x| (x.0, x.1))
                .unwrap()
        };
        let (up, down) = (pos("↑ 1 KB/s"), pos("↓ 20 KB/s"));
        assert!(up.1 < down.1, "위 → 아래");
        assert_eq!(up.0 + 8 * 7, down.0 + 9 * 7, "오른쪽 끝이 맞는다");
        assert_eq!(down.0 + 9 * 7, r.right() - 7);
        // 조각별 크기: 기본 = 칸의 크기 · 지정하면 그 조각만.
        assert_eq!(StatusPart::new("MB/s").font_delta_c, None);
        assert_eq!(
            StatusPart::new("MB/s").font_delta(-2.5).font_delta_c,
            Some(-250)
        );
    }

    /// 줄 표식(147차): 삼각형 자리는 **고정 폭**(6 + 4)으로 줄 왼쪽에 잡히고 글은 그 오른쪽에서 오른쪽 정렬 → 글이 길어져도 칸 폭 ·
    /// 표식 자리가 같다 · 덤프 글에는 ▲/▼ · 깜빡임: 단계 0 = 깨우지 않음 · 단계가 높을수록 반주기가 짧다 · 위상이 바뀔 때만 다시 그린다.
    #[test]
    fn row_markers_keep_a_fixed_slot_and_blink_by_level() {
        let seg = |up: &str, lvl: u8| {
            StatusSeg::with_parts("net", vec![StatusPart::new("N")]).rows(vec![
                StatusPart::new(up)
                    .hints(vec!["999.9 MB/s".into()])
                    .marker(StatusMarker::Up, lvl),
                StatusPart::new("0 B/s")
                    .hints(vec!["999.9 MB/s".into()])
                    .marker(StatusMarker::Down, 0),
            ])
        };
        assert_eq!(seg("1 KB/s", 3).text, "N ▲ 1 KB/s ▼ 0 B/s");
        let mut sb = StatusBar::new();
        let mut inv = Invalidations::default();
        sb.set_bounds(Rect::new(0, 0, 400, 22), &mut inv);
        let paint = |sb: &mut StatusBar, s: StatusSeg| {
            let mut inv = Invalidations::default();
            sb.set_segments(vec![s], &mut inv);
            let mut rec = RecordCtx::with_surface(400, 60);
            sb.paint(&mut rec, &Theme::dark());
            sb.seg_rect("net").unwrap()
        };
        let a = paint(&mut sb, seg("1 KB/s", 3));
        let b = paint(&mut sb, seg("512.3 MB/s", 9));
        assert_eq!(a, b, "값 · 단계가 바뀌어도 칸 자리 불변");
        // 폭 = N(7) + 간격 4 + 표식 자리(6 + 4) + 견본 10자 + 좌우 여백 14.
        assert_eq!(a.w, 7 + 4 + 10 + 10 * 7 + 14);
        // 깜빡임 단계.
        assert_eq!(StatusBar::blink_half_ms(0), None);
        assert_eq!(StatusBar::blink_half_ms(1), Some(500));
        assert_eq!(StatusBar::blink_half_ms(9), Some(60));
        assert!((1..9).all(|l| StatusBar::blink_half_ms(l) > StatusBar::blink_half_ms(l + 1)));
        // 단계 9: 60 ms마다 위상이 바뀐다 → 그때만 tick이 true · 다음 전환까지 남은 시간을 알려 준다.
        assert!(sb.tick(0) || !sb.tick(0), "첫 호출은 위상 기록");
        assert!(!sb.tick(30), "같은 위상");
        assert_eq!(sb.next_blink_ms(30), Some(30));
        assert!(sb.tick(60), "위상 전환");
        assert!(sb.tick(120) && !sb.tick(150));
        // 깜빡이는 표식이 없으면(전부 단계 0) 깨우지 않는다.
        let _ = paint(&mut sb, seg("0 B/s", 0));
        assert_eq!(sb.next_blink_ms(1000), None);
        let _ = sb.tick(1000);
        assert!(!sb.tick(5000));
    }

    /// 왼쪽이 길면 오른쪽 글은 `x + pad`까지만 밀린다(겹침 허용 · dir2 규약).
    #[test]
    fn right_text_never_goes_left_of_pad() {
        let mut sb = StatusBar::new();
        let mut inv = Invalidations::default();
        sb.set_bounds(Rect::new(0, 0, 60, 22), &mut inv);
        sb.set_text("", "a very long right text", &mut inv);
        let mut rec = RecordCtx::default();
        sb.paint(&mut rec, &Theme::dark());
        let rx = rec.texts[0].0;
        assert_eq!(rx, 8);
    }
}

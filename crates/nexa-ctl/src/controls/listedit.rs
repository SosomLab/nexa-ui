//! **목록 편집기** — 문자열 목록을 ListBox + 추가/삭제 + 행 인라인 편집으로(08-31 사용자
//! 요청 · 차단 페이지·제외 앱). 값 모델은 `;` 구분 한 줄 — 저장 형식은 그대로 두고
//! **화면만** 목록이 된다(설정 파일·게이트 판정 코드 무수정).
//!
//! | 조작 | 동작 |
//! |---|---|
//! | 행 클릭 | 선택 · **선택된 행 재클릭 = 인라인 편집**(더블클릭 사건이 없는 이벤트 모델) |
//! | `＋` | 빈 행 추가 + 즉시 편집 |
//! | `−` / `Delete` | 선택 행 삭제 |
//! | `Enter` | 편집 확정(빈 값 = 그 행 삭제 — 빈 줄을 목록에 두지 않는다) |
//! | `Esc` | 편집 취소(원래 값 유지) |
//! | ↑↓ · 휠 | 선택 이동 · 스크롤 |
//!
//! 편집은 [`TextBox`]를 그 행 위에 겹쳐 재사용한다 — IME·클립보드 라우팅은 호스트가
//! [`ListEditor::editing_input`]으로 콤보(`Combo::editing_input`)와 같은 문법으로 잇는다.

use super::{Button, Control, ControlBase, TextBox};
use crate::draw::DrawCtx;
use crate::event::{InputEvent, Key};
use crate::geom::{Point, Rect};
use crate::theme::Theme;
use crate::widget::{Invalidations, Widget};

/// 행 높이(논리 px).
const ROW_H: i32 = 24;
/// 보이는 행 수 — 넘치면 휠 스크롤.
const VISIBLE: usize = 5;
/// 버튼 한 변(논리 px) — 정사각 아이콘 버튼(09-01 "75% 크기·머티리얼 스타일").
const BTN: i32 = 16;
/// 목록과 버튼 사이(논리 px).
const GAP: i32 = 6;
/// 행 안 좌우 여백(논리 px).
const PAD_X: i32 = 8;

/// 문자열 목록 편집 컨트롤.
#[derive(Debug)]
pub struct ListEditor {
    base: ControlBase,
    items: Vec<String>,
    sel: Option<usize>,
    /// 첫 번째로 보이는 행(스크롤).
    top: usize,
    /// 편집 중인 행 — 있으면 `input`이 그 행 위에 떠 있다.
    editing: Option<usize>,
    input: TextBox,
    add: Button,
    del: Button,
    /// 목록이 바뀌었다 — [`Self::take_changed`] 1회성.
    changed: bool,
    /// 빈 목록 표시 문구(i18n은 호스트 몸 — 컨트롤은 언어를 모른다).
    empty_label: String,
}

impl ListEditor {
    /// `;` 구분 한 줄에서 만든다(빈 조각은 버린다).
    #[must_use]
    pub fn new(joined: &str, placeholder: &str) -> Self {
        Self {
            base: ControlBase::default(),
            items: split(joined),
            sel: None,
            top: 0,
            editing: None,
            input: TextBox::new(placeholder),
            // 기호는 글꼴이 아니라 **벡터로 직접** 그린다(paint — 두부·글립 폭 문제 원천 차단).
            add: Button::new(""),
            del: Button::new(""),
            changed: false,
            empty_label: "—".into(),
        }
    }

    /// 빈 목록 문구 지정(예: tr(lang, Msg::ListEmpty)).
    #[must_use]
    pub fn with_empty_label(mut self, label: impl Into<String>) -> Self {
        self.empty_label = label.into();
        self
    }

    /// 키 입력을 받을 상태인가 — 포커스 또는 편집 중(호스트 키 라우팅 근거).
    #[must_use]
    pub fn wants_keys(&self) -> bool {
        self.base.focused || self.editing.is_some()
    }

    /// 행 인라인 편집 중인가.
    #[must_use]
    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    /// 보이는 창(5행)을 넘치는가 — 호스트의 휠 라우팅 근거.
    #[must_use]
    pub fn overflows(&self) -> bool {
        self.items.len() > VISIBLE
    }

    /// 권장 크기(물리 px) — 폭은 호스트 몫이라 높이만 의미 있다.
    #[must_use]
    pub fn preferred_height(&self) -> i32 {
        self.s(ROW_H) * VISIBLE as i32 + self.s(GAP) + self.s(BTN)
    }

    /// 현재 값 — `; ` 구분 한 줄(저장 형식).
    #[must_use]
    pub fn value(&self) -> String {
        self.items.join("; ")
    }

    /// 외부 값 반영(보고 없음) — 편집 중이면 취소된다.
    pub fn set_value(&mut self, joined: &str) {
        self.items = split(joined);
        self.sel = None;
        self.top = 0;
        self.editing = None;
    }

    /// 목록 변경 1회성 보고(`; ` 구분 한 줄).
    pub fn take_changed(&mut self) -> Option<String> {
        std::mem::take(&mut self.changed).then(|| self.value())
    }

    /// 편집 중인 입력 상자(IME 프리에딧·클립보드 라우팅용 — `Combo`와 같은 문법).
    pub fn editing_input(&mut self) -> Option<&mut TextBox> {
        self.editing.is_some().then_some(&mut self.input)
    }

    /// 읽기 전용 접근(복사 라우팅).
    #[must_use]
    pub fn editing_input_ref(&self) -> Option<&TextBox> {
        self.editing.is_some().then_some(&self.input)
    }

    /// hover 페이드 틱 — 다시 그릴 것이 있으면 true.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        let a = self.add.tick(now_ms);
        let b = self.del.tick(now_ms);
        let c = self.input.tick(now_ms);
        a || b || c
    }

    // ── 내부 기하 ──

    fn list_rect(&self) -> Rect {
        let b = self.base.bounds;
        Rect::new(b.x, b.y, b.w, self.s(ROW_H) * VISIBLE as i32)
    }

    fn row_rect(&self, vi: usize) -> Rect {
        let l = self.list_rect();
        let rh = self.s(ROW_H);
        Rect::new(l.x, l.y + rh * vi as i32, l.w, rh)
    }

    fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        let l = self.list_rect();
        if !l.contains(Point { x, y }) {
            return None;
        }
        let vi = ((y - l.y) / self.s(ROW_H)) as usize;
        let i = self.top + vi;
        (i < self.items.len()).then_some(i)
    }

    fn scroll_to(&mut self, i: usize) {
        if i < self.top {
            self.top = i;
        } else if i >= self.top + VISIBLE {
            self.top = i + 1 - VISIBLE;
        }
    }

    fn begin_edit(&mut self, i: usize, inv: &mut Invalidations) {
        self.scroll_to(i);
        self.editing = Some(i);
        self.input.set_text(&self.items[i]);
        self.input.set_focused(true);
        let vi = i - self.top;
        self.input.set_bounds(self.row_rect(vi), inv);
        inv.push(self.base.bounds);
    }

    /// 편집 확정 — 빈 값이면 그 행을 지운다(빈 줄을 목록에 두지 않는다).
    fn commit_edit(&mut self, text: &str, inv: &mut Invalidations) {
        let Some(i) = self.editing.take() else { return };
        let t = text.trim().to_string();
        if t.is_empty() {
            self.items.remove(i);
            self.sel = None;
        } else if self.items[i] != t {
            self.items[i] = t;
            self.sel = Some(i);
        } else {
            self.sel = Some(i);
            inv.push(self.base.bounds);
            return; // 값 그대로 — 변경 보고 없음
        }
        self.changed = true;
        inv.push(self.base.bounds);
    }

    fn cancel_edit(&mut self, inv: &mut Invalidations) {
        if let Some(i) = self.editing.take() {
            // ＋로 만든 빈 행을 그대로 두면 유령 줄이 남는다 — 걷어낸다.
            if self.items.get(i).is_some_and(|s| s.is_empty()) {
                self.items.remove(i);
                self.sel = None;
            }
            inv.push(self.base.bounds);
        }
    }

    fn remove_selected(&mut self, inv: &mut Invalidations) {
        if let Some(i) = self.sel.take() {
            if i < self.items.len() {
                self.items.remove(i);
                self.changed = true;
            }
            self.top = self.top.min(self.items.len().saturating_sub(VISIBLE));
            inv.push(self.base.bounds);
        }
    }
}

fn split(joined: &str) -> Vec<String> {
    joined
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

impl Control for ListEditor {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
}

impl Widget for ListEditor {
    fn bounds(&self) -> Rect {
        self.base.bounds
    }

    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        self.base.bounds = bounds;
        let (btn, gap) = (self.s(BTN), self.s(GAP));
        let by = bounds.y + self.s(ROW_H) * VISIBLE as i32 + gap;
        self.add.set_bounds(Rect::new(bounds.x, by, btn, btn), inv);
        self.del
            .set_bounds(Rect::new(bounds.x + btn + gap, by, btn, btn), inv);
        if let Some(i) = self.editing {
            if i >= self.top && i < self.top + VISIBLE {
                self.input.set_bounds(self.row_rect(i - self.top), inv);
            }
        }
        inv.push(bounds);
    }

    fn on_event(&mut self, ev: &InputEvent, inv: &mut Invalidations) {
        // ── 편집 중 — Esc/Enter만 가로채고 나머지는 입력 상자로 ──
        if self.editing.is_some() {
            match *ev {
                InputEvent::Key {
                    key: Key::Escape, ..
                } => {
                    self.input.set_focused(false);
                    self.cancel_edit(inv);
                    return;
                }
                // 편집 중 다른 행/바깥 클릭 = 지금 값으로 확정 후 그 클릭을 계속 처리.
                InputEvent::MouseDown { x, y, .. }
                    if !self.input.bounds().contains(Point { x, y }) =>
                {
                    let text = self.input.text();
                    self.input.set_focused(false);
                    self.commit_edit(&text, inv);
                    // 아래 일반 처리로 계속 흐른다.
                }
                _ => {
                    self.input.on_event(ev, inv);
                    if let Some(t) = self.input.take_committed() {
                        self.commit_edit(&t, inv);
                    }
                    return;
                }
            }
        }

        match *ev {
            InputEvent::MouseDown { x, y, .. } => {
                // 버튼은 누름→놓음으로 확정된다(Button 문법) — 누름 상태만 전달.
                self.add.on_event(ev, inv);
                self.del.on_event(ev, inv);
                if let Some(i) = self.row_at(x, y) {
                    self.base.focused = true;
                    if self.sel == Some(i) {
                        // ★ 선택된 행 재클릭 = 인라인 편집(더블클릭 대용).
                        self.begin_edit(i, inv);
                    } else {
                        self.sel = Some(i);
                        inv.push(self.base.bounds);
                    }
                } else if self.list_rect().contains(Point { x, y }) {
                    self.sel = None;
                    inv.push(self.base.bounds);
                }
            }
            InputEvent::MouseUp { .. } => {
                self.add.on_event(ev, inv);
                self.del.on_event(ev, inv);
                if self.add.take_clicked() {
                    self.base.focused = true;
                    self.items.push(String::new());
                    let i = self.items.len() - 1;
                    self.sel = Some(i);
                    self.begin_edit(i, inv);
                } else if self.del.take_clicked() {
                    self.remove_selected(inv);
                }
            }
            InputEvent::MouseMove { .. } => {
                // hover 페이드는 버튼이 스스로 관리한다(공용 Fade — T-12d4 문법).
                self.add.on_event(ev, inv);
                self.del.on_event(ev, inv);
            }
            InputEvent::Wheel { delta } if self.items.len() > VISIBLE => {
                let max_top = self.items.len() - VISIBLE;
                let step = if delta > 0 { -1i64 } else { 1i64 };
                self.top =
                    usize::try_from((self.top as i64 + step).clamp(0, max_top as i64)).unwrap_or(0);
                inv.push(self.base.bounds);
            }
            InputEvent::Key { key, .. } if self.base.focused => match key {
                Key::Up => {
                    if let Some(i) = self.sel {
                        let n = i.saturating_sub(1);
                        self.sel = Some(n);
                        self.scroll_to(n);
                        inv.push(self.base.bounds);
                    }
                }
                Key::Down => {
                    if let Some(i) = self.sel {
                        let n = (i + 1).min(self.items.len().saturating_sub(1));
                        self.sel = Some(n);
                        self.scroll_to(n);
                        inv.push(self.base.bounds);
                    }
                }
                Key::Delete => self.remove_selected(inv),
                Key::Enter => {
                    if let Some(i) = self.sel {
                        self.begin_edit(i, inv);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let l = self.list_rect();
        let r = self.s(4);
        ctx.fill_round_rect(l, r, theme.field_bg);
        let (fg, dim) = (theme.text, theme.text_dim);
        let rh = self.s(ROW_H);
        let pad = self.s(PAD_X);
        let ty = (rh - ctx.text_height()) / 2;
        for vi in 0..VISIBLE {
            let i = self.top + vi;
            let Some(item) = self.items.get(i) else { break };
            let row = self.row_rect(vi);
            if self.sel == Some(i) {
                ctx.fill_rect(row, theme.sel_bg);
            }
            let clip = Rect::new(row.x + pad, row.y, (row.w - pad * 2).max(0), row.h);
            ctx.text(row.x + pad, row.y + ty, clip, item, fg);
        }
        if self.items.is_empty() {
            let clip = Rect::new(l.x + pad, l.y, (l.w - pad * 2).max(0), rh);
            ctx.text(l.x + pad, l.y + ty, clip, &self.empty_label, dim);
        }
        // 넘침 표시 — 우측 얇은 스크롤 자국(조작은 휠).
        if self.items.len() > VISIBLE {
            let track_h = l.h - self.s(4);
            let th = (track_h * VISIBLE as i32 / self.items.len() as i32).max(self.s(12));
            let max_top = (self.items.len() - VISIBLE) as i32;
            let off = (track_h - th) * self.top as i32 / max_top.max(1);
            ctx.fill_round_rect(
                Rect::new(l.right() - self.s(5), l.y + self.s(2) + off, self.s(3), th),
                self.s(1),
                theme.border,
            );
        }
        ctx.stroke_round_rect(
            l,
            r,
            if self.base.focused {
                self.accent_now(theme)
            } else {
                theme.border
            },
            1.0,
        );
        self.add.paint(ctx, theme);
        self.del.paint(ctx, theme);
        // 기호 = 벡터 라인(머티리얼풍 — 중앙 정렬 · 2px 굵기 · 본문색).
        let stroke = self.s(2).max(2);
        let arm = self.s(8);
        for (btn, plus) in [(&self.add, true), (&self.del, false)] {
            let b = btn.bounds();
            let (cx, cy) = (b.x + b.w / 2, b.y + b.h / 2);
            ctx.fill_rect(
                Rect::new(cx - arm / 2, cy - stroke / 2, arm, stroke),
                theme.text,
            );
            if plus {
                ctx.fill_rect(
                    Rect::new(cx - stroke / 2, cy - arm / 2, stroke, arm),
                    theme.text,
                );
            }
        }
        if self.editing.is_some() {
            self.input.paint(ctx, theme);
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn ed(joined: &str) -> (ListEditor, Invalidations) {
        let mut e = ListEditor::new(joined, "");
        let mut inv = Invalidations::default();
        let h = e.preferred_height();
        e.set_bounds(Rect::new(0, 0, 300, h), &mut inv);
        (e, inv)
    }
    /// 누름+놓음 한 쌍(버튼은 놓음에서 확정된다).
    fn click_on(e: &mut ListEditor, inv: &mut Invalidations, x: i32, y: i32) {
        e.on_event(
            &InputEvent::MouseDown {
                x,
                y,
                shift: false,
                primary: false,
            },
            inv,
        );
        e.on_event(&InputEvent::MouseUp { x, y }, inv);
    }
    fn key(k: Key) -> InputEvent {
        InputEvent::Key {
            key: k,
            shift: false,
            primary: false,
        }
    }

    /// `;` 값 모델 왕복 — 빈 조각·공백은 정리된다(저장 형식 그대로).
    #[test]
    fn value_model_round_trips_semicolons() {
        let (e, _) = ed(" a ;; b;c ");
        assert_eq!(e.value(), "a; b; c");
    }

    /// 클릭 = 선택, **재클릭 = 편집**, Enter 확정이 변경을 1회 보고한다.
    #[test]
    fn click_select_reclick_edit_enter_commits() {
        let (mut e, mut inv) = ed("one; two");
        let r0 = e.row_rect(0);
        click_on(&mut e, &mut inv, r0.x + 5, r0.y + 5);
        assert_eq!(e.sel, Some(0));
        assert!(e.take_changed().is_none(), "선택만으로는 변경 아님");
        click_on(&mut e, &mut inv, r0.x + 5, r0.y + 5); // 재클릭 = 편집
        assert!(e.editing_input().is_some());
        e.editing_input().unwrap().set_text("ONE");
        e.on_event(&key(Key::Enter), &mut inv);
        assert_eq!(e.take_changed().as_deref(), Some("ONE; two"));
        assert!(e.take_changed().is_none(), "1회성");
    }

    /// ＋ = 빈 행 추가 + 즉시 편집 · Esc = 유령 줄 없이 취소.
    #[test]
    fn add_then_escape_leaves_no_ghost_row() {
        let (mut e, mut inv) = ed("x");
        let b = e.add.bounds();
        click_on(&mut e, &mut inv, b.x + 2, b.y + 2);
        assert!(e.editing_input().is_some(), "＋ = 즉시 편집");
        e.on_event(&key(Key::Escape), &mut inv);
        assert_eq!(e.value(), "x", "빈 행이 남지 않는다");
        assert!(e.take_changed().is_none());
    }

    /// − 버튼·Delete 키 = 선택 행 삭제(보고 포함).
    #[test]
    fn delete_removes_selected() {
        let (mut e, mut inv) = ed("a; b; c");
        let r1 = e.row_rect(1);
        click_on(&mut e, &mut inv, r1.x + 3, r1.y + 3);
        e.on_event(&key(Key::Delete), &mut inv);
        assert_eq!(e.take_changed().as_deref(), Some("a; c"));
        let d = e.del.bounds();
        // 선택이 없으면 − 는 no-op.
        click_on(&mut e, &mut inv, d.x + 2, d.y + 2);
        assert!(e.take_changed().is_none());
    }

    /// 편집 확정에서 빈 값 = 그 행 삭제.
    #[test]
    fn committing_empty_deletes_row() {
        let (mut e, mut inv) = ed("a; b");
        let r0 = e.row_rect(0);
        click_on(&mut e, &mut inv, r0.x + 3, r0.y + 3);
        click_on(&mut e, &mut inv, r0.x + 3, r0.y + 3);
        e.editing_input().unwrap().set_text("   ");
        e.on_event(&key(Key::Enter), &mut inv);
        assert_eq!(e.take_changed().as_deref(), Some("b"));
    }

    /// 휠 스크롤 — 5행 창이 목록을 따라 움직인다.
    #[test]
    fn wheel_scrolls_overflowing_list() {
        let (mut e, mut inv) = ed("1;2;3;4;5;6;7");
        assert_eq!(e.top, 0);
        e.on_event(&InputEvent::Wheel { delta: -120 }, &mut inv);
        assert_eq!(e.top, 1);
        e.on_event(&InputEvent::Wheel { delta: -120 }, &mut inv);
        assert_eq!(e.top, 2, "최대 2(7-5)");
        e.on_event(&InputEvent::Wheel { delta: -120 }, &mut inv);
        assert_eq!(e.top, 2, "하한 클램프");
        e.on_event(&InputEvent::Wheel { delta: 120 }, &mut inv);
        assert_eq!(e.top, 1);
    }
}

//! `SegProgress` — **세그먼트 진행 바**(nexa-dir2 `dialog.rs::paint_segments` 이식 · nexa-dir3 T-70 · docs/port/16 DLG-060 · 22 OPS-216).
//!
//! 전송 항목마다 **크기 비례 구간**(누적 경계 — 오차 비누적) · 항목별 5색 순환 · 완료 = 전체 채움 · 진행 = 부분 채움 · 건너뜀 = 회색 · 실패 = 적색 ·
//! 구간 경계선 1px · **모든 항목 최소 3px**(부족분은 가장 넓은 구간에서 1px씩 — 공간이 `n×4 ≤ 폭`일 때) · `total==0`·항목 없음·512개 초과 = 단색 바.
//! 입력은 받지 않는다(표시 전용). 호스트가 `set_items`/`set_totals`로 스냅숏을 밀어 넣는다.

use super::{Control, ControlBase};
use crate::draw::DrawCtx;
use crate::event::InputEvent;
use crate::geom::Rect;
use crate::theme::{Color, Theme};
use crate::widget::{Invalidations, Widget};

/// 항목(파일/폴더) 세그먼트 상태.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegStatus {
    Pending,
    Active,
    Done,
    Skipped,
    Failed,
}

/// 항목 1개의 진행(크기 · 완료 바이트 · 상태) — 워커가 채우고 UI가 스냅숏으로 받는다.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegItem {
    pub size: u64,
    pub done: u64,
    pub status: SegStatus,
}

/// 항목별 순환 팔레트(파랑 · 초록 · 주황 · 보라 · 분홍 — dir2 07-21).
pub const SEG_PALETTE: [Color; 5] = [
    Color(0x0026_7BD4),
    Color(0x0034_C759),
    Color(0x00FF_9F0A),
    Color(0x00AF_52DE),
    Color(0x00FF_375F),
];
const SKIP: Color = Color(0x00A0_A0A0);
const FAIL: Color = Color(0x00DC_3C3C);
const DIVIDER: Color = Color(0x0080_8080);
const TRACK: Color = Color(0x00F0_F0F0);
/// 구간 최소 가시 폭(경계선 1px + 본체).
const MIN_W: i32 = 3;
/// 세그먼트 표시 상한(초과 = 단색 바).
pub const MAX_SEGMENTS: usize = 512;

/// 구간 폭 배분(순수 · 시험): 크기 비례 누적 경계 → 공간이 허락하면 최소 폭 보정. `total == 0`이면 빈 목록.
#[must_use]
pub fn allocate_widths(sizes: &[u64], total: u64, w: i32) -> Vec<i32> {
    if total == 0 || sizes.is_empty() || w <= 0 {
        return Vec::new();
    }
    let n = sizes.len();
    let mut widths: Vec<i32> = Vec::with_capacity(n);
    let mut cum = 0u64;
    let mut prev_x = 0i32;
    for &s in sizes {
        cum = cum.saturating_add(s);
        let x = ((i128::from(w) * i128::from(cum)) / i128::from(total)) as i32;
        widths.push(x - prev_x);
        prev_x = x;
    }
    if (n as i64) * i64::from(MIN_W + 1) <= i64::from(w) {
        for i in 0..n {
            while widths[i] < MIN_W {
                let Some(j) = (0..n)
                    .filter(|&j| j != i && widths[j] > MIN_W)
                    .max_by_key(|&j| widths[j])
                else {
                    break;
                };
                widths[j] -= 1;
                widths[i] += 1;
            }
        }
    }
    widths
}

/// 세그먼트 진행 바 컨트롤.
#[derive(Debug)]
pub struct SegProgress {
    base: ControlBase,
    items: Vec<SegItem>,
    done: u64,
    total: u64,
}

impl Default for SegProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl SegProgress {
    #[must_use]
    pub fn new() -> Self {
        SegProgress {
            base: ControlBase::default(),
            items: Vec::new(),
            done: 0,
            total: 0,
        }
    }

    /// 항목 스냅숏 교체(바뀌었을 때만 무효화).
    pub fn set_items(&mut self, items: Vec<SegItem>, inv: &mut Invalidations) {
        if self.items != items {
            self.items = items;
            inv.push(self.base.bounds);
        }
    }

    /// 전체 진행(폴백 단색 바 · 백분율 표기용).
    pub fn set_totals(&mut self, done: u64, total: u64, inv: &mut Invalidations) {
        if (self.done, self.total) != (done, total) {
            self.done = done;
            self.total = total;
            inv.push(self.base.bounds);
        }
    }

    #[must_use]
    pub fn items(&self) -> &[SegItem] {
        &self.items
    }

    #[must_use]
    pub fn totals(&self) -> (u64, u64) {
        (self.done, self.total)
    }

    /// 백분율(0..=100 · `total == 0` = 100 — dir2 "total 0 완료는 100 %").
    #[must_use]
    pub fn percent(&self) -> u64 {
        (self.done.min(self.total) * 100)
            .checked_div(self.total)
            .unwrap_or(100)
    }
}

impl Control for SegProgress {
    fn base(&self) -> &ControlBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut ControlBase {
        &mut self.base
    }
}

impl Widget for SegProgress {
    fn bounds(&self) -> Rect {
        self.base.bounds
    }

    fn set_bounds(&mut self, bounds: Rect, inv: &mut Invalidations) {
        if self.base.bounds != bounds {
            inv.push(self.base.bounds);
            self.base.bounds = bounds;
            inv.push(bounds);
        }
    }

    fn on_event(&mut self, _ev: &InputEvent, _inv: &mut Invalidations) {}

    fn paint(&self, ctx: &mut dyn DrawCtx, theme: &Theme) {
        let b = self.base.bounds;
        if b.w <= 2 || b.h <= 2 {
            return;
        }
        // 회색 1px 테두리 + 밝은 바탕(dir2 DLG-059).
        ctx.fill_rect(b, theme.border);
        let inner = Rect::new(b.x + 1, b.y + 1, b.w - 2, b.h - 2);
        ctx.fill_rect(inner, TRACK);
        let widths = if self.items.len() <= MAX_SEGMENTS {
            let sizes: Vec<u64> = self.items.iter().map(|i| i.size).collect();
            allocate_widths(&sizes, self.total, inner.w)
        } else {
            Vec::new()
        };
        if widths.is_empty() {
            // 폴백 — 단색 바(전체 백분율 · total 0 = 0).
            let frac = if self.total > 0 {
                (self.done as f64 / self.total as f64).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let fw = (f64::from(inner.w) * frac) as i32;
            if fw > 0 {
                ctx.fill_rect(Rect::new(inner.x, inner.y, fw, inner.h), SEG_PALETTE[0]);
            }
            return;
        }
        let mut x0 = inner.x;
        for (k, (it, w)) in self.items.iter().zip(&widths).enumerate() {
            let x1 = x0 + w;
            if x1 <= x0 {
                continue;
            }
            let seg = Rect::new(x0, inner.y, x1 - x0, inner.h);
            let color = SEG_PALETTE[k % SEG_PALETTE.len()];
            match it.status {
                SegStatus::Done => ctx.fill_rect(seg, color),
                SegStatus::Skipped => ctx.fill_rect(seg, SKIP),
                SegStatus::Failed => ctx.fill_rect(seg, FAIL),
                SegStatus::Active => {
                    let frac = if it.size > 0 {
                        (it.done as f64 / it.size as f64).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let fw = (f64::from(seg.w) * frac) as i32;
                    if fw > 0 {
                        ctx.fill_rect(Rect::new(seg.x, seg.y, fw, seg.h), color);
                    }
                }
                SegStatus::Pending => {}
            }
            if k > 0 {
                ctx.fill_rect(Rect::new(x0, inner.y, 1, inner.h), DIVIDER);
            }
            x0 = x1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 크기 비례 + 최소 3px 보정: 0바이트 항목도 구간을 가진다 · 합 = 폭 · 공간 부족이면 보정 없음 · total 0 = 빈 목록.
    #[test]
    fn allocate_widths_proportional_with_min_width() {
        let w = allocate_widths(&[100, 0, 100, 1], 201, 200);
        assert_eq!(w.iter().sum::<i32>(), 200);
        assert!(w.iter().all(|&x| x >= MIN_W), "{w:?}");
        assert!(w[0] > 90 && w[2] > 90, "{w:?}");
        let tight = allocate_widths(&[1, 1, 1, 1, 1], 5, 10);
        assert_eq!(tight.iter().sum::<i32>(), 10);
        assert!(allocate_widths(&[1, 2], 0, 100).is_empty());
        assert!(allocate_widths(&[], 3, 100).is_empty());
    }

    /// 컨트롤: 스냅숏 교체는 바뀔 때만 무효화 · 백분율(total 0 = 100) · 기록기로 세그먼트 수만큼 채움.
    #[test]
    fn control_snapshot_percent_and_paint() {
        let mut p = SegProgress::new();
        let mut inv = Invalidations::default();
        p.set_bounds(Rect::new(0, 0, 202, 12), &mut inv);
        assert_eq!(p.percent(), 100, "total 0 = 100 %");
        p.set_totals(50, 200, &mut inv);
        assert_eq!(p.percent(), 25);
        let items = vec![
            SegItem {
                size: 100,
                done: 100,
                status: SegStatus::Done,
            },
            SegItem {
                size: 100,
                done: 50,
                status: SegStatus::Active,
            },
        ];
        p.set_items(items.clone(), &mut inv);
        let before = inv.drain().count();
        p.set_items(items, &mut inv);
        assert_eq!(inv.drain().count(), 0, "같은 스냅숏 = 무효화 없음");
        assert!(before > 0);
        let mut rec = crate::RecordCtx::with_surface(300, 40);
        p.paint(&mut rec, &Theme::default());
        // 테두리 + 바탕 + 완료 구간 + 진행 구간(부분) + 경계선 = 5 이상.
        assert!(rec.fills.len() >= 5, "{}", rec.fills.len());
        assert!(rec.all_inside(Rect::new(0, 0, 300, 40)));
    }
}

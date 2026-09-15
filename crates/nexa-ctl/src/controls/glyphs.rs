//! 메뉴·버튼용 **코드 도형 아이콘**(알파 마스크 · 64px · 4×4 슈퍼샘플링) — 폰트 글리프도 이미지 파일도 아니다(정적 자원 0바이트 ·
//! 3-OS 동일). 색은 그릴 때 상태색으로 틴트한다([`MenuIcon`]). 사용자 09-15 "메뉴 항목 앞 이미지 · 켜짐/꺼짐 아이콘".
//!
//! 한 번 래스터한 마스크는 스레드 로컬 캐시(`Rc`)에 두고 재사용한다.

use super::ctxmenu::MenuIcon;
use std::cell::RefCell;
use std::collections::HashMap;

/// 도형 종류.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GlyphKind {
    /// 폴더(탭 폴더 외곽선).
    Folder,
    /// 새 폴더(폴더 + ⊕).
    FolderNew,
    /// 새로 고침(원형 화살표).
    Refresh,
    /// 토글 켜짐(채운 알약 · 손잡이 오른쪽).
    ToggleOn,
    /// 토글 꺼짐(알약 외곽선 · 손잡이 왼쪽).
    ToggleOff,
    /// 복사(겹친 사각형).
    Copy,
    /// 링크/경로(사슬 고리 둘).
    Link,
    /// 열기(폴더에서 나가는 화살표).
    Open,
    /// 이름/텍스트(줄 셋).
    Text,
}

const SIDE: u32 = 64;
const SS: u32 = 4;

fn seg_dist(x: f32, y: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (vx, vy) = (bx - ax, by - ay);
    let (wx, wy) = (x - ax, y - ay);
    let t = ((wx * vx + wy * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    let (px, py) = (ax + t * vx, ay + t * vy);
    ((x - px) * (x - px) + (y - py) * (y - py)).sqrt()
}

fn stroke(x: f32, y: f32, a: (f32, f32), b: (f32, f32), w: f32) -> bool {
    seg_dist(x, y, a.0, a.1, b.0, b.1) <= w / 2.0
}

fn rrect(x: f32, y: f32, x0: f32, y0: f32, w: f32, h: f32, r: f32) -> bool {
    if x < x0 || y < y0 || x > x0 + w || y > y0 + h {
        return false;
    }
    let cx = x.clamp(x0 + r, x0 + w - r);
    let cy = y.clamp(y0 + r, y0 + h - r);
    (x - cx) * (x - cx) + (y - cy) * (y - cy) <= r * r
}

fn ring(x: f32, y: f32, cx: f32, cy: f32, r_in: f32, r_out: f32) -> bool {
    let d = ((x - cx) * (x - cx) + (y - cy) * (y - cy)).sqrt();
    (r_in..=r_out).contains(&d)
}

fn disc(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> bool {
    (x - cx) * (x - cx) + (y - cy) * (y - cy) <= r * r
}

fn shape_folder(x: f32, y: f32) -> bool {
    let body =
        rrect(x, y, 36.0, 76.0, 184.0, 128.0, 14.0) && !rrect(x, y, 58.0, 98.0, 140.0, 84.0, 8.0);
    let tab =
        rrect(x, y, 36.0, 52.0, 84.0, 40.0, 12.0) && !rrect(x, y, 58.0, 74.0, 40.0, 30.0, 6.0);
    body || tab
}

fn shape_folder_new(x: f32, y: f32) -> bool {
    // 폴더 + 오른쪽 아래 ⊕(테두리에서 살짝 겹침).
    let f = shape_folder(x, y) && !disc(x, y, 184.0, 176.0, 46.0);
    let plus = ring(x, y, 184.0, 176.0, 28.0, 38.0)
        || stroke(x, y, (170.0, 176.0), (198.0, 176.0), 12.0)
        || stroke(x, y, (184.0, 162.0), (184.0, 190.0), 12.0);
    f || plus
}

fn shape_refresh(x: f32, y: f32) -> bool {
    // 원호(위 오른쪽 틈) + 화살촉.
    let ang = (y - 128.0).atan2(x - 128.0); // -π..π · 0 = 오른쪽
    let arc = ring(x, y, 128.0, 128.0, 60.0, 82.0) && !(ang > -1.35 && ang < -0.15);
    let head = stroke(x, y, (196.0, 62.0), (196.0, 108.0), 22.0)
        || stroke(x, y, (196.0, 62.0), (150.0, 62.0), 22.0);
    arc || head
}

fn shape_toggle_on(x: f32, y: f32) -> bool {
    let pill = rrect(x, y, 28.0, 76.0, 200.0, 104.0, 52.0);
    let knob_hole = disc(x, y, 176.0, 128.0, 40.0);
    let knob = disc(x, y, 176.0, 128.0, 30.0);
    (pill && !knob_hole) || knob
}

fn shape_toggle_off(x: f32, y: f32) -> bool {
    let outline =
        rrect(x, y, 28.0, 76.0, 200.0, 104.0, 52.0) && !rrect(x, y, 48.0, 96.0, 160.0, 64.0, 32.0);
    let knob = disc(x, y, 80.0, 128.0, 30.0);
    outline || knob
}

fn shape_copy(x: f32, y: f32) -> bool {
    let back = rrect(x, y, 48.0, 40.0, 120.0, 140.0, 12.0)
        && !rrect(x, y, 70.0, 62.0, 76.0, 96.0, 6.0)
        && !rrect(x, y, 96.0, 84.0, 120.0, 140.0, 12.0);
    let front =
        rrect(x, y, 96.0, 84.0, 120.0, 140.0, 12.0) && !rrect(x, y, 118.0, 106.0, 76.0, 96.0, 6.0);
    back || front
}

fn shape_link(x: f32, y: f32) -> bool {
    // 사슬 고리 둘(대각선) + 가운데 연결 획.
    let a =
        rrect(x, y, 30.0, 96.0, 110.0, 64.0, 32.0) && !rrect(x, y, 50.0, 116.0, 70.0, 24.0, 12.0);
    let b =
        rrect(x, y, 116.0, 96.0, 110.0, 64.0, 32.0) && !rrect(x, y, 136.0, 116.0, 70.0, 24.0, 12.0);
    let bar = stroke(x, y, (96.0, 128.0), (160.0, 128.0), 20.0);
    a || b || bar
}

fn shape_open(x: f32, y: f32) -> bool {
    // 폴더 + 오른쪽 위로 나가는 화살표.
    let f = shape_folder(x, y) && !rrect(x, y, 128.0, 30.0, 110.0, 90.0, 0.0);
    let arrow = stroke(x, y, (150.0, 108.0), (214.0, 44.0), 20.0)
        || stroke(x, y, (214.0, 44.0), (214.0, 96.0), 20.0)
        || stroke(x, y, (214.0, 44.0), (162.0, 44.0), 20.0);
    f || arrow
}

fn shape_text(x: f32, y: f32) -> bool {
    stroke(x, y, (48.0, 72.0), (208.0, 72.0), 20.0)
        || stroke(x, y, (48.0, 128.0), (208.0, 128.0), 20.0)
        || stroke(x, y, (48.0, 184.0), (150.0, 184.0), 20.0)
}

fn shape_of(kind: GlyphKind) -> fn(f32, f32) -> bool {
    match kind {
        GlyphKind::Folder => shape_folder,
        GlyphKind::FolderNew => shape_folder_new,
        GlyphKind::Refresh => shape_refresh,
        GlyphKind::ToggleOn => shape_toggle_on,
        GlyphKind::ToggleOff => shape_toggle_off,
        GlyphKind::Copy => shape_copy,
        GlyphKind::Link => shape_link,
        GlyphKind::Open => shape_open,
        GlyphKind::Text => shape_text,
    }
}

fn rasterize(shape: fn(f32, f32) -> bool) -> Vec<u8> {
    let unit = 256.0 / SIDE as f32;
    let mut out = Vec::with_capacity((SIDE * SIDE) as usize);
    for py in 0..SIDE {
        for px in 0..SIDE {
            let mut hit = 0u32;
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = (px as f32 + (sx as f32 + 0.5) / SS as f32) * unit;
                    let y = (py as f32 + (sy as f32 + 0.5) / SS as f32) * unit;
                    if shape(x, y) {
                        hit += 1;
                    }
                }
            }
            out.push((hit * 255 / (SS * SS)) as u8);
        }
    }
    out
}

thread_local! {
    static CACHE: RefCell<HashMap<GlyphKind, MenuIcon>> = RefCell::new(HashMap::new());
}

/// 도형 아이콘(스레드 로컬 캐시 · 처음 한 번만 래스터).
#[must_use]
pub fn glyph(kind: GlyphKind) -> MenuIcon {
    CACHE.with(|c| {
        if let Some(m) = c.borrow().get(&kind) {
            return m.clone();
        }
        let m = MenuIcon::from_alpha(SIDE, SIDE, &rasterize(shape_of(kind)));
        c.borrow_mut().insert(kind, m.clone());
        m
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_has_body_and_is_cached() {
        for k in [
            GlyphKind::Folder,
            GlyphKind::FolderNew,
            GlyphKind::Refresh,
            GlyphKind::ToggleOn,
            GlyphKind::ToggleOff,
            GlyphKind::Copy,
            GlyphKind::Link,
            GlyphKind::Open,
            GlyphKind::Text,
        ] {
            let m = glyph(k);
            let on = m.alpha.iter().filter(|&&a| a > 128).count();
            assert!(on > 60 && on < (SIDE * SIDE) as usize / 2, "{k:?}: {on}");
            let again = glyph(k);
            assert!(std::rc::Rc::ptr_eq(&m.alpha, &again.alpha), "캐시 재사용");
        }
        // 켜짐이 꺼짐보다 채워진 픽셀이 많다(알약 채움).
        let on = glyph(GlyphKind::ToggleOn)
            .alpha
            .iter()
            .filter(|&&a| a > 128)
            .count();
        let off = glyph(GlyphKind::ToggleOff)
            .alpha
            .iter()
            .filter(|&&a| a > 128)
            .count();
        assert!(on > off);
    }
}

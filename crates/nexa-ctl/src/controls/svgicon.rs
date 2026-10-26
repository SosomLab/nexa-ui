//! SVG 경로 → 아이콘 마스크(nexa-sql `toolicons.rs` 09-16 이식 · 10-26 · 첫 공용 소비자 = [`super::searchbox::SearchBox`]):
//! `d` 문자열(M/L/H/V/C/S/Q/T/A/Z · 상대/절대 · 암묵 반복)을 다각형으로 펴고(베지어·호 = 선분 근사) 채운다 —
//! Material Symbols(`viewBox="0 -960 960 960"`)는 구멍을 반대 방향으로 감으므로 **비영(non-zero) winding** · VS Code codicon(16×16)은
//! **짝홀**. 결과는 `64×64` 커버리지(4×4 슈퍼샘플링) · 한 번 래스터해 프로세스 수명 동안 보관(경로 주소·길이 키).
//!
//! 색은 그리는 쪽이 틴트한다([`MenuIcon::from_alpha`]).

use super::ctxmenu::MenuIcon;
use crate::shape::{polys_evenodd, polys_nonzero};

/// 마스크 한 변(px).
pub const SIDE: u32 = 64;
const SS: u32 = 4;
/// SVG 960 좌표 → 256 좌표.
const M: f32 = 256.0 / 960.0;

/// Material Symbols viewBox(`0 -960 960 960`).
pub const VB_MATERIAL: [f32; 4] = [0.0, -960.0, 960.0, 960.0];
/// VS Code codicon viewBox(`0 0 16 16`).
pub const VB_CODICON: [f32; 4] = [0.0, 0.0, 16.0, 16.0];

/// 모양별 메모(키 = 경로 주소·길이·종류). 값은 프로세스 수명 동안 산다.
fn memo(key: (usize, usize, u8), make: impl FnOnce() -> Vec<u8>) -> &'static [u8] {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    type Cache = Mutex<HashMap<(usize, usize, u8), &'static [u8]>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let m = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(a) = m.lock().ok().and_then(|g| g.get(&key).copied()) {
        return a;
    }
    let a: &'static [u8] = Box::leak(make().into_boxed_slice());
    if let Ok(mut g) = m.lock() {
        g.insert(key, a);
    }
    a
}

/// 경로 토큰(명령 글자 · 숫자 = `('#', v)`).
fn svg_tokens(d: &str) -> Vec<(char, f32)> {
    let mut out = Vec::new();
    let b = d.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_ascii_alphabetic() {
            out.push((c, 0.0));
            i += 1;
        } else if c == '-' || c == '.' || c.is_ascii_digit() {
            let start = i;
            // 앞글자('-' 또는 '.')를 먼저 소비한다 — '.'로 시작하는 수(".5" · codicon)에서 인덱스가 멈춰 무한 루프였다.
            if c == '-' || c == '.' {
                i += 1;
            }
            let mut dot = c == '.';
            while i < b.len() {
                let ch = b[i] as char;
                if ch.is_ascii_digit() {
                    i += 1;
                } else if ch == '.' && !dot {
                    dot = true;
                    i += 1;
                } else {
                    break;
                }
            }
            if let Ok(v) = d[start..i].parse::<f32>() {
                out.push(('#', v));
            }
        } else {
            i += 1;
        }
    }
    out
}

/// 타원호(SVG F.6.5 끝점 → 중심 변환) → 선분 근사.
#[allow(clippy::too_many_arguments)]
fn flatten_arc(
    cur: &mut Vec<(f32, f32)>,
    p0: (f32, f32),
    rx: f32,
    ry: f32,
    phi_deg: f32,
    large: bool,
    sweep: bool,
    p1: (f32, f32),
) {
    if p0 == p1 {
        return;
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 {
        cur.push(p1);
        return;
    }
    let phi = phi_deg.to_radians();
    let (cp, sp) = (phi.cos(), phi.sin());
    let dx2 = (p0.0 - p1.0) / 2.0;
    let dy2 = (p0.1 - p1.1) / 2.0;
    let x1p = cp * dx2 + sp * dy2;
    let y1p = -sp * dx2 + cp * dy2;
    let lambda = x1p * x1p / (rx * rx) + y1p * y1p / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1p * y1p - ry * ry * x1p * x1p;
    let den = rx * rx * y1p * y1p + ry * ry * x1p * x1p;
    let coef = (num / den).max(0.0).sqrt() * if large == sweep { -1.0 } else { 1.0 };
    let cxp = coef * rx * y1p / ry;
    let cyp = -coef * ry * x1p / rx;
    let cx = cp * cxp - sp * cyp + (p0.0 + p1.0) / 2.0;
    let cy = sp * cxp + cp * cyp + (p0.1 + p1.1) / 2.0;
    let ang = |u: (f32, f32), v: (f32, f32)| -> f32 {
        let dot = u.0 * v.0 + u.1 * v.1;
        let len = (u.0 * u.0 + u.1 * u.1).sqrt() * (v.0 * v.0 + v.1 * v.1).sqrt();
        let a = (dot / len).clamp(-1.0, 1.0).acos();
        if u.0 * v.1 - u.1 * v.0 < 0.0 {
            -a
        } else {
            a
        }
    };
    let u = ((x1p - cxp) / rx, (y1p - cyp) / ry);
    let v = ((-x1p - cxp) / rx, (-y1p - cyp) / ry);
    let theta1 = ang((1.0, 0.0), u);
    let mut dtheta = ang(u, v);
    if !sweep && dtheta > 0.0 {
        dtheta -= std::f32::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += std::f32::consts::TAU;
    }
    const N: usize = 16;
    for k in 1..=N {
        let t = theta1 + dtheta * k as f32 / N as f32;
        let (ct, st) = (t.cos(), t.sin());
        cur.push((
            cx + rx * ct * cp - ry * st * sp,
            cy + rx * ct * sp + ry * st * cp,
        ));
    }
}

/// 경로 → 닫힌 다각형들(960 좌표) — `vb` = viewBox(min-x · min-y · w · h)를 960 정사각으로 맞춘다.
pub fn svg_polys_vb(d: &str, vb: [f32; 4]) -> Vec<Vec<(f32, f32)>> {
    let toks = svg_tokens(d);
    let mut polys: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut cur: Vec<(f32, f32)> = Vec::new();
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    let mut cmd = 'M';
    let mut i = 0;
    let mut last_ctrl: Option<(f32, f32)> = None;
    let num = |toks: &[(char, f32)], i: &mut usize| -> Option<f32> {
        if *i < toks.len() && toks[*i].0 == '#' {
            let v = toks[*i].1;
            *i += 1;
            Some(v)
        } else {
            None
        }
    };
    let flatten_q = |cur: &mut Vec<(f32, f32)>, p0: (f32, f32), c: (f32, f32), p1: (f32, f32)| {
        for k in 1..=8 {
            let t = k as f32 / 8.0;
            let u = 1.0 - t;
            cur.push((
                u * u * p0.0 + 2.0 * u * t * c.0 + t * t * p1.0,
                u * u * p0.1 + 2.0 * u * t * c.1 + t * t * p1.1,
            ));
        }
    };
    let flatten_c = |cur: &mut Vec<(f32, f32)>,
                     p0: (f32, f32),
                     c1: (f32, f32),
                     c2: (f32, f32),
                     p1: (f32, f32)| {
        for k in 1..=12 {
            let t = k as f32 / 12.0;
            let u = 1.0 - t;
            cur.push((
                u * u * u * p0.0
                    + 3.0 * u * u * t * c1.0
                    + 3.0 * u * t * t * c2.0
                    + t * t * t * p1.0,
                u * u * u * p0.1
                    + 3.0 * u * u * t * c1.1
                    + 3.0 * u * t * t * c2.1
                    + t * t * t * p1.1,
            ));
        }
    };
    while i < toks.len() {
        if toks[i].0 != '#' {
            cmd = toks[i].0;
            i += 1;
        }
        let rel = cmd.is_ascii_lowercase();
        let up = cmd.to_ascii_uppercase();
        match up {
            'M' => {
                let (Some(a), Some(b)) = (num(&toks, &mut i), num(&toks, &mut i)) else {
                    break;
                };
                if cur.len() > 1 {
                    polys.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
                x = if rel { x + a } else { a };
                y = if rel { y + b } else { b };
                (sx, sy) = (x, y);
                cur.push((x, y));
                cmd = if rel { 'l' } else { 'L' };
                last_ctrl = None;
            }
            'L' => {
                let (Some(a), Some(b)) = (num(&toks, &mut i), num(&toks, &mut i)) else {
                    break;
                };
                x = if rel { x + a } else { a };
                y = if rel { y + b } else { b };
                cur.push((x, y));
                last_ctrl = None;
            }
            'H' => {
                let Some(a) = num(&toks, &mut i) else { break };
                x = if rel { x + a } else { a };
                cur.push((x, y));
                last_ctrl = None;
            }
            'V' => {
                let Some(b) = num(&toks, &mut i) else { break };
                y = if rel { y + b } else { b };
                cur.push((x, y));
                last_ctrl = None;
            }
            'Q' => {
                let (Some(a), Some(b), Some(c), Some(d2)) = (
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                ) else {
                    break;
                };
                let ctrl = if rel { (x + a, y + b) } else { (a, b) };
                let p1 = if rel { (x + c, y + d2) } else { (c, d2) };
                flatten_q(&mut cur, (x, y), ctrl, p1);
                last_ctrl = Some(ctrl);
                (x, y) = p1;
            }
            'T' => {
                let (Some(c), Some(d2)) = (num(&toks, &mut i), num(&toks, &mut i)) else {
                    break;
                };
                let ctrl = last_ctrl.map_or((x, y), |(cx, cy)| (2.0 * x - cx, 2.0 * y - cy));
                let p1 = if rel { (x + c, y + d2) } else { (c, d2) };
                flatten_q(&mut cur, (x, y), ctrl, p1);
                last_ctrl = Some(ctrl);
                (x, y) = p1;
            }
            'C' => {
                let (Some(a), Some(b), Some(c), Some(d2), Some(e), Some(f)) = (
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                ) else {
                    break;
                };
                let c1 = if rel { (x + a, y + b) } else { (a, b) };
                let c2 = if rel { (x + c, y + d2) } else { (c, d2) };
                let p1 = if rel { (x + e, y + f) } else { (e, f) };
                flatten_c(&mut cur, (x, y), c1, c2, p1);
                last_ctrl = Some(c2);
                (x, y) = p1;
            }
            'S' => {
                let (Some(c), Some(d2), Some(e), Some(f)) = (
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                ) else {
                    break;
                };
                let c1 = last_ctrl.map_or((x, y), |(cx, cy)| (2.0 * x - cx, 2.0 * y - cy));
                let c2 = if rel { (x + c, y + d2) } else { (c, d2) };
                let p1 = if rel { (x + e, y + f) } else { (e, f) };
                flatten_c(&mut cur, (x, y), c1, c2, p1);
                last_ctrl = Some(c2);
                (x, y) = p1;
            }
            'A' => {
                let (Some(rx), Some(ry), Some(rot), Some(la), Some(sw), Some(e), Some(f)) = (
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                    num(&toks, &mut i),
                ) else {
                    break;
                };
                let p1 = if rel { (x + e, y + f) } else { (e, f) };
                flatten_arc(&mut cur, (x, y), rx, ry, rot, la != 0.0, sw != 0.0, p1);
                last_ctrl = None;
                (x, y) = p1;
            }
            'Z' => {
                if cur.len() > 1 {
                    polys.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
                (x, y) = (sx, sy);
                cur.push((x, y));
                last_ctrl = None;
            }
            _ => {
                i += 1;
            }
        }
    }
    if cur.len() > 2 {
        polys.push(cur);
    }
    let [vx, vy, vw, vh] = vb;
    for p in &mut polys {
        for v in p.iter_mut() {
            v.0 = (v.0 - vx) * 960.0 / vw;
            v.1 = (v.1 - vy) * 960.0 / vh;
        }
    }
    polys
}

/// 클로저 도형 → 커버리지(4×4 슈퍼샘플링 · 256 좌표).
fn rasterize_dyn(shape: &dyn Fn(f32, f32) -> bool) -> Vec<u8> {
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

/// Material Symbols `d`(viewBox `0 -960 960 960` · 비영 채움) → 아이콘 마스크(색은 그리는 쪽이 틴트).
#[must_use]
pub fn svg_glyph(d: &str) -> MenuIcon {
    let alpha = memo((d.as_ptr() as usize, d.len(), 1), || {
        let polys = svg_polys_vb(d, VB_MATERIAL);
        rasterize_dyn(&|x, y| polys_nonzero(x / M, y / M, &polys))
    });
    MenuIcon::from_alpha(SIDE, SIDE, alpha)
}

/// 임의 viewBox SVG `d` → 아이콘 마스크(`evenodd` = 짝홀 채움 · VS Code codicon).
#[must_use]
pub fn svg_glyph_vb(d: &str, vb: [f32; 4], evenodd: bool) -> MenuIcon {
    let vbk = vb
        .iter()
        .fold(0usize, |a, v| a.rotate_left(13) ^ v.to_bits() as usize);
    let alpha = memo(
        (d.as_ptr() as usize, d.len() ^ vbk, 2 + u8::from(evenodd)),
        || {
            let polys = svg_polys_vb(d, vb);
            if evenodd {
                rasterize_dyn(&|x, y| polys_evenodd(x / M, y / M, &polys))
            } else {
                rasterize_dyn(&|x, y| polys_nonzero(x / M, y / M, &polys))
            }
        },
    );
    MenuIcon::from_alpha(SIDE, SIDE, alpha)
}

/// Material Symbols Outlined 24px(Apache-2.0) 중 공용 컨트롤이 쓰는 것 — 찾기/검색 토글(VS Code 배치 · nexa-sql 09-16).
pub mod material {
    use super::{svg_glyph, MenuIcon};

    /// `match_case` — 대소문자 구분(Aa).
    pub const MATCH_CASE: &str = "m131-252 165-440h79l165 440h-76l-39-112H247l-40 112h-76Zm139-176h131l-64-182h-4l-63 182Zm395 186q-51 0-81-27.5T554-342q0-44 34.5-72.5T677-443q23 0 45 4t38 11v-12q0-29-20.5-47T685-505q-23 0-42 9.5T610-468l-47-35q24-29 54.5-43t68.5-14q69 0 103 32.5t34 97.5v178h-63v-37h-4q-14 23-38 35t-53 12Zm12-54q35 0 59.5-24t24.5-56q-14-8-33.5-12.5T689-393q-32 0-50 14t-18 37q0 20 16 33t40 13Z";
    /// `match_word` — 단어 단위(ab 밑줄).
    pub const MATCH_WORD: &str = "M40-199v-200h80v120h720v-120h80v200H40Zm342-161v-34h-3q-13 20-35 31.5T294-351q-49 0-77-25.5T189-446q0-42 32.5-68.5T305-541q23 0 42.5 3.5T381-526v-14q0-27-18.5-43T312-599q-21 0-39.5 9T241-564l-43-32q19-27 48-41t67-14q62 0 95 29.5t33 85.5v176h-59Zm-66-134q-32 0-49 12.5T250-446q0 20 15 32.5t39 12.5q32 0 54.5-22.5T381-478q-14-8-32-12t-33-4Zm185 134v-401h62v113l-3 40h3q3-5 24-25.5t66-20.5q64 0 101 46t37 106q0 60-36.5 105.5T653-351q-41 0-62.5-18T563-397h-3v37h-59Zm143-238q-40 0-62 29.5T560-503q0 37 22 66t62 29q40 0 62.5-29t22.5-66q0-37-22.5-66T644-598Z";
    /// `folder_check` — 범위 토글(dir3 "하위 폴더 검색" · nexa-sql `mi_path_match` "경로까지 검색"과 같은 아이콘).
    pub const FOLDER_CHECK: &str = "m434-297 226-227-56-56-170 170-85-85-57 57 142 141ZM160-160q-33 0-56.5-23.5T80-240v-480q0-33 23.5-56.5T160-800h240l80 80h320q33 0 56.5 23.5T880-640v400q0 33-23.5 56.5T800-160H160Zm0-80h640v-400H447l-80-80H160v480Zm0 0v-480 480Z";
    /// `regular_expression` — 정규식(.*).
    pub const REGEX: &str = "M197-199q-56-57-86.5-130T80-482q0-80 30-153t87-130l57 57q-46 45-70 103.5T160-482q0 64 24.5 122.5T254-256l-57 57Zm183-41q-25 0-42.5-17.5T320-300q0-25 17.5-42.5T380-360q25 0 42.5 17.5T440-300q0 25-17.5 42.5T380-240Zm139-200v-71l-61 36-40-70 61-35-61-35 40-70 61 36v-71h80v71l61-36 40 70-61 35 61 35-40 70-61-36v71h-80Zm244 241-57-57q46-45 70-103.5T800-482q0-64-24.5-122.5T706-708l57-57q56 57 86.5 130T880-482q0 80-30 153t-87 130Z";

    /// 대소문자 구분(Aa).
    #[must_use]
    pub fn match_case() -> MenuIcon {
        svg_glyph(MATCH_CASE)
    }
    /// 단어 단위(ab).
    #[must_use]
    pub fn match_word() -> MenuIcon {
        svg_glyph(MATCH_WORD)
    }
    /// 정규식(.*).
    #[must_use]
    pub fn regex() -> MenuIcon {
        svg_glyph(REGEX)
    }
    /// 범위(폴더 + 체크 · 하위 폴더까지).
    #[must_use]
    pub fn folder_check() -> MenuIcon {
        svg_glyph(FOLDER_CHECK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `.5-.526`처럼 점으로 시작하는 수(codicon)가 토큰으로 나뉜다 — 예전엔 무한 루프.
    #[test]
    fn svg_tokens_dot_leading_numbers() {
        let v: Vec<f32> = svg_tokens("a.523.523 0 0 1 .5-.526")
            .iter()
            .filter(|t| t.0 == '#')
            .map(|t| t.1)
            .collect();
        assert_eq!(v, vec![0.523, 0.523, 0.0, 0.0, 1.0, 0.5, -0.526]);
    }

    /// codicon `replace`(16 viewBox · 호 `a` · evenodd): 상자 테두리는 채워지고 상자 속(구멍)은 빈다.
    #[test]
    fn codicon_replace_arc_and_evenodd() {
        let d = "M4 7L3 8v6l1 1h7l1-1V8l-1-1H4zm0 1h7v6H4V8zM5 2.489A1.482 1.482 0 0 1 6.48 1H8v.965H6.48a.523.523 0 0 0-.5.526V4.1H5z";
        let polys = svg_polys_vb(d, VB_CODICON);
        let k = 960.0 / 16.0;
        assert!(
            polys_evenodd(3.5 * k, 11.0 * k, &polys),
            "왼쪽 테두리(x 3~4)"
        );
        assert!(!polys_evenodd(7.5 * k, 11.0 * k, &polys), "상자 속은 구멍");
        assert!(
            polys_evenodd(5.4 * k, 3.5 * k, &polys),
            "호로 이어진 세로획(x 5~6)"
        );
        let icon = svg_glyph_vb(d, VB_CODICON, true);
        assert_eq!(icon.w, SIDE);
    }

    /// Material 토글 아이콘 셋 = 몸통(불투명)과 가장자리(부분 커버리지)가 있고 슬롯을 다 덮지 않는다 · 같은 경로 = 같은 마스크(메모).
    #[test]
    fn material_toggle_icons_have_body_and_edges() {
        for icon in [
            material::match_case(),
            material::match_word(),
            material::regex(),
        ] {
            let opaque = icon.alpha.iter().filter(|&&a| a == 255).count();
            let partial = icon.alpha.iter().filter(|&&a| a > 0 && a < 255).count();
            assert!(opaque > 100, "채워진 픽셀: {opaque}");
            assert!(partial > 20, "가장자리 AA: {partial}");
            assert!(opaque < (SIDE * SIDE) as usize / 2);
        }
        let a = material::regex();
        let b = material::regex();
        assert_eq!(a.alpha, b.alpha, "메모 = 같은 마스크");
    }
}

//! ★ macOS CoreText 글리프 원천(T-100 · nexa-sql 사용자 09-17 "보조(1x) 모니터에서 파인더 수준으로") — Windows
//! [`crate::gdi`]와 같은 포트. **CoreText가 그린 회색 커버리지 + 소수 전진 폭**을 그대로 쓰므로 같은 모니터의
//! Finder·TextEdit과 같은 픽셀(Apple 스무딩 · 감마 · 서브픽셀 위치 · SF 광학 크기)이 나온다. 다른 OS · CoreText가
//! 이름을 못 찾는 face · 비BMP 문자는 `ab_glyph` 경로로 돌아간다.
//!
//! 프레임워크는 `extern "C"` 선언으로 직접 링크(외부 crate 0 · DR-3). CTFont는 스레드 안전하지만 캐시는 GDI와 같은
//! 규약으로 스레드 로컬((패밀리, em px, 굵게) → face).

use std::cell::RefCell;
use std::ffi::c_void;

type CFTypeRef = *const c_void;
type CFStringRef = CFTypeRef;
type CTFontRef = CFTypeRef;
type CGColorSpaceRef = CFTypeRef;
type CGContextRef = *mut c_void;
type CGGlyph = u16;
type UniChar = u16;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGPoint {
    x: f64,
    y: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGSize {
    width: f64,
    height: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGAffineTransform {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}

const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const CT_FONT_BOLD_TRAIT: u32 = 1 << 1;
const CT_FONT_ORIENTATION_DEFAULT: u32 = 0;
/// `kCGImageAlphaNone` — 회색 8bpp.
const CG_IMAGE_ALPHA_NONE: u32 = 0;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithBytes(
        alloc: CFTypeRef,
        bytes: *const u8,
        num_bytes: isize,
        encoding: u32,
        is_external: bool,
    ) -> CFStringRef;
    fn CFStringGetCString(s: CFStringRef, buf: *mut u8, size: isize, encoding: u32) -> bool;
    fn CFRelease(cf: CFTypeRef);
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGColorSpaceCreateDeviceGray() -> CGColorSpaceRef;
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: CGColorSpaceRef,
        bitmap_info: u32,
    ) -> CGContextRef;
    fn CGContextRelease(c: CGContextRef);
    fn CGContextSetGrayFillColor(c: CGContextRef, gray: f64, alpha: f64);
    fn CGContextSetShouldAntialias(c: CGContextRef, on: bool);
    fn CGContextSetShouldSmoothFonts(c: CGContextRef, on: bool);
    fn CGContextSetAllowsFontSmoothing(c: CGContextRef, on: bool);
    fn CGContextSetShouldSubpixelPositionFonts(c: CGContextRef, on: bool);
    fn CGContextSetAllowsFontSubpixelPositioning(c: CGContextRef, on: bool);
    fn CGContextSetShouldSubpixelQuantizeFonts(c: CGContextRef, on: bool);
    fn CGContextSetAllowsFontSubpixelQuantization(c: CGContextRef, on: bool);
    fn CGContextSetTextMatrix(c: CGContextRef, t: CGAffineTransform);
}

#[link(name = "CoreText", kind = "framework")]
extern "C" {
    fn CTFontCreateWithName(
        name: CFStringRef,
        size: f64,
        matrix: *const CGAffineTransform,
    ) -> CTFontRef;
    fn CTFontCreateCopyWithSymbolicTraits(
        font: CTFontRef,
        size: f64,
        matrix: *const CGAffineTransform,
        value: u32,
        mask: u32,
    ) -> CTFontRef;
    fn CTFontCopyFamilyName(font: CTFontRef) -> CFStringRef;
    fn CTFontGetGlyphsForCharacters(
        font: CTFontRef,
        chars: *const UniChar,
        glyphs: *mut CGGlyph,
        count: isize,
    ) -> bool;
    fn CTFontGetAdvancesForGlyphs(
        font: CTFontRef,
        orientation: u32,
        glyphs: *const CGGlyph,
        advances: *mut CGSize,
        count: isize,
    ) -> f64;
    fn CTFontGetBoundingRectsForGlyphs(
        font: CTFontRef,
        orientation: u32,
        glyphs: *const CGGlyph,
        rects: *mut CGRect,
        count: isize,
    ) -> CGRect;
    fn CTFontDrawGlyphs(
        font: CTFontRef,
        glyphs: *const CGGlyph,
        positions: *const CGPoint,
        count: usize,
        context: CGContextRef,
    );
}

/// CoreText face 하나(굵게는 볼드 trait 사본 · 없으면 같은 face).
struct Face {
    font: CTFontRef,
}

impl Drop for Face {
    fn drop(&mut self) {
        // SAFETY: 이 스레드가 만든 CFType을 한 번만 놓는다.
        unsafe { CFRelease(self.font) }
    }
}

/// 래스터된 CoreText 글리프 — 원점(펜 정수 x · 베이스라인) 기준. `w == 0`이면 외곽선 없음 · `adv`만 유효.
/// `cov`는 픽셀당 1바이트(회색 커버리지). 전진 폭은 [`advance`](소수 · 서브픽셀 위치)로 따로.
pub(crate) struct Glyph {
    pub w: u16,
    pub h: u16,
    pub ox: i16,
    pub oy: i16,
    pub cov: Vec<u8>,
}

type FaceList = Vec<(String, i32, bool, Option<Face>)>;

thread_local! {
    static FACES: RefCell<FaceList> = const { RefCell::new(Vec::new()) };
}

fn cf_string(s: &str) -> CFStringRef {
    // SAFETY: 바이트 슬라이스와 길이가 맞는다 · 결과는 호출자가 CFRelease.
    unsafe {
        CFStringCreateWithBytes(
            std::ptr::null(),
            s.as_ptr(),
            s.len() as isize,
            CF_STRING_ENCODING_UTF8,
            false,
        )
    }
}

fn cf_to_string(s: CFStringRef) -> String {
    if s.is_null() {
        return String::new();
    }
    let mut buf = vec![0u8; 256];
    // SAFETY: 버퍼 크기를 그대로 넘긴다 · 실패하면 빈 문자열.
    let ok = unsafe {
        CFStringGetCString(
            s,
            buf.as_mut_ptr(),
            buf.len() as isize,
            CF_STRING_ENCODING_UTF8,
        )
    };
    if !ok {
        return String::new();
    }
    let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

/// 이름 하나로 face를 만들고 CoreText가 실제로 고른 패밀리가 후보 안이면 성공 — 대체 글꼴(Helvetica 폴백)이면 `None`.
fn try_name(name: &str, names: &[String], em_px: i32, bold: bool) -> Option<Face> {
    let cfname = cf_string(name);
    if cfname.is_null() {
        return None;
    }
    // SAFETY: 유효한 CFString · 결과 CTFont는 Face가 소유.
    let font = unsafe { CTFontCreateWithName(cfname, f64::from(em_px), std::ptr::null()) };
    unsafe { CFRelease(cfname) };
    if font.is_null() {
        return None;
    }
    let got = unsafe {
        let fam = CTFontCopyFamilyName(font);
        let s = cf_to_string(fam);
        if !fam.is_null() {
            CFRelease(fam);
        }
        s
    };
    let ok = names.iter().any(|n| norm(n) == norm(&got));
    if !ok {
        unsafe { CFRelease(font) };
        return None;
    }
    if !bold {
        return Some(Face { font });
    }
    // 굵게 = 볼드 trait 사본(없으면 같은 face 유지 → 호스트가 faux 볼드).
    let b = unsafe {
        CTFontCreateCopyWithSymbolicTraits(
            font,
            f64::from(em_px),
            std::ptr::null(),
            CT_FONT_BOLD_TRAIT,
            CT_FONT_BOLD_TRAIT,
        )
    };
    if b.is_null() {
        return Some(Face { font });
    }
    unsafe { CFRelease(font) };
    Some(Face { font: b })
}

fn make_face(names: &[String], em_px: i32, bold: bool) -> Option<Face> {
    names.iter().find_map(|n| try_name(n, names, em_px, bold))
}

fn with_face<R>(names: &[String], em_px: i32, bold: bool, f: impl FnOnce(&Face) -> R) -> Option<R> {
    let key = names.first()?;
    FACES.with(|c| {
        let mut c = c.borrow_mut();
        let pos = match c
            .iter()
            .position(|(k, e, b, _)| k == key && *e == em_px && *b == bold)
        {
            Some(p) => p,
            None => {
                let face = make_face(names, em_px, bold);
                c.push((key.clone(), em_px, bold, face));
                c.len() - 1
            }
        };
        c[pos].3.as_ref().map(f)
    })
}

/// 후보 이름 중 하나로 CoreText가 같은 패밀리를 여는가(못 열면 이 face는 ab_glyph 경로).
pub(crate) fn face_ok(names: &[String], em_px: i32) -> bool {
    with_face(names, em_px, false, |_| ()).is_some()
}

fn glyph_id(face: &Face, code: u32) -> Option<CGGlyph> {
    let ch = [code as UniChar];
    let mut g = [0 as CGGlyph];
    // SAFETY: 배열 길이 1을 그대로 넘긴다.
    let ok = unsafe { CTFontGetGlyphsForCharacters(face.font, ch.as_ptr(), g.as_mut_ptr(), 1) };
    (ok && g[0] != 0).then_some(g[0])
}

/// 소수 전진 폭(px). 비BMP·글리프 없음·face 없음 = `None`.
pub(crate) fn advance(names: &[String], em_px: i32, bold: bool, ch: char) -> Option<f32> {
    let code = u32::from(ch);
    if code > 0xFFFF {
        return None;
    }
    with_face(names, em_px, bold, |face| {
        let g = [glyph_id(face, code)?];
        let mut adv = [CGSize::default()];
        // SAFETY: 길이 1 배열.
        unsafe {
            CTFontGetAdvancesForGlyphs(
                face.font,
                CT_FONT_ORIENTATION_DEFAULT,
                g.as_ptr(),
                adv.as_mut_ptr(),
                1,
            )
        };
        Some(adv[0].width as f32)
    })
    .flatten()
}

const PAD: i32 = 2;

/// CoreText 글리프(회색 커버리지) + 소수 전진 폭 — `sub` = 서브픽셀 x 위치(0..3 · 1/3px 단위).
pub(crate) fn glyph(names: &[String], em_px: i32, bold: bool, ch: char, sub: u8) -> Option<Glyph> {
    let code = u32::from(ch);
    if code > 0xFFFF {
        return None;
    }
    with_face(names, em_px, bold, |face| render(face, code, sub)).flatten()
}

fn render(face: &Face, code: u32, sub: u8) -> Option<Glyph> {
    let gid = glyph_id(face, code)?;
    let g = [gid];
    let mut rect = [CGRect::default()];
    // SAFETY: 길이 1 배열.
    unsafe {
        CTFontGetBoundingRectsForGlyphs(
            face.font,
            CT_FONT_ORIENTATION_DEFAULT,
            g.as_ptr(),
            rect.as_mut_ptr(),
            1,
        );
    }
    let r = rect[0];
    if r.size.width <= 0.0 || r.size.height <= 0.0 {
        return Some(Glyph {
            w: 0,
            h: 0,
            ox: 0,
            oy: 0,
            cov: Vec::new(),
        });
    }
    // 비트맵: 검은 상자 + 여백(서브픽셀 이동·스무딩 번짐 수용). CG 좌표는 아래가 원점.
    let bw = (r.size.width.ceil() as i32 + PAD * 2 + 2).max(1) as usize;
    let bh = (r.size.height.ceil() as i32 + PAD * 2 + 2).max(1) as usize;
    let pen_x = PAD - r.origin.x.floor() as i32;
    let pen_y = PAD - r.origin.y.floor() as i32; // CG y(위로 양수) · 정수
    let mut bits = vec![0u8; bw * bh];
    // SAFETY: 버퍼 크기 = bw*bh · 컨텍스트는 이 함수 안에서 만들고 놓는다.
    unsafe {
        let cs = CGColorSpaceCreateDeviceGray();
        let ctx = CGBitmapContextCreate(
            bits.as_mut_ptr().cast(),
            bw,
            bh,
            8,
            bw,
            cs,
            CG_IMAGE_ALPHA_NONE,
        );
        CFRelease(cs);
        if ctx.is_null() {
            return None;
        }
        CGContextSetGrayFillColor(ctx, 1.0, 1.0);
        CGContextSetShouldAntialias(ctx, true);
        // Apple 스무딩(파인더와 같은 획 두께·감마) · 서브픽셀 위치 · 양자화 없음.
        CGContextSetAllowsFontSmoothing(ctx, true);
        CGContextSetShouldSmoothFonts(ctx, true);
        CGContextSetAllowsFontSubpixelPositioning(ctx, true);
        CGContextSetShouldSubpixelPositionFonts(ctx, true);
        CGContextSetAllowsFontSubpixelQuantization(ctx, false);
        CGContextSetShouldSubpixelQuantizeFonts(ctx, false);
        CGContextSetTextMatrix(
            ctx,
            CGAffineTransform {
                a: 1.0,
                b: 0.0,
                c: 0.0,
                d: 1.0,
                tx: 0.0,
                ty: 0.0,
            },
        );
        let pos = [CGPoint {
            x: f64::from(pen_x) + f64::from(sub) / 3.0,
            y: f64::from(pen_y),
        }];
        CTFontDrawGlyphs(face.font, g.as_ptr(), pos.as_ptr(), 1, ctx);
        CGContextRelease(ctx);
    }
    // CGBitmapContext 메모리의 첫 행 = 이미지 **위쪽**(CG 좌표 y는 아래가 0이지만 저장은 위→아래) → 그대로 읽는다.
    let px = |x: usize, y_td: usize| bits[y_td * bw + x];
    let (mut minx, mut maxx, mut miny, mut maxy) = (usize::MAX, 0usize, usize::MAX, 0usize);
    for y in 0..bh {
        for x in 0..bw {
            if px(x, y) != 0 {
                minx = minx.min(x);
                maxx = maxx.max(x);
                miny = miny.min(y);
                maxy = maxy.max(y);
            }
        }
    }
    if minx == usize::MAX {
        return Some(Glyph {
            w: 0,
            h: 0,
            ox: 0,
            oy: 0,
            cov: Vec::new(),
        });
    }
    let (w, h) = (maxx - minx + 1, maxy - miny + 1);
    let mut cov = Vec::with_capacity(w * h);
    for y in miny..=maxy {
        for x in minx..=maxx {
            cov.push(px(x, y));
        }
    }
    // 베이스라인의 top-down 행 = bh − pen_y(베이스라인 바로 아래 행) — GDI의 pen_y와 같은 뜻.
    let baseline_td = bh as i32 - pen_y;
    Some(Glyph {
        w: w as u16,
        h: h as u16,
        ox: (minx as i32 - pen_x) as i16,
        oy: (miny as i32 - baseline_td) as i16,
        cov,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 시스템 글꼴로 라틴·한글 글리프가 나오고(회색 커버리지 · 베이스라인 위 ox/oy) 전진 폭이 소수로 온다.
    #[test]
    fn helvetica_and_hangul_render() {
        let names = vec!["Helvetica".to_string()];
        assert!(face_ok(&names, 13));
        let g = glyph(&names, 13, false, 'H', 0).expect("H");
        assert!(g.w > 0 && g.h > 0);
        assert!(g.oy < 0, "대문자는 베이스라인 위: oy={}", g.oy);
        assert!(g.cov.contains(&255), "완전 검은 픽셀이 있어야(줄기)");
        assert!(g.cov.iter().any(|&c| c > 0 && c < 255), "AA 회색이 있어야");
        let a = advance(&names, 13, false, 'H').expect("adv");
        assert!(a > 5.0 && a < 13.0, "adv={a}");
        // 굵게 = 볼드 face(잉크가 더 많다).
        let gb = glyph(&names, 13, true, 'H', 0).expect("bold");
        let ink = |g: &Glyph| g.cov.iter().map(|&c| c as u32).sum::<u32>();
        assert!(ink(&gb) > ink(&g));
        // 한글은 Apple SD Gothic Neo.
        let ko = vec!["Apple SD Gothic Neo".to_string()];
        assert!(face_ok(&ko, 15));
        let h = glyph(&ko, 15, false, '한', 0).expect("한");
        assert!(h.w > 6 && h.h > 6);
        // 없는 이름은 None(폴백 Helvetica를 받아들이지 않는다).
        assert!(!face_ok(&["NoSuchFontFamily_XYZ".to_string()], 13));
        // 방향: 'L'의 가로획은 **맨 아래 행**(뒤집히면 맨 위) · 'j'는 베이스라인 아래로 내려간다(oy+h > 0).
        let l = glyph(&names, 13, false, 'L', 0).expect("L");
        let row_ink = |g: &Glyph, r: usize| {
            g.cov[r * g.w as usize..(r + 1) * g.w as usize]
                .iter()
                .map(|&c| u32::from(c))
                .sum::<u32>()
        };
        assert!(
            row_ink(&l, l.h as usize - 1) > row_ink(&l, 0) * 2,
            "L 발이 아래 행에"
        );
        let j = glyph(&names, 13, false, 'j', 0).expect("j");
        assert!(
            i32::from(j.oy) + i32::from(j.h) > 0,
            "j 디센더는 베이스라인 아래: oy={} h={}",
            j.oy,
            j.h
        );
        // 서브픽셀 위치가 다르면 비트맵이 다르다.
        let g1 = glyph(&names, 13, false, 'l', 1).expect("l");
        let g0 = glyph(&names, 13, false, 'l', 0).expect("l");
        assert!(g1.cov != g0.cov || g1.ox != g0.ox);
    }
}

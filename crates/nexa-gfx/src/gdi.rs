//! ★ Windows GDI 글리프 원천(사용자 09-16 "Golden처럼 선명하게") — `GetGlyphOutlineW(GGO_GRAY8_BITMAP)`로
//! **OS 힌팅(TrueType 바이트코드)이 적용된 회색 AA 비트맵**과 **정수 전진 폭**을 얻는다. Golden(GDI 텍스트)과 같은
//! 래스터라이저를 쓰므로 획 정렬·굵기·자간이 같아진다(색 프린지 없는 ClearType급 선명도). 다른 OS·GDI가 이름을 못
//! 찾는 face·비BMP 문자는 `ab_glyph` 경로로 돌아간다.
//!
//! HDC·HFONT는 스레드 소속이라 **스레드 로컬** 캐시((패밀리, em px) → face)로 둔다.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;

type H = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct Fixed {
    fract: u16,
    value: i16,
}

#[repr(C)]
struct Mat2 {
    m11: Fixed,
    m12: Fixed,
    m21: Fixed,
    m22: Fixed,
}

#[repr(C)]
#[derive(Default)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Default)]
struct GlyphMetrics {
    black_box_x: u32,
    black_box_y: u32,
    origin: Point,
    cell_inc_x: i16,
    cell_inc_y: i16,
}

#[repr(C)]
struct LogFontW {
    height: i32,
    width: i32,
    escapement: i32,
    orientation: i32,
    weight: i32,
    italic: u8,
    underline: u8,
    strike_out: u8,
    char_set: u8,
    out_precision: u8,
    clip_precision: u8,
    quality: u8,
    pitch_and_family: u8,
    face_name: [u16; 32],
}

#[link(name = "gdi32")]
extern "system" {
    fn CreateFontIndirectW(lf: *const LogFontW) -> H;
    fn CreateCompatibleDC(hdc: H) -> H;
    fn DeleteDC(hdc: H) -> i32;
    fn DeleteObject(h: H) -> i32;
    fn SelectObject(hdc: H, h: H) -> H;
    fn GetGlyphOutlineW(
        hdc: H,
        ch: u32,
        format: u32,
        gm: *mut GlyphMetrics,
        cb: u32,
        buf: *mut c_void,
        mat: *const Mat2,
    ) -> u32;
    fn GetTextFaceW(hdc: H, n: i32, buf: *mut u16) -> i32;
}

const GGO_GRAY8_BITMAP: u32 = 6;
const GDI_ERROR: u32 = 0xFFFF_FFFF;
const FW_NORMAL: i32 = 400;
const DEFAULT_CHARSET: u8 = 1;
const ANTIALIASED_QUALITY: u8 = 4;

/// GDI face 하나(메모리 DC + 선택된 HFONT).
struct Face {
    hdc: H,
    hfont: H,
}

impl Drop for Face {
    fn drop(&mut self) {
        // SAFETY: 이 스레드가 만든 핸들을 한 번만 해제한다.
        unsafe {
            DeleteDC(self.hdc);
            DeleteObject(self.hfont);
        }
    }
}

/// 래스터된 GDI 글리프 — 원점(펜 정수 x · 베이스라인) 기준. `w == 0`이면 외곽선 없음(공백) · `adv`만 유효.
pub(crate) struct Glyph {
    pub w: u16,
    pub h: u16,
    pub ox: i16,
    pub oy: i16,
    pub cov: Vec<u8>,
    pub adv: i32,
}

thread_local! {
    static FACES: RefCell<HashMap<(String, i32), Option<Face>>> = RefCell::new(HashMap::new());
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

/// face를 만든다 — GDI가 이름을 못 찾아 다른 글꼴로 대체하면(`GetTextFaceW` 불일치) `None`(ab_glyph 경로로).
fn make_face(family: &str, em_px: i32) -> Option<Face> {
    let mut name = [0u16; 32];
    for (i, u) in family.encode_utf16().take(31).enumerate() {
        name[i] = u;
    }
    let lf = LogFontW {
        height: -em_px.max(1),
        width: 0,
        escapement: 0,
        orientation: 0,
        weight: FW_NORMAL,
        italic: 0,
        underline: 0,
        strike_out: 0,
        char_set: DEFAULT_CHARSET,
        out_precision: 0,
        clip_precision: 0,
        quality: ANTIALIASED_QUALITY,
        pitch_and_family: 0,
        face_name: name,
    };
    // SAFETY: 유효한 LOGFONTW · 반환 핸들은 즉시 검사한다.
    unsafe {
        let hfont = CreateFontIndirectW(&lf);
        if hfont.is_null() {
            return None;
        }
        let hdc = CreateCompatibleDC(std::ptr::null_mut());
        if hdc.is_null() {
            DeleteObject(hfont);
            return None;
        }
        SelectObject(hdc, hfont);
        let mut got = [0u16; 64];
        let n = GetTextFaceW(hdc, 64, got.as_mut_ptr());
        let got = if n > 0 {
            String::from_utf16_lossy(&got[..(n as usize).saturating_sub(1)])
        } else {
            String::new()
        };
        if norm(&got) != norm(family) {
            DeleteDC(hdc);
            DeleteObject(hfont);
            return None;
        }
        Some(Face { hdc, hfont })
    }
}

fn with_face<R>(family: &str, em_px: i32, f: impl FnOnce(&Face) -> R) -> Option<R> {
    FACES.with(|c| {
        let mut c = c.borrow_mut();
        let key = (family.to_string(), em_px);
        if !c.contains_key(&key) {
            let face = make_face(family, em_px);
            c.insert(key.clone(), face);
        }
        c.get(&key).and_then(|f| f.as_ref()).map(f)
    })
}

/// `family`를 GDI가 그 이름 그대로 찾는가(못 찾으면 이 face는 ab_glyph 경로).
pub(crate) fn face_ok(family: &str, em_px: i32) -> bool {
    with_face(family, em_px, |_| ()).is_some()
}

/// 힌팅된 회색 AA 글리프 + 정수 전진 폭. 비BMP 문자·실패는 `None`.
pub(crate) fn glyph(family: &str, em_px: i32, ch: char) -> Option<Glyph> {
    let code = u32::from(ch);
    if code > 0xFFFF {
        return None;
    }
    with_face(family, em_px, |face| {
        let one = Fixed { fract: 0, value: 1 };
        let zero = Fixed { fract: 0, value: 0 };
        let mat = Mat2 {
            m11: one,
            m12: zero,
            m21: zero,
            m22: one,
        };
        let mut gm = GlyphMetrics::default();
        // SAFETY: 유효한 DC · 출력 구조체 · 첫 호출은 크기만 묻는다(버퍼 0).
        let size = unsafe {
            GetGlyphOutlineW(
                face.hdc,
                code,
                GGO_GRAY8_BITMAP,
                &mut gm,
                0,
                std::ptr::null_mut(),
                &mat,
            )
        };
        if size == GDI_ERROR {
            return None;
        }
        let adv = i32::from(gm.cell_inc_x);
        if size == 0 || gm.black_box_x == 0 || gm.black_box_y == 0 {
            return Some(Glyph {
                w: 0,
                h: 0,
                ox: 0,
                oy: 0,
                cov: Vec::new(),
                adv,
            });
        }
        let mut buf = vec![0u8; size as usize];
        // SAFETY: 버퍼 크기 = 첫 호출이 알려 준 바이트 수.
        let got = unsafe {
            GetGlyphOutlineW(
                face.hdc,
                code,
                GGO_GRAY8_BITMAP,
                &mut gm,
                size,
                buf.as_mut_ptr().cast(),
                &mat,
            )
        };
        if got == GDI_ERROR {
            return None;
        }
        let (w, h) = (gm.black_box_x as usize, gm.black_box_y as usize);
        // 행은 DWORD(4바이트) 정렬 · 값은 0..=64.
        let stride = (w + 3) & !3;
        let mut cov = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let v = buf.get(y * stride + x).copied().unwrap_or(0);
                cov[y * w + x] = (u32::from(v) * 255 / 64).min(255) as u8;
            }
        }
        Some(Glyph {
            w: w as u16,
            h: h as u16,
            ox: gm.origin.x as i16,
            oy: (-gm.origin.y) as i16,
            cov,
            adv,
        })
    })
    .flatten()
}

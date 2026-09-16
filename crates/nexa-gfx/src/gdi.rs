//! ★ Windows GDI 글리프 원천(사용자 09-16 "Golden처럼 선명하게") — **GDI ClearType**을 32bpp DIB에 그려
//! 채널별(R·G·B 서브픽셀) 커버리지와 **정수 전진 폭**을 얻는다. Windows·Golden이 화면에 그리는 것과 같은
//! 래스터(TrueType 힌팅 + 서브픽셀 + 대비 보강)라 맑은 고딕처럼 획이 반 픽셀보다 얇은 글꼴도 작은 크기에서
//! 또렷하다(회색 `GGO_GRAY8`은 세로 획이 흐려졌다 — 09-16 덤프 `닫`의 ㅏ). 굵게는 진짜 볼드 face(FW_BOLD).
//! 다른 OS·GDI가 이름을 못 찾는 face·비BMP 문자는 `ab_glyph` 경로로 돌아간다.
//!
//! 검증 = nexa-font `gdi_cleartype_stems_bold_and_advances`(줄기 가시성·볼드 잉크·정수 폭) · nexa-sql
//! `scripts/win-capture.ps1`(실기 캡처 ×3 크롭).
//!
//! HDC·HFONT·DIB는 스레드 소속이라 **스레드 로컬** 캐시((패밀리, em px, 굵게) → face)로 둔다. 패밀리는 `name`
//! 테이블의 이름 후보 전부(영문·현지어 · `names.rs`)로 시도한다 — 로케일마다 GDI가 찾는 이름이 다르다.

use std::cell::RefCell;
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

#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_ppm: i32,
    y_ppm: i32,
    clr_used: u32,
    clr_important: u32,
}

#[repr(C)]
struct TextMetricW {
    height: i32,
    ascent: i32,
    descent: i32,
    internal_leading: i32,
    external_leading: i32,
    ave_char_width: i32,
    max_char_width: i32,
    weight: i32,
    overhang: i32,
    digitized_aspect_x: i32,
    digitized_aspect_y: i32,
    first_char: u16,
    last_char: u16,
    default_char: u16,
    break_char: u16,
    italic: u8,
    underlined: u8,
    struck_out: u8,
    pitch_and_family: u8,
    char_set: u8,
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
    fn GetTextMetricsW(hdc: H, tm: *mut TextMetricW) -> i32;
    fn CreateDIBSection(
        hdc: H,
        bmi: *const BitmapInfoHeader,
        usage: u32,
        bits: *mut *mut c_void,
        section: H,
        offset: u32,
    ) -> H;
    fn SetTextColor(hdc: H, color: u32) -> u32;
    fn SetBkColor(hdc: H, color: u32) -> u32;
    fn SetBkMode(hdc: H, mode: i32) -> i32;
    fn SetTextAlign(hdc: H, mode: u32) -> u32;
    fn ExtTextOutW(
        hdc: H,
        x: i32,
        y: i32,
        options: u32,
        rect: *const c_void,
        text: *const u16,
        n: u32,
        dx: *const i32,
    ) -> i32;
    fn GdiFlush() -> i32;
}

const GGO_METRICS: u32 = 0;
const GDI_ERROR: u32 = 0xFFFF_FFFF;
const FW_NORMAL: i32 = 400;
const FW_BOLD: i32 = 700;
const DEFAULT_CHARSET: u8 = 1;
const CLEARTYPE_QUALITY: u8 = 5;
const TRANSPARENT: i32 = 1;
const TA_BASELINE: u32 = 24;
/// DIB 여백(음의 베어링·ClearType 가로 번짐 1px·오버슈트).
const PAD: i32 = 6;

/// GDI face 하나(메모리 DC + 선택된 HFONT + 32bpp 상향 DIB).
struct Face {
    hdc: H,
    hfont: H,
    hbm: H,
    bits: *mut u32,
    dib_w: i32,
    dib_h: i32,
    /// 베이스라인 위 높이(tmAscent) — DIB 안 펜 y.
    ascent: i32,
}

impl Drop for Face {
    fn drop(&mut self) {
        // SAFETY: 이 스레드가 만든 핸들을 한 번만 해제한다.
        unsafe {
            DeleteDC(self.hdc);
            DeleteObject(self.hfont);
            DeleteObject(self.hbm);
        }
    }
}

/// 래스터된 GDI 글리프 — 원점(펜 정수 x · 베이스라인) 기준. `w == 0`이면 외곽선 없음(공백) · `adv`만 유효.
/// `cov`는 픽셀당 **3바이트(R·G·B 커버리지)**.
pub(crate) struct Glyph {
    pub w: u16,
    pub h: u16,
    pub ox: i16,
    pub oy: i16,
    pub cov: Vec<u8>,
    pub adv: i32,
}

/// (패밀리 키 = 후보 첫 이름, em px, 굵게, face) — 항목 수는 face×크기(수십)라 선형 탐색.
type FaceList = Vec<(String, i32, bool, Option<Face>)>;

thread_local! {
    static FACES: RefCell<FaceList> = const { RefCell::new(Vec::new()) };
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

/// 이름 하나로 face를 만들고 GDI가 실제로 연 이름(`GetTextFaceW`)이 후보 목록 안이면 성공 —
/// 대체 글꼴(이름 못 찾음 · 영문 로케일에서 현지어 이름 요청)이면 `None`.
fn try_name(name: &str, names: &[String], em_px: i32, bold: bool) -> Option<Face> {
    let mut buf = [0u16; 32];
    for (i, u) in name.encode_utf16().take(31).enumerate() {
        buf[i] = u;
    }
    let lf = LogFontW {
        height: -em_px.max(1),
        width: 0,
        escapement: 0,
        orientation: 0,
        weight: if bold { FW_BOLD } else { FW_NORMAL },
        italic: 0,
        underline: 0,
        strike_out: 0,
        char_set: DEFAULT_CHARSET,
        out_precision: 0,
        clip_precision: 0,
        quality: CLEARTYPE_QUALITY,
        pitch_and_family: 0,
        face_name: buf,
    };
    // SAFETY: 유효한 구조체·핸들만 넘기고 실패 시 만든 것을 되돌린다.
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
        let g = norm(&got);
        let mut tm = std::mem::zeroed::<TextMetricW>();
        if !names.iter().any(|c| norm(c) == g) || GetTextMetricsW(hdc, &mut tm) == 0 {
            DeleteDC(hdc);
            DeleteObject(hfont);
            return None;
        }
        // DIB: 글리프 하나가 넉넉히 들어갈 크기(가로 em×3 · 세로 em×2 + 여백).
        let dib_w = em_px * 3 + PAD * 2;
        let dib_h = tm.height.max(em_px * 2) + PAD * 2;
        let bmi = BitmapInfoHeader {
            size: std::mem::size_of::<BitmapInfoHeader>() as u32,
            width: dib_w,
            height: -dib_h,
            planes: 1,
            bit_count: 32,
            compression: 0,
            size_image: 0,
            x_ppm: 0,
            y_ppm: 0,
            clr_used: 0,
            clr_important: 0,
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let hbm = CreateDIBSection(hdc, &bmi, 0, &mut bits, std::ptr::null_mut(), 0);
        if hbm.is_null() || bits.is_null() {
            DeleteDC(hdc);
            DeleteObject(hfont);
            return None;
        }
        SelectObject(hdc, hbm);
        SetTextColor(hdc, 0x00FF_FFFF);
        SetBkColor(hdc, 0);
        SetBkMode(hdc, TRANSPARENT);
        SetTextAlign(hdc, TA_BASELINE);
        Some(Face {
            hdc,
            hfont,
            hbm,
            bits: bits.cast::<u32>(),
            dib_w,
            dib_h,
            ascent: tm.ascent,
        })
    }
}

/// 후보 이름을 차례로 시도(영문·현지어 · 09-16 CI: 영문 Windows는 "맑은 고딕"을 못 찾고 "Malgun Gothic"은 찾는다).
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

/// 후보 이름 중 하나로 GDI가 같은 글꼴을 여는가(못 열면 이 face는 ab_glyph 경로).
pub(crate) fn face_ok(names: &[String], em_px: i32) -> bool {
    with_face(names, em_px, false, |_| ()).is_some()
}

/// ClearType 글리프(채널별 커버리지) + 정수 전진 폭. 비BMP 문자·실패는 `None`.
pub(crate) fn glyph(names: &[String], em_px: i32, bold: bool, ch: char) -> Option<Glyph> {
    let code = u32::from(ch);
    if code > 0xFFFF {
        return None;
    }
    with_face(names, em_px, bold, |face| render(face, code)).flatten()
}

fn render(face: &Face, code: u32) -> Option<Glyph> {
    let one = Fixed { fract: 0, value: 1 };
    let zero = Fixed { fract: 0, value: 0 };
    let mat = Mat2 {
        m11: one,
        m12: zero,
        m21: zero,
        m22: one,
    };
    let mut gm = GlyphMetrics::default();
    // SAFETY: 유효한 DC · 출력 구조체 · 측정만(버퍼 0).
    let r = unsafe {
        GetGlyphOutlineW(
            face.hdc,
            code,
            GGO_METRICS,
            &mut gm,
            0,
            std::ptr::null_mut(),
            &mat,
        )
    };
    if r == GDI_ERROR {
        return None;
    }
    let adv = i32::from(gm.cell_inc_x);
    let (bbx, bby) = (gm.black_box_x as i32, gm.black_box_y as i32);
    if bbx == 0 || bby == 0 {
        return Some(Glyph {
            w: 0,
            h: 0,
            ox: 0,
            oy: 0,
            cov: Vec::new(),
            adv,
        });
    }
    // 펜 위치: 검은 상자가 여백 안에 들어오게(음의 베어링·어센트 초과도 수용).
    let pen_x = PAD - gm.origin.x.min(0);
    let pen_y = (face.ascent.max(gm.origin.y) + PAD).min(face.dib_h - 1);
    let x0 = (pen_x + gm.origin.x - 2).max(0);
    let x1 = (pen_x + gm.origin.x + bbx + 2).min(face.dib_w);
    let y0 = (pen_y - gm.origin.y - 1).max(0);
    let y1 = (pen_y - gm.origin.y + bby + 1).min(face.dib_h);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let n = (face.dib_w * face.dib_h) as usize;
    let wch = [code as u16];
    // SAFETY: DIB 비트는 이 face가 소유(dib_w×dib_h u32) · GDI 그리기 뒤 GdiFlush로 동기화.
    let pixels: &[u32] = unsafe {
        std::ptr::write_bytes(face.bits, 0, n);
        if ExtTextOutW(
            face.hdc,
            pen_x,
            pen_y,
            0,
            std::ptr::null(),
            wch.as_ptr(),
            1,
            std::ptr::null(),
        ) == 0
        {
            return None;
        }
        GdiFlush();
        std::slice::from_raw_parts(face.bits, n)
    };
    let stride = face.dib_w as usize;
    let px = |x: i32, y: i32| pixels[y as usize * stride + x as usize] & 0x00FF_FFFF;
    // 실제 칠해진 범위로 조인다.
    let (mut minx, mut maxx, mut miny, mut maxy) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    for y in y0..y1 {
        for x in x0..x1 {
            if px(x, y) != 0 {
                minx = minx.min(x);
                maxx = maxx.max(x);
                miny = miny.min(y);
                maxy = maxy.max(y);
            }
        }
    }
    if minx > maxx {
        return Some(Glyph {
            w: 0,
            h: 0,
            ox: 0,
            oy: 0,
            cov: Vec::new(),
            adv,
        });
    }
    let (w, h) = ((maxx - minx + 1) as usize, (maxy - miny + 1) as usize);
    let mut cov = Vec::with_capacity(w * h * 3);
    for y in miny..=maxy {
        for x in minx..=maxx {
            let p = px(x, y);
            cov.push(((p >> 16) & 0xFF) as u8);
            cov.push(((p >> 8) & 0xFF) as u8);
            cov.push((p & 0xFF) as u8);
        }
    }
    Some(Glyph {
        w: w as u16,
        h: h as u16,
        ox: (minx - pen_x) as i16,
        oy: (miny - pen_y) as i16,
        cov,
        adv,
    })
}

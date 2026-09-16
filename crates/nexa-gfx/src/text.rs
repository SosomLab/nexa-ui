//! 텍스트 스택 최소 경로 — **ab_glyph**(SP-1c 실측 후 사용자 확정 08-08).
//!
//! 폰트 파싱(ttf-parser 계열)·글리프 래스터만 쓴다. **셰이핑 엔진은 v1에 없다** — 한글은
//! 완성형 음절이 cmap에 직접 있어 글리프 치환이 필요 없고(아랍어·인도계와 다른 점), v1 요구는
//! 한/영(FR-U-3)이다. 복잡 문자는 v2에서 이 모듈 뒤(DR-21 이음새)에 셰이핑을 추가한다.
//!
//! **폰트 바이트는 밖에서 온다** — 이 크레이트는 파일을 읽지 않는다(플랫폼 중립).
//! 시스템 폰트 경로 발견은 `<app>-plat` 소관(ADR-0001 — 폰트 열거는 플랫폼 계층).

/// ★ 탭 폭(칸 · 기본 4) — 프로세스 전역(nexa-sql `editor.tab_size` · 09-15). 측정·그리기·캐럿이 같은 값을 쓴다.
static TAB_COLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(4);

/// 탭 폭 설정(1~16 · 그 밖은 4).
pub fn set_tab_cols(n: u32) {
    let n = if (1..=16).contains(&n) { n } else { 4 };
    TAB_COLS.store(n, std::sync::atomic::Ordering::Relaxed);
}

/// 현재 탭 폭(칸).
#[must_use]
pub fn tab_cols() -> u32 {
    TAB_COLS.load(std::sync::atomic::Ordering::Relaxed)
}

/// ★ 탭 = **정지점**(줄 시작부터 탭 폭의 배수 열까지 · Golden/Sublime/VS Code 관례 · 기본) 또는 **절대 폭**(항상 탭 폭만큼 ·
/// 종전 동작). nexa-sql `editor.tab_stops`(사용자 09-16 "앞 글자 수를 고려해 1~4칸").
static TAB_STOPS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// 탭 정지점 방식 설정(`false` = 절대 폭).
pub fn set_tab_stops(on: bool) {
    TAB_STOPS.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// 탭이 정지점 방식인가.
#[must_use]
pub fn tab_stops() -> bool {
    TAB_STOPS.load(std::sync::atomic::Ordering::Relaxed)
}

use crate::surface::{Color, Surface};
use ab_glyph::{Font as _, FontRef, ScaleFont as _};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// ★ 글리프 비트맵 캐시 키(09-15 사용자 "글리프 캐시가 없다") — (폴백 face · 글리프 id · 크기 비트 · 가로 서브픽셀 1/3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GlyphKey {
    face: u8,
    gid: u16,
    size: u32,
    sub: u8,
}

/// 래스터된 글리프 — 원점(펜 정수 x · 베이스라인 정수 y) 기준 오프셋 + 8비트 커버리지.
#[derive(Debug)]
pub struct GlyphBitmap {
    /// 폭·높이(px).
    pub w: u16,
    /// 폭·높이(px).
    pub h: u16,
    /// 원점 기준 좌상단 오프셋.
    pub ox: i16,
    /// 원점 기준 좌상단 오프셋.
    pub oy: i16,
    /// `w*h` 커버리지(0~255).
    pub cov: Vec<u8>,
}

/// 캐시 상한(항목 수) — 넘으면 비우고 다시 채운다(글꼴 크기를 여러 번 바꿔도 메모리가 제자리).
const GLYPH_CACHE_MAX: usize = 8192;
/// 가로 서브픽셀 단계(1/3 px — 비례 글꼴의 자간 떨림을 막으면서 캐시 적중을 높인다).
const SUBPX: f32 = 3.0;

/// 로드된 폰트 — **프로세스 수명 자원**(로드 1회 · 앱 종료까지 사용).
///
/// 바이트는 `&'static`이다 — `<app>-plat`의 mmap(파일 백드 페이지 · 힙 0)이 정상 경로이고,
/// [`Font::from_bytes`]는 소유 바이트를 의도적으로 누수해 같은 표현으로 수렴한다(테스트·특수 경로용).
pub struct Font {
    /// ★ 폴백 체인(09-01 사용자 요청 "두부 예방") — [0] = 주 폰트, 이후 = 대체.
    /// 글자마다 글리프가 있는 첫 보을 쓴다(JetBrains Mono + 한글 = 시스템 본이 받는다).
    faces: Vec<FontRef<'static>>,
    /// ★ 글리프 비트맵 캐시 — 외곽선 추출+래스터(글리프당 ≈1.4µs)를 **처음 한 번만**. 복제본끼리 공유(Arc).
    cache: Arc<Mutex<HashMap<GlyphKey, Arc<GlyphBitmap>>>>,
}

impl Clone for Font {
    fn clone(&self) -> Self {
        Self {
            faces: self.faces.clone(),
            cache: Arc::clone(&self.cache),
        }
    }
}

impl core::fmt::Debug for Font {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Font").finish_non_exhaustive()
    }
}

/// 폰트 로드 실패(파싱 불가·인덱스 없음).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontError;

/// 텍스트 스타일(faux 볼드·이탤릭).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct TextStyle {
    /// 굵게(faux — 이중 그리기).
    pub bold: bool,
    /// 기울임(faux — 전단).
    pub italic: bool,
}

impl TextStyle {
    /// 스타일 없음.
    pub const PLAIN: Self = Self {
        bold: false,
        italic: false,
    };
}

impl Font {
    /// `'static` 폰트 바이트에서 로드한다(mmap 정상 경로). `index`는 TTC 컬렉션 인덱스.
    ///
    /// # Errors
    /// 파싱 불가·인덱스 범위 밖이면 [`FontError`].
    pub fn from_static(data: &'static [u8], index: u32) -> Result<Self, FontError> {
        FontRef::try_from_slice_and_index(data, index)
            .map(|f| Self {
                faces: vec![f],
                cache: Arc::new(Mutex::new(HashMap::new())),
            })
            .map_err(|_| FontError)
    }

    /// ★ 폴백 폰트 추가(09-01) — 주 폰트에 없는 글자만 이 보이 받는다.
    /// 기준선(ascent·줄 높이)은 주 폰트가 계속 정한다 — 줌 안 주 글꼴이 섮여도 행이 안 흔들린다.
    ///
    /// # Errors
    /// 파싱 불가·인덱스 범위 밖이면 [`FontError`].
    pub fn push_fallback(&mut self, data: &'static [u8], index: u32) -> Result<(), FontError> {
        let f = FontRef::try_from_slice_and_index(data, index).map_err(|_| FontError)?;
        self.faces.push(f);
        self.clear_glyph_cache();
        Ok(())
    }

    /// 글자가 있는 첫 보 — 없으면 주 폰트(.notdef 표시가 정직하다).
    /// ★ 다른 글꼴의 얼굴 전부를 폴백으로 잇는다(09-04 — 고정폭 글꼴에 주 글꼴 체인을 통째로).
    pub fn push_fallback_font(&mut self, other: &Font) {
        self.faces.extend(other.faces.iter().cloned());
        self.clear_glyph_cache();
    }

    /// 글리프 캐시를 비운다(face 목록이 바뀔 때).
    pub fn clear_glyph_cache(&self) {
        if let Ok(mut c) = self.cache.lock() {
            c.clear();
        }
    }

    /// 캐시된 글리프 수(진단·테스트).
    #[must_use]
    pub fn glyph_cache_len(&self) -> usize {
        self.cache.lock().map(|c| c.len()).unwrap_or(0)
    }

    fn face_for(&self, ch: char) -> &FontRef<'static> {
        self.faces
            .iter()
            .find(|f| f.glyph_id(ch).0 != 0)
            .unwrap_or(&self.faces[0])
    }

    /// 글자가 있는 face의 **인덱스**(캐시 키용).
    fn face_index_for(&self, ch: char) -> usize {
        self.faces
            .iter()
            .position(|f| f.glyph_id(ch).0 != 0)
            .unwrap_or(0)
    }

    /// 글리프 비트맵 — 캐시 적중이면 그대로, 아니면 래스터해 넣는다. 외곽선이 없는 글자(공백)는 `None`.
    fn glyph_bitmap(
        &self,
        face_i: usize,
        ch: char,
        size: f32,
        sub: u8,
    ) -> Option<Arc<GlyphBitmap>> {
        let face = &self.faces[face_i];
        let gid = face.glyph_id(ch);
        let key = GlyphKey {
            face: face_i as u8,
            gid: gid.0,
            size: size.to_bits(),
            sub,
        };
        if let Ok(c) = self.cache.lock() {
            if let Some(bm) = c.get(&key) {
                return Some(Arc::clone(bm));
            }
        }
        let scaled = face.as_scaled(size);
        let glyph = gid.with_scale_and_position(size, ab_glyph::point(f32::from(sub) / SUBPX, 0.0));
        let outlined = scaled.outline_glyph(glyph)?;
        let b = outlined.px_bounds();
        let (w, h) = (
            (b.max.x - b.min.x).ceil().max(0.0) as usize + 1,
            (b.max.y - b.min.y).ceil().max(0.0) as usize + 1,
        );
        let mut cov = vec![0u8; w * h];
        outlined.draw(|gx, gy, c| {
            let (gx, gy) = (gx as usize, gy as usize);
            if gx < w && gy < h {
                cov[gy * w + gx] = (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        });
        let bm = Arc::new(GlyphBitmap {
            w: w as u16,
            h: h as u16,
            ox: b.min.x.floor() as i16,
            oy: b.min.y.floor() as i16,
            cov,
        });
        if let Ok(mut c) = self.cache.lock() {
            if c.len() >= GLYPH_CACHE_MAX {
                c.clear();
            }
            c.insert(key, Arc::clone(&bm));
        }
        Some(bm)
    }

    /// 소유 바이트에서 로드 — **의도적 누수**로 `'static`화(폰트는 프로세스 수명 자원).
    ///
    /// # Errors
    /// 파싱 불가·인덱스 범위 밖이면 [`FontError`].
    pub fn from_bytes(data: Vec<u8>, index: u32) -> Result<Self, FontError> {
        Self::from_static(Box::leak(data.into_boxed_slice()), index)
    }

    /// 이 폰트가 문자의 글리프를 갖고 있는가(폴백 체인 판단 근거).
    #[must_use]
    pub fn covers(&self, ch: char) -> bool {
        self.faces.iter().any(|f| f.glyph_id(ch).0 != 0)
    }

    /// `size`(px)에서의 줄 높이.
    #[must_use]
    pub fn line_height(&self, size: f32) -> f32 {
        let s = self.faces[0].as_scaled(size);
        s.ascent() - s.descent() + s.line_gap()
    }

    /// 텍스트 폭(px) — 그리지 않고 잰다(라벨 실측 정렬 — [docs/12 §B]). 탭은 이 문자열의 시작을 줄 시작(원점)으로 본다.
    #[must_use]
    pub fn measure(&self, text: &str, size: f32) -> f32 {
        self.measure_from(text, size, 0.0)
    }

    /// 텍스트 폭(px) — 텍스트가 **줄 시작(탭 원점)에서 `start_rel`px 떨어진 곳**에서 시작할 때. 탭 정지점은 원점 기준이라
    /// 같은 문자열도 시작 위치에 따라 폭이 다르다. 반환값은 `text`만의 폭(시작 위치 제외) · 누적 순서는 [`Self::measure`]와
    /// 같다(호출자가 런을 이어 붙여도 접두사 폭과 비트 동일 — nexa-ctl `text_prefix_widths` 계약).
    #[must_use]
    pub fn measure_from(&self, text: &str, size: f32, start_rel: f32) -> f32 {
        let mut local = 0f32;
        for c in text.chars() {
            local += if c == '\t' {
                let rel = (start_rel + local).ceil();
                rel + self.tab_advance(size, rel) - (start_rel + local)
            } else {
                Self::control_advance(c).unwrap_or_else(|| {
                    let face = self.face_for(c);
                    face.as_scaled(size).h_advance(face.glyph_id(c))
                })
            };
        }
        local
    }

    /// 탭 한 개의 전진 폭 — `rel` = 줄 시작(탭 원점)부터 펜까지의 거리(px · 정수로 올림한 값). 정지점 방식이면 다음 정지점까지
    /// (탭 폭 = 공백 폭 × `tab_cols` · 정지점 위면 한 칸 전체) · 절대 방식이면 늘 탭 폭.
    #[must_use]
    pub fn tab_advance(&self, size: f32, rel: f32) -> f32 {
        let face = self.face_for(' ');
        let tabw = face.as_scaled(size).h_advance(face.glyph_id(' ')) * tab_cols() as f32;
        if !tab_stops() || tabw <= 0.0 {
            return tabw;
        }
        let n = (rel / tabw + 1e-4).floor();
        ((n + 1.0) * tabw - rel).max(0.0)
    }

    /// ★ 제어 문자 표시 규칙(09-03 실기 — 탭이 두부(□)로 그려졌다): 탭은 [`Self::tab_advance`](글리프는 그리지 않음) ·
    /// 그 외 제어(CR 등) = 폭 0. 측정과 그리기가 같은 규칙을 쓰므로 캐럿 좌표도 일관된다.
    fn control_advance(c: char) -> Option<f32> {
        c.is_control().then_some(0.0)
    }

    /// `size`에서의 어센트(베이스라인 위 높이, px) — 상단 기준 배치를 베이스라인으로 변환.
    #[must_use]
    pub fn ascent(&self, size: f32) -> f32 {
        self.faces[0].as_scaled(size).ascent()
    }

    /// 이 폰트에 `c`의 글리프가 있는가(.notdef = 없음) — 슬롯 폴백 판단용(08-10).
    #[must_use]
    pub fn has_glyph(&self, c: char) -> bool {
        self.covers(c)
    }

    /// `size`에서 숫자 '0'의 **실측 외곽 높이**(px) — 광학 크기 보정용(08-10).
    /// 같은 px라도 폰트마다 숫자가 차지하는 높이가 달라(Consolas ≫ 맑은 고딕)
    /// 나란히 그리면 커 보인다. 외곽선이 없으면 경험 근사(0.7em).
    #[must_use]
    pub fn digit_height(&self, size: f32) -> f32 {
        let g = self.faces[0]
            .glyph_id('0')
            .with_scale_and_position(size, ab_glyph::point(0.0, 0.0));
        self.faces[0]
            .outline_glyph(g)
            .map_or(size * 0.7, |og| og.px_bounds().height())
    }

    /// `size`에서의 텍스트 상자 높이(어센트+디센트, px) — 세로 중앙 정렬 실측용.
    /// `line_height`와 달리 줄 간격(line gap)을 빼서 한 줄 배치에 쓴다.
    #[must_use]
    pub fn text_box_height(&self, size: f32) -> f32 {
        let s = self.faces[0].as_scaled(size);
        s.ascent() - s.descent()
    }

    /// [`Font::draw_text`]의 클립 변형 — `clip = (x0, y0, x1, y1)` 밖 픽셀은 찍지 않는다
    /// (행 배경 안에서만 그리는 `text_opaque` 모델의 기초).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_text_clipped(
        &self,
        surface: &mut Surface<'_>,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        text: &str,
        clip: (i32, i32, i32, i32),
    ) -> f32 {
        self.draw_styled(surface, x, y, size, color, text, clip, TextStyle::PLAIN, x)
    }

    /// [`Font::draw_text_clipped`]의 **스타일 변형** — 실제 볼드/이탤릭 폰트 파일 없이
    /// **faux 볼드**(x축 2회 그리기)·**faux 이탤릭**(베이스라인 위 거리 비례 전단)로 근사한다.
    /// 진짜 글꼴 패밀리·굵기 face는 폰트 열거(M3-3 확장)에서. 지금은 시스템 폰트 1벌 위 근사.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_styled(
        &self,
        surface: &mut Surface<'_>,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        text: &str,
        clip: (i32, i32, i32, i32),
        style: TextStyle,
        tab_origin: f32,
    ) -> f32 {
        let slant = if style.italic { 0.22 } else { 0.0 };
        let bold_pass = if style.bold { 2 } else { 1 };
        let mut pen = x;
        // 베이스라인은 정수로 스냅(캐시 비트맵은 y 서브픽셀을 갖지 않는다 — 한 줄 안에서 일관).
        let base_y = y.round() as i32;
        for ch in text.chars() {
            // ★ 탭 = 원점(`tab_origin` · 줄 시작 x) 기준 다음 정지점까지 폭만 옮긴다(글리프 없음 · 측정과 같은 올림 규칙).
            if ch == '\t' {
                let rel = (pen - tab_origin).ceil();
                pen = tab_origin + rel + self.tab_advance(size, rel);
                continue;
            }
            // ★ 제어 문자 = 폭만 옮기고 글리프 없음(탭 두부 차단 · 09-03).
            if let Some(adv) = Self::control_advance(ch) {
                pen += adv;
                continue;
            }
            let face_i = self.face_index_for(ch);
            let face = &self.faces[face_i];
            let scaled = face.as_scaled(size);
            let gid = face.glyph_id(ch);
            let pen_i = pen.floor();
            let sub = ((pen - pen_i) * SUBPX).floor().clamp(0.0, SUBPX - 1.0) as u8;
            if let Some(bm) = self.glyph_bitmap(face_i, ch, size, sub) {
                let gx0 = pen_i as i32 + i32::from(bm.ox);
                if gx0 >= clip.2 {
                    break;
                }
                let gy0 = base_y + i32::from(bm.oy);
                for dx in 0..bold_pass {
                    surface.blend_mask(gx0 + dx, gy0, &bm, color, clip, slant, base_y);
                }
            }
            pen += scaled.h_advance(gid);
        }
        pen - x
    }

    /// `(x, y)`를 **베이스라인 왼쪽 끝**으로 텍스트를 그린다. 그린 폭(px)을 돌려준다.
    ///
    /// 커버리지를 배경과 블렌드(안티에일리어싱). 표면 밖은 [`Surface`]가 클립한다.
    pub fn draw_text(
        &self,
        surface: &mut Surface<'_>,
        x: f32,
        y: f32,
        size: f32,
        color: Color,
        text: &str,
    ) -> f32 {
        let clip = (0, 0, surface.width() as i32, surface.height() as i32);
        self.draw_styled(surface, x, y, size, color, text, clip, TextStyle::PLAIN, x)
    }
}

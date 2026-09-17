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

/// ★ 텍스트 대비(커버리지 감마 · 기본 1.0 = 선형) — 프로세스 전역(nexa-sql `ui.text_contrast` · 09-16). GDI ClearType의
///   진한 획에 가깝게 중간 알파를 올린다: `cov' = cov^(1/γ)` · γ 1.4 ≈ Windows 텍스트 감마.
static TEXT_GAMMA: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3F80_0000);
/// ★ 글리프 원점 정수 스냅(기본 끔) — 켜면 가로 서브픽셀(1/3px) 배치를 끄고 펜 x를 반올림해 세로획이 한 픽셀에 앉는다
///   (힌팅 없는 래스터에서 흐림의 주원인 · 작은 UI 글꼴에 효과 · nexa-sql `ui.text_snap`).
static TEXT_SNAP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 텍스트 대비 감마(0.5~3.0 · 그 밖은 1.0).
pub fn set_text_contrast(gamma: f32) {
    let g = if (0.5..=3.0).contains(&gamma) {
        gamma
    } else {
        1.0
    };
    TEXT_GAMMA.store(g.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// 현재 텍스트 대비 감마.
#[must_use]
pub fn text_contrast() -> f32 {
    f32::from_bits(TEXT_GAMMA.load(std::sync::atomic::Ordering::Relaxed))
}

/// ★ 오토힌트 근사(기본 끔 · nexa-sql `ui.text_hint` · T-99 09-16) — GDI/FreeType light처럼 **가로 획은 x-높이를 정수 px에
///   맞추는 크기 보정**으로, **세로 줄기는 이웃 행과 이어진 좁은 열을 한 픽셀 열로 모으는 후처리**로 격자에 앉힌다.
///   대각선·곡선은 손대지 않는다(AA 유지). 정수 스냅·대비 감마와 조합.
static TEXT_HINT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// ★ OS 래스터라이저(Windows GDI) 글리프 사용 여부(기본 끔 · 다른 OS에서는 늘 끔).
static TEXT_GDI: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// OS(GDI) ClearType 글리프를 쓴다 — Windows에서만 효과. 켜면 그 face의 글리프(채널별 커버리지)·전진 폭·진짜
/// 볼드를 GDI에서 얻는다(Windows·Golden과 같은 래스터 · 감마/오토힌트/굵기 보강은 그 face에 적용하지 않는다).
/// face 패밀리 이름은 글꼴 `name` 테이블 + [`Font::set_face_family`](없으면 ab_glyph 경로).
pub fn set_text_gdi(on: bool) {
    TEXT_GDI.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// 현재 GDI 글리프 사용 여부(Windows 밖에서는 늘 `false`).
#[must_use]
pub fn text_gdi() -> bool {
    (cfg!(windows) || cfg!(target_os = "macos"))
        && TEXT_GDI.load(std::sync::atomic::Ordering::Relaxed)
}

/// ★ 획 두께 보강(0.0~0.6 · 기본 0.0 · nexa-sql `ui.text_weight` 25%) — FreeType stem darkening 근사: 커버리지를 오른쪽으로
///   `w`px만큼 번지게 해(`cov[x] += cov[x-1]·w`) 세로 줄기를 1px에서 1+w px로. GDI ClearType의 진한 획(≈1.2px)에 가깝게.
static TEXT_WEIGHT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// 획 두께 보강(0.0~0.6).
pub fn set_text_weight(w: f32) {
    let w = if (0.0..=0.6).contains(&w) { w } else { 0.0 };
    TEXT_WEIGHT.store(w.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// 현재 획 두께 보강.
#[must_use]
pub fn text_weight() -> f32 {
    f32::from_bits(TEXT_WEIGHT.load(std::sync::atomic::Ordering::Relaxed))
}

/// 커버리지를 오른쪽으로 `w`(0~1)만큼 번지게 한다 — 행마다 `cov[x] = min(1, cov[x] + cov[x-1]·w)`.
pub(crate) fn embolden(cov: &mut [u8], w: usize, h: usize, weight: f32) {
    if weight <= 0.0 || w < 2 {
        return;
    }
    for r in 0..h {
        let row = &mut cov[r * w..(r + 1) * w];
        let mut prev = 0.0f32;
        for c in row.iter_mut() {
            let cur = f32::from(*c) / 255.0;
            let v = (cur + prev * weight).min(1.0);
            *c = (v * 255.0 + 0.5) as u8;
            prev = cur;
        }
    }
}

/// 오토힌트 근사 켬/끔.
pub fn set_text_hint(on: bool) {
    TEXT_HINT.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// 오토힌트 상태.
#[must_use]
pub fn text_hint() -> bool {
    TEXT_HINT.load(std::sync::atomic::Ordering::Relaxed)
}

/// 세로 줄기 정렬(힌트 후처리): 행마다 커버리지 런을 보고, **위아래 행과 같은 자리에 이어지는**(= 세로 줄기) 런이
/// 정수 폭보다 한 열 넓게 번져 있으면(예 `[0.5, 0.5]`) 무게중심 쪽 한 열로 모은다(`[1.0, 0]`). 대각선은 행마다
/// 중심이 옮겨가므로(0.3px 초과) 건드리지 않는다.
pub(crate) fn snap_stems(cov: &mut [u8], w: usize, h: usize) {
    if w == 0 || h == 0 {
        return;
    }
    // 행별 런 (시작, 끝, 합, 무게중심)
    let mut runs: Vec<Vec<(usize, usize, f32, f32)>> = Vec::with_capacity(h);
    for r in 0..h {
        let row = &cov[r * w..(r + 1) * w];
        let mut v = Vec::new();
        let mut c = 0;
        while c < w {
            if row[c] == 0 {
                c += 1;
                continue;
            }
            let s0 = c;
            let mut total = 0.0f32;
            let mut moment = 0.0f32;
            while c < w && row[c] != 0 {
                let a = f32::from(row[c]) / 255.0;
                total += a;
                moment += (c as f32 + 0.5) * a;
                c += 1;
            }
            v.push((s0, c, total, moment / total.max(1e-6)));
        }
        runs.push(v);
    }
    let mut out = cov.to_vec();
    for r in 0..h {
        for &(s0, e0, total, center) in &runs[r] {
            let len = e0 - s0;
            let wpx = (total.round().max(1.0)) as usize;
            // 정수 폭보다 딱 한 열 넓게 번진 런만(그 이상은 곡선/대각선 · 그 이하는 이미 또렷).
            if len != wpx + 1 {
                continue;
            }
            // 위·아래 행에 중심이 0.6px 안에서 이어지는 런이 있어야 세로 줄기.
            let near = |rr: usize| {
                runs[rr]
                    .iter()
                    // 같은 줄기의 이웃 행은 중심이 거의 같다(0.3px 안) — 대각선의 끝 행(1px 런)은 0.5px 어긋나 제외.
                    .any(|&(a, b, _, c)| a < e0 && b > s0 && (c - center).abs() <= 0.3)
            };
            let up = r > 0 && near(r - 1);
            let down = r + 1 < h && near(r + 1);
            // 줄기의 맨 위/아래 행은 한쪽만 이어진다.
            if !(up || down) {
                continue;
            }
            let start = ((center - wpx as f32 / 2.0).round().max(s0 as f32)) as usize;
            let start = start.min(e0 - wpx);
            for c in s0..e0 {
                out[r * w + c] = 0;
            }
            let level = ((total / wpx as f32) * 255.0).round().min(255.0) as u8;
            for k in 0..wpx {
                out[r * w + start + k] = level;
            }
        }
    }
    cov.copy_from_slice(&out);
}

/// 글리프 정수 스냅 켬/끔.
pub fn set_text_snap(on: bool) {
    TEXT_SNAP.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// 글리프 정수 스냅 상태.
#[must_use]
pub fn text_snap() -> bool {
    TEXT_SNAP.load(std::sync::atomic::Ordering::Relaxed)
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
    /// 대비 감마 비트(바뀌면 다른 비트맵 · 캐시를 비울 필요 없음).
    gamma: u32,
    /// 오토힌트 켬 여부(비트맵이 다르다).
    hint: bool,
    /// 획 두께 보강 비트.
    weight: u32,
    /// GDI 글리프 여부.
    gdi: bool,
    /// 진짜 볼드 face(GDI) 여부 — ab_glyph 경로는 늘 false(faux 볼드는 그리기 때).
    bold: bool,
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
    /// `w*h` 커버리지(0~255) · [`Self::rgb`]면 픽셀당 3바이트(R·G·B 서브픽셀 커버리지 · `w*h*3`).
    pub cov: Vec<u8>,
    /// 채널별(ClearType) 커버리지 여부.
    pub rgb: bool,
}

/// 캐시 상한 기본값(항목 수) — 넘으면 비우고 다시 채운다(글꼴 크기를 여러 번 바꿔도 메모리가 제자리).
pub const GLYPH_CACHE_MAX: usize = 8192;
/// 현재 상한(전역 · 호스트가 설정 `ui.glyph_cache`로 [`set_glyph_cache_max`] · nexa-sql docs/39 §3-6 T-90d).
static GLYPH_CACHE_LIMIT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(GLYPH_CACHE_MAX);

/// 글리프 비트맵 캐시 상한을 바꾼다(0은 1로). 이미 넘친 캐시는 **다음 삽입** 때 비워진다(폰트 인스턴스마다 캐시가 있어 즉시 순회하지 않는다).
pub fn set_glyph_cache_max(n: usize) {
    GLYPH_CACHE_LIMIT.store(n.max(1), std::sync::atomic::Ordering::Relaxed);
}

/// 현재 글리프 캐시 상한.
#[must_use]
pub fn glyph_cache_max() -> usize {
    GLYPH_CACHE_LIMIT.load(std::sync::atomic::Ordering::Relaxed)
}
/// 가로 서브픽셀 단계(1/3 px — 비례 글꼴의 자간 떨림을 막으면서 캐시 적중을 높인다).
const SUBPX: f32 = 3.0;

/// GDI 전진 폭 캐시 — (face, em px, 문자) → px.
type GdiAdvCache = HashMap<(u8, i32, char, bool), f32>;
/// (face, size 비트) → GDI face(패밀리 이름 후보들, em px) 또는 없음.
type GdiFaceCache = HashMap<(u8, u32), Option<(Arc<[String]>, i32)>>;

/// 로드된 폰트 — **프로세스 수명 자원**(로드 1회 · 앱 종료까지 사용).
///
/// 바이트는 `&'static`이다 — `<app>-plat`의 mmap(파일 백드 페이지 · 힙 0)이 정상 경로이고,
/// [`Font::from_bytes`]는 소유 바이트를 의도적으로 누수해 같은 표현으로 수렴한다(테스트·특수 경로용).
pub struct Font {
    /// ★ 폴백 체인(09-01 사용자 요청 "두부 예방") — [0] = 주 폰트, 이후 = 대체.
    /// 글자마다 글리프가 있는 첫 보을 쓴다(JetBrains Mono + 한글 = 시스템 본이 받는다).
    faces: Vec<FontRef<'static>>,
    /// face별 원본 바이트·컬렉션 인덱스(`name` 테이블 읽기용 · OS 래스터라이저 이름 후보).
    raw: Vec<(&'static [u8], u32)>,
    /// ★ 글리프 비트맵 캐시 — 외곽선 추출+래스터(글리프당 ≈1.4µs)를 **처음 한 번만**. 복제본끼리 공유(Arc).
    cache: Arc<Mutex<HashMap<GlyphKey, Arc<GlyphBitmap>>>>,
    /// 힌트용 크기 보정 캐시 — (face, size 비트) → 래스터 크기(x-높이가 정수 px가 되게).
    hint_size: Arc<Mutex<HashMap<(u8, u32), f32>>>,
    /// face별 패밀리 이름(OS 래스터라이저용 · [`Font::set_face_family`]) — 없으면 그 face는 ab_glyph 경로.
    families: Arc<Mutex<Vec<Option<String>>>>,
    /// GDI 전진 폭 캐시 — (face, em px, 문자) → px.
    gdi_adv: Arc<Mutex<GdiAdvCache>>,
    /// (face, size 비트) → GDI face(패밀리, em px) 또는 없음.
    gdi_ok: Arc<Mutex<GdiFaceCache>>,
}

impl Clone for Font {
    fn clone(&self) -> Self {
        Self {
            faces: self.faces.clone(),
            raw: self.raw.clone(),
            cache: Arc::clone(&self.cache),
            hint_size: Arc::clone(&self.hint_size),
            families: Arc::clone(&self.families),
            gdi_adv: Arc::clone(&self.gdi_adv),
            gdi_ok: Arc::clone(&self.gdi_ok),
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
                raw: vec![(data, index)],
                cache: Arc::new(Mutex::new(HashMap::new())),
                hint_size: Arc::new(Mutex::new(HashMap::new())),
                families: Arc::new(Mutex::new(vec![None])),
                gdi_adv: Arc::new(Mutex::new(HashMap::new())),
                gdi_ok: Arc::new(Mutex::new(HashMap::new())),
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
        self.raw.push((data, index));
        if let Ok(mut fam) = self.families.lock() {
            fam.push(None);
        }
        self.clear_glyph_cache();
        Ok(())
    }

    /// face `i`의 패밀리 이름(OS 래스터라이저가 같은 글꼴을 열 수 있게 · `set_text_gdi`). 범위 밖이면 무시.
    pub fn set_face_family(&mut self, i: usize, family: &str) {
        if let Ok(mut fam) = self.families.lock() {
            if i < fam.len() {
                fam[i] = Some(family.to_string());
            }
        }
        if let Ok(mut c) = self.gdi_ok.lock() {
            c.clear();
        }
        self.clear_glyph_cache();
    }

    /// GDI 경로가 이 face에 적용되는가 → (패밀리, em px). (face, size) 단위로 캐시 — 글자마다 문자열을 만들지 않는다.
    fn gdi_face(&self, face_i: usize, size: f32) -> Option<(Arc<[String]>, i32)> {
        if !text_gdi() {
            return None;
        }
        let key = (face_i as u8, size.to_bits());
        if let Ok(c) = self.gdi_ok.lock() {
            if let Some(v) = c.get(&key) {
                return v.clone();
            }
        }
        let v = self.gdi_face_uncached(face_i, size);
        if let Ok(mut c) = self.gdi_ok.lock() {
            c.insert(key, v.clone());
        }
        v
    }

    /// `size`(ab_glyph 높이 스케일 = ascent−descent px)를 em 픽셀로 바꿔 OS face(Windows GDI · macOS CoreText)를 연다.
    #[cfg(any(windows, target_os = "macos"))]
    fn gdi_face_uncached(&self, face_i: usize, size: f32) -> Option<(Arc<[String]>, i32)> {
        let names = self.face_family_names(face_i);
        if names.is_empty() {
            return None;
        }
        let face = &self.faces[face_i];
        let upm = face.units_per_em()?;
        let h = face.height_unscaled();
        let em = if h > 0.0 { size * upm / h } else { size };
        let em_px = em.round().max(1.0) as i32;
        #[cfg(windows)]
        let ok = crate::gdi::face_ok(&names, em_px);
        #[cfg(target_os = "macos")]
        let ok = crate::coretext::face_ok(&names, em_px);
        ok.then(|| (Arc::from(names), em_px))
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[allow(clippy::unused_self)]
    fn gdi_face_uncached(&self, _face_i: usize, _size: f32) -> Option<(Arc<[String]>, i32)> {
        None
    }

    /// face `i`의 패밀리 이름 후보 — [`Font::set_face_family`]로 준 이름을 앞에, 글꼴 `name` 테이블의 패밀리
    /// 이름(영문·현지어 전부)을 뒤에(중복 제거). 비어 있으면 OS 래스터라이저 경로를 쓰지 않는다.
    #[must_use]
    pub fn face_family_names(&self, i: usize) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if let Ok(fam) = self.families.lock() {
            if let Some(Some(n)) = fam.get(i) {
                out.push(n.clone());
            }
        }
        if let Some((data, index)) = self.raw.get(i) {
            for n in crate::names::family_names(data, *index) {
                if !out.iter().any(|o| o == &n) {
                    out.push(n);
                }
            }
        }
        out
    }

    /// 문자 하나의 전진 폭(px) — GDI face면 정수 전진(캐시 · 굵게는 볼드 face 폭) · 아니면 ab_glyph(굵게 무시 = faux).
    fn advance_of(&self, face_i: usize, ch: char, size: f32, bold: bool) -> f32 {
        if let Some((names, em_px)) = self.gdi_face(face_i, size) {
            let key = (face_i as u8, em_px, ch, bold);
            if let Ok(c) = self.gdi_adv.lock() {
                if let Some(a) = c.get(&key) {
                    return *a;
                }
            }
            #[cfg(windows)]
            if let Some(g) = crate::gdi::glyph(&names, em_px, bold, ch) {
                if let Ok(mut c) = self.gdi_adv.lock() {
                    c.insert(key, g.adv as f32);
                }
                return g.adv as f32;
            }
            // macOS CoreText: 소수 전진 폭(서브픽셀 위치 · 파인더와 같은 자간).
            #[cfg(target_os = "macos")]
            if let Some(a) = crate::coretext::advance(&names, em_px, bold, ch) {
                if let Ok(mut c) = self.gdi_adv.lock() {
                    c.insert(key, a);
                }
                return a;
            }
            let _ = (names, bold);
        }
        let face = &self.faces[face_i];
        face.as_scaled(size).h_advance(face.glyph_id(ch))
    }

    /// 글자가 있는 첫 보 — 없으면 주 폰트(.notdef 표시가 정직하다).
    /// ★ 다른 글꼴의 얼굴 전부를 폴백으로 잇는다(09-04 — 고정폭 글꼴에 주 글꼴 체인을 통째로).
    pub fn push_fallback_font(&mut self, other: &Font) {
        self.faces.extend(other.faces.iter().cloned());
        self.raw.extend(other.raw.iter().copied());
        let extra: Vec<Option<String>> =
            other.families.lock().map(|f| f.clone()).unwrap_or_default();
        if let Ok(mut fam) = self.families.lock() {
            fam.extend(extra);
            fam.resize(self.faces.len(), None);
        }
        if let Ok(mut c) = self.gdi_ok.lock() {
            c.clear();
        }
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

    /// 글자가 있는 face의 **인덱스**(캐시 키용).
    fn face_index_for(&self, ch: char) -> usize {
        self.faces
            .iter()
            .position(|f| f.glyph_id(ch).0 != 0)
            .unwrap_or(0)
    }

    /// 힌트 크기 보정: 이 face·크기에서 `x`의 높이(x-높이)가 정수 px가 되도록 래스터 크기를 살짝(±수%) 조정한다
    /// (FreeType light의 세로 격자 맞춤 근사). `x`가 없거나 너무 작으면 그대로.
    fn hint_size_for(&self, face_i: usize, size: f32) -> f32 {
        let key = (face_i as u8, size.to_bits());
        if let Ok(c) = self.hint_size.lock() {
            if let Some(v) = c.get(&key) {
                return *v;
            }
        }
        let face = &self.faces[face_i];
        let gid = face.glyph_id('x');
        // 소수 px 크기(10pt = 13.33px)는 먼저 정수로(가로 획이 반 픽셀에 걸치는 흐림의 주원인 · 09-16) — 전진 폭은 원래 크기.
        let base = size.round().max(1.0);
        let adjusted = if gid.0 == 0 {
            base
        } else {
            let scaled = face.as_scaled(base);
            match scaled.outline_glyph(gid.with_scale(base)) {
                Some(o) => {
                    let b = o.px_bounds();
                    let xh = b.max.y - b.min.y;
                    if xh < 4.0 {
                        base
                    } else {
                        let target = xh.round().max(1.0);
                        (base * target / xh).clamp(base * 0.9, base * 1.1)
                    }
                }
                None => base,
            }
        };
        if let Ok(mut c) = self.hint_size.lock() {
            c.insert(key, adjusted);
        }
        adjusted
    }

    /// 글리프 비트맵 — 캐시 적중이면 그대로, 아니면 래스터해 넣는다. 외곽선이 없는 글자(공백)는 `None`.
    fn glyph_bitmap(
        &self,
        face_i: usize,
        ch: char,
        size: f32,
        sub: u8,
        bold: bool,
    ) -> Option<Arc<GlyphBitmap>> {
        let face = &self.faces[face_i];
        let gid = face.glyph_id(ch);
        let gamma_bits = TEXT_GAMMA.load(std::sync::atomic::Ordering::Relaxed);
        let hint = text_hint();
        let weight_bits = TEXT_WEIGHT.load(std::sync::atomic::Ordering::Relaxed);
        let gdi = self.gdi_face(face_i, size);
        let key = GlyphKey {
            face: face_i as u8,
            gid: gid.0,
            size: size.to_bits(),
            sub,
            gamma: gamma_bits,
            hint,
            weight: weight_bits,
            gdi: gdi.is_some(),
            bold: bold && gdi.is_some(),
        };
        if let Ok(c) = self.cache.lock() {
            if let Some(bm) = c.get(&key) {
                return Some(Arc::clone(bm));
            }
        }
        // ★ GDI 경로: ClearType 채널별 커버리지를 그대로(감마·오토힌트·굵기 보강 없음 — GDI가 이미 했다).
        #[cfg(windows)]
        if let Some((names, em_px)) = gdi {
            let g = crate::gdi::glyph(&names, em_px, bold, ch)?;
            if g.w == 0 || g.h == 0 {
                return None;
            }
            let bm = Arc::new(GlyphBitmap {
                w: g.w,
                h: g.h,
                ox: g.ox,
                oy: g.oy,
                cov: g.cov,
                rgb: true,
            });
            if let Ok(mut c) = self.cache.lock() {
                if c.len() >= glyph_cache_max() {
                    c.clear();
                }
                c.insert(key, Arc::clone(&bm));
            }
            return Some(bm);
        }
        // ★ CoreText 경로(macOS · T-100): 회색 커버리지 + 서브픽셀 위치 그대로(스무딩·감마는 CoreText가 했다).
        #[cfg(target_os = "macos")]
        if let Some((names, em_px)) = gdi {
            let g = crate::coretext::glyph(&names, em_px, bold, ch, sub)?;
            if g.w == 0 || g.h == 0 {
                return None;
            }
            let bm = Arc::new(GlyphBitmap {
                w: g.w,
                h: g.h,
                ox: g.ox,
                oy: g.oy,
                cov: g.cov,
                rgb: false,
            });
            if let Ok(mut c) = self.cache.lock() {
                if c.len() >= glyph_cache_max() {
                    c.clear();
                }
                c.insert(key, Arc::clone(&bm));
            }
            return Some(bm);
        }
        // 힌트: x-높이가 정수 px가 되는 크기로 래스터(자간·전진 폭은 원래 크기 그대로 — 배치 불변).
        let rsize = if hint {
            self.hint_size_for(face_i, size)
        } else {
            size
        };
        let scaled = face.as_scaled(rsize);
        let glyph =
            gid.with_scale_and_position(rsize, ab_glyph::point(f32::from(sub) / SUBPX, 0.0));
        let outlined = scaled.outline_glyph(glyph)?;
        let b = outlined.px_bounds();
        let (w, h) = (
            (b.max.x - b.min.x).ceil().max(0.0) as usize + 1,
            (b.max.y - b.min.y).ceil().max(0.0) as usize + 1,
        );
        let mut cov = vec![0u8; w * h];
        let gamma = f32::from_bits(gamma_bits);
        let inv = if (gamma - 1.0).abs() < 1e-3 {
            None
        } else {
            Some(1.0 / gamma)
        };
        outlined.draw(|gx, gy, c| {
            let (gx, gy) = (gx as usize, gy as usize);
            if gx < w && gy < h {
                let c = c.clamp(0.0, 1.0);
                let c = match inv {
                    Some(e) => c.powf(e),
                    None => c,
                };
                cov[gy * w + gx] = (c * 255.0 + 0.5) as u8;
            }
        });
        if hint {
            snap_stems(&mut cov, w, h);
        }
        embolden(&mut cov, w, h, f32::from_bits(weight_bits));
        let bm = Arc::new(GlyphBitmap {
            w: w as u16,
            h: h as u16,
            ox: b.min.x.floor() as i16,
            oy: b.min.y.floor() as i16,
            cov,
            rgb: false,
        });
        if let Ok(mut c) = self.cache.lock() {
            if c.len() >= glyph_cache_max() {
                c.clear();
            }
            c.insert(key, Arc::clone(&bm));
        }
        Some(bm)
    }

    /// 테스트용: 글리프의 **세로 획 가시성** — 열마다 "커버리지 ≥ 0.375(rgb는 최대 채널)인 행 수"를 재서 가장 높은 열의
    /// 행 비율(0~1). ClearType은 서브픽셀에 걸친 줄기를 채널 하나에 반쯤 담을 수 있어 문턱을 중간 아래로 둔다.
    /// 세로 획이 있는 글자(ㅏ·l·|)가 흐리면(반 픽셀에 걸쳐 두 열로 번짐) 이 값이 낮다(09-16 GGO 회색 `닫` = 0).
    #[must_use]
    pub fn glyph_stem_visibility(&self, ch: char, size: f32, bold: bool) -> f32 {
        let face_i = self.face_index_for(ch);
        let Some(bm) = self.glyph_bitmap(face_i, ch, size, 0, bold) else {
            return 0.0;
        };
        let (w, h) = (bm.w as usize, bm.h as usize);
        if w == 0 || h == 0 {
            return 0.0;
        }
        let at = |r: usize, c: usize| -> u8 {
            let i = r * w + c;
            if bm.rgb {
                bm.cov[i * 3].max(bm.cov[i * 3 + 1]).max(bm.cov[i * 3 + 2])
            } else {
                bm.cov[i]
            }
        };
        let best = (0..w)
            .map(|c| (0..h).filter(|&r| at(r, c) >= 96).count())
            .max()
            .unwrap_or(0);
        best as f32 / h as f32
    }

    /// 테스트용: 글리프가 **서브픽셀(ClearType) 색 프린지**를 갖는가 — 채널 차가 32를 넘는 픽셀이 하나라도 있으면.
    /// 회색으로 떨어진 환경(헤드리스 러너 · ClearType 끔)에서는 `false`.
    #[must_use]
    pub fn glyph_is_subpixel(&self, ch: char, size: f32) -> bool {
        let face_i = self.face_index_for(ch);
        let Some(bm) = self.glyph_bitmap(face_i, ch, size, 0, false) else {
            return false;
        };
        bm.rgb
            && bm.cov.chunks_exact(3).any(|p| {
                let (mx, mn) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
                mx - mn > 32
            })
    }

    /// 테스트용: 글리프 커버리지 합(잉크 양 · rgb면 채널 평균).
    #[must_use]
    pub fn glyph_ink(&self, ch: char, size: f32, bold: bool) -> f32 {
        let face_i = self.face_index_for(ch);
        let Some(bm) = self.glyph_bitmap(face_i, ch, size, 0, bold) else {
            return 0.0;
        };
        let sum: f32 = bm.cov.iter().map(|&v| f32::from(v) / 255.0).sum();
        if bm.rgb {
            sum / 3.0
        } else {
            sum
        }
    }

    /// 진단·테스트용: 글리프 비트맵을 ASCII 아트로(행마다 `#`(≥192) `+`(≥96) `.`(>0) 공백) — 첫 줄은
    /// `w×h ox,oy adv` 메타. 외곽선이 없으면 `"(none)"`. 자동 캡처 대신 래스터 결과를 곧바로 검사한다.
    #[must_use]
    pub fn glyph_ascii(&self, ch: char, size: f32) -> String {
        let face_i = self.face_index_for(ch);
        let adv = self.advance_of(face_i, ch, size, false);
        let Some(bm) = self.glyph_bitmap(face_i, ch, size, 0, false) else {
            return "(none)".to_string();
        };
        let mut out = format!("{}x{} {},{} adv {adv}\n", bm.w, bm.h, bm.ox, bm.oy);
        for r in 0..bm.h as usize {
            for c in 0..bm.w as usize {
                let i = r * bm.w as usize + c;
                let v = if bm.rgb {
                    bm.cov[i * 3].max(bm.cov[i * 3 + 1]).max(bm.cov[i * 3 + 2])
                } else {
                    bm.cov[i]
                };
                out.push(match v {
                    0 => ' ',
                    1..=95 => '.',
                    96..=191 => '+',
                    _ => '#',
                });
            }
            out.push('\n');
        }
        out
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
        self.measure_from_styled(text, size, start_rel, false)
    }

    /// [`Self::measure_from`]의 스타일 변형 — `bold`면 GDI face는 볼드 face 전진 폭(그리기와 일치) · ab_glyph는 같다.
    #[must_use]
    pub fn measure_from_styled(&self, text: &str, size: f32, start_rel: f32, bold: bool) -> f32 {
        let mut local = 0f32;
        for c in text.chars() {
            local += if c == '\t' {
                let rel = (start_rel + local).ceil();
                rel + self.tab_advance(size, rel) - (start_rel + local)
            } else {
                Self::control_advance(c)
                    .unwrap_or_else(|| self.advance_of(self.face_index_for(c), c, size, bold))
            };
        }
        local
    }

    /// 탭 한 개의 전진 폭 — `rel` = 줄 시작(탭 원점)부터 펜까지의 거리(px · 정수로 올림한 값). 정지점 방식이면 다음 정지점까지
    /// (탭 폭 = 공백 폭 × `tab_cols` · 정지점 위면 한 칸 전체) · 절대 방식이면 늘 탭 폭.
    #[must_use]
    pub fn tab_advance(&self, size: f32, rel: f32) -> f32 {
        let tabw = self.advance_of(self.face_index_for(' '), ' ', size, false) * tab_cols() as f32;
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
            let gdi = self.gdi_face(face_i, size).is_some();
            // 굵게: GDI face는 진짜 볼드 face · ab_glyph는 faux(x축 2회).
            let bold_pass = if style.bold && !gdi { 2 } else { 1 };
            // 정수 스냅이면(또는 GDI 글리프 — 정수 전진) 펜을 반올림하고 서브픽셀 0 · 아니면 1/3px 배치.
            let (pen_i, sub) = if text_snap() || (gdi && cfg!(windows)) {
                (pen.round(), 0u8)
            } else {
                let pi = pen.floor();
                (
                    pi,
                    ((pen - pi) * SUBPX).floor().clamp(0.0, SUBPX - 1.0) as u8,
                )
            };
            if let Some(bm) = self.glyph_bitmap(face_i, ch, size, sub, style.bold) {
                let gx0 = pen_i as i32 + i32::from(bm.ox);
                if gx0 >= clip.2 {
                    break;
                }
                let gy0 = base_y + i32::from(bm.oy);
                for dx in 0..bold_pass {
                    surface.blend_mask(gx0 + dx, gy0, &bm, color, clip, slant, base_y);
                }
            }
            pen += self.advance_of(face_i, ch, size, style.bold);
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

#[cfg(test)]
mod hint_tests {
    use super::*;

    /// T-90d(nexa-sql docs/39 §3-6): 글리프 캐시 상한 세터 — 기본 8192 · 0은 1로 · 삽입 경로가 `glyph_cache_max()`를 본다.
    #[test]
    fn glyph_cache_max_setter() {
        assert_eq!(glyph_cache_max(), GLYPH_CACHE_MAX);
        set_glyph_cache_max(16);
        assert_eq!(glyph_cache_max(), 16);
        set_glyph_cache_max(0);
        assert_eq!(glyph_cache_max(), 1);
        set_glyph_cache_max(GLYPH_CACHE_MAX);
    }

    #[test]
    fn snap_stems_merges_vertical_stem_but_keeps_diagonal() {
        // 3행 × 4열: 세로 줄기가 [0.5, 0.5]로 두 열에 번짐 → 한 열로.
        let w = 4;
        let mut cov = vec![0u8; 12];
        for r in 0..3 {
            cov[r * w + 1] = 128;
            cov[r * w + 2] = 127;
        }
        snap_stems(&mut cov, w, 3);
        for r in 0..3 {
            let row = &cov[r * w..(r + 1) * w];
            let on: Vec<usize> = (0..w).filter(|&c| row[c] > 0).collect();
            assert_eq!(on.len(), 1, "row {r}: {row:?}");
            assert!(row[on[0]] >= 250);
        }
        // 대각선(행마다 중심이 1px씩 이동)은 그대로.
        let mut diag = vec![0u8; 16];
        for r in 0..4 {
            diag[r * 4 + r] = 128;
            if r + 1 < 4 {
                diag[r * 4 + r + 1] = 127;
            }
        }
        let before = diag.clone();
        snap_stems(&mut diag, 4, 4);
        assert_eq!(diag, before);
    }
}

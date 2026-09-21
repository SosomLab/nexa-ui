//! CPU 래스터 백엔드 — [`DrawCtx`]를 `nexa-gfx` 위에 구현(ADR-0001 B안의 실체).
//!
//! 원본(nexa-dir2)의 백엔드는 GDI/DirectWrite였다 — **같은 어휘를 우리 래스터라이저로 다시
//! 구현**하는 것이 이식의 핵심이다(위젯 코드는 백엔드를 모른다). AA 도형은 픽셀별
//! **부호 거리(SDF) 커버리지**로 그린다 — 1비트 리전 클립을 쓰지 않는다([docs/12 §B] `behind`
//! 함정의 근본 해소: 우리는 배경 위 진짜 블렌드가 된다).

use crate::draw::{DrawCtx, FontSlot};
use crate::geom::Rect;
use crate::theme::{Color, FontPrefs, SlotFont};
use nexa_gfx::{Font, Surface, TextStyle};

/// [`Surface`] + [`Font`] 위의 [`DrawCtx`] 구현체.
/// 슬롯별 글꼴 **페이스** 묶음 — 없는 슬롯은 [`FontSet::base`]로 폴백한다.
///
/// 크기·굵기는 [`FontPrefs`]가, **얼굴은 여기가** 정한다. 고정폭 슬롯([`FontSet::mono`])은
/// 숫자 폭이 변해 화면이 떨리는 것을 막는 자리다(사용자 요청 08-09).
#[allow(missing_debug_implementations)]
pub struct FontSet<'f> {
    /// 기본 얼굴(항상 있어야 한다 — 나머지의 폴백).
    pub base: &'f Font,
    /// 사용자 목록.
    pub peerlist: Option<&'f Font>,
    /// 대화 본문.
    pub message: Option<&'f Font>,
    /// 상태바.
    pub status: Option<&'f Font>,
    /// 고정폭(시각·수치 표시).
    pub mono: Option<&'f Font>,
}

impl<'f> FontSet<'f> {
    /// 기본 얼굴 하나로 만든다(전 슬롯 동일).
    #[must_use]
    pub fn single(base: &'f Font) -> Self {
        Self {
            base,
            peerlist: None,
            message: None,
            status: None,
            mono: None,
        }
    }

    /// 슬롯의 얼굴(없으면 기본).
    #[must_use]
    pub fn face(&self, slot: FontSlot) -> &'f Font {
        match slot {
            FontSlot::Base => Some(self.base),
            FontSlot::PeerList => self.peerlist,
            FontSlot::Message => self.message,
            FontSlot::Status => self.status,
            FontSlot::Mono => self.mono,
        }
        .unwrap_or(self.base)
    }
}

#[allow(missing_debug_implementations)]
pub struct RasterCtx<'s, 'b, 'f> {
    surface: &'s mut Surface<'b>,
    fonts: FontSet<'f>,
    /// 현재 슬롯의 얼굴(select_font가 갱신).
    font: &'f Font,
    /// 영역별 글꼴 설정(사용자 설정).
    prefs: FontPrefs,
    /// 현재 선택된 슬롯 글꼴(select_font로 전환).
    cur: SlotFont,
    /// 배율(고DPI — FR-U-6). 크기에 곱한다. 좌표는 이미 물리 px(호출자 몫).
    scale: f32,
    /// 광학 크기 보정(고정폭 전용 · 08-10) — 같은 px에서 숫자가 차지하는 높이를
    /// 기본 얼굴과 일치시키는 배수(Consolas는 숫자가 커서 <1.0). 다른 슬롯은 1.0.
    mono_mult: f32,
    /// 탭 원점(화면 x) — [`DrawCtx::set_tab_origin`].
    tab_origin: Option<i32>,
    /// 이번 프레임의 캐럿 표시 위상(08-13 — 깜빡임). 호스트가 시각·포커스로 주입.
    caret_on: bool,
}

impl<'s, 'b, 'f> RasterCtx<'s, 'b, 'f> {
    /// 표면과 폰트로 컨텍스트를 만든다(기본 슬롯 = Base).
    ///
    /// ★ **`scale`은 선택이 아니라 필수다**(08-27 macOS 회귀 정정).
    ///
    /// 예전에는 기본값 1.0으로 만들고 `with_scale()`로 얹는 구조였는데,
    /// 그 한 줄을 빠뜨리면 **레이아웃만 2배가 되고 글자는 1배로 남는다** —
    /// Retina에서 *"빈 줄만 큼직하고 글씨는 깨알"* 이 된다. Windows(배율 1.0)에서는
    /// 우연히 맞아 보여서 **한쪽 OS에서만 조용히 틀리는** 종류의 버그였다.
    /// 인자로 만들면 잊을 수가 없다.
    pub fn new(surface: &'s mut Surface<'b>, font: &'f Font, scale: f32) -> Self {
        Self::with_font_set(surface, FontSet::single(font), scale)
    }

    /// 슬롯별 얼굴을 가진 컨텍스트(기본 슬롯 = Base).
    pub fn with_font_set(surface: &'s mut Surface<'b>, fonts: FontSet<'f>, scale: f32) -> Self {
        let prefs = FontPrefs::default();
        let font = fonts.base;
        Self {
            surface,
            fonts,
            font,
            prefs,
            cur: prefs.base,
            scale: scale.max(0.5),
            mono_mult: 1.0,
            tab_origin: None,
            caret_on: true,
        }
    }

    /// 이번 프레임의 캐럿 표시 위상 지정(깜빡임 — 호스트가 시각·포커스로 계산).
    #[must_use]
    pub fn with_caret_on(mut self, on: bool) -> Self {
        self.caret_on = on;
        self
    }

    /// 사용자 글꼴 설정 지정.
    #[must_use]
    pub fn with_fonts(mut self, prefs: FontPrefs) -> Self {
        self.prefs = prefs;
        self.cur = prefs.base;
        self
    }

    /// 그리는 중간에 글꼴 설정 교체(한 창 안에서 본문과 푸터의 크기가 다를 때 · nexa-sql 로그 창 09-16).
    /// 현재 선택은 새 설정의 Base로 돌아간다.
    pub fn set_fonts(&mut self, prefs: FontPrefs) {
        self.prefs = prefs;
        self.cur = prefs.base;
        self.font = self.fonts.face(FontSlot::Base);
    }

    /// 지금 배율(호스트가 위젯과 같은 값을 쓰는지 확인할 때).
    #[must_use]
    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// 현재 슬롯의 물리 픽셀 크기.
    fn px_size(&self) -> f32 {
        self.cur.size * self.scale
    }

    fn cur_style(&self) -> TextStyle {
        TextStyle {
            bold: self.cur.bold,
            italic: self.cur.italic,
        }
    }

    fn clip_of(rect: Rect) -> (i32, i32, i32, i32) {
        (rect.x, rect.y, rect.right(), rect.bottom())
    }

    /// rect 영역을 픽셀별 SDF 커버리지로 채운다.
    fn coverage_fill(&mut self, rect: Rect, color: Color, dist: impl Fn(f32, f32) -> f32) {
        self.coverage_fill_alpha(rect, color, 1.0, dist);
    }

    /// [`Self::coverage_fill`]의 불투명도 변형 — 커버리지에 `alpha`를 곱한다(반투명 테두리).
    fn coverage_fill_alpha(
        &mut self,
        rect: Rect,
        color: Color,
        alpha: f32,
        dist: impl Fn(f32, f32) -> f32,
    ) {
        let a = alpha.clamp(0.0, 1.0);
        for py in rect.y..rect.bottom() {
            for px in rect.x..rect.right() {
                // 픽셀 중심 기준 거리 → 커버리지(0.5px 폭 안티에일리어싱).
                let d = dist(px as f32 + 0.5, py as f32 + 0.5);
                let cov = (0.5 - d).clamp(0.0, 1.0) * a;
                if cov > 0.0 {
                    self.surface.blend_px(px, py, color, cov);
                }
            }
        }
    }
}

/// 커버리지 채움(픽셀 중심 SDF · 0.5px 안티에일리어싱 · `alpha` 곱) — `rect` 안만 순회.
fn cov_fill(
    s: &mut Surface<'_>,
    rect: Rect,
    color: Color,
    alpha: f32,
    dist: &dyn Fn(f32, f32) -> f32,
) {
    for py in rect.y..rect.bottom() {
        for px in rect.x..rect.right() {
            let d = dist(px as f32 + 0.5, py as f32 + 0.5);
            let cov = (0.5 - d).clamp(0.0, 1.0) * alpha;
            if cov > 0.0 {
                s.blend_px(px, py, color, cov);
            }
        }
    }
}

/// 단색 띠 — 불투명이면 메모리 채움 · 반투명이면 픽셀 블렌드(`cov_fill`의 커버리지 1과 같은 식).
fn solid_band(s: &mut Surface<'_>, rect: Rect, color: Color, alpha: f32) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    if alpha >= 1.0 {
        s.fill_rect(rect.x, rect.y, rect.w as u32, rect.h as u32, color);
        return;
    }
    for py in rect.y..rect.bottom() {
        for px in rect.x..rect.right() {
            s.blend_px(px, py, color, alpha);
        }
    }
}

/// 유효 모서리 반경(정수 · 사각형 절반을 넘지 않게).
fn round_r(rect: Rect, radius: i32) -> i32 {
    radius.min(rect.w / 2).min(rect.h / 2).max(0)
}

/// 사각형의 SDF 클로저(중심 좌표계).
fn round_sdf(rect: Rect, r: i32) -> impl Fn(f32, f32) -> f32 + Copy {
    let (cx, cy) = (
        rect.x as f32 + rect.w as f32 / 2.0,
        rect.y as f32 + rect.h as f32 / 2.0,
    );
    let (hw, hh) = (rect.w as f32 / 2.0, rect.h as f32 / 2.0);
    let r = (r as f32).min(hw).min(hh).max(0.0);
    move |x: f32, y: f32| round_rect_sdf(x, y, cx, cy, hw, hh, r)
}

/// ★ 라운드 채움 — 안쪽은 단색 띠 · SDF는 네 모서리 r×r만(09-16 계측: 편집기 배경 전면 SDF가 프레임의 80% = 5ms).
///   직선 변은 픽셀 경계에 정확히 놓여 커버리지가 안 1 · 밖 0이므로 전면 SDF 결과와 픽셀 단위로 같다(테스트).
fn rr_fill(s: &mut Surface<'_>, rect: Rect, radius: i32, color: Color, alpha: f32) {
    if rect.is_empty() {
        return;
    }
    let a = alpha.clamp(0.0, 1.0);
    if a <= 0.0 {
        return;
    }
    let r = round_r(rect, radius);
    let (x, y, w, h) = (rect.x, rect.y, rect.w, rect.h);
    if r <= 0 {
        solid_band(s, rect, color, a);
        return;
    }
    // 가운데 가로 띠(전폭) + 위/아래 띠의 가운데(모서리 제외) — 겹침 없음(반투명도 한 번만).
    solid_band(s, Rect::new(x, y + r, w, h - 2 * r), color, a);
    solid_band(s, Rect::new(x + r, y, w - 2 * r, r), color, a);
    solid_band(s, Rect::new(x + r, y + h - r, w - 2 * r, r), color, a);
    let sdf = round_sdf(rect, r);
    for c in [
        Rect::new(x, y, r, r),
        Rect::new(x + w - r, y, r, r),
        Rect::new(x, y + h - r, r, r),
        Rect::new(x + w - r, y + h - r, r, r),
    ] {
        cov_fill(s, c, color, a, &sdf);
    }
}

/// ★ 라운드 외곽선 — 커버리지는 경계에서 `half_w + 0.5`px 안에만 있으므로 가장자리 띠 4개만 순회한다(안쪽 가운데는 건너뜀).
fn rr_stroke(s: &mut Surface<'_>, rect: Rect, radius: i32, color: Color, width: f32, alpha: f32) {
    if rect.is_empty() {
        return;
    }
    let r = round_r(rect, radius);
    let half_w = width / 2.0;
    let pad = half_w.ceil() as i32 + 1;
    let area = Rect::new(
        rect.x - pad,
        rect.y - pad,
        rect.w + pad * 2,
        rect.h + pad * 2,
    );
    let band = pad * 2 + r.max(1);
    let sdf = round_sdf(rect, r);
    let stroke = move |x: f32, y: f32| sdf(x, y).abs() - half_w;
    let (x, y, w, h) = (area.x, area.y, area.w, area.h);
    if band * 2 >= w || band * 2 >= h {
        cov_fill(s, area, color, alpha, &stroke);
        return;
    }
    for b in [
        Rect::new(x, y, w, band),
        Rect::new(x, y + h - band, w, band),
        Rect::new(x, y + band, band, h - 2 * band),
        Rect::new(x + w - band, y + band, band, h - 2 * band),
    ] {
        cov_fill(s, b, color, alpha, &stroke);
    }
}

/// 라운드 사각형 SDF — 중심 좌표계, 반코너 반경 `r`.
fn round_rect_sdf(x: f32, y: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = (x - cx).abs() - (hw - r);
    let qy = (y - cy).abs() - (hh - r);
    let ax = qx.max(0.0);
    let ay = qy.max(0.0);
    (ax * ax + ay * ay).sqrt() + qx.max(qy).min(0.0) - r
}

/// 텍스트를 (폴백 필요 여부, 연속 구간)으로 나눈다 — 글리프 폴백용(08-10).
/// 공백은 앞 구간에 붙인다(구간 난립 방지 — 공백 폭 차이는 미세해 무시).
fn split_runs(text: &str, has: impl Fn(char) -> bool) -> Vec<(bool, String)> {
    let mut out: Vec<(bool, String)> = Vec::new();
    for c in text.chars() {
        if c == ' ' {
            if let Some((_, run)) = out.last_mut() {
                run.push(c);
                continue;
            }
        }
        let fb = !has(c);
        match out.last_mut() {
            Some((lf, run)) if *lf == fb => run.push(c),
            _ => out.push((fb, c.to_string())),
        }
    }
    out
}

/// 점-선분 거리.
fn seg_dist(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= f32::EPSILON {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
    };
    let (ex, ey) = (ax + t * dx - px, ay + t * dy - py);
    (ex * ex + ey * ey).sqrt()
}

impl DrawCtx for RasterCtx<'_, '_, '_> {
    fn surface_size(&self) -> Option<(i32, i32)> {
        Some((self.surface.width() as i32, self.surface.height() as i32))
    }

    fn caret_on(&self) -> bool {
        self.caret_on
    }

    fn select_font(&mut self, slot: FontSlot, bold: bool) {
        self.cur = match slot {
            FontSlot::Base => self.prefs.base,
            FontSlot::PeerList => self.prefs.peerlist,
            FontSlot::Message => self.prefs.message,
            FontSlot::Status => self.prefs.status,
            // 고정폭은 별도 크기 설정이 없다(얼굴만 지정). 크기는 **Status를 따른다** —
            // 쓰이는 곳이 보조 정보 줄(설명문 13px 사이)이라 Base(16px)면 혼자 커 보인다
            // (사용자 지적 08-10 — Consolas 밀도까지 겹쳐 차이가 도드라졌다).
            FontSlot::Mono => self.prefs.status,
        };
        self.font = self.fonts.face(slot);
        // 광학 크기 보정 — 같은 px라도 고정폭 숫자는 본문보다 커 보인다(사용자 지적
        // 08-10 2차: px를 맞춰도 차이가 심하다). 숫자 '0'의 실측 외곽 높이 비로 맞춘다.
        self.mono_mult = if matches!(slot, FontSlot::Mono)
            && !core::ptr::eq(self.font as *const Font, self.fonts.base as *const Font)
        {
            (self.fonts.base.digit_height(100.0) / self.font.digit_height(100.0)).clamp(0.75, 1.15)
        } else {
            1.0
        };
        // 인자 bold는 슬롯 설정 위 강제 볼드(예: 강조 라벨).
        self.cur.bold |= bold;
    }

    fn select_font_sized(&mut self, slot: FontSlot, bold: bool, delta_px: f32) {
        self.select_font(slot, bold);
        // 증분은 슬롯 크기 **위에** 얹는다 — 사용자가 글꼴 크기를 키우면 제목도 같이 큰다.
        self.cur.size = (self.cur.size + delta_px).max(1.0);
    }

    fn fill_rect(&mut self, rect: Rect, color: Color) {
        if rect.is_empty() {
            return;
        }
        self.surface.fill_rect(
            rect.x,
            rect.y,
            rect.w.max(0) as u32,
            rect.h.max(0) as u32,
            color,
        );
    }

    fn text_opaque(&mut self, x: i32, y: i32, clip: Rect, text: &str, fg: Color, bg: Color) {
        self.fill_rect(clip, bg);
        self.text(x, y, clip, text, fg);
    }

    fn fill_triangle(&mut self, a: (i32, i32), b: (i32, i32), c: (i32, i32), color: Color) {
        let (a, b, c) = (
            (a.0 as f32, a.1 as f32),
            (b.0 as f32, b.1 as f32),
            (c.0 as f32, c.1 as f32),
        );
        // 세 반평면의 부호 거리 — 무게중심에서 부호를 재 감김 방향을 정규화한다.
        let edge = |p: (f32, f32), q: (f32, f32), x: f32, y: f32| {
            let (ex, ey) = (q.0 - p.0, q.1 - p.1);
            let len = (ex * ex + ey * ey).sqrt().max(f32::EPSILON);
            ((x - p.0) * ey - (y - p.1) * ex) / len
        };
        let cx = (a.0 + b.0 + c.0) / 3.0;
        let cy = (a.1 + b.1 + c.1) / 3.0;
        let s = if edge(a, b, cx, cy) < 0.0 { -1.0 } else { 1.0 };
        let rect = Rect::new(
            a.0.min(b.0).min(c.0).floor() as i32 - 1,
            a.1.min(b.1).min(c.1).floor() as i32 - 1,
            (a.0.max(b.0).max(c.0) - a.0.min(b.0).min(c.0)).ceil() as i32 + 2,
            (a.1.max(b.1).max(c.1) - a.1.min(b.1).min(c.1)).ceil() as i32 + 2,
        );
        // 내부 = 세 거리 모두 양수(정규화 후) → dist = -(최솟값): 내부 음수·외부 양수.
        self.coverage_fill(rect, color, move |x, y| {
            -(s * edge(a, b, x, y))
                .min(s * edge(b, c, x, y))
                .min(s * edge(c, a, x, y))
        });
    }

    fn set_tab_origin(&mut self, x: Option<i32>) {
        self.tab_origin = x;
    }

    fn text(&mut self, x: i32, y: i32, clip: Rect, text: &str, fg: Color) {
        let size = self.px_size();
        let origin = self.tab_origin.map_or(x as f32, |o| o as f32);
        // 광학 보정 — 슬롯 얼굴(고정폭)은 보정 크기로, 폴백(기본 얼굴)은 명목 크기로.
        let own = size * self.mono_mult;
        // 슬롯 얼굴에 없는 글자는 **기본 얼굴로 폴백**(08-10 — 고정폭 Consolas에 한글이
        // 없어 두부(□)가 나던 문제. 베이스라인은 공유 = 줄이 안 흔들린다).
        if core::ptr::eq(self.font, self.fonts.base) || text.chars().all(|c| self.font.has_glyph(c))
        {
            let baseline = y as f32 + self.font.ascent(own);
            self.font.draw_styled(
                self.surface,
                x as f32,
                baseline,
                own,
                fg,
                text,
                Self::clip_of(clip),
                self.cur_style(),
                origin,
            );
            return;
        }
        let baseline = y as f32 + self.font.ascent(own).max(self.fonts.base.ascent(size));
        let mut fx = x as f32;
        for (fallback, run) in split_runs(text, |c| self.font.has_glyph(c)) {
            let (f, s): (&Font, f32) = if fallback {
                (self.fonts.base, size)
            } else {
                (self.font, own)
            };
            f.draw_styled(
                self.surface,
                fx,
                baseline,
                s,
                fg,
                &run,
                Self::clip_of(clip),
                self.cur_style(),
                origin,
            );
            fx += f.measure_from_styled(&run, s, fx - origin, self.cur.bold);
        }
    }

    fn text_width(&mut self, text: &str) -> i32 {
        let size = self.px_size();
        let own = size * self.mono_mult;
        if core::ptr::eq(self.font, self.fonts.base) || text.chars().all(|c| self.font.has_glyph(c))
        {
            return self
                .font
                .measure_from_styled(text, own, 0.0, self.cur.bold)
                .ceil() as i32;
        }
        // 런을 이어 재되 탭 정지점은 문자열 시작(원점 0)부터 — 런 시작 위치를 넘긴다(접기 순서는 종전과 동일).
        let mut acc = 0f32;
        for (fallback, run) in split_runs(text, |c| self.font.has_glyph(c)) {
            acc += if fallback {
                self.fonts
                    .base
                    .measure_from_styled(&run, size, acc, self.cur.bold)
            } else {
                self.font.measure_from_styled(&run, own, acc, self.cur.bold)
            };
        }
        acc.ceil() as i32
    }

    /// 단일 패스 누적 폭(08-14 성능) — [`Self::text_width`]의 두 경로(기본·폴백 런)를
    /// **같은 f32 접기 순서·같은 자리의 ceil**로 복제한다: 접두사 `i`의 결과가
    /// `text_width(&text[..i])`와 **비트 동일**해야 캐럿·선택 좌표가 어긋나지 않는다
    /// (값 동일 = 기능 불변의 근거 · 종전 O(n²) 접두사 재측정을 O(n)으로).
    fn text_prefix_widths(&mut self, text: &str, out: &mut Vec<i32>) {
        out.clear();
        out.push(0);
        let size = self.px_size();
        let own = size * self.mono_mult;
        let mut buf = [0u8; 4];
        if core::ptr::eq(self.font, self.fonts.base) || text.chars().all(|c| self.font.has_glyph(c))
        {
            // 빠른 경로 — measure()와 같은 문자 순서의 f32 누적 · 접두사마다 ceil.
            let mut sum = 0f32;
            for c in text.chars() {
                sum +=
                    self.font
                        .measure_from_styled(c.encode_utf8(&mut buf), own, sum, self.cur.bold);
                out.push(sum.ceil() as i32);
            }
            return;
        }
        // 폴백 경로 — split_runs의 런 구획(공백은 앞 런에 붙음)과 합산 접기 순서를
        // 접두사 자리마다 복제: 접두사 폭 = (완료 런들의 좌측 접기) + 진행 런 부분합.
        let (mut completed, mut run_sum) = (0f32, 0f32);
        let mut cur_fb: Option<bool> = None;
        for c in text.chars() {
            let fb = if c == ' ' && cur_fb.is_some() {
                cur_fb.unwrap_or(false) // 공백 = 현재 런 유지(split_runs 규칙)
            } else {
                !self.font.has_glyph(c)
            };
            if cur_fb != Some(fb) {
                if cur_fb.is_some() {
                    completed += run_sum; // 런 종결 = sum() 좌측 접기와 동일
                }
                run_sum = 0.0;
                cur_fb = Some(fb);
            }
            let at = completed + run_sum;
            run_sum += if fb {
                self.fonts.base.measure_from_styled(
                    c.encode_utf8(&mut buf),
                    size,
                    at,
                    self.cur.bold,
                )
            } else {
                self.font
                    .measure_from_styled(c.encode_utf8(&mut buf), own, at, self.cur.bold)
            };
            out.push((completed + run_sum).ceil() as i32);
        }
    }

    fn text_ascent(&mut self) -> i32 {
        let size = self.px_size();
        if (self.mono_mult - 1.0).abs() > f32::EPSILON {
            return self.fonts.base.ascent(size).ceil() as i32;
        }
        self.font.ascent(size).ceil() as i32
    }

    fn text_height(&mut self) -> i32 {
        if (self.mono_mult - 1.0).abs() > f32::EPSILON {
            // 고정폭 줄엔 한글 폴백(기본 얼굴·명목 크기)이 섞인다 — 그 기준으로 센터링.
            return self.fonts.base.text_box_height(self.px_size()).ceil() as i32;
        }
        self.font.text_box_height(self.px_size()).ceil() as i32
    }

    fn text_center_y(&mut self, y: i32, h: i32) -> i32 {
        // 잉크 가운데: 기준선 = 행 가운데 + 숫자 높이/2 → top = 기준선 − 어센트. 글꼴·OS가 달라도 같은 자리.
        let size = self.px_size();
        let f = if (self.mono_mult - 1.0).abs() > f32::EPSILON {
            self.fonts.base
        } else {
            self.font
        };
        let (asc, cap) = (f.ascent(size), f.digit_height(size));
        (y as f32 + h as f32 / 2.0 + cap / 2.0 - asc).round() as i32
    }

    fn image(&mut self, x: i32, y: i32, img: &crate::theme::IconImage, clip: Rect) {
        self.surface.blend_image(x, y, img, Self::clip_of(clip));
    }

    fn image_scaled(&mut self, dst: Rect, img: &crate::theme::IconImage, clip: Rect) {
        self.surface
            .blend_image_scaled(dst.x, dst.y, dst.w, dst.h, img, Self::clip_of(clip));
    }

    fn fill_ellipse(&mut self, rect: Rect, color: Color) {
        if rect.is_empty() {
            return;
        }
        let (cx, cy) = (
            rect.x as f32 + rect.w as f32 / 2.0,
            rect.y as f32 + rect.h as f32 / 2.0,
        );
        let (rx, ry) = (rect.w as f32 / 2.0, rect.h as f32 / 2.0);
        self.coverage_fill(rect, color, move |x, y| {
            // 근사 SDF — 정규화 거리에 짧은 반경을 곱한다(원에 가까울수록 정확).
            let nx = (x - cx) / rx;
            let ny = (y - cy) / ry;
            ((nx * nx + ny * ny).sqrt() - 1.0) * rx.min(ry)
        });
    }

    fn stroke_ellipse(&mut self, rect: Rect, color: Color, width: f32) {
        if rect.is_empty() || width <= 0.0 {
            return;
        }
        let (cx, cy) = (
            rect.x as f32 + rect.w as f32 / 2.0,
            rect.y as f32 + rect.h as f32 / 2.0,
        );
        let (rx, ry) = (rect.w as f32 / 2.0, rect.h as f32 / 2.0);
        self.coverage_fill(rect, color, move |x, y| {
            let nx = (x - cx) / rx;
            let ny = (y - cy) / ry;
            let d = ((nx * nx + ny * ny).sqrt() - 1.0) * rx.min(ry); // 음수 = 안쪽
                                                                     // 가장자리 안쪽 width 밴드(-width..0) — 링 SDF.
            (d + width / 2.0).abs() - width / 2.0
        });
    }

    fn fill_pie(&mut self, rect: Rect, start_deg: f32, sweep_deg: f32, color: Color) {
        if rect.is_empty() || sweep_deg <= 0.0 {
            return;
        }
        let (cx, cy) = (
            rect.x as f32 + rect.w as f32 / 2.0,
            rect.y as f32 + rect.h as f32 / 2.0,
        );
        let (rx, ry) = (rect.w as f32 / 2.0, rect.h as f32 / 2.0);
        // 12시 = 0° · 시계 방향(화면 좌표는 y가 아래로 큰다) — 각도의 단위 방향.
        let dir = |deg: f32| {
            let r = deg.to_radians();
            (r.sin(), -r.cos())
        };
        let (sx, sy) = dir(start_deg);
        let (ex, ey) = dir(start_deg + sweep_deg.min(180.0));
        self.coverage_fill(rect, color, move |x, y| {
            let (px, py) = (x - cx, y - cy);
            let nx = px / rx;
            let ny = py / ry;
            let de = ((nx * nx + ny * ny).sqrt() - 1.0) * rx.min(ry);
            // 시작 변의 안쪽 = 시계쪽(외적 양수) · 끝 변의 안쪽 = 반시계쪽 —
            // 부호 거리(음수 = 안)로 뒤집어 타원 SDF와 max 교집합.
            let d0 = -(sx * py - sy * px);
            let d1 = ex * py - ey * px;
            de.max(d0).max(d1)
        });
    }

    fn fill_round_rect(&mut self, rect: Rect, radius: i32, color: Color) {
        rr_fill(self.surface, rect, radius, color, 1.0);
    }

    fn fill_rect_alpha(&mut self, rect: Rect, color: Color, alpha: f32) {
        if rect.w <= 0 || rect.h <= 0 {
            return;
        }
        self.surface
            .fill_rect_alpha(rect.x, rect.y, rect.w as u32, rect.h as u32, color, alpha);
    }

    fn fill_round_rect_alpha(&mut self, rect: Rect, radius: i32, color: Color, alpha: f32) {
        rr_fill(self.surface, rect, radius, color, alpha);
    }

    fn stroke_round_rect(&mut self, rect: Rect, radius: i32, color: Color, width: f32) {
        rr_stroke(self.surface, rect, radius, color, width, 1.0);
    }

    fn stroke_round_rect_alpha(
        &mut self,
        rect: Rect,
        radius: i32,
        color: Color,
        width: f32,
        alpha: f32,
    ) {
        rr_stroke(self.surface, rect, radius, color, width, alpha);
    }

    fn polyline(&mut self, pts: &[(i32, i32)], color: Color, width: f32) {
        self.polyline_in(pts, color, width, None);
    }

    fn polyline_clipped(&mut self, pts: &[(i32, i32)], color: Color, width: f32, clip: Rect) {
        self.polyline_in(pts, color, width, Some(clip));
    }
}

impl RasterCtx<'_, '_, '_> {
    fn polyline_in(&mut self, pts: &[(i32, i32)], color: Color, width: f32, clip: Option<Rect>) {
        if pts.len() < 2 {
            return;
        }
        let half_w = width / 2.0;
        for seg in pts.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            let pad = half_w.ceil() as i32 + 1;
            let x0 = a.0.min(b.0) - pad;
            let y0 = a.1.min(b.1) - pad;
            let mut area = Rect::new(x0, y0, (a.0.max(b.0) + pad) - x0, (a.1.max(b.1) + pad) - y0);
            if let Some(c) = clip {
                area = area.intersection(&c);
                if area.w <= 0 || area.h <= 0 {
                    continue;
                }
            }
            let (ax, ay, bx, by) = (a.0 as f32, a.1 as f32, b.0 as f32, b.1 as f32);
            self.coverage_fill(area, color, move |x, y| {
                seg_dist(x, y, ax, ay, bx, by) - half_w
            });
        }
    }
}

#[cfg(test)]
mod round_rect_tests {
    use super::*;

    /// 전면 SDF(종전 구현) — 비교 기준.
    fn full_fill(
        buf: &mut [u32],
        w: usize,
        h: usize,
        rect: Rect,
        radius: i32,
        color: Color,
        alpha: f32,
    ) {
        let mut s = Surface::new(buf, w, h);
        let sdf = round_sdf(rect, round_r(rect, radius));
        cov_fill(&mut s, rect, color, alpha, &sdf);
    }

    #[allow(clippy::too_many_arguments)]
    fn full_stroke(
        buf: &mut [u32],
        w: usize,
        h: usize,
        rect: Rect,
        radius: i32,
        color: Color,
        width: f32,
        alpha: f32,
    ) {
        let mut s = Surface::new(buf, w, h);
        let sdf = round_sdf(rect, round_r(rect, radius));
        let half = width / 2.0;
        let pad = half.ceil() as i32 + 1;
        let area = Rect::new(
            rect.x - pad,
            rect.y - pad,
            rect.w + pad * 2,
            rect.h + pad * 2,
        );
        cov_fill(&mut s, area, color, alpha, &move |x, y| {
            sdf(x, y).abs() - half
        });
    }

    /// 영역 분해 결과 = 전면 SDF 결과(픽셀 단위 동일) — 채움·외곽선 · 불투명/반투명 · 반경 0·작은 사각형·전체 화면.
    #[test]
    fn region_split_matches_full_sdf() {
        let (w, h) = (120usize, 80usize);
        let bg = 0xFF20_3040u32;
        let color = Color::from_rgb(200, 120, 40);
        for (rect, radius) in [
            (Rect::new(10, 10, 90, 50), 6),
            (Rect::new(3, 5, 40, 12), 3),
            (Rect::new(20, 20, 30, 30), 0),
            (Rect::new(0, 0, 120, 80), 8),
            (Rect::new(30, 30, 9, 9), 20),
            (Rect::new(50, 40, 2, 2), 1),
        ] {
            for alpha in [1.0f32, 0.5] {
                let mut a = vec![bg; w * h];
                let mut b = vec![bg; w * h];
                rr_fill(&mut Surface::new(&mut a, w, h), rect, radius, color, alpha);
                full_fill(&mut b, w, h, rect, radius, color, alpha);
                assert_eq!(a, b, "fill {rect:?} r={radius} a={alpha}");
                for width in [1.0f32, 2.0, 3.0] {
                    let mut a = vec![bg; w * h];
                    let mut b = vec![bg; w * h];
                    rr_stroke(
                        &mut Surface::new(&mut a, w, h),
                        rect,
                        radius,
                        color,
                        width,
                        alpha,
                    );
                    full_stroke(&mut b, w, h, rect, radius, color, width, alpha);
                    assert_eq!(a, b, "stroke {rect:?} r={radius} w={width} a={alpha}");
                }
            }
        }
    }
}

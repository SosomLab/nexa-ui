//! 드로잉 어휘 — 위젯이 그리는 최소 인터페이스([docs/14 §2]).
//!
//! `nexa-dir2/crates/nexa-gui/src/draw.rs` 이식([docs/12 §A]) — **이미 백엔드 교체를 전제로
//! 검증된 추상**이다(원본은 GDI/DirectWrite, 우리는 CPU 래스터라이저 [`crate::raster`]).
//! dir2 전용 어휘(터미널 셀·아이콘·이미지)는 제외 — 이미지는 M4에서 `imgdec` 격리 경유로
//! 별도 설계(FR-S-12), 아이콘은 위젯 셋과 함께.
//!
//! 규약: **래스터 호출은 구현체에만 존재** — 위젯·컨트롤은 이 인터페이스만 쓴다(DR-21의 UI판).

use crate::geom::Rect;
use crate::theme::Color;
use std::sync::atomic::{AtomicBool, Ordering};

/// 폰트 슬롯 — 위젯이 페인트 시작에 자신의 슬롯을 선택한다(상태 공유 · 순서 무관 보장).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FontSlot {
    /// 기본 UI(메뉴·버튼·설정).
    #[default]
    Base,
    /// 사용자(피어) 목록.
    PeerList,
    /// 대화 본문.
    Message,
    /// 상태바·보조.
    Status,
    /// **고정폭** — 시각·수치처럼 폭이 흔들리면 안 되는 표시(크기는 Base와 공유).
    Mono,
}

/// 위젯의 그리기 어휘. 기본 구현이 있는 메서드는 백엔드가 미구현해도 된다(테스트 백엔드).
pub trait DrawCtx {
    /// 이번 프레임에 **캐럿을 그릴 것인가**(깜빡임 위상 · 08-13 사용자 요청).
    /// 위젯은 시계가 없으므로 호스트가 프레임마다 위상을 주입한다 — 포커스 창이
    /// 아니거나 어두운 위상이면 `false`. 기본 = 항상 표시(테스트 백엔드·정적 렌더).
    /// ★ 그리기 표면의 크기(물리 px) — 팝업(메뉴·툴팁)이 **표면 밖으로 나가지 않게** 스스로 맞추는 데 쓴다(호출자가 영역을
    /// 잘못 넘겨도 잘리지 않는 안전망 · nexa-sql 사용자 09-21). 크기를 모르는 컨텍스트(테스트용 기록기)는 `None`.
    fn surface_size(&self) -> Option<(i32, i32)> {
        None
    }

    fn caret_on(&self) -> bool {
        true
    }
    /// 폰트 슬롯/장식 선택 — 이후의 `text*`/`text_width`에 적용. 기본 = no-op(단일 폰트 백엔드).
    fn select_font(&mut self, slot: FontSlot, bold: bool) {
        let _ = (slot, bold);
    }

    /// 슬롯 선택 + **크기 증분**(논리 px) — 제목처럼 "본문보다 조금 크게"를 표현할 때.
    /// 절대 크기를 박으면 사용자가 글꼴 크기를 바꿔도 제목만 그대로 남아 위계가 깨진다.
    /// 기본 = 증분 무시(단일 폰트 백엔드).
    fn select_font_sized(&mut self, slot: FontSlot, bold: bool, delta_px: f32) {
        let _ = delta_px;
        self.select_font(slot, bold);
    }

    /// rect를 단색으로 불투명하게 채운다.
    fn fill_rect(&mut self, rect: Rect, color: Color);

    /// `clip`을 `bg`로 채우면서 텍스트를 `(x, y)`(왼쪽 위)에 그린다 — 행 배경+텍스트 1회 호출
    /// (원본 GDI `ETO_OPAQUE` 모델의 실증을 계승). `clip` 초과분은 잘린다.
    fn text_opaque(&mut self, x: i32, y: i32, clip: Rect, text: &str, fg: Color, bg: Color);

    /// 배경 없이 텍스트만 — 선택 하이라이트 위 겹쳐 그리기. 1회 호출(경계 이음새 방지).
    fn text(&mut self, x: i32, y: i32, clip: Rect, text: &str, fg: Color);

    /// 텍스트 렌더 폭(px) — 우측 정렬·라벨 실측 정렬용.
    fn text_width(&mut self, text: &str) -> i32;

    /// ★ 탭 원점(줄 시작 x · 화면 좌표) — 정해 두면 이후 `text*` 호출의 탭 정지점은 이 x를 기준으로 센다(편집기가 한 줄을
    /// 여러 색 구간으로 나눠 그릴 때 구간마다 정지점이 어긋나지 않게). `None`(기본) = 각 호출의 시작 x가 원점.
    /// `text_width`/`text_prefix_widths`는 늘 문자열 시작을 원점으로 잰다(접두사 폭 계약). 기본 = no-op.
    fn set_tab_origin(&mut self, x: Option<i32>) {
        let _ = x;
    }

    /// 문자 경계 **누적 폭**(08-14 성능) — `out[i]` = 앞 `i`글자 접두사의
    /// [`text_width`](Self::text_width)와 **동일 값**(0 포함 · 길이 = 문자수+1).
    /// 캐럿·선택 좌표의 원천이라 **값 동일이 계약**이다. 기본 구현 = 접두사
    /// 재측정(O(n²) — 종전 호출부 로직 그대로) · 렌더러는 단일 패스(O(n))로
    /// 오버라이드한다(매 페인트 실측이라 캐럿 깜빡임 상시 리페인트에서 비용이 컸다).
    fn text_prefix_widths(&mut self, text: &str, out: &mut Vec<i32>) {
        out.clear();
        out.push(0);
        let mut acc = String::new();
        for c in text.chars() {
            acc.push(c);
            out.push(self.text_width(&acc));
        }
    }

    /// 현재 글꼴의 텍스트 상자 높이(px · 어센트+디센트) — 세로 중앙 정렬 실측용.
    /// 기본 = 16(레거시 근사) — 실제 렌더러는 폰트 메트릭으로 오버라이드.
    fn text_height(&mut self) -> i32 {
        16
    }

    /// 현재 글꼴의 어센트(px · 줄 상단 → 기준선). 글리프의 **시각 중심**(대문자·한글 몸통 ≈ 기준선 − 어센트×0.38)에
    /// 아이콘/셰브론을 맞출 때 쓴다(nexa-sql 탐색기 09-15). 기본 = 줄 높이의 3/4.
    fn text_ascent(&mut self) -> i32 {
        self.text_height() * 3 / 4
    }

    /// 높이 `h`인 행(위 `y`) 안에서 글자를 **잉크 기준**으로 세로 가운데에 둘 때의 텍스트 top.
    /// 상자(어센트+디센트) 가운데가 아니라 대문자·숫자·한글 몸통(≈ 숫자 높이)의 가운데를 행 가운데에 맞춘다 —
    /// 맥 Apple SD Gothic Neo(어센트 0.75em · 디센트 0.25em · 어센트 안 여백 없음)는 상자 가운데로 놓으면 글자가
    /// 위로 치우치고, Windows 맑은 고딕(어센트 안 여백 큼)은 우연히 맞아 보였다(09-16 사용자 캡처 비교).
    /// 기본 구현 = 상자 가운데(측정 전용 백엔드).
    fn text_center_y(&mut self, y: i32, h: i32) -> i32 {
        y + (h - self.text_height()) / 2
    }

    /// 삼각형을 단색 AA로 채운다(말풍선 꼬리 등 — 08-10). 기본 = no-op.
    fn fill_triangle(&mut self, a: (i32, i32), b: (i32, i32), c: (i32, i32), color: Color) {
        let _ = (a, b, c, color);
    }

    /// RGBA 이미지 아이콘을 `(x, y)`(좌상단)에 알파 블렌드 — `clip` 밖은 잘린다. 기본 = no-op.
    fn image(&mut self, x: i32, y: i32, img: &crate::theme::IconImage, clip: Rect) {
        let _ = (x, y, img, clip);
    }

    /// RGBA 이미지를 `dst`로 **스케일**해 블렌드(큰 이미지 축소·이미지 버튼) — `clip` 밖은 잘린다.
    /// 기본 = no-op.
    fn image_scaled(&mut self, dst: Rect, img: &crate::theme::IconImage, clip: Rect) {
        let _ = (dst, img, clip);
    }

    /// 원/타원 AA 채움. 기본 = no-op.
    fn fill_ellipse(&mut self, rect: Rect, color: Color) {
        let _ = (rect, color);
    }

    /// 타원 **테두리 링** AA(08-14 — 아바타 보더). `width`px 밴드를 가장자리 **안쪽**에
    /// 그린다(rect 밖으로 나가지 않는다). 기본 = no-op.
    fn stroke_ellipse(&mut self, rect: Rect, color: Color, width: f32) {
        let _ = (rect, color, width);
    }

    /// 부채꼴(파이) AA 채움 — `rect` 내접 타원에서 **12시 = 0° · 시계 방향**으로
    /// `start_deg`부터 `sweep_deg`만큼. 반평면 2장의 교집합이라 **`sweep_deg` ≤ 180°만
    /// 보증**한다(M3-19 갭 링의 "파냄" 용도 — 그 이상이 필요하면 두 번 부른다). 기본 = no-op.
    fn fill_pie(&mut self, rect: Rect, start_deg: f32, sweep_deg: f32, color: Color) {
        let _ = (rect, start_deg, sweep_deg, color);
    }

    /// 라운드 사각형 AA 채움. 기본 = no-op.
    fn fill_round_rect(&mut self, rect: Rect, radius: i32, color: Color) {
        let _ = (rect, radius, color);
    }

    /// ★ **상태 레이어를 덮는다**(Material) — hover·press·selected를 **알파 오버레이**로.
    ///
    /// 색을 새로 만들지 않는 것이 요점이다([docs/25 §3-4]). `Rest`·`Disabled`면 아무것도 안 그린다.
    fn state_layer(&mut self, rect: Rect, color: Color, state: crate::tokens::State) {
        let a = state.overlay_alpha();
        if a > 0.0 {
            self.fill_rect_alpha(rect, color, a);
        }
    }

    /// ★ **엘리베이션 그림자** — 두 겹으로 그린다(한 겹은 딱딱해 보인다).
    ///
    /// `rect` **아래**에 깔리는 것이므로 본체보다 **먼저** 부른다.
    fn shadow(&mut self, rect: Rect, color: Color, level: crate::tokens::Elevation, radius: i32) {
        for &(dy, spread, alpha) in level.layers() {
            let r = Rect::new(
                rect.x - spread,
                rect.y - spread + dy,
                rect.w + spread * 2,
                rect.h + spread * 2,
            );
            self.fill_round_rect_alpha(r, radius + spread, color, alpha);
        }
    }

    /// ★ **반투명 사각형 채움**(`alpha` 0..=1) — 오버레이·시트 배경·그림자.
    ///
    /// 기본 구현은 **불투명 폴백**이라 백엔드가 미구현이어도 화면이 비지 않는다
    /// (테스트 백엔드·단순 백엔드 배려). 실제 블렌드는 [`crate::raster::RasterCtx`]가 한다.
    fn fill_rect_alpha(&mut self, rect: Rect, color: Color, alpha: f32) {
        let _ = alpha;
        self.fill_rect(rect, color);
    }

    /// 라운드 사각형 AA 채움 + **불투명도**(`alpha` 0..=1 — 반투명 스크롤바 등).
    /// 기본 = 알파 무시하고 [`Self::fill_round_rect`] 위임(테스트 백엔드).
    fn fill_round_rect_alpha(&mut self, rect: Rect, radius: i32, color: Color, alpha: f32) {
        let _ = alpha;
        self.fill_round_rect(rect, radius, color);
    }

    /// 라운드 사각형 AA 외곽선(폭 `width`px). 기본 = no-op.
    fn stroke_round_rect(&mut self, rect: Rect, radius: i32, color: Color, width: f32) {
        let _ = (rect, radius, color, width);
    }

    /// 라운드 사각형 AA 외곽선 + **불투명도**(`alpha` 0..=1 — 포커스 링 반투명 테두리).
    /// 기본 = 알파 무시하고 [`Self::stroke_round_rect`] 위임(테스트 백엔드).
    fn stroke_round_rect_alpha(
        &mut self,
        rect: Rect,
        radius: i32,
        color: Color,
        width: f32,
        alpha: f32,
    ) {
        let _ = alpha;
        self.stroke_round_rect(rect, radius, color, width);
    }

    /// 꺾은선(✓·셰브론 등) — 둥근 캡, 폭 `width`px AA. 기본 = no-op.
    fn polyline(&mut self, pts: &[(i32, i32)], color: Color, width: f32) {
        let _ = (pts, color, width);
    }

    /// [`Self::polyline`]에 클립 — `clip` 밖은 그리지 않는다(부분적으로 잘린 행의 셰브론 · nexa-sql 09-19). 기본 = 클립 없이.
    fn polyline_clipped(&mut self, pts: &[(i32, i32)], color: Color, width: f32, clip: Rect) {
        let _ = clip;
        self.polyline(pts, color, width);
    }
}

/// 공용 툴팁(08-23 — 툴바·필터 바): `anchor` 아래 6px에 역상 캡슐(어두운 바탕 +
/// 밝은 글자 — 다크/라이트 공용)로 `text`를 그린다. `clamp_w` 오른쪽을 넘지 않게
/// 왼쪽으로 민다. 호출자는 **팝업 레이어**(다른 위젯 위)에서 불러야 한다.
pub fn draw_tooltip(
    ctx: &mut dyn DrawCtx,
    theme: &crate::theme::Theme,
    anchor: Rect,
    clamp_w: i32,
    text: &str,
    scale: f32,
) {
    draw_tooltip_in(ctx, theme, anchor, (0, clamp_w), text, scale);
}

/// [`draw_tooltip`]과 같되 가로 클램프를 `(x0, x1)`로 — 창 왼쪽이 아니라 **바 자신의 x부터** 놓아야 왼쪽 패널에
/// 가려지지 않는다(nexa-sql 결과 도구줄 09-16: 툴팁이 활동 막대 밑으로 들어갔다).
pub fn draw_tooltip_in(
    ctx: &mut dyn DrawCtx,
    theme: &crate::theme::Theme,
    anchor: Rect,
    clamp_x: (i32, i32),
    text: &str,
    scale: f32,
) {
    if text.is_empty() {
        return;
    }
    let s = |v: i32| (v as f32 * scale).round() as i32;
    let (clamp_x0, clamp_w) = clamp_x;
    ctx.select_font(FontSlot::Status, false);
    // ★ 여러 줄(`\n`) 카드 — 줄마다 폭을 재서 가장 넓은 줄 · 높이 = 줄 수(한 줄 상자에 여러 줄을 넣어 아래가 잘리던
    //   결함 · nexa-sql 탭 툴팁 · 사용자 09-16).
    let lines: Vec<&str> = text.split('\n').collect();
    let tw = lines.iter().map(|l| ctx.text_width(l)).max().unwrap_or(0);
    let th = ctx.text_height();
    let w = tw + s(12);
    let h = th * lines.len() as i32 + s(8);
    // 가로: 호출자의 범위와 표면의 겹침 안에서 · 세로: 기준 아래 6px → 아래로 넘치면 **기준 위**로 → 그래도 안 되면 밀어 넣는다
    // (창 아래쪽 도구줄·상태줄의 툴팁이 잘리던 것 · nexa-sql 사용자 09-21). 표면 크기를 모르면 종전대로 아래.
    let surface = ctx.surface_size();
    let clamp_w = surface.map_or(clamp_w, |(sw, _)| clamp_w.min(sw));
    let lo = clamp_x0.max(0) + s(4);
    let x = (anchor.x + (anchor.w - w) / 2).clamp(lo, (clamp_w - w - s(4)).max(lo));
    let below = anchor.bottom() + s(6);
    let y = match surface {
        Some((_, sh)) if below + h > sh => {
            let above = anchor.y - s(6) - h;
            if above >= 0 {
                above
            } else {
                (sh - h).max(0)
            }
        }
        _ => below,
    };
    let r = Rect::new(x, y, w, h);
    ctx.fill_round_rect_alpha(r, s(4), theme.text, 0.92);
    for (i, line) in lines.iter().enumerate() {
        ctx.text(
            r.x + s(6),
            r.y + s(4) + th * i as i32,
            r,
            line,
            theme.panel_bg,
        );
    }
}

/// ★ **전체 경로 보기 스위치**(nexa-sql 사용자 09-22 "긴 경로는 가운데 …로 축약 · Alt를 누르는 동안 전체 경로"): 호스트가
/// 수식키 사건에서 켜고 끈다(프로세스 전역 · 어느 창이든). 켜져 있으면 [`ellipsize_middle`]이 축약하지 않는다.
static SHOW_FULL: AtomicBool = AtomicBool::new(false);

/// 전체 경로 보기를 켜거나 끈다 — **바뀌었으면 true**(호스트가 다시 그린다).
pub fn set_show_full(on: bool) -> bool {
    SHOW_FULL.swap(on, Ordering::Relaxed) != on
}

/// 지금 전체 경로 보기인가.
#[must_use]
pub fn show_full() -> bool {
    SHOW_FULL.load(Ordering::Relaxed)
}

/// 글이 `max_w`보다 넓으면 **가운데를 `…`로 줄여** 맞춘다(앞·뒤를 번갈아 늘려 앞 ≈ 뒤 · 경로·긴 라벨 공통 · 폭은 지금 글꼴로 실측).
/// [`show_full`]이면 그대로 돌려준다. `max_w`가 `…` 하나도 못 넣으면 `…`.
pub fn ellipsize_middle(ctx: &mut dyn DrawCtx, text: &str, max_w: i32) -> String {
    if show_full() || text.is_empty() {
        return text.to_string();
    }
    let mut pw = Vec::new();
    ctx.text_prefix_widths(text, &mut pw);
    let n = pw.len().saturating_sub(1);
    if n == 0 || pw[n] <= max_w {
        return text.to_string();
    }
    let ell = ctx.text_width("…");
    let budget = max_w - ell;
    if budget <= 0 {
        return "…".to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut head, mut tail) = (0usize, 0usize);
    // 앞·뒤를 번갈아 한 글자씩 늘리되 합이 예산을 넘기 직전에 멈춘다(앞이 뒤보다 한 글자 많을 수 있다).
    loop {
        if head + tail >= n {
            break;
        }
        let try_head = head <= tail;
        let (h2, t2) = if try_head {
            (head + 1, tail)
        } else {
            (head, tail + 1)
        };
        let w = pw[h2] + (pw[n] - pw[n - t2]);
        if w > budget {
            // 이쪽은 더 못 늘린다 — 다른 쪽을 한 번 더 시도한 뒤 끝.
            let (h3, t3) = if try_head {
                (head, tail + 1)
            } else {
                (head + 1, tail)
            };
            if h3 + t3 <= n && pw[h3] + (pw[n] - pw[n - t3]) <= budget {
                head = h3;
                tail = t3;
            }
            break;
        }
        head = h2;
        tail = t2;
    }
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(chars[n - tail..].iter());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;
    use crate::theme::Color;

    /// 비선형 폭 목업 — 누적이 "선형 가정"을 하면 어긋나도록 폭 = 문자수²×3.
    struct Quirky;
    impl DrawCtx for Quirky {
        fn fill_rect(&mut self, _r: Rect, _c: Color) {}
        fn text_opaque(&mut self, _x: i32, _y: i32, _c: Rect, _t: &str, _f: Color, _b: Color) {}
        fn text(&mut self, _x: i32, _y: i32, _c: Rect, _t: &str, _f: Color) {}
        fn text_width(&mut self, text: &str) -> i32 {
            let n = i32::try_from(text.chars().count()).unwrap_or(i32::MAX);
            n * n * 3
        }
    }

    /// 가운데 축약: 넓으면 앞…뒤(예산 안 · 앞 ≈ 뒤) · 맞으면 그대로 · 전체 보기 스위치면 그대로 · 아주 좁으면 `…`만.
    #[test]
    fn ellipsize_middle_fits_and_respects_switch() {
        let mut ctx = Quirky; // 폭 = 글자수²×3 — 비선형이라 접두사 폭 표로만 맞출 수 있다.
        let text = "abcdefghij"; // 폭 300
        assert_eq!(ellipsize_middle(&mut ctx, text, 300), text);
        let e = ellipsize_middle(&mut ctx, text, 100); // 예산 100 − '…'(3) = 97 → head²·3 + tail²·3 ≤ 97
        assert!(
            e.contains('…') && e.starts_with('a') && e.ends_with('j'),
            "{e}"
        );
        assert!(ctx.text_width(&e) <= 100, "{e} 폭 {}", ctx.text_width(&e));
        assert_eq!(ellipsize_middle(&mut ctx, text, 2), "…");
        assert!(set_show_full(true));
        assert_eq!(
            ellipsize_middle(&mut ctx, text, 10),
            text,
            "전체 보기 = 축약 없음"
        );
        assert!(set_show_full(false));
        assert!(!set_show_full(false), "같은 값 = 안 바뀜");
    }

    /// 계약(08-14): `out[i]` == `text_width(접두사 i)` · `out[0]` == 0 · 길이 = 문자수+1.
    /// 캐럿·선택 좌표의 원천이라 이 동일성이 곧 기능 불변의 근거다.
    #[test]
    fn prefix_widths_default_matches_prefix_text_width() {
        let mut ctx = Quirky;
        let text = "한a b글";
        let mut out = Vec::new();
        ctx.text_prefix_widths(text, &mut out);
        assert_eq!(out.len(), text.chars().count() + 1);
        assert_eq!(out[0], 0);
        for (i, w) in out.iter().enumerate() {
            let prefix: String = text.chars().take(i).collect();
            assert_eq!(*w, ctx.text_width(&prefix), "접두사 {i}");
        }
    }
}

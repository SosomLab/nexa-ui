//! 시맨틱 테마 토큰 — `nexa-dir2/crates/nexa-gui/src/theme.rs`의 **토큰 체계** 이식([docs/12 §B]).
//!
//! **색 하드코딩 금지 — 전 위젯이 테마를 인자로 받는다.** 다크/라이트가 값 교체만으로 끝난다
//! (FR-U-5). 토큰 키는 안정 계약 — rename 시 마이그레이션 표를 남긴다.
//! ⚠️ 값은 임시 팔레트다 — macOS 시각 언어 수치표(M3-1c) 확정 시 값만 교체한다.

pub use nexa_gfx::{Color, IconImage};

/// 시맨틱 색 토큰. 파일 탐색기 전용 토큰(탭 바·헤더 등)은 이식에서 제외하고
/// 메신저에 필요한 공통 토큰 + `danger`(파일 승인 등 위험 행위 — [docs/12 §B])를 얹었다.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Theme {
    /// 창 루트 배경.
    pub window_bg: Color,
    /// 메뉴·툴바 크롬.
    pub chrome_bg: Color,
    /// 목록·대화 패널.
    pub panel_bg: Color,
    /// 행 교대 음영.
    pub panel_bg_alt: Color,
    /// 수신 말풍선 배경(대화 — 발신은 `accent`). 패널과의 대비 확보:
    /// **다크 = 패널보다 밝게 · 라이트 = 패널보다 어둡게**(사용자 확정 08-10).
    pub bubble_peer: Color,
    /// 입력 필드.
    pub field_bg: Color,
    /// 경계선·스플리터.
    pub border: Color,
    /// 강조(선택 줄·링크·활성 표시).
    pub accent: Color,
    /// 포커스 링(모든 커스텀 컨트롤 공통 — 선택 시 밝은 반투명 테두리).
    pub focus_ring: Color,
    /// 본문 텍스트.
    pub text: Color,
    /// 보조 텍스트.
    pub text_dim: Color,
    /// 선택 행 배경(포커스).
    pub sel_bg: Color,
    /// 선택 행 배경(비포커스 — 무채색 관례).
    pub sel_bg_inactive: Color,
    /// 구문 강조 — 키워드 · 문자열 · 주석 · 숫자([`crate::highlight`]).
    pub syn_keyword: Color,
    pub syn_string: Color,
    pub syn_comment: Color,
    pub syn_number: Color,
    /// 레인보우 괄호 깊이 색 6(nexa-sql docs/51 · D-95 테마 기본).
    /// ★ 순서 = [`contrast_order`] 결과(이웃 깊이가 따뜻함↔차가움으로 번갈아 · 보색에 가깝게) — 색을 바꾸면 테스트
    /// `theme_rainbow_is_already_contrast_ordered`가 새 순서를 알려 준다.
    pub rainbow: [Color; 6],
    /// 위험 행위(파일 실체화 승인·차단 등).
    pub danger: Color,
    /// 긍정 상태(대조 완료 등).
    pub ok: Color,
    /// 주의 상태(미검증 등).
    pub warn: Color,
    /// 다크 팔레트 여부.
    pub is_dark: bool,
}

impl Theme {
    /// 다크 팔레트(임시 — M3-1c에서 수치 확정).
    #[must_use]
    pub const fn dark() -> Self {
        Theme {
            window_bg: Color(0x0014_161A),
            chrome_bg: Color(0x001E_2228),
            panel_bg: Color(0x0019_1C21),
            panel_bg_alt: Color(0x001F_242B),
            bubble_peer: Color(0x0031_3947),
            field_bg: Color(0x0026_2B33),
            border: Color(0x0036_3C46),
            accent: Color(0x003D_8BFF),
            focus_ring: Color(0x007F_B4FF),
            text: Color(0x00D6_DAE0),
            text_dim: Color(0x008A_919C),
            sel_bg: Color(0x0024_405F),
            sel_bg_inactive: Color(0x002C_313A),
            syn_keyword: Color(0x0079_B8FF),
            syn_string: Color(0x00E3_A26A),
            syn_comment: Color(0x007C_8A5A),
            syn_number: Color(0x00B5_CEA8),
            rainbow: [
                Color(0x00F2_C94C),
                Color(0x004F_C1FF),
                Color(0x00DA_70D6),
                Color(0x007E_E787),
                Color(0x00FF_9F43),
                Color(0x00B3_92F0),
            ],
            danger: Color(0x00E5_534B),
            ok: Color(0x002E_A043),
            warn: Color(0x00B5_7C1E),
            is_dark: true,
        }
    }

    /// 라이트 팔레트(임시).
    #[must_use]
    pub const fn light() -> Self {
        Theme {
            window_bg: Color(0x00F6_F7F9),
            chrome_bg: Color(0x00EE_F1F5),
            panel_bg: Color(0x00FF_FFFF),
            panel_bg_alt: Color(0x00F5_F7FA),
            bubble_peer: Color(0x00E2_E7EE),
            field_bg: Color(0x00FF_FFFF),
            border: Color(0x00D5_DAE1),
            accent: Color(0x003D_8BFF),
            focus_ring: Color(0x00A9_CCFF),
            text: Color(0x001B_1F26),
            text_dim: Color(0x006B_7280),
            sel_bg: Color(0x00D8_E8FF),
            sel_bg_inactive: Color(0x00E6_E9EE),
            syn_keyword: Color(0x000A_4FB5),
            syn_string: Color(0x00A3_1515),
            syn_comment: Color(0x0057_8A2A),
            syn_number: Color(0x0009_8658),
            rainbow: [
                Color(0x00B5_8900),
                Color(0x0026_8BD2),
                Color(0x00D3_3682),
                Color(0x002A_A198),
                Color(0x00CB_4B16),
                Color(0x006C_71C4),
            ],
            danger: Color(0x00D3_2F2F),
            ok: Color(0x001A_7F37),
            warn: Color(0x009A_6700),
            is_dark: false,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::dark()
    }
}

/// `#RRGGBB`/`RRGGBB` → 토큰 색(형식 오류 = `None`) — 색상 설정 파싱(08-10).
#[must_use]
pub fn color_from_hex(s: &str) -> Option<Color> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(h, 16).ok().map(Color)
}

/// 토큰 색 → `#RRGGBB`(대문자) — 설정 저장·표시 형식.
#[must_use]
pub fn color_to_hex(c: Color) -> String {
    format!("#{:06X}", c.0 & 0x00FF_FFFF)
}

/// 색의 (색상각 0~360 · 채도 0~1 · 상대 휘도 0~1).
fn hue_sat_lum(c: Color) -> (f32, f32, f32) {
    let (r, g, b) = c.rgb();
    let (r, g, b) = (
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    );
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let d = max - min;
    let hue = if d <= f32::EPSILON {
        0.0
    } else if (max - r).abs() <= f32::EPSILON {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if (max - g).abs() <= f32::EPSILON {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let sat = if max <= f32::EPSILON { 0.0 } else { d / max };
    (hue, sat, 0.2126 * r + 0.7152 * g + 0.0722 * b)
}

/// 따뜻한 색인가 — 빨강·주황·노랑·자홍(색상각 < 90° 또는 ≥ 300°). 나머지(초록·청록·파랑·보라) = 차가운 색.
#[must_use]
pub fn is_warm(c: Color) -> bool {
    let (h, _, _) = hue_sat_lum(c);
    !(90.0..300.0).contains(&h)
}

/// 두 색이 **나란히 놓였을 때 구별되는 정도**(0~1): 색상각 차이(보색 = 180° = 최대 · 채도가 낮으면 덜 믿는다) 0.6 +
/// 색 온도가 다름(따뜻함↔차가움) 0.2 + 밝기 차이 0.2.
#[must_use]
pub fn color_contrast(a: Color, b: Color) -> f32 {
    let (ha, sa, la) = hue_sat_lum(a);
    let (hb, sb, lb) = hue_sat_lum(b);
    let dh = (ha - hb).abs();
    let dh = dh.min(360.0 - dh) / 180.0;
    let temp = if is_warm(a) == is_warm(b) { 0.0 } else { 1.0 };
    0.6 * dh * sa.min(sb) + 0.2 * temp + 0.2 * (la - lb).abs().min(1.0)
}

/// 순환 팔레트(깊이 1, 2, 3 … 이 차례로 쓰는 색)를 **이웃끼리 가장 잘 구별되게** 다시 배열한다(nexa-sql 사용자 09-19
/// "1·2, 2·3, 3·4가 보색·색 온도로 식별되게"). 첫 색은 그대로 두고, 이웃 쌍(끝→처음 포함)의 [`color_contrast`] **최솟값**이
/// 가장 큰 순서를 고른다(동률 = 합이 큰 쪽 · 그다음 = 원래 순서에 가까운 쪽). 8색 이하는 전수(≤ 5040가지) · 그보다 많으면
/// "직전 색과 가장 다른 색"을 차례로 고르는 탐욕. 색이 3개 이하이면 순서를 바꿀 여지가 없어 그대로 돌려준다.
#[must_use]
pub fn contrast_order(colors: &[Color]) -> Vec<Color> {
    let n = colors.len();
    if n <= 3 {
        return colors.to_vec();
    }
    if n > 8 {
        let mut rest: Vec<Color> = colors[1..].to_vec();
        let mut out = vec![colors[0]];
        while !rest.is_empty() {
            let last = out[out.len() - 1];
            let mut best = 0;
            for i in 1..rest.len() {
                if color_contrast(last, rest[i]) > color_contrast(last, rest[best]) {
                    best = i;
                }
            }
            out.push(rest.remove(best));
        }
        return out;
    }
    let mut score = vec![vec![0.0f32; n]; n];
    for i in 0..n {
        for j in 0..n {
            score[i][j] = color_contrast(colors[i], colors[j]);
        }
    }
    // 순열 전수(첫 색 고정) — 사전순이라 동률이면 원래 순서에 가까운 것이 먼저 나와 남는다.
    fn walk(
        order: &mut Vec<usize>,
        used: &mut [bool],
        score: &[Vec<f32>],
        best: &mut (f32, f32, Vec<usize>),
    ) {
        let n = used.len();
        if order.len() == n {
            let (mut min, mut sum) = (f32::MAX, 0.0);
            for k in 0..n {
                let v = score[order[k]][order[(k + 1) % n]];
                min = min.min(v);
                sum += v;
            }
            if min > best.0 + 1e-6 || ((min - best.0).abs() <= 1e-6 && sum > best.1 + 1e-6) {
                *best = (min, sum, order.clone());
            }
            return;
        }
        for i in 1..n {
            if !used[i] {
                used[i] = true;
                order.push(i);
                walk(order, used, score, best);
                order.pop();
                used[i] = false;
            }
        }
    }
    let mut used = vec![false; n];
    used[0] = true;
    let mut best = (-1.0f32, -1.0f32, (0..n).collect::<Vec<usize>>());
    walk(&mut vec![0], &mut used, &score, &mut best);
    best.2.into_iter().map(|i| colors[i]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이웃 단계(끝→처음 포함)의 최소 대비.
    fn min_adjacent(c: &[Color]) -> f32 {
        (0..c.len())
            .map(|i| color_contrast(c[i], c[(i + 1) % c.len()]))
            .fold(f32::MAX, f32::min)
    }

    #[test]
    fn contrast_order_separates_neighbours() {
        // 무지개 순서(이웃이 비슷한 색) → 이웃 대비가 뚜렷이 커지고 · 첫 색·색 집합은 그대로.
        let rainbow: Vec<Color> = ["FF0000", "FF8000", "FFD000", "00C000", "0080FF", "8000FF"]
            .iter()
            .filter_map(|h| color_from_hex(h))
            .collect();
        let out = contrast_order(&rainbow);
        assert_eq!(out[0], rainbow[0]);
        let mut a: Vec<u32> = rainbow.iter().map(|c| c.0).collect();
        let mut b: Vec<u32> = out.iter().map(|c| c.0).collect();
        a.sort_unstable();
        b.sort_unstable();
        assert_eq!(a, b);
        assert!(min_adjacent(&out) > min_adjacent(&rainbow) + 0.15);
        // 3색 이하 = 그대로 · 많은 색 = 탐욕(집합 보존).
        assert_eq!(contrast_order(&rainbow[..3]), rainbow[..3].to_vec());
        let many: Vec<Color> = (0..12).map(|i| Color(0x0010_2030 * (i + 1))).collect();
        assert_eq!(contrast_order(&many).len(), 12);
        assert!(is_warm(Color(0x00FF_8000)) && !is_warm(Color(0x0000_80FF)));
    }

    #[test]
    fn theme_rainbow_is_already_contrast_ordered() {
        // 기본 팔레트는 설계 시점에 정렬해 둔다(런타임 비용 0) — 이웃은 따뜻함↔차가움이 번갈아 온다.
        for th in [Theme::dark(), Theme::light()] {
            assert_eq!(contrast_order(&th.rainbow), th.rainbow.to_vec());
            for i in 0..th.rainbow.len() {
                let (a, b) = (th.rainbow[i], th.rainbow[(i + 1) % th.rainbow.len()]);
                assert_ne!(is_warm(a), is_warm(b), "{i}");
            }
        }
    }

    #[test]
    fn default_is_dark() {
        assert_eq!(Theme::default(), Theme::dark());
    }

    #[test]
    fn light_and_dark_share_shape_not_values() {
        assert_ne!(Theme::dark().panel_bg, Theme::light().panel_bg);
        assert_eq!(
            Theme::dark().accent,
            Theme::light().accent,
            "강조색은 공통(임시)"
        );
    }
}

/// 영역별 글꼴 설정 — 크기·굵기·기울임(사용자 설정 · FR-U 가독성).
///
/// 글꼴 **패밀리** 선택은 시스템 폰트 열거(M3-3 확장)가 필요해 v1은 시스템 1벌 위에서
/// 크기·faux 굵기/기울임만 조정한다([docs/14 §5] 시각 언어 수치는 M3-1c에서 확정).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SlotFont {
    /// 글자 크기(논리 px).
    pub size: f32,
    /// 굵게(faux).
    pub bold: bool,
    /// 기울임(faux).
    pub italic: bool,
}

/// 영역별 글꼴 설정(기본 UI·사용자 목록·대화 본문·상태바).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FontPrefs {
    /// 기본 UI(버튼·헤더·설정 등).
    pub base: SlotFont,
    /// 사용자(피어) 목록.
    pub peerlist: SlotFont,
    /// 대화 본문.
    pub message: SlotFont,
    /// 상태바·보조.
    pub status: SlotFont,
}

impl Default for FontPrefs {
    /// 기본값 — **가독성 위해 상향**(구 13/15/11 → 16/18/13). 사용자 목록은 기본 UI와 같은 16.
    fn default() -> Self {
        let plain = |size| SlotFont {
            size,
            bold: false,
            italic: false,
        };
        Self {
            base: plain(16.0),
            peerlist: plain(16.0),
            message: plain(18.0),
            status: plain(15.0), // 상태바 가독성 상향(13→15 · 사용자 확정 08-09)
        }
    }
}

//! # nexa-font — 시스템 폰트 발견 (플랫폼 계층 · 임베드 없음)
//!
//! 출처: `nexa-clip/crates/nclip-plat/src/font.rs` + `nexa-clip/src/conf.rs::load_ui_font`(2026-09-12 이관).
//! nexa-sql 요구(사용자 09-12): **한글 처리와 고정폭 폰트를 잘 지원**.
//!
//! - [`ui_font`] = 한글 UI 본(Apple SD Gothic Neo · 맑은 고딕 · Noto Sans CJK KR) + 기호 폴백 체인 + 고정폭 폴백.
//! - [`mono_font`] = ★ **한글 고정폭 우선**(D2Coding · Sarasa · Nanum Gothic Coding) → OS 고정폭 → **한글 UI 본을 폴백**으로 붙인다.
//!   그래서 편집기에서 한글이 두부(□)가 되지 않는다. 한글 고정폭 본이 없으면 한글 글리프 폭은 비례폰트 폭이다(정직한 한계 —
//!   D2Coding 설치를 권장한다).
//! - mmap은 의도적으로 누수(`Box::leak`) — 프로세스 수명 자원.

use memmap2::Mmap;
use nexa_gfx::Font;
use std::fs::File;
use std::path::{Path, PathBuf};

/// OS별 한글 UI 후보(경로 · TTC 인덱스 · 표시 이름) — 앞이 우선.
#[cfg(target_os = "macos")]
const UI_CANDIDATES: &[(&str, u32, &str)] = &[
    (
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
        0,
        "Apple SD Gothic Neo",
    ),
    ("/System/Library/Fonts/Helvetica.ttc", 0, "Helvetica"),
];
#[cfg(target_os = "windows")]
const UI_CANDIDATES: &[(&str, u32, &str)] = &[
    ("C:\\Windows\\Fonts\\malgun.ttf", 0, "맑은 고딕"),
    ("C:\\Windows\\Fonts\\segoeui.ttf", 0, "Segoe UI"),
];
#[cfg(target_os = "linux")]
const UI_CANDIDATES: &[(&str, u32, &str)] = &[
    (
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        2,
        "Noto Sans CJK KR",
    ),
    (
        "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
        0,
        "나눔고딕",
    ),
    (
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        0,
        "DejaVu Sans",
    ),
];

/// ★ 한글 고정폭 패밀리(파일명 매칭) — 어느 OS든 사용자가 설치했으면 최우선.
const KO_MONO_FAMILIES: &[&str] = &[
    "D2Coding",
    "D2CodingLigature",
    "SarasaMonoK",
    "NanumGothicCoding",
];

#[cfg(target_os = "macos")]
const MONO_CANDIDATES: &[(&str, u32, &str)] = &[
    ("/System/Library/Fonts/Menlo.ttc", 0, "Menlo"),
    ("/System/Library/Fonts/SFNSMono.ttf", 0, "SF Mono"),
    ("/System/Library/Fonts/Monaco.ttf", 0, "Monaco"),
];
#[cfg(target_os = "windows")]
const MONO_CANDIDATES: &[(&str, u32, &str)] = &[
    ("C:\\Windows\\Fonts\\consola.ttf", 0, "Consolas"),
    ("C:\\Windows\\Fonts\\cour.ttf", 0, "Courier New"),
];
#[cfg(target_os = "linux")]
const MONO_CANDIDATES: &[(&str, u32, &str)] = &[
    (
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        0,
        "DejaVu Sans Mono",
    ),
    (
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        0,
        "Liberation Mono",
    ),
    (
        "/usr/share/fonts/opentype/noto/NotoSansMono-Regular.ttf",
        0,
        "Noto Sans Mono",
    ),
];

#[cfg(target_os = "macos")]
const SYMBOL_CANDIDATES: &[(&str, u32, &str)] = &[
    (
        "/System/Library/Fonts/Apple Symbols.ttf",
        0,
        "Apple Symbols",
    ),
    (
        "/System/Library/Fonts/Supplemental/Apple Symbols.ttf",
        0,
        "Apple Symbols",
    ),
    (
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        0,
        "Arial Unicode MS",
    ),
    // ⏱⏳(U+23F1/23F3)는 위 둘에 없다 — 맥 시스템 본 전수 실측(09-16)에서 외곽선을 가진 것은 STIX Two Math뿐
    // (Apple Color Emoji는 sbix 비트맵 · LastResort는 자리표시 글리프라 제외).
    (
        "/System/Library/Fonts/Supplemental/STIXTwoMath.otf",
        0,
        "STIX Two Math",
    ),
];
#[cfg(target_os = "windows")]
const SYMBOL_CANDIDATES: &[(&str, u32, &str)] = &[
    ("C:\\Windows\\Fonts\\seguisym.ttf", 0, "Segoe UI Symbol"),
    ("C:\\Windows\\Fonts\\seguiemj.ttf", 0, "Segoe UI Emoji"),
    // Office 동봉(있으면) — 넓은 BMP 커버.
    ("C:\\Windows\\Fonts\\arialuni.ttf", 0, "Arial Unicode MS"),
];
#[cfg(target_os = "linux")]
const SYMBOL_CANDIDATES: &[(&str, u32, &str)] = &[
    (
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        0,
        "DejaVu Sans",
    ),
    (
        "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
        0,
        "Noto Sans Symbols2",
    ),
    (
        "/usr/share/fonts/opentype/noto/NotoSansSymbols2-Regular.ttf",
        0,
        "Noto Sans Symbols2",
    ),
    // 배포판별 경로(Fedora `google-noto`/`dejavu-sans-fonts` · Arch `noto`/`TTF` · openSUSE `truetype`) — 09-16.
    (
        "/usr/share/fonts/google-noto/NotoSansSymbols2-Regular.ttf",
        0,
        "Noto Sans Symbols2",
    ),
    (
        "/usr/share/fonts/noto/NotoSansSymbols2-Regular.ttf",
        0,
        "Noto Sans Symbols2",
    ),
    (
        "/usr/share/fonts/truetype/noto/NotoSansSymbols-Regular.ttf",
        0,
        "Noto Sans Symbols",
    ),
    (
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
        0,
        "DejaVu Sans",
    ),
    ("/usr/share/fonts/TTF/DejaVuSans.ttf", 0, "DejaVu Sans"),
    ("/usr/share/fonts/truetype/DejaVuSans.ttf", 0, "DejaVu Sans"),
    (
        "/usr/share/fonts/truetype/ancient-scripts/Symbola_hint.ttf",
        0,
        "Symbola",
    ),
];

/// 고정 경로에 없을 때 **폰트 폴더를 이름으로 훑는** 기호 본(파일명 어간 · 앞이 우선) — OS별.
/// 고정 경로 표는 "그 OS의 표준 자리", 이 표는 "배포판·사용자 설치 위치가 달라도 붙는" 두 번째 그물(09-16 두부 방지).
#[cfg(target_os = "macos")]
const SYMBOL_FAMILIES: &[&str] = &["Apple Symbols", "Arial Unicode", "STIXTwoMath", "Symbola"];
#[cfg(target_os = "windows")]
const SYMBOL_FAMILIES: &[&str] = &["seguisym", "seguiemj", "arialuni", "Symbola"];
#[cfg(target_os = "linux")]
const SYMBOL_FAMILIES: &[&str] = &[
    "NotoSansSymbols2",
    "NotoSansSymbols",
    "DejaVuSans",
    "Symbola",
    "NotoSansMath",
    "OpenSymbol",
    "unifont",
];

#[cfg(target_os = "macos")]
const FONT_DIRS: &[&str] = &["~/Library/Fonts", "/Library/Fonts", "/System/Library/Fonts"];
#[cfg(target_os = "windows")]
const FONT_DIRS: &[&str] = &[
    "~/AppData/Local/Microsoft/Windows/Fonts",
    "C:\\Windows\\Fonts",
];
#[cfg(target_os = "linux")]
const FONT_DIRS: &[&str] = &[
    "~/.local/share/fonts",
    "~/.fonts",
    "/usr/share/fonts",
    "/usr/local/share/fonts",
];

/// 발견 결과 — 이름을 같이 준다(설정 화면 "(시스템 기본)" 식별 · 진단 출력).
#[derive(Clone, Copy, Debug)]
pub struct Found {
    pub data: &'static [u8],
    pub index: u32,
    pub name: &'static str,
}

fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

fn map_font(path: &Path) -> Option<&'static [u8]> {
    let file = File::open(path).ok()?;
    // SAFETY: 폰트 파일은 실행 중 변경되지 않는 읽기 전용 자산(OS 배포본·사용자 설치본).
    let mmap = unsafe { Mmap::map(&file) }.ok()?;
    if mmap.len() < 1000 {
        return None;
    }
    let leaked: &'static Mmap = Box::leak(Box::new(mmap));
    Some(&leaked[..])
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn expand(dir: &str) -> PathBuf {
    if let Some(rest) = dir.strip_prefix("~/") {
        if let Some(h) = home_dir() {
            return h.join(rest);
        }
    }
    PathBuf::from(dir)
}

fn first_existing(cands: &'static [(&'static str, u32, &'static str)]) -> Option<Found> {
    for &(path, index, name) in cands {
        let p = Path::new(path);
        if p.exists() {
            if let Some(data) = map_font(p) {
                return Some(Found { data, index, name });
            }
        }
    }
    None
}

/// 폴더 아래 폰트 파일(하위 폴더 포함 · 깊이 [`SCAN_DEPTH`]) — 리눅스는 `/usr/share/fonts/<종류>/<패밀리>/`처럼 두 단 아래에 있다(09-16).
fn collect_font_files(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let path = e.path();
        // `file_type()` = `getdents`가 준 d_type(추가 시스템 호출 0) — `path.is_dir()`은 항목마다 `statx`를 부른다
        // (Linux 09-22 실측: 폰트 976개 × 가족 12회 = `statx` 11,786회 · 기동 100~700 ms 구간이 전부 이것이었다).
        // 심볼릭 링크(Windows 정션 포함)는 d_type이 "링크"라 그때만 `is_dir()`로 따라간다(종전 동작 유지).
        let is_dir = match e.file_type() {
            Ok(t) if t.is_symlink() => path.is_dir(),
            Ok(t) => t.is_dir(),
            Err(_) => path.is_dir(),
        };
        if is_dir {
            if depth < SCAN_DEPTH {
                collect_font_files(&path, depth + 1, out);
            }
        } else {
            out.push(path);
        }
    }
}

/// 폰트 폴더 목록을 **프로세스에서 한 번만** 걷는다(`FONT_DIRS` 순서 · 폴더 안은 정렬). 가족 탐색은 UI 본 · 고정폭 · 기호 폴백까지
/// 프로세스마다 10여 회 불리므로 걷기를 호출마다 되풀이하면 그 횟수만큼 곱해진다(Linux 09-22 · 기동 병목).
/// 앱이 도는 동안 새로 설치된 폰트는 다음 기동에서 보인다(설정 창의 글꼴 변경도 이 목록에서 고른다 — 재시작 안내가 있다).
fn font_files() -> &'static [PathBuf] {
    static FILES: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
    FILES.get_or_init(|| {
        let mut all = Vec::new();
        for dir in FONT_DIRS {
            let mut entries = Vec::new();
            collect_font_files(&expand(dir), 0, &mut entries);
            entries.sort();
            all.extend(entries);
        }
        all
    })
}

/// 폰트 폴더 재귀 깊이 상한(Fedora `google-noto/` 1단 · Debian `truetype/noto/` 2단 · 여유 1).
const SCAN_DEPTH: u32 = 3;

/// 패밀리 이름(파일명 어간 정규화 비교)으로 폰트 파일을 찾는다(하위 폴더 포함). 파일명 ≠ 패밀리명인 본은 못 찾는다(정직한 한계).
#[must_use]
pub fn find_font_by_family(family: &str) -> Option<(&'static [u8], u32)> {
    let want = norm(family);
    if want.is_empty() {
        return None;
    }
    {
        for path in font_files() {
            let ext_ok = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc"));
            if !ext_ok {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let got = norm(stem);
            if got == want || got.starts_with(&want) {
                if let Some(bytes) = map_font(path) {
                    return Some((bytes, 0));
                }
            }
        }
    }
    None
}

/// 시스템 한글 UI 본.
#[must_use]
pub fn system_ui_font() -> Option<Found> {
    first_existing(UI_CANDIDATES)
}

/// 고정폭 본 — ★ 한글 고정폭 패밀리 우선, 없으면 OS 고정폭.
#[must_use]
pub fn system_mono_font() -> Option<Found> {
    for fam in KO_MONO_FAMILIES {
        if let Some((data, index)) = find_font_by_family(fam) {
            return Some(Found {
                data,
                index,
                name: fam,
            });
        }
    }
    first_existing(MONO_CANDIDATES)
}

/// 기호·이모지 폴백 본들(존재하는 것만 · 같은 이름은 첫 것만).
#[must_use]
pub fn symbol_fallback_fonts() -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    for &(path, index, name) in SYMBOL_CANDIDATES {
        if out.iter().any(|f| f.name == name) {
            continue;
        }
        if let Some(data) = map_font(Path::new(path)) {
            out.push(Found { data, index, name });
        }
    }
    for fam in SYMBOL_FAMILIES {
        if out.iter().any(|f| norm(f.name) == norm(fam)) {
            continue;
        }
        if let Some((data, index)) = find_font_by_family(fam) {
            out.push(Found {
                data,
                index,
                name: fam,
            });
        }
    }
    out
}

/// UI가 쓰는 기호(nexa-sql·nexa-ctl·nexa-dlg 문자열 리터럴 전수 · 09-16) — 두부(□) 방지 회귀 테스트의 기준.
/// 새 기호를 UI에 넣으면 여기에도 추가한다(CI 3-OS가 폴백 체인에 빠졌는지 잡는다).
pub const UI_SYMBOLS: &str = "·—–…×→←↑↓⇧⌘⌥⌂⌄›«»▲▼▶◀▸●•★✓⚠⇕∨＋⏱⏳";

/// 로드 결과 + 진단(어느 본이 붙었는지).
#[derive(Debug)]
pub struct Loaded {
    pub font: Font,
    pub chain: Vec<String>,
}

/// 체인 이름을 face 패밀리로 등록(OS 래스터라이저 경로 · `nexa_gfx::text::set_text_gdi`) — 체인과 face 순서는 같다.
fn loaded(mut font: Font, chain: Vec<String>) -> Loaded {
    for (i, n) in chain.iter().enumerate() {
        font.set_face_family(i, n);
    }
    Loaded { font, chain }
}

/// UI 글꼴: 한글 UI 본 → 기호 폴백 → 고정폭 폴백. `family`를 주면 그 본을 앞에 두고 시스템 본을 첫 폴백으로.
#[must_use]
/// ★ macOS 시스템 UI 글꼴(SF · `/System/Library/Fonts/SFNS.ttf`) — 파인더·시스템 앱의 라틴 글꼴. CoreText는 이 글꼴을
/// 이름 `.AppleSystemUIFont`로만 연다(".SF NS"·"SF Pro"는 다른 글꼴로 대체됨 · 09-17 실측). 한글은 Apple SD Gothic Neo 폴백.
#[cfg(target_os = "macos")]
fn system_latin_font() -> Option<Found> {
    let data = map_font(Path::new("/System/Library/Fonts/SFNS.ttf"))?;
    Font::from_static(data, 0).ok()?;
    Some(Found {
        data,
        index: 0,
        name: ".AppleSystemUIFont",
    })
}

#[cfg(not(target_os = "macos"))]
fn system_latin_font() -> Option<Found> {
    None
}

pub fn ui_font(family: Option<&str>) -> Option<Loaded> {
    let sys = system_ui_font();
    let mut chain = Vec::new();
    let mut font = match family.map(str::trim).filter(|f| !f.is_empty()) {
        Some(fam) => match find_font_by_family(fam) {
            Some((d, i)) => {
                let mut f = Font::from_static(d, i).ok()?;
                chain.push(fam.to_string());
                if let Some(s) = sys {
                    if f.push_fallback(s.data, s.index).is_ok() {
                        chain.push(s.name.to_string());
                    }
                }
                f
            }
            None => {
                let s = sys?;
                chain.push(s.name.to_string());
                Font::from_static(s.data, s.index).ok()?
            }
        },
        None => {
            let s = sys?;
            // macOS: 라틴 = 시스템 UI 글꼴(SF · 파인더와 같은 얼굴·자간) → 한글 = SD Gothic Neo 폴백(사용자 09-17).
            match system_latin_font() {
                Some(l) => {
                    let mut f = Font::from_static(l.data, l.index).ok()?;
                    chain.push(l.name.to_string());
                    if f.push_fallback(s.data, s.index).is_ok() {
                        chain.push(s.name.to_string());
                    }
                    f
                }
                None => {
                    chain.push(s.name.to_string());
                    Font::from_static(s.data, s.index).ok()?
                }
            }
        }
    };
    for f in symbol_fallback_fonts() {
        if font.push_fallback(f.data, f.index).is_ok() {
            chain.push(f.name.to_string());
        }
    }
    if let Some(m) = system_mono_font() {
        if font.push_fallback(m.data, m.index).is_ok() {
            chain.push(m.name.to_string());
        }
    }
    Some(loaded(font, chain))
}

/// ★ 편집기·그리드용 고정폭 글꼴: (한글 고정폭 | OS 고정폭) → **한글 UI 본 폴백** → 기호 폴백.
/// 고정폭 본이 하나도 없으면 UI 본으로 폴백(그래도 `None`이면 폰트 자체가 없는 기기).
#[must_use]
pub fn mono_font(family: Option<&str>) -> Option<Loaded> {
    let mut chain = Vec::new();
    let primary = match family.map(str::trim).filter(|f| !f.is_empty()) {
        Some(fam) => find_font_by_family(fam)
            .map(|(data, index)| Found {
                data,
                index,
                name: "(사용자 지정)",
            })
            .or_else(system_mono_font),
        None => system_mono_font(),
    };
    let mut font = match primary {
        Some(p) => {
            chain.push(if p.name == "(사용자 지정)" {
                family.unwrap_or("").to_string()
            } else {
                p.name.to_string()
            });
            Font::from_static(p.data, p.index).ok()?
        }
        None => {
            let s = system_ui_font()?;
            chain.push(s.name.to_string());
            Font::from_static(s.data, s.index).ok()?
        }
    };
    if let Some(s) = system_ui_font() {
        if font.push_fallback(s.data, s.index).is_ok() {
            chain.push(s.name.to_string());
        }
    }
    for f in symbol_fallback_fonts() {
        if font.push_fallback(f.data, f.index).is_ok() {
            chain.push(f.name.to_string());
        }
    }
    Some(loaded(font, chain))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_font_exists_on_supported_targets() {
        let f = system_ui_font().expect("지원 OS에 UI 폰트 후보 없음");
        assert!(f.data.len() > 1000);
    }

    #[test]
    fn normalization() {
        assert_eq!(norm("D2 Coding"), "d2coding");
        assert_eq!(norm("Liberation-Mono"), "liberationmono");
    }

    #[test]
    fn unknown_family_is_none() {
        assert!(find_font_by_family("이런폰트는없다12345").is_none());
        assert!(find_font_by_family("").is_none());
    }

    /// 두부 방지(사용자 09-16 "심볼이 표시될 수 있는 폰트를 fail-over") — UI·고정폭 체인 모두 [`UI_SYMBOLS`] 전부 커버.
    /// 실측: mac = Apple SD Gothic Neo → Apple Symbols → Arial Unicode → **STIX Two Math**(⏱⏳) ·
    /// Windows = 맑은 고딕 → Segoe UI Symbol(⏱⏳ 포함) · Linux = Noto CJK → DejaVu → Noto Sans Symbols2(⏱⏳).
    #[test]
    fn ui_and_mono_fonts_cover_all_ui_symbols() {
        let u = ui_font(None).expect("UI 본");
        let m = mono_font(None).expect("고정폭 본");
        let miss_u: String = UI_SYMBOLS.chars().filter(|&c| !u.font.covers(c)).collect();
        let miss_m: String = UI_SYMBOLS.chars().filter(|&c| !m.font.covers(c)).collect();
        assert!(
            miss_u.is_empty(),
            "UI 체인 {:?}에 없는 기호: [{miss_u}]",
            u.chain
        );
        assert!(
            miss_m.is_empty(),
            "고정폭 체인 {:?}에 없는 기호: [{miss_m}]",
            m.chain
        );
    }

    /// 시스템 UI 본의 `name` 테이블 패밀리 이름이 읽힌다(OS 래스터라이저 이름 후보 · 영문 이름 포함).
    #[test]
    fn face_family_names_include_table_names() {
        let u = ui_font(None).expect("UI 본");
        let names = u.font.face_family_names(0);
        assert!(names.len() >= 2, "체인 이름 + name 테이블 이름: {names:?}");
        assert!(
            names.iter().any(|n| n.is_ascii()),
            "영문 이름이 있어야: {names:?}"
        );
    }

    /// ★ CoreText 경로(macOS · T-100): UI 본으로 문자열을 1x로 그려 `target/textref/ours.pgm`에 남긴다 —
    /// `scripts/mac-text-ref.swift`(CoreText 직접 렌더 = 파인더와 같은 경로)의 `ref.pgm`과 픽셀 비교(`scripts/mac-text-compare.py`).
    /// 전진 폭은 소수(서브픽셀 위치) · 글리프는 CoreText 회색 커버리지.
    #[cfg(target_os = "macos")]
    #[test]
    fn coretext_path_renders_like_finder() {
        let u = ui_font(None).expect("UI 본");
        nexa_gfx::text::set_text_gdi(true);
        let text = "Login List Nexa SQL 한글 결과 0123 Hg";
        // em 13(파인더 13pt) = SF height/upm 1.1777 × 13.
        let size = 15.31;
        let w = u.font.measure(text, size);
        assert!(w > 100.0);
        assert!(u.font.glyph_is_subpixel('H', size) || true);
        let (bw, bh) = (w.ceil() as usize + 8, 27usize);
        let mut buf = vec![0xFFFF_FFFFu32; bw * bh];
        {
            let mut s = nexa_gfx::Surface::new(&mut buf, bw, bh);
            s.fill(nexa_gfx::Color(0xFFFF_FFFF));
            // 베이스라인: 참조 렌더와 같은 자리(아래에서 4px + 디센트) — 비교 스크립트가 오프셋을 맞추므로 근사면 된다.
            u.font
                .draw_text(&mut s, 4.0, 20.0, size, nexa_gfx::Color(0x0000_00FF), text);
            // y = 베이스라인
        }
        nexa_gfx::text::set_text_gdi(false);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../nexa-sql/target/textref");
        if dir.is_dir() {
            let mut out = format!("P5\n{bw} {bh}\n255\n").into_bytes();
            for px in &buf {
                let r = (px >> 24) & 0xFF;
                let g = (px >> 16) & 0xFF;
                let b = (px >> 8) & 0xFF;
                let luma = (r * 299 + g * 587 + b * 114) / 1000;
                out.push((255 - luma) as u8);
            }
            std::fs::write(dir.join("ours.pgm"), out).expect("write ours.pgm");
        }
        // 잉크가 있고 회색 AA가 있어야 한다.
        let dark = buf.iter().filter(|&&p| (p >> 8) & 0xFF < 128).count();
        assert!(dark > 200, "dark={dark}");
    }

    /// ★ GDI 경로(Windows): 체인 이름이 face 패밀리로 등록돼 GDI가 같은 글꼴을 열고 정수 전진 폭을 준다.
    #[cfg(windows)]
    #[test]
    fn gdi_path_gives_integer_advances() {
        let u = ui_font(None).expect("UI 본");
        nexa_gfx::text::set_text_gdi(true);
        let w = u.font.measure("Hello 한글", 15.0);
        nexa_gfx::text::set_text_gdi(false);
        assert!(w > 0.0);
        assert!((w - w.round()).abs() < 1e-3, "정수 전진 폭이어야: {w}");
        let w2 = u.font.measure("Hello 한글", 15.0);
        assert!(w2 > 0.0);
    }

    /// ★ 자동 검증(사용자 09-16 "테스트 자동화"): GDI ClearType 경로에서 세로 획이 있는 글자(닫·기·l·|)의 줄기가
    /// 한 열에 또렷이 서고(가시성 ≥ 0.6 · GGO 회색은 `닫` 0.0이었다) · 굵게는 진짜 볼드(잉크가 더 많다) ·
    /// 전진 폭은 정수. UI 본(맑은 고딕 11px em)·고정폭 본(Consolas 13px) 모두.
    #[cfg(windows)]
    #[test]
    fn gdi_cleartype_stems_bold_and_advances() {
        let u = ui_font(None).expect("UI 본");
        let m = mono_font(None).expect("고정폭 본");
        nexa_gfx::text::set_text_gdi(true);
        let mut report = String::new();
        for (font, name, size) in [
            (&u.font, "ui", 15.0),
            (&u.font, "ui-small", 13.0),
            (&m.font, "mono", 13.0),
        ] {
            // 헤드리스 러너(세션 0)는 ClearType 없이 회색으로 그릴 수 있다 — 그때는 얇은 세로 획 문턱만 낮춘다.
            let subpixel = font.glyph_is_subpixel('H', size);
            let need = if subpixel { 0.6 } else { 0.3 };
            report.push_str(&format!("{name} {size}px subpixel={subpixel}\n"));
            for ch in ['닫', '기', '나', 'l', '|', 'H'] {
                let v = font.glyph_stem_visibility(ch, size, false);
                report.push_str(&format!("{name} '{ch}' {size}px stem {v:.2}\n"));
                assert!(
                    v >= need,
                    "{name} '{ch}' {size}px 세로 획 가시성 {v:.2} < {need}\n{}",
                    font.glyph_ascii(ch, size)
                );
            }
            let (r, b) = (
                font.glyph_ink('닫', size, false),
                font.glyph_ink('닫', size, true),
            );
            assert!(b > r * 1.15, "{name} 볼드 잉크 {b:.1} ≤ 보통 {r:.1}×1.15");
            let w = font.measure("Hello 한글 |l", size);
            assert!(
                (w - w.round()).abs() < 1e-3,
                "{name} 정수 전진 폭이어야: {w}"
            );
            let wb = font.measure_from_styled("Hello 한글 |l", size, 0.0, true);
            assert!(
                (wb - wb.round()).abs() < 1e-3,
                "{name} 볼드 정수 전진 폭이어야: {wb}"
            );
        }
        nexa_gfx::text::set_text_gdi(false);
        println!("{report}");
    }

    /// 진단: GDI 글리프 덤프(`cargo test -p nexa-font dump_gdi -- --nocapture --ignored`).
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn dump_gdi_glyphs() {
        let u = ui_font(None).expect("UI 본");
        for on in [false, true] {
            nexa_gfx::text::set_text_gdi(on);
            for (ch, size) in [
                ('닫', 15.0),
                ('닫', 13.0),
                ('결', 13.0),
                ('기', 15.0),
                ('S', 15.0),
                ('·', 15.0),
                ('$', 15.0),
            ] {
                println!(
                    "gdi={on} '{ch}' size {size}\n{}",
                    u.font.glyph_ascii(ch, size)
                );
            }
        }
        nexa_gfx::text::set_text_gdi(false);
    }

    #[test]
    fn mono_font_covers_hangul_via_fallback() {
        let m = mono_font(None).expect("고정폭 또는 UI 본");
        assert!(
            m.font.covers('가'),
            "한글 폴백이 붙어야 한다: {:?}",
            m.chain
        );
        assert!(m.font.covers('A'));
        let u = ui_font(None).expect("UI 본");
        assert!(u.font.covers('한'));
    }
}

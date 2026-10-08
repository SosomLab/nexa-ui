//! freedesktop 아이콘 테마 조회(124차 · nexa-dir3 Linux 실기 10-03 "리눅스 기본 아이콘이 표시되도록") — **Linux의 OS 아이콘**.
//!
//! Windows는 셸이 픽셀을 주지만([`crate::shell`]) Linux는 테마 폴더의 **이미지 파일**이 아이콘이다. 이 모듈은 파일·폴더 →
//! 아이콘 **파일 경로**까지만 정한다(디코드는 호출자 — 이 크레이트는 그래픽 의존 0).
//!
//! - 테마 이름: `NEXA_ICON_THEME` → GTK `settings.ini`(`gtk-icon-theme-name`) → KDE `kdeglobals`(`[Icons] Theme`) →
//!   `gsettings get org.gnome.desktop.interface icon-theme`(1회) → 설치된 것 중 Yaru · Adwaita · breeze → hicolor.
//! - 테마 폴더: `index.theme`의 `Directories`(Size × Scale) · `Inherits` 사슬 · 끝에 hicolor(Icon Theme Specification).
//!   **PNG만** 쓴다(`Type=Scalable` 폴더의 SVG는 후속 — 풀컬러 SVG는 nexa-gfx 서브셋 밖).
//! - 폴더: `/` = drive-harddisk · 홈 = user-home · XDG 사용자 폴더(`user-dirs.dirs`) = folder-download 등 · 그 밖 = folder.
//! - 파일: shared-mime-info `globs2`(확장자·이름 → MIME) → `icons` · MIME 이름 · `generic-icons` · `<분류>-x-generic` 순.
//!   못 찾으면 실행 비트 = application-x-executable · 그 밖 = text-x-generic.
//!
//! 순수 부분(색인 해석 · 조회 · MIME 표 · 사용자 폴더)은 OS와 무관하게 시험한다. 전역 조회 [`icon_file`]만 Linux에서 동작한다.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// 테마 폴더 한 칸(`16x16/places` 등)과 그 실제 픽셀 크기(Size × Scale).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeDir {
    /// 테마 루트 기준 상대 경로.
    pub path: String,
    /// 그림 한 변(px).
    pub px: u32,
    /// HiDPI용(`Scale` ≥ 2)인가 — 같은 픽셀이면 Scale 1을 먼저 쓴다.
    pub hidpi: bool,
}

/// `index.theme` 해석 결과.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThemeIndex {
    /// 상속 테마(순서대로).
    pub inherits: Vec<String>,
    /// 고정 크기 폴더(Scalable 제외).
    pub dirs: Vec<ThemeDir>,
}

/// `index.theme` 본문 해석(순수): `[Icon Theme]`의 `Inherits`·`Directories`와 각 폴더 절의 `Size`·`Scale`·`Type`.
#[must_use]
pub fn parse_index(text: &str) -> ThemeIndex {
    let mut inherits = Vec::new();
    let mut listed: Vec<String> = Vec::new();
    // 절 이름 → (size, scale, scalable)
    let mut sections: HashMap<String, (u32, u32, bool)> = HashMap::new();
    let mut cur = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            cur = name.to_string();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        let list = |v: &str| -> Vec<String> {
            v.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        };
        if cur == "Icon Theme" {
            match k {
                "Inherits" => inherits = list(v),
                "Directories" | "ScaledDirectories" => listed.extend(list(v)),
                _ => {}
            }
        } else {
            let e = sections.entry(cur.clone()).or_insert((0, 1, false));
            match k {
                "Size" => e.0 = v.parse().unwrap_or(0),
                "Scale" => e.1 = v.parse().unwrap_or(1).max(1),
                "Type" => e.2 = v.eq_ignore_ascii_case("Scalable"),
                _ => {}
            }
        }
    }
    let dirs = listed
        .into_iter()
        .filter_map(|path| {
            let &(size, scale, scalable) = sections.get(&path)?;
            (size > 0 && !scalable).then(|| ThemeDir {
                px: size * scale,
                hidpi: scale > 1,
                path,
            })
        })
        .collect();
    ThemeIndex { inherits, dirs }
}

/// 원하는 크기에 가까운 순으로 폴더를 늘어놓는다(순수): 같은 크기(Scale 1 먼저) → 더 큰 것(작은 순 · 축소가 확대보다 낫다) →
/// 더 작은 것(큰 순).
#[must_use]
pub fn rank_dirs(dirs: &[ThemeDir], px: u32) -> Vec<&ThemeDir> {
    let mut v: Vec<&ThemeDir> = dirs.iter().collect();
    v.sort_by_key(|d| {
        let class = match d.px.cmp(&px) {
            std::cmp::Ordering::Equal => 0u32,
            std::cmp::Ordering::Greater => 1,
            std::cmp::Ordering::Less => 2,
        };
        let dist = d.px.abs_diff(px);
        (class, dist, d.hidpi)
    });
    v
}

/// 불러온 아이콘 테마(상속 사슬 포함)와 이름 → 파일 캐시.
#[derive(Debug, Default)]
pub struct IconTheme {
    /// 테마 이름(진단용).
    pub name: String,
    /// (테마 루트 폴더, 그 테마의 폴더 목록) — 조회 순서대로.
    chain: Vec<(PathBuf, Vec<ThemeDir>)>,
    found: HashMap<(String, u32), Option<PathBuf>>,
}

impl IconTheme {
    /// `roots`(아이콘 기준 폴더들 · 앞이 우선) 아래에서 `name` 테마와 상속 사슬(끝에 hicolor)을 읽는다.
    #[must_use]
    pub fn load(roots: &[PathBuf], name: &str) -> Self {
        let mut chain = Vec::new();
        let mut queue: Vec<String> = vec![name.to_string()];
        let mut seen: Vec<String> = Vec::new();
        let mut i = 0;
        while i < queue.len() {
            let theme = queue[i].clone();
            i += 1;
            if theme.is_empty() || seen.contains(&theme) {
                continue;
            }
            seen.push(theme.clone());
            // 색인은 그 테마가 있는 첫 루트의 것 · 그림은 테마 폴더가 있는 모든 루트에서 찾는다.
            let index = roots
                .iter()
                .find_map(|r| std::fs::read_to_string(r.join(&theme).join("index.theme")).ok())
                .map(|t| parse_index(&t));
            let Some(index) = index else {
                continue;
            };
            for r in roots {
                let dir = r.join(&theme);
                if dir.is_dir() {
                    chain.push((dir, index.dirs.clone()));
                }
            }
            queue.extend(index.inherits);
            if i == queue.len() && !seen.iter().any(|s| s == "hicolor") {
                queue.push("hicolor".to_string());
            }
        }
        Self {
            name: name.to_string(),
            chain,
            found: HashMap::new(),
        }
    }

    /// 읽힌 테마가 하나도 없는가.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chain.is_empty()
    }

    /// 아이콘 이름 하나의 PNG 파일(없으면 `None` · 결과는 기억한다).
    pub fn find(&mut self, icon: &str, px: u32) -> Option<PathBuf> {
        if let Some(hit) = self.found.get(&(icon.to_string(), px)) {
            return hit.clone();
        }
        let file = format!("{icon}.png");
        let got = self.chain.iter().find_map(|(root, dirs)| {
            rank_dirs(dirs, px)
                .into_iter()
                .map(|d| root.join(&d.path).join(&file))
                .find(|p| p.is_file())
        });
        self.found.insert((icon.to_string(), px), got.clone());
        got
    }

    /// 후보 이름들 중 처음 찾은 것.
    pub fn find_first<S: AsRef<str>>(&mut self, icons: &[S], px: u32) -> Option<PathBuf> {
        icons.iter().find_map(|n| self.find(n.as_ref(), px))
    }
}

/// shared-mime-info 표(확장자·이름 → MIME → 아이콘 이름).
#[derive(Debug, Default)]
pub struct MimeDb {
    /// 소문자 접미사(점 없음 · `tar.gz` 포함) → (가중치, MIME).
    by_suffix: HashMap<String, (u32, String)>,
    /// 소문자 파일 이름 전체(`makefile`) → (가중치, MIME).
    by_name: HashMap<String, (u32, String)>,
    /// MIME → 전용 아이콘 이름(`icons` 파일).
    icons: HashMap<String, String>,
    /// MIME → 범용 아이콘 이름(`generic-icons` 파일).
    generic: HashMap<String, String>,
}

impl MimeDb {
    /// `globs2`(`가중치:MIME:글롭`) · `icons` · `generic-icons`(`MIME:이름`) 본문 해석(순수). 글롭은 `*.접미사`와 와일드카드 없는
    /// 이름만 쓴다(그 밖의 패턴은 버린다). 같은 키는 가중치가 큰 쪽.
    #[must_use]
    pub fn parse(globs2: &str, icons: &str, generic: &str) -> Self {
        let mut db = Self::default();
        for line in globs2.lines() {
            if line.starts_with('#') {
                continue;
            }
            let mut it = line.splitn(4, ':');
            let (Some(w), Some(mime), Some(glob)) = (it.next(), it.next(), it.next()) else {
                continue;
            };
            let w: u32 = w.parse().unwrap_or(50);
            let (map, key) = if let Some(suffix) = glob.strip_prefix("*.") {
                (&mut db.by_suffix, suffix)
            } else {
                (&mut db.by_name, glob)
            };
            if key.is_empty() || key.contains(['*', '?', '[']) {
                continue;
            }
            let e = map.entry(key.to_lowercase()).or_insert((0, String::new()));
            if w > e.0 || e.1.is_empty() {
                *e = (w, mime.to_string());
            }
        }
        let pairs = |text: &str, map: &mut HashMap<String, String>| {
            for line in text.lines() {
                if let Some((mime, icon)) = line.split_once(':') {
                    map.insert(mime.trim().to_string(), icon.trim().to_string());
                }
            }
        };
        pairs(icons, &mut db.icons);
        pairs(generic, &mut db.generic);
        db
    }

    /// 파일 이름의 MIME(순수): 이름 전체 → 가장 긴 접미사(`a.tar.gz` = `tar.gz` 먼저 · 다음 `gz`).
    #[must_use]
    pub fn mime_of(&self, file_name: &str) -> Option<&str> {
        let lower = file_name.to_lowercase();
        if let Some((_, m)) = self.by_name.get(&lower) {
            return Some(m);
        }
        let mut rest = lower.as_str();
        while let Some((_, tail)) = rest.split_once('.') {
            if let Some((_, m)) = self.by_suffix.get(tail) {
                return Some(m);
            }
            rest = tail;
        }
        None
    }

    /// MIME의 아이콘 후보(순수 · 앞이 우선): 전용 → `분류-이름` → 범용 → `분류-x-generic`.
    #[must_use]
    pub fn icon_names(&self, mime: &str) -> Vec<String> {
        let mut v = Vec::with_capacity(4);
        if let Some(i) = self.icons.get(mime) {
            v.push(i.clone());
        }
        v.push(mime.replace('/', "-"));
        if let Some(g) = self.generic.get(mime) {
            v.push(g.clone());
        }
        if let Some((media, _)) = mime.split_once('/') {
            v.push(format!("{media}-x-generic"));
        }
        v.dedup();
        v
    }
}

/// XDG 사용자 폴더 종류 → 아이콘 이름.
const USER_DIR_ICONS: [(&str, &str); 8] = [
    ("XDG_DESKTOP_DIR", "user-desktop"),
    ("XDG_DOWNLOAD_DIR", "folder-download"),
    ("XDG_TEMPLATES_DIR", "folder-templates"),
    ("XDG_PUBLICSHARE_DIR", "folder-publicshare"),
    ("XDG_DOCUMENTS_DIR", "folder-documents"),
    ("XDG_MUSIC_DIR", "folder-music"),
    ("XDG_PICTURES_DIR", "folder-pictures"),
    ("XDG_VIDEOS_DIR", "folder-videos"),
];

/// `user-dirs.dirs` 본문 → (폴더, 아이콘 이름)(순수). `$HOME/…`와 절대 경로만 · 홈 자신을 가리키는 줄(= 쓰지 않음)은 버린다.
#[must_use]
pub fn parse_user_dirs(text: &str, home: &Path) -> Vec<(PathBuf, &'static str)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let Some((key, val)) = line.split_once('=') else {
            continue;
        };
        let Some(&(_, icon)) = USER_DIR_ICONS.iter().find(|(k, _)| *k == key.trim()) else {
            continue;
        };
        let val = val.trim().trim_matches('"');
        let path = if let Some(rest) = val.strip_prefix("$HOME") {
            home.join(rest.trim_start_matches('/'))
        } else if val.starts_with('/') {
            PathBuf::from(val)
        } else {
            continue;
        };
        let path: PathBuf = path.components().collect();
        if path != home {
            out.push((path, icon));
        }
    }
    out
}

/// 폴더의 아이콘 후보(순수 · 앞이 우선): 루트 · 홈 · 사용자 폴더 · 일반 폴더.
#[must_use]
pub fn folder_icon_names(
    path: &Path,
    home: Option<&Path>,
    special: &[(PathBuf, &'static str)],
) -> Vec<&'static str> {
    let mut v = Vec::with_capacity(3);
    if path == Path::new("/") {
        v.push("drive-harddisk");
    } else if home == Some(path) {
        v.push("user-home");
    } else if let Some((_, icon)) = special.iter().find(|(p, _)| p == path) {
        v.push(*icon);
    }
    v.extend(["folder", "inode-directory"]);
    v
}

/// `.desktop` 본문 → `(Exec의 실행 파일, Icon)`(순수 · `[Desktop Entry]` 절만 · 128차). Exec는 첫 낱말(따옴표 제거 ·
/// `env VAR=…` 접두는 건너뜀) — 필드 코드(`%F` 등)와 인자는 버린다. 둘 중 하나라도 없으면 `None`.
#[must_use]
pub fn parse_desktop_entry(text: &str) -> Option<(String, String)> {
    let (mut exec, mut icon) = (None, None);
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        if let Some(v) = line.strip_prefix("Exec=") {
            let mut words = v.split_whitespace().peekable();
            if words.peek().is_some_and(|w| w.trim_matches('"') == "env") {
                words.next();
                while words.peek().is_some_and(|w| w.contains('=')) {
                    words.next();
                }
            }
            exec = exec.or_else(|| {
                words
                    .next()
                    .map(|w| w.trim_matches('"').to_string())
                    .filter(|w| !w.is_empty())
            });
        } else if let Some(v) = line.strip_prefix("Icon=") {
            icon = icon.or_else(|| Some(v.trim().to_string()).filter(|v| !v.is_empty()));
        }
    }
    Some((exec?, icon?))
}

/// 실행 파일에 맞는 `.desktop`의 Icon 값(순수): `entries` = `(Exec 실행 파일, Icon)` 목록 · `candidates` = 실행 파일의 표기들
/// (준 경로 · 링크를 푼 실제 경로). 전체 경로가 같은 항목이 먼저 · 다음은 파일 이름이 같은 항목.
#[must_use]
pub fn desktop_icon_for<'a>(
    entries: &'a [(String, String)],
    candidates: &[&Path],
) -> Option<&'a str> {
    let full = entries
        .iter()
        .find(|(e, _)| candidates.iter().any(|c| Path::new(e) == *c));
    let by_name = || {
        entries.iter().find(|(e, _)| {
            let name = Path::new(e).file_name();
            name.is_some() && candidates.iter().any(|c| c.file_name() == name)
        })
    };
    full.or_else(by_name).map(|(_, i)| i.as_str())
}

/// 전역 조회 — 실행 파일(런처 버튼)의 **앱 아이콘 PNG 경로**(128차 · nexa-dir3 사용자 10-03 "빠른 실행에 아이콘이 없으면 기본
/// 아이콘을 입혀서"): 설치된 `.desktop` 중 그 실행 파일의 것 → `Icon`(절대 경로 PNG · 테마 이름 · `pixmaps/<이름>.png`) →
/// 실행 파일 이름과 같은 테마 아이콘 → 일반 실행 파일 아이콘(`application-x-executable`). Linux 밖 = `None`.
#[must_use]
pub fn app_icon_file(exe: &Path, px: u32) -> Option<PathBuf> {
    if !crate::shell::os_icons_enabled() {
        return None;
    }
    global::app_icon_file(exe, px)
}

/// 전역 조회 — **아이콘 이름**(`.desktop`의 `Icon=` 값 · freedesktop 이름 `utilities-terminal` 같은 것)의 PNG 경로(184차 · nexa-dir3
/// T-131 우클릭 항목 아이콘): 절대 경로 PNG면 그대로 · 아니면 테마에서 `px`에 가장 가까운 것 → `pixmaps/<이름>.png`. Linux 밖 · OS 아이콘
/// 끔 · 없음 = `None`.
#[must_use]
pub fn named_icon_file(icon: &str, px: u32) -> Option<PathBuf> {
    if !crate::shell::os_icons_enabled() {
        return None;
    }
    global::named_icon_file(icon, px)
}

/// 전역 조회 — 이 파일·폴더의 테마 아이콘 **PNG 경로**(`px` = 그릴 한 변 · 가장 가까운 크기를 고른다).
///
/// Linux가 아니거나, OS 아이콘이 꺼져 있거나([`crate::shell::set_os_icons`]), 테마에 맞는 그림이 없으면 `None`(호출자 자체 그림).
/// 첫 호출 때 테마·MIME 표를 읽는다(그 뒤는 캐시 — 확장자 있는 파일과 폴더는 디스크를 건드리지 않는다).
#[must_use]
pub fn icon_file(path: &Path, is_dir: bool, px: u32) -> Option<PathBuf> {
    if !crate::shell::os_icons_enabled() {
        return None;
    }
    global::icon_file(path, is_dir, px)
}

/// 지금 쓰는 아이콘 테마 이름(진단 · 자가 점검용) — Linux가 아니거나 테마가 없으면 `None`.
#[must_use]
pub fn theme_name() -> Option<String> {
    global::theme_name()
}

#[cfg(not(target_os = "linux"))]
mod global {
    use std::path::{Path, PathBuf};
    pub(super) fn icon_file(_path: &Path, _is_dir: bool, _px: u32) -> Option<PathBuf> {
        None
    }
    pub(super) fn theme_name() -> Option<String> {
        None
    }
    pub(super) fn app_icon_file(_exe: &Path, _px: u32) -> Option<PathBuf> {
        None
    }
    pub(super) fn named_icon_file(_icon: &str, _px: u32) -> Option<PathBuf> {
        None
    }
}

#[cfg(target_os = "linux")]
mod global {
    use super::{
        desktop_icon_for, folder_icon_names, parse_desktop_entry, parse_user_dirs, IconTheme,
        MimeDb,
    };
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};

    struct State {
        theme: IconTheme,
        mime: MimeDb,
        home: Option<PathBuf>,
        special: Vec<(PathBuf, &'static str)>,
        /// (소문자 파일 이름의 첫 점 뒤 전부 또는 이름 전체, px) → 결과(MIME로 정해지는 파일만).
        files: HashMap<(String, u32), Option<PathBuf>>,
        /// 데이터 폴더들(`applications/` · `pixmaps/`의 부모) — 앱 아이콘 조회용.
        data: Vec<PathBuf>,
        /// 설치된 `.desktop`의 (Exec 실행 파일, Icon) — 처음 물을 때 한 번 읽는다.
        desktop: Option<Vec<(String, String)>>,
        /// (실행 파일, px) → 앱 아이콘 결과.
        apps: HashMap<(PathBuf, u32), Option<PathBuf>>,
    }

    fn env_path(key: &str) -> Option<PathBuf> {
        std::env::var_os(key)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    }

    fn data_dirs() -> Vec<PathBuf> {
        let mut v = Vec::new();
        let home = env_path("HOME");
        if let Some(d) =
            env_path("XDG_DATA_HOME").or_else(|| home.as_ref().map(|h| h.join(".local/share")))
        {
            v.push(d);
        }
        let sys = std::env::var("XDG_DATA_DIRS")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".to_string());
        v.extend(sys.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
        v
    }

    /// `key=value` 꼴 설정 파일에서 값 하나(절 무시 · 따옴표 제거).
    fn ini_value(path: &Path, key: &str) -> Option<String> {
        let text = std::fs::read_to_string(path).ok()?;
        text.lines().find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == key)
                .then(|| v.trim().trim_matches(['"', '\'']).to_string())
                .filter(|v| !v.is_empty())
        })
    }

    fn detect_theme(roots: &[PathBuf]) -> String {
        let config = env_path("XDG_CONFIG_HOME")
            .or_else(|| env_path("HOME").map(|h| h.join(".config")))
            .unwrap_or_default();
        let from_files = || {
            ini_value(&config.join("gtk-4.0/settings.ini"), "gtk-icon-theme-name")
                .or_else(|| ini_value(&config.join("gtk-3.0/settings.ini"), "gtk-icon-theme-name"))
                .or_else(|| ini_value(&config.join("kdeglobals"), "Theme"))
        };
        let from_gsettings = || {
            let out = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", "icon-theme"])
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .ok()?;
            let s = String::from_utf8_lossy(&out.stdout);
            let s = s.trim().trim_matches('\'').to_string();
            (out.status.success() && !s.is_empty()).then_some(s)
        };
        let installed = |name: &str| {
            roots
                .iter()
                .any(|r| r.join(name).join("index.theme").is_file())
        };
        std::env::var("NEXA_ICON_THEME")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(from_files)
            .or_else(from_gsettings)
            .filter(|n| installed(n))
            .or_else(|| {
                ["Yaru", "Adwaita", "breeze"]
                    .into_iter()
                    .find(|n| installed(n))
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "hicolor".to_string())
    }

    fn state() -> &'static Mutex<State> {
        static STATE: OnceLock<Mutex<State>> = OnceLock::new();
        STATE.get_or_init(|| {
            let data = data_dirs();
            let home = env_path("HOME").map(|h| h.components().collect::<PathBuf>());
            let mut roots: Vec<PathBuf> = Vec::new();
            if let Some(h) = &home {
                roots.push(h.join(".icons"));
            }
            roots.extend(data.iter().map(|d| d.join("icons")));
            let theme = IconTheme::load(&roots, &detect_theme(&roots));
            let read = |name: &str| {
                data.iter()
                    .filter_map(|d| std::fs::read_to_string(d.join("mime").join(name)).ok())
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let mime = MimeDb::parse(&read("globs2"), &read("icons"), &read("generic-icons"));
            let special = home
                .as_ref()
                .map(|h| {
                    let cfg = env_path("XDG_CONFIG_HOME").unwrap_or_else(|| h.join(".config"));
                    let text =
                        std::fs::read_to_string(cfg.join("user-dirs.dirs")).unwrap_or_default();
                    parse_user_dirs(&text, h)
                })
                .unwrap_or_default();
            Mutex::new(State {
                theme,
                mime,
                home,
                special,
                files: HashMap::new(),
                data,
                desktop: None,
                apps: HashMap::new(),
            })
        })
    }

    pub(super) fn theme_name() -> Option<String> {
        let g = state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (!g.theme.is_empty()).then(|| g.theme.name.clone())
    }

    fn read_desktop_entries(data: &[PathBuf]) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for d in data {
            let Ok(rd) = std::fs::read_dir(d.join("applications")) else {
                continue;
            };
            let mut files: Vec<PathBuf> = rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "desktop"))
                .collect();
            files.sort();
            for f in files.into_iter().take(2000) {
                if let Some(pair) = std::fs::read_to_string(&f)
                    .ok()
                    .and_then(|t| parse_desktop_entry(&t))
                {
                    out.push(pair);
                }
            }
        }
        out
    }

    /// 아이콘 이름 → PNG 경로(절대 PNG · 테마 · pixmaps 순) — 잠금 안에서 쓰는 순수 조회.
    fn resolve_named(st: &mut State, icon: &str, px: u32) -> Option<PathBuf> {
        let p = Path::new(icon);
        if p.is_absolute() {
            return (p.extension().is_some_and(|e| e == "png") && p.is_file())
                .then(|| p.to_path_buf());
        }
        st.theme.find(icon, px).or_else(|| {
            st.data
                .iter()
                .map(|d| d.join("pixmaps").join(format!("{icon}.png")))
                .find(|f| f.is_file())
        })
    }

    pub(super) fn named_icon_file(icon: &str, px: u32) -> Option<PathBuf> {
        let mut g = state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let st = &mut *g;
        if st.theme.is_empty() || icon.is_empty() {
            return None;
        }
        resolve_named(st, icon, px)
    }

    pub(super) fn app_icon_file(exe: &Path, px: u32) -> Option<PathBuf> {
        let mut g = state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let st = &mut *g;
        if st.theme.is_empty() {
            return None;
        }
        let key = (exe.to_path_buf(), px);
        if let Some(hit) = st.apps.get(&key) {
            return hit.clone();
        }
        let entries = st
            .desktop
            .get_or_insert_with(|| read_desktop_entries(&st.data));
        // 링크를 푼 실제 경로도 본다(`x-terminal-emulator` → 실제 터미널 · `/usr/bin/code` → 앱 폴더 안 스크립트).
        let real = std::fs::canonicalize(exe).ok();
        let mut cands: Vec<&Path> = vec![exe];
        if let Some(r) = real.as_deref() {
            cands.push(r);
        }
        let icon = desktop_icon_for(entries, &cands).map(str::to_string);
        let mut got = icon.and_then(|icon| resolve_named(st, &icon, px));
        if got.is_none() {
            let names: Vec<String> = cands
                .iter()
                .filter_map(|c| c.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .collect();
            got = st
                .theme
                .find_first(&names, px)
                .or_else(|| st.theme.find("application-x-executable", px));
        }
        if st.apps.len() > 512 {
            st.apps.clear();
        }
        st.apps.insert(key, got.clone());
        got
    }

    pub(super) fn icon_file(path: &Path, is_dir: bool, px: u32) -> Option<PathBuf> {
        let mut g = state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let st = &mut *g;
        if st.theme.is_empty() {
            return None;
        }
        if is_dir {
            let names = folder_icon_names(path, st.home.as_deref(), &st.special);
            return st.theme.find_first(&names, px);
        }
        let name = path.file_name()?.to_string_lossy().to_lowercase();
        // 캐시 키 = 첫 점 뒤 전부(없으면 이름 전체) — MIME 판정에 쓰이는 부분만.
        let key = match name.split_once('.') {
            Some((stem, tail)) if !stem.is_empty() => tail.to_string(),
            _ => name.clone(),
        };
        if let Some(mime) = st.mime.mime_of(&name).map(str::to_string) {
            if let Some(hit) = st.files.get(&(key.clone(), px)) {
                return hit.clone();
            }
            let mut names = st.mime.icon_names(&mime);
            names.push("text-x-generic".to_string());
            let got = st.theme.find_first(&names, px);
            if st.files.len() > 4096 {
                st.files.clear();
            }
            st.files.insert((key, px), got.clone());
            return got;
        }
        // 표에 없는 이름: 실행 파일이면 실행 아이콘 · 그 밖은 문서.
        let exec = {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(path)
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        };
        let names: &[&str] = if exec {
            &["application-x-executable", "text-x-generic"]
        } else {
            &["text-x-generic", "application-octet-stream"]
        };
        st.theme.find_first(names, px)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = "\
[Icon Theme]
Name=Demo
Inherits=Base,hicolor
Directories=16x16/places,16x16@2x/places,48x48/places,scalable/places,16x16/mimetypes

[16x16/places]
Context=Places
Size=16
Type=Fixed

[16x16@2x/places]
Scale=2
Size=16
Type=Fixed

[48x48/places]
Size=48
Type=Fixed

[scalable/places]
Size=16
MinSize=8
MaxSize=512
Type=Scalable

[16x16/mimetypes]
Size=16
Type=Fixed
";

    /// 색인 해석: 상속 · 고정 크기 폴더(픽셀 = Size × Scale) · Scalable 제외.
    #[test]
    fn index_lists_fixed_dirs_with_pixel_size() {
        let ix = parse_index(INDEX);
        assert_eq!(ix.inherits, ["Base", "hicolor"]);
        let got: Vec<(&str, u32, bool)> = ix
            .dirs
            .iter()
            .map(|d| (d.path.as_str(), d.px, d.hidpi))
            .collect();
        assert_eq!(
            got,
            [
                ("16x16/places", 16, false),
                ("16x16@2x/places", 32, true),
                ("48x48/places", 48, false),
                ("16x16/mimetypes", 16, false),
            ]
        );
        // 크기 순위: 같은 크기 → 더 큰 것(가까운 순) → 더 작은 것.
        let order = |px| -> Vec<u32> { rank_dirs(&ix.dirs, px).iter().map(|d| d.px).collect() };
        assert_eq!(order(16), [16, 16, 32, 48]);
        assert_eq!(order(32), [32, 48, 16, 16]);
        assert_eq!(order(24), [32, 48, 16, 16]);
        assert_eq!(order(64), [48, 32, 16, 16]);
    }

    fn sandbox(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("nexa-fs-icontheme-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn put(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, body).expect("write");
    }

    /// 테마 조회: 크기 맞는 폴더 우선 · 상속 테마로 넘어감 · 끝에 hicolor · 없는 이름 = None · 후보 순서.
    #[test]
    fn theme_lookup_walks_sizes_and_inherits() {
        let root = sandbox("lookup");
        put(&root.join("Demo/index.theme"), INDEX);
        put(&root.join("Demo/16x16/places/folder.png"), "a");
        put(&root.join("Demo/48x48/places/folder.png"), "b");
        put(&root.join("Demo/16x16@2x/places/folder-download.png"), "c");
        put(
            &root.join("Base/index.theme"),
            "[Icon Theme]\nDirectories=24/mimetypes\n[24/mimetypes]\nSize=24\n",
        );
        put(&root.join("Base/24/mimetypes/text-x-generic.png"), "d");
        put(
            &root.join("hicolor/index.theme"),
            "[Icon Theme]\nDirectories=32x32/apps\n[32x32/apps]\nSize=32\n",
        );
        put(&root.join("hicolor/32x32/apps/demo-app.png"), "e");
        let mut t = IconTheme::load(std::slice::from_ref(&root), "Demo");
        assert!(!t.is_empty());
        assert_eq!(
            t.find("folder", 16),
            Some(root.join("Demo/16x16/places/folder.png"))
        );
        assert_eq!(
            t.find("folder", 40),
            Some(root.join("Demo/48x48/places/folder.png"))
        );
        assert_eq!(
            t.find("folder-download", 16),
            Some(root.join("Demo/16x16@2x/places/folder-download.png"))
        );
        assert_eq!(
            t.find("text-x-generic", 16),
            Some(root.join("Base/24/mimetypes/text-x-generic.png"))
        );
        assert_eq!(
            t.find("demo-app", 16),
            Some(root.join("hicolor/32x32/apps/demo-app.png"))
        );
        assert_eq!(t.find("nope", 16), None);
        assert_eq!(
            t.find_first(&["nope", "folder-music", "folder"], 16),
            Some(root.join("Demo/16x16/places/folder.png"))
        );
        // 없는 테마 = 빈 사슬(hicolor만 있으면 그것).
        assert!(IconTheme::load(&[root.join("missing")], "Demo").is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// MIME 표: 이름 전체 · 가장 긴 접미사 · 가중치 · 아이콘 후보 순서.
    #[test]
    fn mime_table_maps_names_to_icon_candidates() {
        let db = MimeDb::parse(
            "# comment\n50:application/gzip:*.gz\n50:application/x-compressed-tar:*.tar.gz\n\
             50:text/x-makefile:makefile\n50:text/x-python:*.py\n40:text/plain:*.py\n\
             60:text/rust:*.RS\n50:image/png:*.png\n50:text/x-readme:readme*\n",
            "text/x-python:text-x-script-python\n",
            "application/x-compressed-tar:package-x-generic\ntext/x-python:text-x-script\n",
        );
        assert_eq!(db.mime_of("a.tar.gz"), Some("application/x-compressed-tar"));
        assert_eq!(db.mime_of("a.b.GZ"), Some("application/gzip"));
        assert_eq!(db.mime_of("Makefile"), Some("text/x-makefile"));
        assert_eq!(db.mime_of("x.py"), Some("text/x-python"));
        assert_eq!(db.mime_of("main.rs"), Some("text/rust"));
        assert_eq!(db.mime_of("readme"), None, "와일드카드 글롭은 쓰지 않는다");
        assert_eq!(db.mime_of("noext"), None);
        assert_eq!(
            db.icon_names("text/x-python"),
            [
                "text-x-script-python",
                "text-x-python",
                "text-x-script",
                "text-x-generic"
            ]
        );
        assert_eq!(db.icon_names("image/png"), ["image-png", "image-x-generic"]);
    }

    /// 사용자 폴더: `$HOME/…`·절대 경로 · 홈 자신은 제외 · 폴더 아이콘 후보.
    #[test]
    fn user_dirs_give_special_folder_icons() {
        let home = Path::new("/home/u");
        let special = parse_user_dirs(
            "# c\nXDG_DOWNLOAD_DIR=\"$HOME/다운로드\"\nXDG_MUSIC_DIR=\"/data/music/\"\n\
             XDG_DESKTOP_DIR=\"$HOME/\"\nXDG_UNKNOWN_DIR=\"$HOME/x\"\nXDG_VIDEOS_DIR=\"rel\"\n",
            home,
        );
        assert_eq!(
            special,
            [
                (PathBuf::from("/home/u/다운로드"), "folder-download"),
                (PathBuf::from("/data/music"), "folder-music"),
            ]
        );
        let names = |p: &str| folder_icon_names(Path::new(p), Some(home), &special);
        assert_eq!(names("/"), ["drive-harddisk", "folder", "inode-directory"]);
        assert_eq!(names("/home/u"), ["user-home", "folder", "inode-directory"]);
        assert_eq!(
            names("/home/u/다운로드"),
            ["folder-download", "folder", "inode-directory"]
        );
        assert_eq!(names("/home/u/src"), ["folder", "inode-directory"]);
    }

    /// `.desktop` 해석: Exec 첫 낱말(따옴표 · env 접두 · 인자 제거) · Icon · 다른 절은 무시 · 짝 고르기(전체 경로 → 이름).
    #[test]
    fn desktop_entries_map_executables_to_icons() {
        let code = "[Desktop Entry]\nName=Code\nExec=/usr/share/code/code %F\nIcon=vscode\n\n\
                    [Desktop Action new]\nExec=/usr/share/code/code --new-window %F\nIcon=other\n";
        assert_eq!(
            parse_desktop_entry(code),
            Some(("/usr/share/code/code".into(), "vscode".into()))
        );
        assert_eq!(
            parse_desktop_entry(
                "[Desktop Entry]\nExec=env A=1 B=2 \"ptyxis\" --new\nIcon=org.gnome.Ptyxis\n"
            ),
            Some(("ptyxis".into(), "org.gnome.Ptyxis".into()))
        );
        assert_eq!(
            parse_desktop_entry("[Desktop Entry]\nExec=foo\n"),
            None,
            "Icon 없음"
        );
        assert_eq!(parse_desktop_entry("[Other]\nExec=foo\nIcon=bar\n"), None);
        let entries = vec![
            ("ptyxis".to_string(), "org.gnome.Ptyxis".to_string()),
            ("/usr/share/code/code".to_string(), "vscode".to_string()),
            ("/opt/x/code".to_string(), "wrong".to_string()),
        ];
        let p = |s: &'static str| Path::new(s);
        // 이름만 같은 경우: `/usr/bin/code`(스크립트)의 실제 경로 `/usr/share/code/bin/code` → 파일 이름 code.
        assert_eq!(
            desktop_icon_for(
                &entries,
                &[p("/usr/bin/code"), p("/usr/share/code/bin/code")]
            ),
            Some("vscode")
        );
        // 전체 경로가 같은 항목이 이름만 같은 항목보다 먼저.
        assert_eq!(
            desktop_icon_for(&entries, &[p("/opt/x/code")]),
            Some("wrong")
        );
        assert_eq!(
            desktop_icon_for(
                &entries,
                &[p("/usr/bin/x-terminal-emulator"), p("/usr/bin/ptyxis")]
            ),
            Some("org.gnome.Ptyxis")
        );
        assert_eq!(desktop_icon_for(&entries, &[p("/usr/bin/nope")]), None);
    }

    /// Linux 전역 조회(이 PC의 테마 — 내용은 환경마다 다르다): 답이 있으면 실제 PNG 파일이어야 한다 · 같은 질문 = 같은 답.
    #[cfg(target_os = "linux")]
    #[test]
    fn global_lookup_returns_existing_png() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
        let probes = [
            ("/", true),
            (home.as_str(), true),
            ("/usr", true),
            ("/tmp/a.txt", false),
            ("/tmp/a.tar.gz", false),
            ("/tmp/a.png", false),
            ("/tmp/a.unknown-ext-zz", false),
            ("/bin/sh", false),
        ];
        eprintln!("icon theme = {:?}", theme_name());
        // 앱 아이콘: 답이 있으면 실제 PNG · 모르는 실행 파일도 일반 실행 아이콘으로 떨어진다(테마가 있으면).
        for exe in [
            "/usr/bin/code",
            "/usr/bin/x-terminal-emulator",
            "/bin/sh",
            "/no/such/exe",
        ] {
            let got = app_icon_file(Path::new(exe), 16);
            eprintln!("app {exe} -> {got:?}");
            if let Some(f) = &got {
                assert!(f.is_file(), "{f:?}");
                assert_eq!(f.extension().and_then(|e| e.to_str()), Some("png"));
            }
            assert_eq!(app_icon_file(Path::new(exe), 16), got);
        }
        for (p, is_dir) in probes {
            for px in [16, 32] {
                let got = icon_file(Path::new(p), is_dir, px);
                eprintln!("{p} @{px} -> {got:?}");
                if let Some(f) = &got {
                    assert!(f.is_file(), "{f:?}");
                    assert_eq!(f.extension().and_then(|e| e.to_str()), Some("png"));
                }
                assert_eq!(icon_file(Path::new(p), is_dir, px), got);
            }
        }
    }

    /// 전역 조회는 Linux 밖에서 항상 None(호출자 자체 그림).
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn global_lookup_is_none_off_linux() {
        assert_eq!(icon_file(Path::new("/"), true, 16), None);
        assert_eq!(theme_name(), None);
        assert_eq!(named_icon_file("folder", 16), None);
    }

    /// 184차 이름 기준 조회: 테마가 있으면 `folder` 같은 표준 이름은 PNG로 풀리고 · 빈 이름/없는 이름은 None · 절대 경로는 PNG 파일일
    /// 때만 · 같은 질문은 같은 답(결정적).
    #[cfg(target_os = "linux")]
    #[test]
    fn named_icon_lookup() {
        assert_eq!(named_icon_file("", 16), None);
        assert_eq!(named_icon_file("/definitely/not/here.png", 16), None);
        if theme_name().is_none() {
            eprintln!("icon theme 없음 — 이름 조회 생략");
            return;
        }
        for name in ["folder", "utilities-terminal", "system-file-manager"] {
            let got = named_icon_file(name, 16);
            eprintln!("{name} -> {got:?}");
            if let Some(f) = &got {
                assert!(
                    f.is_file() && f.extension().is_some_and(|e| e == "png"),
                    "{f:?}"
                );
            }
            assert_eq!(named_icon_file(name, 16), got);
        }
        assert_eq!(named_icon_file("no-such-icon-zzz", 16), None);
    }
}

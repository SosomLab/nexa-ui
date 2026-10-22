//! nexa-fs — 파일시스템 **중립 모델**(docs/20 §2). 파일 대화상자·파일 관리 컨트롤이 보는 유일한 층.
//!
//! - [`places`]: 홈·바탕 화면·문서·다운로드·드라이브/볼륨 — OS별 발견 규칙은 여기에만.
//! - [`list`]: 폴더 열거(숨김 판정 = unix 점 파일 / Windows hidden 속성) · [`sort`]: 폴더 먼저 · **자연 정렬**(숫자 덩어리) · 대소문자 무관.
//! - [`naming::validate`]: 세 OS **공통 최소 집합**(한 OS에서 만든 파일이 다른 OS에서도 열리게).
//! - [`path::display`]: 홈 → `~` · [`path::parent_chain`]: 브레드크럼.
//! - [`local_time`]: 파일 시각 → 로컬 달력(외부 crate 0 · Windows `SystemTimeToTzSpecificLocalTime` · Unix `localtime_r`).
//!
//! 출처: nexa-dir2 `nexa-vfs`(`Entry`·`read_dir_entries`·`drive_entries`) · `nexa-tree`(정렬) · `nexa-app/pathinput.rs`(경로 입력) —
//! 이름을 중립화해 **추출**한 것이다(재발명 아님 · docs/20 §7).

pub mod dirwatch;
pub mod icontheme;
pub mod lister;
pub mod shell;
pub mod watch;

pub use lister::{ListHandle, ListMsg, ListOpts};

use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 폴더 항목.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 표시 이름(파일명).
    pub name: String,
    /// 전체 경로.
    pub path: PathBuf,
    /// 폴더인가(심볼릭 링크는 대상 기준).
    pub is_dir: bool,
    /// 바이트 크기(폴더 = 0).
    pub size: u64,
    /// 수정 시각.
    pub modified: Option<SystemTime>,
    /// 숨김(unix 점 파일 · Windows hidden 속성).
    pub hidden: bool,
}

impl Entry {
    /// 확장자(소문자 · 점 없음 · 없으면 빈 문자열).
    #[must_use]
    pub fn ext(&self) -> String {
        if self.is_dir {
            return String::new();
        }
        self.path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    }
}

/// 정렬 열.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SortKey {
    /// 이름(자연 정렬).
    #[default]
    Name,
    /// 수정 시각.
    Modified,
    /// 크기.
    Size,
    /// 종류(확장자).
    Kind,
}

#[cfg(windows)]
fn is_hidden_meta(m: &fs::Metadata, _name: &str) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    m.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
}

#[cfg(not(windows))]
fn is_hidden_meta(_m: &fs::Metadata, name: &str) -> bool {
    name.starts_with('.')
}

/// 가상 최상위("내 PC" · nexa-dir2 X-17 `::PC::` 이식 · 사용자 09-15) — 콜론은 파일명에 못 쓰는 문자라 실제 경로와 충돌하지 않는다.
/// 여기를 열면 [`drives`]가 항목이 된다(Windows `C:\` · macOS `/`·`/Volumes/*` · Linux `/`·`/media/$USER/*`·`/mnt/*`) —
/// 세 OS 모두 "루트 위"가 생겨 ↑가 막히지 않는다.
pub const VIRTUAL_ROOT: &str = "::PC::";

/// `p`가 가상 최상위인가.
#[must_use]
pub fn is_virtual_root(p: &Path) -> bool {
    p.as_os_str() == VIRTUAL_ROOT
}

/// 드라이브/볼륨 항목(가상 최상위의 내용).
#[must_use]
pub fn drive_entries() -> Vec<Entry> {
    drives()
        .into_iter()
        .map(|p| {
            let name = if p.parent().is_none() {
                // 루트(`C:\` · `/`) — 표기 그대로.
                p.to_string_lossy().into_owned()
            } else {
                p.file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.to_string_lossy().into_owned())
            };
            Entry {
                name,
                path: p,
                is_dir: true,
                size: 0,
                modified: None,
                hidden: false,
            }
        })
        .collect()
}

/// 폴더 열거 — 한 항목의 실패(권한 등)가 전체를 막지 않는다(메타데이터 실패 = 크기·시각만 기본값).
/// `show_hidden`이 거짓이면 숨김 항목을 뺀다. 가상 최상위는 드라이브 목록.
pub fn list(dir: &Path, show_hidden: bool) -> io::Result<Vec<Entry>> {
    list_opts(dir, show_hidden, true)
}

/// [`list`] + **점 파일 토글**(`.git` · `.cargo` … 이름이 `.`으로 시작 — Windows에서는 숨김 속성과 별개 · dir2 "숨김/점 필터" · 사용자 09-15).
pub fn list_opts(dir: &Path, show_hidden: bool, show_dot: bool) -> io::Result<Vec<Entry>> {
    if is_virtual_root(dir) {
        return Ok(drive_entries());
    }
    let mut out = Vec::new();
    for res in fs::read_dir(dir)? {
        let Ok(de) = res else { continue };
        if let Some(e) = entry_of(&de, show_hidden, show_dot) {
            out.push(e);
        }
    }
    Ok(out)
}

/// 메타데이터 — 보통은 `DirEntry::metadata()`(Windows = `FindNextFile`이 준 값 · syscall 0) · **링크일 때만** 경로 stat(대상 기준).
fn link_aware_meta(de: &fs::DirEntry, path: &Path) -> io::Result<fs::Metadata> {
    match de.file_type() {
        Ok(t) if t.is_symlink() => fs::metadata(path).or_else(|_| de.metadata()),
        _ => de.metadata(),
    }
}

/// `DirEntry` → [`Entry`](숨김·점 규칙 적용 · 걸러지면 `None`). 동기 열거와 백그라운드 열거가 같은 규칙을 쓴다.
pub(crate) fn entry_of(de: &fs::DirEntry, show_hidden: bool, show_dot: bool) -> Option<Entry> {
    let name = de.file_name().to_string_lossy().into_owned();
    let path = de.path();
    // 링크는 대상 기준(폴더 링크는 폴더로 진입 가능해야 한다) — 링크일 때만 경로 stat(docs/37 P-3 · 53×).
    let meta = link_aware_meta(de, &path);
    let (is_dir, size, modified, hidden) = match &meta {
        Ok(m) => (
            m.is_dir(),
            if m.is_dir() { 0 } else { m.len() },
            m.modified().ok(),
            is_hidden_meta(m, &name),
        ),
        Err(_) => (
            de.file_type().map(|t| t.is_dir()).unwrap_or(false),
            0,
            None,
            name.starts_with('.'),
        ),
    };
    if hidden && !show_hidden {
        return None;
    }
    if !show_dot && name.starts_with('.') {
        return None;
    }
    Some(Entry {
        name,
        path,
        is_dir,
        size,
        modified,
        hidden,
    })
}

/// 현재 보기 규칙(숨김 · 확장자 필터)으로 **보여줄 자식이 하나라도 있는가** — 첫 일치에서 멈추는 프로브
/// (nexa-dir2 X-43 "빈 폴더 펼침 글리프 억제" 이식 · 사용자 09-15). 읽기 실패 = `true`(글리프 유지 · 클라우드/권한 보호).
/// `exts`가 비면 모든 파일 · 폴더는 언제나 자식으로 친다.
#[must_use]
pub fn has_visible_child(dir: &Path, show_hidden: bool, show_dot: bool, exts: &[String]) -> bool {
    if is_virtual_root(dir) {
        return !drives().is_empty();
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return true;
    };
    for de in rd.flatten() {
        let name = de.file_name().to_string_lossy().into_owned();
        let meta = link_aware_meta(&de, &de.path());
        let (is_dir, hidden) = match &meta {
            Ok(m) => (m.is_dir(), is_hidden_meta(m, &name)),
            Err(_) => (
                de.file_type().map(|t| t.is_dir()).unwrap_or(false),
                name.starts_with('.'),
            ),
        };
        if hidden && !show_hidden {
            continue;
        }
        if !show_dot && name.starts_with('.') {
            continue;
        }
        if is_dir {
            return true;
        }
        if exts.is_empty() {
            return true;
        }
        let ext = Path::new(&name)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if exts.contains(&ext) {
            return true;
        }
    }
    false
}

/// 하위 **폴더**가 하나라도 있는가(폴더 트리용 · 첫 폴더에서 멈춤 · 숨김 규칙 동일 · 읽기 실패 = `true`).
#[must_use]
pub fn has_subfolder(dir: &Path, show_hidden: bool, show_dot: bool) -> bool {
    if is_virtual_root(dir) {
        return !drives().is_empty();
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return true;
    };
    for de in rd.flatten() {
        let name = de.file_name().to_string_lossy().into_owned();
        let meta = link_aware_meta(&de, &de.path());
        let (is_dir, hidden) = match &meta {
            Ok(m) => (m.is_dir(), is_hidden_meta(m, &name)),
            Err(_) => (false, name.starts_with('.')),
        };
        if is_dir && (show_hidden || !hidden) && (show_dot || !name.starts_with('.')) {
            return true;
        }
    }
    false
}

/// 자연 정렬 비교 — 숫자 덩어리는 값으로, 나머지는 대소문자 무관(한글은 코드 순).
#[must_use]
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (ac, bc): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (mut i, mut j) = (0usize, 0usize);
    while i < ac.len() && j < bc.len() {
        if ac[i].is_ascii_digit() && bc[j].is_ascii_digit() {
            let si = i;
            while i < ac.len() && ac[i].is_ascii_digit() {
                i += 1;
            }
            let sj = j;
            while j < bc.len() && bc[j].is_ascii_digit() {
                j += 1;
            }
            // 앞자리 0을 뗀 길이 → 값 비교(길이 다르면 긴 쪽이 크다).
            let na: String = ac[si..i].iter().collect();
            let nb: String = bc[sj..j].iter().collect();
            let ta = na.trim_start_matches('0');
            let tb = nb.trim_start_matches('0');
            let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
            if ord != Ordering::Equal {
                return ord;
            }
            continue;
        }
        let ca = ac[i].to_lowercase().next().unwrap_or(ac[i]);
        let cb = bc[j].to_lowercase().next().unwrap_or(bc[j]);
        if ca != cb {
            return ca.cmp(&cb);
        }
        i += 1;
        j += 1;
    }
    (ac.len() - i).cmp(&(bc.len() - j))
}

/// 결합 정렬(Shift+클릭으로 모은 키 순서 · 폴더 먼저 · 키가 비면 이름 오름차순 · 마지막 동률은 이름).
pub fn sort_by(entries: &mut [Entry], keys: &[(SortKey, bool)]) {
    entries.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            return if a.is_dir {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        for &(key, desc) in keys {
            let ord = match key {
                SortKey::Name => natural_cmp(&a.name, &b.name),
                SortKey::Modified => a.modified.cmp(&b.modified),
                SortKey::Size => a.size.cmp(&b.size),
                SortKey::Kind => natural_cmp(&a.ext(), &b.ext()),
            };
            let ord = if desc { ord.reverse() } else { ord };
            if ord != Ordering::Equal {
                return ord;
            }
        }
        natural_cmp(&a.name, &b.name)
    });
}

/// 정렬 — 폴더 먼저 · `key` 기준 · `desc`면 역순(폴더 우선은 유지).
pub fn sort(entries: &mut [Entry], key: SortKey, desc: bool) {
    entries.sort_by(|a, b| {
        if a.is_dir != b.is_dir {
            return if a.is_dir {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        let ord = match key {
            SortKey::Name => natural_cmp(&a.name, &b.name),
            SortKey::Modified => a.modified.cmp(&b.modified),
            SortKey::Size => a.size.cmp(&b.size),
            SortKey::Kind => natural_cmp(&a.ext(), &b.ext()),
        }
        .then_with(|| natural_cmp(&a.name, &b.name));
        if desc {
            ord.reverse()
        } else {
            ord
        }
    });
}

/// 장소 종류(사이드바 아이콘·라벨 키).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceKind {
    /// 홈.
    Home,
    /// 바탕 화면.
    Desktop,
    /// 문서.
    Documents,
    /// 다운로드.
    Downloads,
    /// 드라이브·볼륨(`C:\` · `/Volumes/x`).
    Drive,
    /// 최근 폴더(앱이 공급).
    Recent,
}

/// 사이드바 장소.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    /// 종류.
    pub kind: PlaceKind,
    /// 표시 이름(드라이브·최근은 경로 이름 · 나머지는 앱이 i18n 라벨로 바꿔 쓴다).
    pub name: String,
    /// 경로.
    pub path: PathBuf,
}

/// 홈 폴더(Windows `USERPROFILE` · unix `HOME`).
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
}

/// 표준 장소 — 존재하는 것만. 순서: 홈 · 바탕 화면 · 문서 · 다운로드 · 드라이브/볼륨.
#[must_use]
pub fn places() -> Vec<Place> {
    let mut out = Vec::new();
    if let Some(home) = home_dir() {
        out.push(Place {
            kind: PlaceKind::Home,
            name: home
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path: home.clone(),
        });
        for (kind, sub) in [
            (PlaceKind::Desktop, "Desktop"),
            (PlaceKind::Documents, "Documents"),
            (PlaceKind::Downloads, "Downloads"),
        ] {
            let p = home.join(sub);
            if p.is_dir() {
                out.push(Place {
                    kind,
                    name: sub.to_string(),
                    path: p,
                });
            }
        }
    }
    for p in drives() {
        let name = p.to_string_lossy().into_owned();
        out.push(Place {
            kind: PlaceKind::Drive,
            name,
            path: p,
        });
    }
    out
}

/// 드라이브(Windows `A:\`~`Z:\` 존재 프로브 — Win32 없이 std만) · 볼륨(macOS `/Volumes/*` · Linux `/`, `/media/$USER/*`, `/mnt/*`).
#[must_use]
pub fn drives() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        for c in b'A'..=b'Z' {
            let root = format!("{}:\\", c as char);
            if fs::metadata(&root).is_ok() {
                out.push(PathBuf::from(root));
            }
        }
    }
    #[cfg(not(windows))]
    {
        out.push(PathBuf::from("/"));
        let mut roots: Vec<PathBuf> = vec![PathBuf::from("/Volumes"), PathBuf::from("/mnt")];
        if let Ok(user) = std::env::var("USER") {
            roots.push(PathBuf::from(format!("/media/{user}")));
            roots.push(PathBuf::from(format!("/run/media/{user}")));
        }
        for r in roots {
            if let Ok(rd) = fs::read_dir(&r) {
                for de in rd.flatten() {
                    let p = de.path();
                    if p.is_dir() {
                        out.push(p);
                    }
                }
            }
        }
    }
    out
}

/// 탐색 히스토리(nexa-dir2 `nav.rs` 이식 · 순수 로직) — 뒤로/앞으로 스택. 새 진입은 현재 이후의 "앞으로"를 버린다.
#[derive(Clone, Debug)]
pub struct History {
    entries: Vec<PathBuf>,
    pos: usize,
}

impl History {
    /// 시작 위치.
    #[must_use]
    pub fn new(start: PathBuf) -> Self {
        History {
            entries: vec![start],
            pos: 0,
        }
    }

    /// 현재 위치.
    #[must_use]
    pub fn current(&self) -> &Path {
        &self.entries[self.pos]
    }

    /// 새 위치 진입(앞으로 기록 절단 · 같은 경로는 무시).
    pub fn push(&mut self, path: PathBuf) {
        if self.current() == path.as_path() {
            return;
        }
        self.entries.truncate(self.pos + 1);
        self.entries.push(path);
        self.pos += 1;
    }

    /// 뒤로 갈 수 있는가.
    #[must_use]
    pub fn can_back(&self) -> bool {
        self.pos > 0
    }

    /// 앞으로 갈 수 있는가.
    #[must_use]
    pub fn can_forward(&self) -> bool {
        self.pos + 1 < self.entries.len()
    }

    /// 뒤로.
    pub fn back(&mut self) -> Option<&Path> {
        if !self.can_back() {
            return None;
        }
        self.pos -= 1;
        Some(self.current())
    }

    /// 앞으로.
    pub fn forward(&mut self) -> Option<&Path> {
        if !self.can_forward() {
            return None;
        }
        self.pos += 1;
        Some(self.current())
    }
}

/// 파일명 규칙(세 OS 공통 최소 집합).
pub mod naming {
    /// 이름 오류.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum NameError {
        /// 빈 이름.
        Empty,
        /// 금지 문자(`\ / : * ? " < > |` · 제어문자).
        BadChar(char),
        /// 예약어(CON · PRN · AUX · NUL · COM1~9 · LPT1~9).
        Reserved,
        /// 끝이 공백·점.
        TrailingDotOrSpace,
        /// 255바이트 초과.
        TooLong,
    }

    /// 세 OS 어디서나 만들고 열 수 있는 이름인가.
    pub fn validate(name: &str) -> Result<(), NameError> {
        if name.is_empty() {
            return Err(NameError::Empty);
        }
        if let Some(c) = name.chars().find(|c| {
            matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()
        }) {
            return Err(NameError::BadChar(c));
        }
        if name.ends_with(' ') || name.ends_with('.') {
            return Err(NameError::TrailingDotOrSpace);
        }
        if name.len() > 255 {
            return Err(NameError::TooLong);
        }
        let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && stem.as_bytes()[3].is_ascii_digit()
                && stem.as_bytes()[3] != b'0');
        if reserved {
            return Err(NameError::Reserved);
        }
        Ok(())
    }
}

/// ★ 휴지통(사용자 10-22 · docs/20 §2 `ops.trash` · OS 차이는 이 층에만): Windows = `SHFileOperationW`(FO_DELETE + FOF_ALLOWUNDO ·
/// shell32 인박스) · macOS = `~/.Trash`로 이동(다른 볼륨이면 Finder `osascript` 폴백) · Linux = XDG trash
/// (`$XDG_DATA_HOME|~/.local/share` 아래 `Trash/{files,info}` · `.trashinfo` 먼저). 실패 = OS 원문 `Err(String)`.
pub mod trash {
    use std::path::{Path, PathBuf};

    /// 휴지통에 들어간 항목의 자리(되돌리기용 · 불투명 · [`restore`]).
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Token {
        original: PathBuf,
        kind: Kind,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    enum Kind {
        /// 옮겨진 곳을 안다(Windows `$R…` · macOS `.Trash` · Linux XDG `files/`) + 메타 파일(Windows `$I…` · Linux `.trashinfo`).
        Moved {
            trashed: PathBuf,
            meta: Option<PathBuf>,
        },
        /// 어디로 갔는지 모른다(복원 불가 · Finder 폴백 · 휴지통 기록을 못 찾음).
        Unknown,
    }

    impl Token {
        /// 원래 경로(절대).
        #[must_use]
        pub fn original(&self) -> &Path {
            &self.original
        }

        /// 되돌릴 수 있는가.
        #[must_use]
        pub fn restorable(&self) -> bool {
            matches!(self.kind, Kind::Moved { .. })
        }
    }

    /// 파일 또는 폴더(통째로)를 휴지통으로 보낸다 → 되돌리기 토큰.
    pub fn send(path: &Path) -> Result<Token, String> {
        if std::fs::symlink_metadata(path).is_err() {
            return Err("not found".to_string());
        }
        let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
        imp::send(&abs)
    }

    /// 휴지통에서 원래 자리로 되돌린다(원래 자리에 이미 무엇이 있으면 실패 · 메타 파일은 지운다).
    pub fn restore(token: &Token) -> Result<(), String> {
        let Kind::Moved { trashed, meta } = &token.kind else {
            return Err("cannot restore: trash location unknown".to_string());
        };
        if std::fs::symlink_metadata(&token.original).is_ok() {
            return Err("already exists".to_string());
        }
        if let Some(parent) = token.original.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::rename(trashed, &token.original).map_err(|e| e.to_string())?;
        if let Some(m) = meta {
            let _ = std::fs::remove_file(m);
        }
        Ok(())
    }

    /// XDG `.trashinfo`의 `Path=` 값 — 예약 문자만 `%XX`(구분자 `/`와 비예약 문자는 그대로).
    #[must_use]
    pub fn percent_encode(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for b in s.bytes() {
            let keep = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/');
            if keep {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{b:02X}"));
            }
        }
        out
    }

    /// XDG `.trashinfo` 본문(`secs` = UTC epoch).
    #[must_use]
    pub fn trash_info(abs: &str, secs: i64) -> String {
        let c = super::civil_from_unix(secs);
        format!(
            "[Trash Info]\nPath={}\nDeletionDate={:04}-{:02}-{:02}T{:02}:{:02}:{:02}\n",
            percent_encode(abs),
            c.year,
            c.month,
            c.day,
            c.hour,
            c.min,
            c.sec
        )
    }

    /// Windows 휴지통 `$I` 기록(버전 1 = Vista~8.1 · 2 = 10+) → (원래 경로, 삭제 시각 FILETIME). 모르는 형식 = None.
    #[must_use]
    pub fn parse_recycle_record(b: &[u8]) -> Option<(String, u64)> {
        fn u64_at(b: &[u8], i: usize) -> Option<u64> {
            b.get(i..i + 8)
                .and_then(|s| s.try_into().ok())
                .map(u64::from_le_bytes)
        }
        let version = u64_at(b, 0)?;
        let time = u64_at(b, 16)?;
        let (start, len) = match version {
            1 => (24usize, 260usize),
            2 => {
                let n = b
                    .get(24..28)
                    .and_then(|s| s.try_into().ok())
                    .map(u32::from_le_bytes)? as usize;
                (28usize, n)
            }
            _ => return None,
        };
        let mut units = Vec::with_capacity(len.min(1024));
        for i in 0..len {
            let o = start + i * 2;
            let u = u16::from_le_bytes([*b.get(o)?, *b.get(o + 1)?]);
            if u == 0 {
                break;
            }
            units.push(u);
        }
        Some((String::from_utf16_lossy(&units), time))
    }

    #[cfg(windows)]
    mod imp {
        use super::{Kind, Token};
        use std::os::windows::ffi::OsStrExt;
        use std::path::{Path, PathBuf};

        /// `SHFILEOPSTRUCTW`(x64 자연 정렬 · 32비트 x86의 packed 배치는 지원 대상 아님).
        #[repr(C)]
        struct ShFileOpStructW {
            hwnd: *mut core::ffi::c_void,
            w_func: u32,
            p_from: *const u16,
            p_to: *const u16,
            f_flags: u16,
            f_any_operations_aborted: i32,
            h_name_mappings: *mut core::ffi::c_void,
            lpsz_progress_title: *const u16,
        }
        #[link(name = "shell32")]
        extern "system" {
            fn SHFileOperationW(op: *mut ShFileOpStructW) -> i32;
        }
        const FO_DELETE: u32 = 3;
        const FOF_SILENT: u16 = 0x0004;
        const FOF_NOCONFIRMATION: u16 = 0x0010;
        const FOF_ALLOWUNDO: u16 = 0x0040;
        const FOF_NOERRORUI: u16 = 0x0400;

        pub(super) fn send(abs: &Path) -> Result<Token, String> {
            // 이중 널 종료 목록 · 뒤 구분자 제거(`C:\x\y\`는 셸이 거부).
            let s = abs.as_os_str().to_string_lossy();
            let s = s.trim_end_matches(['\\', '/']);
            let mut list: Vec<u16> = std::ffi::OsStr::new(s).encode_wide().collect();
            list.push(0);
            list.push(0);
            let mut op = ShFileOpStructW {
                hwnd: std::ptr::null_mut(),
                w_func: FO_DELETE,
                p_from: list.as_ptr(),
                p_to: std::ptr::null(),
                f_flags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI,
                f_any_operations_aborted: 0,
                h_name_mappings: std::ptr::null_mut(),
                lpsz_progress_title: std::ptr::null(),
            };
            // SAFETY: `list`는 호출 동안 살아 있고 이중 널로 끝난다 · 구조체는 x64 SHFILEOPSTRUCTW 배치와 같다.
            let rc = unsafe { SHFileOperationW(&mut op) };
            if rc != 0 {
                return Err(format!("SHFileOperationW error {rc:#x}"));
            }
            if op.f_any_operations_aborted != 0 {
                return Err("operation aborted".to_string());
            }
            // 휴지통 기록(`$I`)을 찾아 두면 되돌릴 수 있다(고정 NTFS 볼륨 · 다른 사용자 폴더는 접근 거부 = 건너뜀).
            let kind = locate(abs)
                .map(|(meta, trashed)| Kind::Moved {
                    trashed,
                    meta: Some(meta),
                })
                .unwrap_or(Kind::Unknown);
            Ok(Token {
                original: abs.to_path_buf(),
                kind,
            })
        }

        /// 드라이브 루트 `$Recycle.Bin\<SID>\$I…` 가운데 원래 경로가 같고(대소문자 무시) 가장 최근 것 → (`$I`, `$R`).
        fn locate(abs: &Path) -> Option<(PathBuf, PathBuf)> {
            let root = match abs.components().next()? {
                std::path::Component::Prefix(p) => {
                    PathBuf::from(format!("{}\\", p.as_os_str().to_string_lossy()))
                }
                _ => return None,
            };
            let want = abs
                .to_string_lossy()
                .trim_end_matches(['\\', '/'])
                .to_lowercase();
            let bin = root.join("$Recycle.Bin");
            let mut best: Option<(u64, PathBuf, PathBuf)> = None;
            for sid in std::fs::read_dir(&bin).ok()?.flatten() {
                let Ok(items) = std::fs::read_dir(sid.path()) else {
                    continue;
                };
                for it in items.flatten() {
                    let name = it.file_name().to_string_lossy().into_owned();
                    if !name.starts_with("$I") {
                        continue;
                    }
                    let Ok(bytes) = std::fs::read(it.path()) else {
                        continue;
                    };
                    let Some((orig, time)) = super::parse_recycle_record(&bytes) else {
                        continue;
                    };
                    if orig.trim_end_matches(['\\', '/']).to_lowercase() != want {
                        continue;
                    }
                    let r = it.path().with_file_name(format!("$R{}", &name[2..]));
                    if std::fs::symlink_metadata(&r).is_err() {
                        continue;
                    }
                    if best.as_ref().is_none_or(|(t, _, _)| time > *t) {
                        best = Some((time, it.path(), r));
                    }
                }
            }
            best.map(|(_, i, r)| (i, r))
        }
    }

    #[cfg(target_os = "macos")]
    mod imp {
        use super::{Kind, Token};
        use std::path::Path;

        pub(super) fn send(abs: &Path) -> Result<Token, String> {
            let home = super::super::home_dir().ok_or_else(|| "no home directory".to_string())?;
            let trash = home.join(".Trash");
            let name = abs
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .ok_or_else(|| "no file name".to_string())?;
            let mut target = trash.join(&name);
            let mut n = 2;
            while std::fs::symlink_metadata(&target).is_ok() {
                target = trash.join(format!("{name} ({n})"));
                n += 1;
            }
            match std::fs::rename(abs, &target) {
                Ok(()) => Ok(Token {
                    original: abs.to_path_buf(),
                    kind: Kind::Moved {
                        trashed: target,
                        meta: None,
                    },
                }),
                Err(e) => {
                    // 다른 볼륨(외장·네트워크) = Finder에 맡긴다(볼륨별 `.Trashes` · 위치를 모르니 되돌리기 불가).
                    let quoted = abs
                        .to_string_lossy()
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"");
                    let script =
                        format!("tell application \"Finder\" to delete POSIX file \"{quoted}\"");
                    let out = std::process::Command::new("osascript")
                        .arg("-e")
                        .arg(&script)
                        .output()
                        .map_err(|e2| format!("{e} · osascript: {e2}"))?;
                    if out.status.success() {
                        Ok(Token {
                            original: abs.to_path_buf(),
                            kind: Kind::Unknown,
                        })
                    } else {
                        Err(format!(
                            "{e} · {}",
                            String::from_utf8_lossy(&out.stderr).trim()
                        ))
                    }
                }
            }
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    mod imp {
        use super::{Kind, Token};
        use std::path::{Path, PathBuf};

        pub(super) fn send(abs: &Path) -> Result<Token, String> {
            let base = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| super::super::home_dir().map(|h| h.join(".local").join("share")))
                .ok_or_else(|| "no home directory".to_string())?;
            let files = base.join("Trash").join("files");
            let info = base.join("Trash").join("info");
            std::fs::create_dir_all(&files).map_err(|e| e.to_string())?;
            std::fs::create_dir_all(&info).map_err(|e| e.to_string())?;
            let name = abs
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .ok_or_else(|| "no file name".to_string())?;
            let mut stem = name.clone();
            let mut n = 2;
            while std::fs::symlink_metadata(files.join(&stem)).is_ok()
                || info.join(format!("{stem}.trashinfo")).exists()
            {
                stem = format!("{name}.{n}");
                n += 1;
            }
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let info_path = info.join(format!("{stem}.trashinfo"));
            std::fs::write(&info_path, super::trash_info(&abs.to_string_lossy(), secs))
                .map_err(|e| e.to_string())?;
            let trashed = files.join(&stem);
            match std::fs::rename(abs, &trashed) {
                Ok(()) => Ok(Token {
                    original: abs.to_path_buf(),
                    kind: Kind::Moved {
                        trashed,
                        meta: Some(info_path),
                    },
                }),
                Err(e) => {
                    let _ = std::fs::remove_file(&info_path);
                    Err(e.to_string())
                }
            }
        }
    }

    #[cfg(not(any(windows, unix)))]
    mod imp {
        use super::Token;
        use std::path::Path;
        pub(super) fn send(_abs: &Path) -> Result<Token, String> {
            Err("trash is not supported on this platform".to_string())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{parse_recycle_record, percent_encode, trash_info};

        #[test]
        fn trashinfo_percent_encodes_reserved_and_keeps_separators() {
            assert_eq!(percent_encode("/home/u/a b#1.sql"), "/home/u/a%20b%231.sql");
            assert_eq!(percent_encode("/x/한.txt"), "/x/%ED%95%9C.txt");
            let s = trash_info("/home/u/a b.sql", 1_700_000_000);
            assert!(
                s.starts_with(
                    "[Trash Info]\nPath=/home/u/a%20b.sql\nDeletionDate=2023-11-14T22:13:20\n"
                ),
                "{s}"
            );
        }

        /// Windows `$I` 기록 두 형식(1 = 고정 260자 · 2 = 길이 접두) · 깨진 입력 = None.
        #[test]
        fn recycle_record_v1_and_v2() {
            let name: Vec<u16> = "C:\\x\\a.txt".encode_utf16().collect();
            let mut v2 = Vec::new();
            v2.extend_from_slice(&2u64.to_le_bytes());
            v2.extend_from_slice(&5u64.to_le_bytes());
            v2.extend_from_slice(&123u64.to_le_bytes());
            v2.extend_from_slice(&((name.len() + 1) as u32).to_le_bytes());
            for u in &name {
                v2.extend_from_slice(&u.to_le_bytes());
            }
            v2.extend_from_slice(&0u16.to_le_bytes());
            assert_eq!(
                parse_recycle_record(&v2),
                Some(("C:\\x\\a.txt".to_string(), 123))
            );
            let mut v1 = Vec::new();
            v1.extend_from_slice(&1u64.to_le_bytes());
            v1.extend_from_slice(&5u64.to_le_bytes());
            v1.extend_from_slice(&7u64.to_le_bytes());
            for u in &name {
                v1.extend_from_slice(&u.to_le_bytes());
            }
            v1.resize(24 + 260 * 2, 0);
            assert_eq!(
                parse_recycle_record(&v1),
                Some(("C:\\x\\a.txt".to_string(), 7))
            );
            assert_eq!(parse_recycle_record(&[0u8; 10]), None);
        }

        /// 실 휴지통 왕복(삭제 → 복원 → 원위치) — 실제 OS 휴지통을 건드리므로 수동 실행(`cargo test -p nexa-fs -- --ignored`).
        #[test]
        #[ignore = "실 휴지통 부수효과 — 수동 실행"]
        fn trash_round_trip_restores_original() {
            let dir = std::env::temp_dir().join(format!("nexa-fs-trash-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("mkdir");
            let f = dir.join("restore-me.txt");
            std::fs::write(&f, "x").expect("write");
            let tok = super::send(&f).expect("send");
            assert!(!f.exists(), "moved to trash");
            assert!(tok.restorable(), "token should be restorable on this OS");
            super::restore(&tok).expect("restore");
            assert!(f.is_file(), "back in place");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// 경로 표시·분해.
pub mod path {
    use super::home_dir;
    use std::path::{Path, PathBuf};

    /// 표시용 — 홈 아래면 `~`로 줄인다(구분자는 OS 그대로).
    #[must_use]
    pub fn display(p: &Path) -> String {
        if let Some(home) = home_dir() {
            if let Ok(rest) = p.strip_prefix(&home) {
                let r = rest.to_string_lossy();
                return if r.is_empty() {
                    "~".to_string()
                } else {
                    format!("~{}{}", std::path::MAIN_SEPARATOR, r)
                };
            }
        }
        p.to_string_lossy().into_owned()
    }

    /// 사용자 입력 → 경로(`~` 확장 · 감싸는 따옴표 제거 · `%VAR%`/`$VAR` 환경변수 · 상대 경로는 `base` 기준).
    #[must_use]
    pub fn resolve(input: &str, base: &Path) -> PathBuf {
        let mut s = input.trim().to_string();
        if s.len() >= 2
            && ((s.starts_with('"') && s.ends_with('"'))
                || (s.starts_with('\'') && s.ends_with('\'')))
        {
            s = s[1..s.len() - 1].to_string();
        }
        if s == "~" || s.starts_with("~/") || s.starts_with("~\\") {
            if let Some(home) = home_dir() {
                s = format!("{}{}", home.to_string_lossy(), &s[1..]);
            }
        }
        s = expand_env(&s);
        // `shell:startup` · `shell:common startup` · `shell:downloads` … (탐색기 별칭 · OS별 해석은 `shell` 모듈 · 사용자 09-15).
        if let Some(p) = crate::shell::resolve_alias(&s) {
            return p;
        }
        let p = PathBuf::from(&s);
        if p.is_absolute() {
            p
        } else {
            base.join(p)
        }
    }

    /// `%NAME%`(CMD) · `$NAME`/`${NAME}`(sh) · `$env:NAME`/`${env:NAME}`(PowerShell · 대소문자 무시) 확장 — 미정의는 원문 유지.
    #[must_use]
    pub fn expand_env(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            // PowerShell `$env:NAME` / `${env:NAME}` — `$NAME`보다 먼저(접두가 겹친다).
            if c == '$' {
                let rest: String = chars[i + 1..].iter().take(6).collect();
                let braced = rest.to_ascii_lowercase().starts_with("{env:");
                let bare = rest.to_ascii_lowercase().starts_with("env:");
                if braced || bare {
                    let start = i + 1 + if braced { 5 } else { 4 };
                    let (name, len) = if braced {
                        match chars[start..].iter().position(|&x| x == '}') {
                            Some(end) => (
                                chars[start..start + end].iter().collect::<String>(),
                                end + 1,
                            ),
                            None => (String::new(), 0),
                        }
                    } else {
                        let n = chars[start..]
                            .iter()
                            .take_while(|x| x.is_alphanumeric() || **x == '_')
                            .count();
                        (chars[start..start + n].iter().collect::<String>(), n)
                    };
                    if !name.is_empty() {
                        if let Ok(v) = std::env::var(&name) {
                            out.push_str(&v);
                            i = start + len;
                            continue;
                        }
                    }
                }
            }
            if c == '%' {
                if let Some(end) = chars[i + 1..].iter().position(|&x| x == '%') {
                    let name: String = chars[i + 1..i + 1 + end].iter().collect();
                    if !name.is_empty() {
                        if let Ok(v) = std::env::var(&name) {
                            out.push_str(&v);
                            i += end + 2;
                            continue;
                        }
                    }
                }
            } else if c == '$' {
                let (name, len) = if chars.get(i + 1) == Some(&'{') {
                    match chars[i + 2..].iter().position(|&x| x == '}') {
                        Some(end) => (
                            chars[i + 2..i + 2 + end].iter().collect::<String>(),
                            end + 3,
                        ),
                        None => (String::new(), 0),
                    }
                } else {
                    let n = chars[i + 1..]
                        .iter()
                        .take_while(|x| x.is_alphanumeric() || **x == '_')
                        .count();
                    (chars[i + 1..i + 1 + n].iter().collect::<String>(), n + 1)
                };
                if !name.is_empty() {
                    if let Ok(v) = std::env::var(&name) {
                        out.push_str(&v);
                        i += len;
                        continue;
                    }
                }
            }
            out.push(c);
            i += 1;
        }
        out
    }

    /// 브레드크럼 — 루트부터 `p`까지의 조상 목록(각각 전체 경로).
    #[must_use]
    pub fn parent_chain(p: &Path) -> Vec<PathBuf> {
        // 맨 앞은 언제나 가상 최상위(탐색기 "내 PC ›" · 루트에서 ↑가 여기로 간다).
        let mut chain = vec![PathBuf::from(crate::VIRTUAL_ROOT)];
        if crate::is_virtual_root(p) {
            return chain;
        }
        let mut anc: Vec<PathBuf> = p.ancestors().map(Path::to_path_buf).collect();
        anc.reverse();
        anc.retain(|c| !c.as_os_str().is_empty());
        chain.extend(anc);
        chain
    }
}

/// 로컬 달력 시각.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LocalTime {
    /// 연.
    pub year: i32,
    /// 월(1~12).
    pub month: u32,
    /// 일.
    pub day: u32,
    /// 시.
    pub hour: u32,
    /// 분.
    pub min: u32,
    /// 초.
    pub sec: u32,
}

impl LocalTime {
    /// `YYYY-MM-DD HH:MM`.
    #[must_use]
    pub fn short(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.min
        )
    }
}

/// UTC 초 → 달력(Howard Hinnant civil_from_days).
#[must_use]
pub fn civil_from_unix(secs: i64) -> LocalTime {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400) as u32;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    LocalTime {
        year: (if m <= 2 { y + 1 } else { y }) as i32,
        month: m as u32,
        day: day as u32,
        hour: rem / 3600,
        min: (rem % 3600) / 60,
        sec: rem % 60,
    }
}

/// 파일 시각 → 로컬 달력(시간대 조회 실패 시 UTC).
#[must_use]
pub fn local_time(t: SystemTime) -> LocalTime {
    let secs = t
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    tz::to_local(secs).unwrap_or_else(|| civil_from_unix(secs))
}

#[cfg(windows)]
mod tz {
    use super::{civil_from_unix, LocalTime};
    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn SystemTimeToTzSpecificLocalTime(
            tz: *const std::ffi::c_void,
            utc: *const SystemTime,
            local: *mut SystemTime,
        ) -> i32;
    }
    pub(super) fn to_local(secs: i64) -> Option<LocalTime> {
        let u = civil_from_unix(secs);
        let utc = SystemTime {
            year: u.year as u16,
            month: u.month as u16,
            day_of_week: 0,
            day: u.day as u16,
            hour: u.hour as u16,
            minute: u.min as u16,
            second: u.sec as u16,
            millis: 0,
        };
        let mut local = SystemTime::default();
        // SAFETY: 두 구조체 포인터만 넘긴다 · NULL tz = 현재 시간대(DST 규칙은 그 날짜 기준).
        let ok = unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) };
        (ok != 0).then_some(LocalTime {
            year: i32::from(local.year),
            month: u32::from(local.month),
            day: u32::from(local.day),
            hour: u32::from(local.hour),
            min: u32::from(local.minute),
            sec: u32::from(local.second),
        })
    }
}

#[cfg(unix)]
mod tz {
    use super::LocalTime;
    /// `struct tm`의 앞 9개 int는 glibc·musl·macOS 공통 배치(뒤의 gmtoff/zone은 읽지 않는다).
    #[repr(C)]
    struct Tm {
        tm_sec: i32,
        tm_min: i32,
        tm_hour: i32,
        tm_mday: i32,
        tm_mon: i32,
        tm_year: i32,
        tm_wday: i32,
        tm_yday: i32,
        tm_isdst: i32,
        _pad: [i64; 4],
    }
    extern "C" {
        fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
    }
    pub(super) fn to_local(secs: i64) -> Option<LocalTime> {
        let mut tm = Tm {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 0,
            tm_mon: 0,
            tm_year: 0,
            tm_wday: 0,
            tm_yday: 0,
            tm_isdst: 0,
            _pad: [0; 4],
        };
        // SAFETY: time_t 포인터와 출력 구조체(여유 패딩 포함)만 넘긴다.
        let r = unsafe { localtime_r(&secs, &mut tm) };
        (!r.is_null()).then_some(LocalTime {
            year: tm.tm_year + 1900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            min: tm.tm_min as u32,
            sec: tm.tm_sec as u32,
        })
    }
}

#[cfg(not(any(windows, unix)))]
mod tz {
    pub(super) fn to_local(_secs: i64) -> Option<super::LocalTime> {
        None
    }
}

/// 크기 표시(`1.2 KB` · `340 B` · `12.5 MB`).
#[must_use]
pub fn fmt_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sort_orders_numbers_by_value() {
        let mut v = vec!["Script_10.sql", "script_2.sql", "Script_1.sql", "b", "A"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            v,
            vec!["A", "b", "Script_1.sql", "script_2.sql", "Script_10.sql"]
        );
    }

    #[test]
    fn sort_puts_folders_first_even_desc() {
        let e = |n: &str, d: bool| Entry {
            name: n.into(),
            path: PathBuf::from(n),
            is_dir: d,
            size: 0,
            modified: None,
            hidden: false,
        };
        let mut v = vec![
            e("z.txt", false),
            e("a", true),
            e("b.txt", false),
            e("c", true),
        ];
        sort(&mut v, SortKey::Name, true);
        let names: Vec<&str> = v.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, vec!["c", "a", "z.txt", "b.txt"]);
    }

    #[test]
    fn naming_rejects_common_set() {
        use naming::{validate, NameError};
        assert_eq!(validate(""), Err(NameError::Empty));
        assert_eq!(validate("a:b"), Err(NameError::BadChar(':')));
        assert_eq!(validate("con.sql"), Err(NameError::Reserved));
        assert_eq!(validate("x."), Err(NameError::TrailingDotOrSpace));
        assert!(validate("보고서 2026.sql").is_ok());
    }

    #[test]
    fn civil_and_size_formats() {
        let t = civil_from_unix(0);
        assert_eq!((t.year, t.month, t.day), (1970, 1, 1));
        assert_eq!(fmt_size(340), "340 B");
        assert_eq!(fmt_size(1536), "1.5 KB");
    }

    #[test]
    fn resolve_expands_home_and_relative() {
        let base = Path::new("/base");
        assert_eq!(path::resolve("x", base), PathBuf::from("/base").join("x"));
        assert!(path::resolve("~", base).is_absolute() || home_dir().is_none());
    }

    #[test]
    fn list_temp_dir_sees_files() {
        let dir = std::env::temp_dir().join(format!("nexa-fs-test-{}", std::process::id()));
        let _ = fs::create_dir_all(dir.join("sub"));
        fs::write(dir.join("a.sql"), b"select 1").unwrap_or(());
        let mut v = list(&dir, true).unwrap_or_default();
        sort(&mut v, SortKey::Name, false);
        assert_eq!(v.len(), 2);
        assert!(v[0].is_dir && v[0].name == "sub");
        assert_eq!(v[1].ext(), "sql");
        let _ = fs::remove_dir_all(&dir);
    }
}

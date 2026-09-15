# 20 · 파일 관리 · 파일 대화상자 — 3-OS 동일 UI · 계층 구조 확장 설계

> **요청**(사용자 09-14 · nexa-sql 세션): *"다른 앱들과 동일하게 OS별 차이를 주지 않고 동일한 UI로 개발. 지금까지 크게 신경 쓰지 않았던 파일 관리 기능과 파일 Dialog는 크게 개선. 컨트롤 라이브러리(nexa-ui)를 만들었으니 일관성 있고 계층 구조로 기능이 확장되도록 구성."*
> **선행**: [01 아키텍처](01-architecture.md) · nexa-beep DR-6(커스텀 렌더링 — 플랫폼 테마가 시각 동일성을 못 깨게) · DR-16(look = macOS · feel = host OS) · nexa-clip [25 디자인 시스템](../../nexa-clip/docs/25-design-system.md) · nexa-dir2 `nexa-gui/widgets`(columns · rows · dock · tabbar · menubar) · dir2 `fsprobe.rs`/`shellnotify.rs`(감시) · nexa-sql [15 외부 변경](../../nexa-sql/docs/15-external-file-changes.md) · [18 프로젝트](../../nexa-sql/docs/18-session-and-projects.md).
> **상태**: 🚧 1차 구현(09-15 · nexa-sql T-74) — `crates/nexa-fs`(F-1) + `crates/nexa-dlg::FilePicker`(F-4 · Open/Save) · nexa-sql `file_win.rs`(F-6). **모달 방식은 1차로 별도 소유 창**(nexa-sql 접속/설정 창과 같은 틀 · D-4 오버레이는 `Overlay` 부품(F-2) 뒤 전환 가능 — 선택기는 창 방식과 무관한 복합 컨트롤이라 코드 변경 없이 옮겨진다). 결정 **D-4~D-9**(§8) · 작업 **F-1~F-7**(§9).
> ⚠️ **충돌 1건**: nexa-beep [ADR-0014](../../nexa-beep/docs/35-adr-0014-native-file-dialog.md)(D-30 · 2026-08-18 Accepted)는 *"창 안은 우리가 그리고, OS 네임스페이스를 여는 문(파일 선택·저장)은 OS 것을 쓴다"* — Windows `IFileOpenDialog` · macOS `NSOpenPanel` · Linux만 자체 피커(구현은 아직 0). 이번 요청(*"OS별 차이 없이 동일 UI · 파일 Dialog 대폭 개선"*)은 그 경계를 **앱 안으로 당긴다**. 이 문서는 사용자 요청을 따르되 beep ADR-0014의 정정 여부를 **D-9**로 남긴다(beep 저장소 결정 · DR-4).

---

## 0. 원칙 세 줄

1. **네이티브 대화상자 0.** 파일 열기·저장·폴더 선택·메시지 상자를 전부 nexa-ui가 그린다. Windows `IFileDialog`·macOS `NSOpenPanel`·GTK/포털을 쓰지 않는다 — 세 OS에서 픽셀·키·동작이 같다(beep DR-6 *"플랫폼 간 동일 UI"* · clip DR-1 *"OS별 차이가 없도록 직접 그린다"* · *"스크린샷으로 OS를 구분할 수 없게"* 계승). 근거가 하나 더 있다: 어느 앱도 아직 네이티브 대화상자를 **구현한 적이 없다**(beep ADR-0014는 결정만 · clip은 경로 타이핑 · dir2는 자체 user32 대화상자) — 지금이 통일 비용이 가장 싼 시점이다.
2. **OS 차이는 `nexa-fs` 한 층에만 산다.** 드라이브·휴지통·숨김 규칙·감시·경로 표기 — UI는 그 층의 **중립 모델**만 본다. 컨트롤·대화상자 코드에 `cfg(target_os)`가 나오면 설계 위반.
3. **계층은 원자 → 조합 → 조립 → 앱.** 새 기능은 아래층을 고치지 않고 위층에서 조합한다. 파일 대화상자는 "특별한 창"이 아니라 **컨트롤 6개를 `Dialog` 프레임에 놓은 것**이다 — 그래서 앱마다 다른 대화상자(프로젝트 열기 · SQL 파일 저장 · 첨부 선택)를 조립만으로 만든다.

---

## 1. 계층

```
┌ 앱 (nexa-sql · nexa-clip · nexa-beep · nexa-dir2) ──────────────────────────────────────────┐
│  호스트 창(winit) · Overlay 스택 소유 · 설정(마지막 폴더·보기 모드) · i18n 문자열 · 앱 특화 필터/동작       │
├ nexa-dlg  (조립 — 대화상자) ──────────────────────────────────────────────────────────────────┤
│  Dialog 프레임(제목·본문·버튼줄·Esc/Enter·포커스 순환·크기 기억)                                       │
│  FilePicker{Open · OpenMany · Save · Folder} · MessageBox · Prompt · Progress(파일 작업)              │
├ nexa-ctl  (조합 — 파일 전용 컨트롤 · 기존 17종 위에) ────────────────────────────────────────────┤
│  PathBar(브레드크럼 ⇄ 편집) · PlacesList(사이드바) · FileList(상세/목록/아이콘 · 가상화 · 컬럼)          │
│  FileTree(폴더 트리) · NameBox(파일명 + 자동완성 + 검증) · FilterCombo(확장자 필터)                      │
│  Overlay(모달·팝업 z 스택) ← Combo/CtxMenu 팝업을 일반화                                              │
├ nexa-ctl  (원자 — 기존) ─────────────────────────────────────────────────────────────────────┤
│  button · textbox · combo · tree(TreeView/TreeGrid) · scroll · ctxmenu · toolbar · checkbox … · tokens  │
├ nexa-fs   (플랫폼 — UI 0 · 도메인 0) ───────────────────────────────────────────────────────────┤
│  Places(드라이브·홈·최근·즐겨찾기) · Listing(열거·메타·정렬·필터 · 배치/취소) · Ops(복사·이동·이름변경·   │
│  휴지통·새 폴더 · 충돌 정책 · 진행) · Watch(폴링 프로브 + OS 통지 어댑터) · Naming(파일명 규칙 공통 집합)   │
│  Kind(확장자→아이콘 키·종류 — OS 아이콘 안 씀) · Path(표시 규칙 · ~ · UNC)                              │
└ nexa-gfx · nexa-conf · nexa-font ───────────────────────────────────────────────────────────────┘
```

| 층 | 크레이트 | 의존 | 무엇을 모르나 |
|---|---|---|---|
| 플랫폼 | **`nexa-fs`**(dir2 `nexa-vfs`+`nexa-ops`+`nexa-tree` 정렬/검색 **추출** · §2) | std(+ Windows: shell32/kernel32 인박스 · macOS: `.Trash`/`osascript` · Linux: XDG trash 자체 구현) | UI · 앱 · 대화상자 |
| 원자·조합 | `nexa-ctl`(기존 + `controls/file/*`) | `nexa-gfx` · `nexa-fs`(모델 타입만) | 창 · 앱 문자열 |
| 조립 | **`nexa-dlg`**(신규) | `nexa-ctl` · `nexa-fs` | 창 · 앱 |
| 앱 | 각 앱 | 전부 | — |

- `nexa-ctl`이 `nexa-fs`에 의존하는 것은 **모델 타입**(`Entry` · `Place` · `Listing`)뿐이다. I/O는 앱이 `nexa-fs`를 호출해 컨트롤에 밀어 넣는다(컨트롤은 동기·순수 — 지금 17종과 같은 규율).
- 파일 관리자 본체인 **nexa-dir2가 `nexa-fs` + `FileList`의 최대 소비자**가 된다 — dir2의 columns/rows/fsprobe/shellnotify를 여기로 올려 보내고 dir2는 소비자로 돌아온다(장기 · dir2 저장소 결정).

---

## 2. `nexa-fs` — OS 차이를 가두는 층

★ **신규 작성이 아니라 추출이다.** nexa-dir2에는 이미 std 전용 크레이트가 있다 — `nexa-vfs`(`Entry{name,kind,size,modified,attrs,target}` · `read_dir_entries` · `drive_entries`(A:~Z: 프로브 · Win32 아님) · `Provider` trait) · `nexa-ops`(`Op{Copy,Move}` · `Conflict{Overwrite,Skip}` · `unique_dest` · `copy/move_onto_with_progress` · `rename` · `create_new_dir/file` · `Progress/Event` · `transfer` · `history.rs` undo/redo · `batch_rename.rs`) · `nexa-tree`(`SortSpec/SortKey` · `find_prefix` · `select_range`) · `nexa-app/src/{pathinput.rs(expand_env·suggest_folders), nav.rs(History), fsprobe.rs, icons.rs(icon_key)}`. `nexa-fs` = 이들을 **한 크레이트로 모아 이름을 중립화**하고 OS 어댑터(휴지통·감시 통지·places)만 채운다.

| 모듈 | API(요지) | OS 차이(여기서만) | 추출 원천(dir2) |
|---|---|---|---|
| `places` | `fn places() -> Vec<Place>` · `Place{kind: Home\|Desktop\|Documents\|Downloads\|Drive\|Volume\|Network\|Recent\|Favorite, name, path, icon}` | Windows 드라이브 문자·UNC · macOS `/Volumes` · Linux `/media` `/mnt` `/run/media` + `~/.config/user-dirs.dirs` | `vfs::drive_entries` · `set_extra_roots` · clip `nclip-plat/paths.rs`(Downloads 탐색 · env만) |
| `listing` | `fn list(dir, opts: ListOpts{hidden, sort, filter}) -> Listing` · `Entry{name, path, kind: Dir\|File\|Link, size, modified, created, hidden, readonly, ext, icon_key}` · **배치**(1,000개 단위 콜백 · 취소 토큰) | 숨김 = dot 파일(unix) / hidden 속성(Windows) · 링크 = symlink/junction/alias | `vfs::Entry` · `read_dir_entries` · `Provider` |
| `sort` | `fn sort(&mut [Entry], key: Name\|Size\|Modified\|Kind, dir, folders_first, natural: bool)` — **자연 정렬(숫자 인식) · 대소문자 무관 · 한글 정렬**(std `str` 비교 + 숫자 덩어리) | 없음 | `nexa-tree` `SortSpec/SortKey` · `typeahead.rs` |
| `ops` | `copy/move/rename/mkdir/trash/delete` — `Conflict{Overwrite\|Skip\|Rename\|Ask}` · `Progress{done, total, current}` · 취소 · **워커 스레드에서 실행**(호스트가 스폰) | 휴지통: Windows `IFileOperation`(shell32 인박스 · dir2 선례) · macOS `~/.Trash` 이동(+ `osascript` 폴백) · Linux XDG trash(`~/.local/share/Trash/{files,info}`) | **`nexa-ops` 전부**(std 전용 · 진행·충돌·undo·일괄 이름변경) · 휴지통 = `win.rs:2759`(`SHFileOperationW`+`FOF_ALLOWUNDO`) · `recycle.rs`(복원) |
| `watch` | `Watcher::new(dir) · poll(now) -> Changed{added, removed, modified}` — **dir2 `fsprobe.rs`(폴더 mtime + 상한 4096 열거 서명) 이식** + OS 통지 어댑터(있으면 즉시 · 없으면 폴링 3s/활성 · 30s/비활성 = dir2 X-40·X-44 실측) | ReadDirectoryChangesW · FSEvents(후속) · inotify(후속) — 1차는 **프로브만**(3-OS 동일 동작 보장) | `fsprobe.rs`(프로브) · `watcher.rs`(RDCW · 300ms 디바운스) · `shellnotify.rs` |
| `naming` | `fn validate(name) -> Result<(), NameError>` — 3-OS **공통 최소 집합**(`\/:*?"<>|` 금지 · 예약어 CON/PRN… · 끝 공백·점 · 255바이트) | 없음(교집합을 쓴다 — 한 OS에서 만든 파일이 다른 OS에서도 열리게) | `nexa-ops::unique_dest` · 신규 검증 |
| `kind` → **`shell`**(09-15 개정 · D-6 ⓑ) | `IconService::global()` — `icon(IconKey{Kind{ext,is_dir} \| Path}, large) -> Lookup{Ready(Option) \| Pending}` · `kind_name(ext, is_dir)` · `version()`/`pending()` · **워커 스레드 + 프로세스 캐시(상한 512 · 실패도 기억)** — UI는 절대 막히지 않고 `Pending`이면 자체 그림으로 그리다 도착 시 제자리 갱신 | Windows `SHGetFileInfoW`(확장자 기반 `USEFILEATTRIBUTES` = 디스크 0 · 경로 기반 = 특수 폴더/드라이브) · macOS `NSWorkspace icon(forFile:)`/UTType(후속) · Linux freedesktop 테마(`shared-mime-info` glob → 아이콘 이름 → 테마 PNG · 후속) · 없으면 `None` | 신규(§2-1) |
| `path` | 표시용 `display(path)`(홈 → `~` · 구분자는 OS 그대로 표시) · `parent_chain(path)`(브레드크럼) · `complete(prefix) -> Vec<String>`(NameBox 자동완성) | 구분자 | `pathinput.rs`(`expand_env`·`suggest_folders`) · `nav.rs::History` · `widgets/pathbar.rs::split_path` |

### 2-1. OS 아이콘 전략(09-15 · 사용자 요청 "각 OS 제공 이미지 최대 활용 · 성능 영향 0 · 확장성 · 메모리 적게")

| 원칙 | 구현 |
|---|---|
| **UI 스레드는 절대 막지 않는다** | 실측(Windows 11) `SHGetFileInfoW` 아이콘 ≈ **12ms/확장자** · 종류 이름 ≈ 2ms · 경로 아이콘 ≈ 17ms → 동기 호출이면 폴더 하나에 수백 ms. `IconService`는 조회를 **워커 스레드**(STA COM · mpsc 채널)에 맡기고 즉시 `Pending`을 돌려준다. 호출자는 자체 그림(폴더 = 호박색 · 파일 = 종이)으로 먼저 그리고, `version()`이 바뀌면 **제자리 갱신**(`TreeModel.roots[i].image` 교체 · 스크롤/선택 유지). |
| **조회는 확장자마다 프로세스 수명에 1회** | 목록은 `IconKey::Kind{ext,is_dir}`(디스크 접근 없음 · 파일 수와 무관 · 폴더에 파일 1만 개여도 확장자 수만큼) · 경로 조회(`IconKey::Path`)는 사이드바 장소·드라이브 등 **소수 항목에만**. 대화상자를 닫았다 다시 열어도 0ms(전역 캐시). |
| **메모리 상한** | 16×16 RGBA = 1KB · 캐시 상한 **512개(≤ 0.5MB)** · 오래된 것부터 버림(`VecDeque`) · 실패(`None`)도 기억해 재조회 0 · 소비자(nexa-dlg)는 `Arc<RgbaIcon>` → 자기 `IconImage`로 1회 변환. 큰 아이콘(32/48 · 아이콘 보기)은 같은 키에 `large` 플래그로 분리 — 필요할 때만. |
| **확장성(OS 어댑터 1곳)** | `shell::imp` 모듈만 OS별 — 공개 API·서비스·캐시는 공통. macOS = `NSWorkspace.shared.icon(forFile:)`/`icon(for: UTType)` → `CGImage` → RGBA(objc 런타임 FFI · crate 0) · Linux = `~/.config/mimeapps` 없이 `shared-mime-info` `globs2`로 MIME → 아이콘 이름(`text-x-sql` …) → 현재 GTK 테마 폴더(`~/.icons` · `/usr/share/icons/<theme>/16x16/mimetypes`) PNG(자체 디코더 · clip `imgdec` 재사용) · 둘 다 `SUPPORTED=false`면 즉시 `Ready(None)`(스레드도 안 만든다). |
| **종류 이름도 OS 것** | `kind_name(ext, is_dir)` — Windows `SHGFI_TYPENAME`("파일 폴더" · "Microsoft Word 문서" · 사용자 언어) · 없으면 앱 i18n 폴백(`Folder`/`SQL File`). |
| **렌더** | nexa-ctl 트리는 16px급 원본을 **원본 크기**로(13px로 줄이면 흐림) · 더 큰 그림은 공용 `LEADING_ICON`. 사전 스케일은 `IconImage::resized`(글리프 캐시 라운드에서 추가). |

작업: **F-8** macOS/Linux 어댑터 · 큰 아이콘 · 아이콘 보기(TODO).

---

## 3. 파일 대화상자(`nexa-dlg::FilePicker`) — 개선 목표

지금까지 "신경 쓰지 않았던" 부분을 **VS Code · 탐색기 · Finder의 교집합** 수준으로 올린다. 한 화면:

```
┌ 열기 ──────────────────────────────────────────────────────────────────── [✕] ┐
│ [←][→][↑]  ⌂ › Projects › kiros33 › nexa-sql › examples        [🔍 검색]  [☰▾] │  ← PathBar(클릭 = 편집 모드 · Ctrl+L)
├───────────────┬───────────────────────────────────────────────────────────────┤
│ 즐겨찾기      │ 이름 ▲            │ 수정 시각        │ 종류      │ 크기       │  ← FileList(상세 · 컬럼 정렬/리사이즈 · 가상화)
│  ⌂ 홈         │ 📁 fixtures        │ 2026-09-13 22:10 │ 폴더      │            │
│  📄 문서       │ 📄 golden-session… │ 2026-09-12 09:41 │ SQL       │ 2.1 KB     │
│  ⬇ 다운로드    │ …                                                             │
│ 최근          │                                                               │
│ 드라이브      │                                                               │
│  💽 C:        │                                                               │
│  💽 D:        │                                                               │
├───────────────┴───────────────────────────────────────────────────────────────┤
│ 파일 이름: [golden-session-vars.sql        ▾]   형식: [SQL (*.sql) ▾]           │  ← NameBox(자동완성) · FilterCombo
│ ☐ 숨김 파일 표시                                    [새 폴더]  [열기]  [취소]  │
└────────────────────────────────────────────────────────────────────────────────┘
```

| 개선 항목 | 동작 |
|---|---|
| **키보드 우선** | 타이핑 = NameBox로 자동 포커스 + 자동완성(현재 폴더 항목 · Tab 완성) · `Ctrl+L` 경로 편집 · `Alt+↑`/`Backspace` 상위 · `Enter` = 폴더면 진입/파일이면 확정 · `F2` 이름 변경 · `Del` 휴지통 · `Ctrl+N` 새 폴더 · `Ctrl+H` 숨김 토글 · `Esc` 취소 · 방향키/Home/End/PageUp·Down 목록 |
| 경로 입력 | PathBar 편집 모드에 **절대 경로 · `~` · 상대(현재 기준) · UNC · 드라이브** 입력 → 존재하면 이동 · 파일이면 바로 확정 |
| 정렬·보기 | 컬럼 헤더 클릭 정렬(자연 정렬 · 폴더 먼저) · 상세/목록/아이콘 보기 · 컬럼 폭·정렬·보기 모드를 **대화상자 종류별로 기억**(`nexa-conf`) |
| 필터 | 확장자 필터(앱이 공급 · `All files` 항상) · 검색 상자 = 현재 폴더 즉시 필터(부분 일치 · 대소문자 무관) |
| 저장 | 덮어쓰기 확인(MessageBox) · 확장자 자동 부여(필터 첫 확장자) · 읽기 전용/권한 없음 사전 안내 · 파일명 검증(§2 `naming`) 즉시 표시 |
| 다중 선택 | `OpenMany` = Shift/Ctrl 범위 · 선택 개수 표시 |
| 폴더 선택 | `Folder` = 같은 화면 · 파일은 흐리게 · "이 폴더 선택" 버튼 |
| 최근·즐겨찾기 | Places에 최근 폴더 10개(앱별) · 즐겨찾기 추가/제거(우클릭) · 드래그로 순서 |
| 외부 변경 | 열려 있는 동안 `watch`가 폴더 변경을 반영(추가·삭제 즉시) |
| 미리보기(옵션) | 오른쪽 패널 — 텍스트 앞 4KB · 이미지(clip `imgdec`가 있으면) · 앱이 provider 주입 |
| 크기·위치 | 대화상자 크기 기억 · 최소 크기 · DPI 배율 · 모든 텍스트 i18n 키(앱 주입 · `set_ctl_labels` 확장) |
| 접근성 | Tab 순환(PathBar → Places → List → Name → Filter → 버튼) · 포커스 링 · 키보드만으로 전 기능 |

---

## 4. 모달 방식 — 창 안 오버레이(권장)

| 방식 | 장점 | 단점 | 판정 |
|---|---|---|---|
| **창 안 오버레이**(호스트 창에 `Overlay` 층 · 뒤는 어둡게 · 입력 독점) | 3-OS 동일(창 관리자 차이 0) · IME·포커스·DPI가 한 창 · 구현 단순(clip `popup` 선례 확장) | 창보다 커질 수 없음 · 다중 모니터 이동 불가 | **기본** |
| 별도 winit 창(clip `settings_win` 선례) | 이동·크기 자유 · 긴 작업 창(Progress)에 적합 | OS별 모달/소유자 창 동작 차이(Wayland 제약) · 두 창 이벤트 라우팅 | Progress·비모달 도구창에만 |

`nexa-ctl::Overlay` = z 스택(`Base → Popup(Combo·CtxMenu·MenuBar) → Modal(Dialog)`) — 입력은 최상단이 먼저, 모달이 열리면 아래층은 마우스·키를 받지 않는다. 지금 라이브러리에는 **모달 개념이 없고**(팝업은 각 컨트롤이 `popup_rect/popup_hit`로 자기 오버레이를 그린다 · `icondrop.rs`의 캡처 플래그가 유일한 '모달') 호스트가 `paint_popup`을 따로 부른다 — 이를 스택으로 승격한다. 별도 창은 beep `Role::Picker`(자체 `FilePicker: ChoosePicker` 시제품 · 창 역할로 분리) · clip `settings_win` 선례.

---

## 5. 파일 관리 기능(대화상자 밖 · 앱 공통)

| 기능 | 층 | 소비자 |
|---|---|---|
| 프로젝트/폴더 사이드바(트리 · 파일 열기 · 이름 변경 · 새 파일) | `FileTree` + `nexa-fs::watch` | nexa-sql([18](../../nexa-sql/docs/18-session-and-projects.md)) |
| 외부 변경 감지 → 다시 읽기/병합 | `nexa-fs::watch` + 앱 | nexa-sql([15](../../nexa-sql/docs/15-external-file-changes.md)) |
| 드래그 앤 드롭 — OS에서 창으로(winit `DroppedFile` · 3-OS) · 창 안 이동 | 앱(수신) · `FileList`(창 안) | 전부 |
| 최근 파일 · 즐겨찾기 | `nexa-fs::places` + `nexa-conf` | 전부 |
| 파일 작업 + 진행 + 취소 + 충돌 | `nexa-fs::ops` + `nexa-dlg::Progress` | dir2 · clip(첨부) |
| 백업/복원 폴더·파일 선택(beep `PickerPurpose` 8종 = BackupDir·RestoreKey·ProfileImage·SettingsBackupDir…) · clip `sync.file_dir`(지금은 경로 타이핑) | `FilePicker{Folder·Open·Save}` | beep · clip · nexa-sql 라이선스 백업([nexa-sql 25 §11-3](../../nexa-sql/docs/25-license-tiers-and-server.md)) |
| 휴지통 | `nexa-fs::ops::trash` | dir2 · 대화상자 `Del` |

---

## 6. 확장 규칙(계층을 지키는 법)

1. **새 대화상자 = `nexa-dlg`에서 조립만.** 컨트롤을 새로 만들지 않고 §1의 조합 컨트롤을 놓는다(예: nexa-sql "프로젝트 열기" = `Folder` + 필터 `.nexa-project` + 미리보기 provider).
2. **새 조합 컨트롤 = 원자 컨트롤 + `nexa-fs` 모델.** I/O를 컨트롤 안에서 하지 않는다(테스트 가능 · 17종과 동일).
3. **OS 분기는 `nexa-fs`에만.** 컨트롤·대화상자·앱에 `cfg(target_os)`가 필요해지면 `nexa-fs`에 함수를 추가한다.
4. **문자열은 키.** `nexa-ctl::CtlMsg`를 파일 대화상자 어휘로 확장(`OpenTitle · SaveTitle · FileName · Filter · NewFolder · Overwrite? …`) · 앱이 `set_ctl_labels`로 번역 주입(nexa-sql `nsql-i18n`).
5. **토큰만.** 색·간격·반경·모션은 `tokens`/`Theme`(clip 25 디자인 시스템) — 대화상자에 하드코딩 수치 0.
6. **공개 API 변경은 소비자 영향 표기**(nexa-ui 규약) — `영향: nexa-sql | clip | beep | dir2`.

---

## 7. 이식 원천(재발명 금지)

| 무엇 | 어디서 | 비고 |
|---|---|---|
| 컬럼 모델 · **가상화 행**(`VirtualRows<S>` · `RowSource` · 헤더 3단 정렬 · 다중 컬럼 배지 · 드래그 리사이즈 · 3,212 LOC · 의존 0) | `nexa-dir2/crates/nexa-gui/{columns.rs, widgets/rows.rs}` | U-3 `nexa-grid`와 **같은 코드** — FileList 상세 보기 = grid 인스턴스 |
| **PathBar**(브레드크럼 · `split_path` · 편집 모드 · `take_navigation`) | `nexa-dir2/crates/nexa-gui/src/widgets/pathbar.rs` | ✅ 09-15 nexa-dlg 안에 조립(브레드크럼 = 페인트+범위 캐시 · 편집 = 기존 TextBox 재사용 · `History` = nav.rs 이식 · `shell:` = `SHParseDisplayName` raw FFI + 3-OS 공통 표) — 독립 컨트롤 승격은 F-3 |
| 파일 열거·작업·정렬·검색(std 전용) | `nexa-dir2/crates/{nexa-vfs,nexa-ops,nexa-tree}` | → `nexa-fs`(§2) |
| 경로 입력 해석·자동완성 · 탐색 히스토리 | `nexa-dir2/crates/nexa-app/src/{pathinput,nav}.rs` | → `nexa-fs::path` |
| 자체 파일 피커 시제품(별도 창 · `ChoosePicker` 어댑터 · 용도 8종) | `nexa-beep/crates/nexa-beep/src/app.rs:2584~2883` | `nexa-dlg::FilePicker`가 대체(F-7) |
| Downloads/홈 폴더 탐색(env만 · 3-OS) | `nexa-clip/crates/nclip-plat/src/paths.rs` · `nbeep-plat/src/paths.rs` | → `nexa-fs::places` |
| 폴더 변경 감지(프로브 서명 · 활성/비활성 폴링) | `nexa-dir2/crates/nexa-app/src/fsprobe.rs` · X-40 · X-44 | 3-OS 동일 동작의 근거(통지 없는 클라우드 폴더까지) |
| 셸 통지(Windows) | `nexa-dir2/.../shellnotify.rs` | `watch` 어댑터(Windows만 · 후속) |
| 휴지통(Windows `IFileOperation`) | dir2 파일 작업 | shell32 인박스 |
| 팝업 z 관리 · 별도 창 | `nexa-clip/src/{popup_win,settings_win}.rs` | Overlay · Progress 창 |
| 트리·스크롤·콤보·컨텍스트 메뉴 | `nexa-ctl::controls::{tree,scroll,combo,ctxmenu}` | 그대로 |
| 컨트롤 문서 형식 | `nexa-dir2/docs/ctl/*.md` | 파일 컨트롤 6종 문서도 같은 틀 |

---

## 8. 결정(사용자)

| # | 결정 | 권장 |
|---|---|---|
| **D-4** | 모달 방식 — 창 안 오버레이 기본 + Progress만 별도 창(권장) / 전부 별도 창 | 오버레이 |
| **D-5** | `nexa-fs`·`nexa-dlg`를 **별도 크레이트**(권장 · 의존 방향 강제 · dir2가 fs만 소비 가능) / `nexa-ctl` 안 모듈 | 별도 크레이트 |
| **D-6** | 아이콘 세트 — ⓐ 자체 알파 마스크 3-OS 동일 / ⓑ **OS 아이콘 우선 + 자체 폴백**(사용자 09-15 확정: "각 OS가 제공하는 폴더/파일 이미지를 최대한 활용 · 성능 영향 0") — §2-1 | **ⓑ 확정** |
| **D-7** | 휴지통 1차 범위 — 3-OS 전부(권장 · Windows shell32 · macOS `.Trash` · Linux XDG) / Windows만 | 3-OS |
| **D-8** | FileList와 U-3 `nexa-grid`의 관계 — 같은 크레이트의 한 컨트롤(권장 · 컬럼 모델 공유) / 분리 | 공유 |
| **D-9** | ★ **beep ADR-0014(네이티브 파일 대화상자) 정정** — ⓐ 계열 전체를 자체 `FilePicker`로 통일(이번 요청 · 권장 — 3-OS 동일 · Linux 포털 문제 소멸 · beep 시제품 대체) ⓑ beep만 예외 유지(OS 관례 우선) — beep 저장소에서 ADR 정정 항목으로 기록(DR-4) | ⓐ |

---

## 9. 작업(F-1~F-7 · TODO 등재)

| ID | 항목 | 의존 |
|---|---|---|
| **F-1** | `nexa-fs` — places · listing(배치·취소) · sort(자연·한글) · naming · kind · path · watch(프로브 이식) · 3-OS 테스트(임시 폴더) | D-5 |
| **F-2** | `nexa-ctl::Overlay`(z 스택 · 모달 입력 독점) + Combo/CtxMenu 팝업 승격 | D-4 |
| **F-3** | 파일 컨트롤 6종 — PathBar · PlacesList · FileList(columns/rows 이식 = U-3 공유) · FileTree · NameBox · FilterCombo · `docs/ctl` 문서 | F-1 U-2 |
| **F-4** | `nexa-dlg` — Dialog 프레임 · MessageBox · Prompt · **FilePicker{Open·OpenMany·Save·Folder}** · 크기/보기 기억 · i18n 어휘 | F-2 F-3 |
| **F-5** | `nexa-fs::ops` + `Progress` — 복사/이동/이름변경/새 폴더/휴지통 · 충돌 · 취소 · 워커 | F-1 F-4 D-7 |
| **F-6** | nexa-sql 배선 — 열기/저장/다른 이름으로/프로젝트 폴더/export 경로 · `DroppedFile` · 최근 파일 | F-4 |
| **F-7** | clip(첨부·내보내기) · beep(파일 전송 선택) · dir2(소비자 전환) 이관 제안 | F-5 · 각 저장소 결정 |

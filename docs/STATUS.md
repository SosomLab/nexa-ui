# STATUS — 지금 상태 한 장

> 시간 역순. 상세는 [journal](journal/), 여기는 요약.

## 2026-09-22 (55차 · win) — nexa-sql 93차 지원: `ScrollBars` 가장자리 접근 = 그 축만 깨움 · `TextBox` 가로 범위 통일(`ml_bars_w`) + 클릭 시 잔여 px 유지 · `TabBar` 클릭 수식키(`last_click_mods`) + **묶인 탭 상단 줄**(`set_group`/`is_grouped` · 동시 편집) · `TreeGrid` **다중 선택 표시**(`set_marked`/`is_marked`) · nexa-dlg **열기 모드 다중 선택**(`marks` 고른 순서 · Ctrl 토글 · Shift 범위 · Space · Ctrl+A · 이름 상자 `"a" "b"` · `labels.multi_selected` · `PickerAction::ConfirmMany` · 테스트 `open_mode_multi_select_rules`). → nexa-sql journal 09-22 §22~§30.

## 2026-09-22 (54차 · win) — 🔧 53차 CI windows-latest `test` 실패의 **진짜 원인 = 시험끼리의 `set_text_gdi` 경주**(프로세스 전역 스위치를 `gdi_path_gives_integer_advances`가 끄는 순간 `gdi_cleartype_stems_bold_and_advances`가 ab_glyph 경로로 떨어져 볼드가 무시됨 = "볼드 잉크 27.7 ≤ 보통 27.7" · `gh run view --log-failed`로 확인 · `file_type()`은 걷기를 빠르게 해 타이밍을 드러냈을 뿐) → 시험 가드 `tests::GdiOn`(뮤텍스 + Drop에서 끔 · 패닉 안전) · **걷기 캐시 + `file_type()`을 3-OS 한 길로 복귀**(`#[cfg(windows)]` 분기 둘 제거) · 실측(Release · 폰트 701개): 걷기 1회 12.6 → 1.2 ms(종전은 가족 탐색마다 되풀이) · 진단 시험 `time_font_walk`(ignored). 테스트 360 · clippy 0 · check-3os ✓ · nexa-font 60회 반복 0 실패 · **CI e5433cb = 3-OS ✓(windows-latest 포함 — 원인 확인)**. → nexa-sql [journal 09-22 §14](../../nexa-sql/docs/journal/2026-09-22.md)

## 2026-09-22 (53차 · linux) — nexa-font **글꼴 가족 탐색이 폰트 트리를 호출마다 걷던 것** 수정(Linux 기동 100~700 ms = `/usr/share/fonts` `statx` 11,786회): 걷기 = 프로세스에서 한 번(`font_files` · `OnceLock`) + `DirEntry::file_type()`(statx 0 · 링크 폴더는 `is_dir()`로 따라감). nexa-sql 창까지 522 → 111 ms의 첫 절반. 테스트 357 · clippy 0 · CI ac57901 = Windows `test` ✗(mac·ubuntu ✓ · 로그 인증 불가) → 링크 안전 수정(9653c33)도 Windows ✗ → 캐시 경로를 Linux·macOS로 한정(c8f2aae · clippy dead_code → de2cb69) → Windows `test` 또 ✗ → **Windows는 `collect_font_files`의 `file_type()`도 종전 `is_dir()`로(b3d8e8b) = CI 3-OS 초록**. 따라서 Windows 실패 원인 = `DirEntry::file_type()` 경로(`C:\Windows\Fonts` 항목의 종류 판정이 달랐던 듯 · Windows 세션에서 확인 뒤 3-OS로 되돌릴지). → nexa-sql [journal 09-22](../../nexa-sql/docs/journal/2026-09-22.md)

## 2026-09-21 (52차 · win) — 편집기 드래그 선택: 포인터가 밖에 **멈춰 있어도** 계속 스크롤(틱 기반 · 새 타이머 0) · 파일 감시 시험의 타이밍 의존 제거. 테스트 360. → [journal](journal/2026-09-18.md)

## 2026-09-21 (51차 · win) — ★ **팝업 배치 규칙**(메뉴·툴팁·드롭다운은 창 밖으로 잘리지 않는다 · 대상 행을 가리지 않는 `open_beside`) · 탭 끌기 고스트 · 편집기 밖 끌기 선택 = 줄 단위(빠름) · 폴더 고르기 대화상자 · 가린 입력란 = 비밀 값(복사 금지 · 0 덮어쓰기). 테스트 359. → [journal](journal/2026-09-18.md)

## 2026-09-21 (50차 · mac) — ★ nexa-sys **`layer_present`** = macOS IOSurface 화면 내보내기(표면 풀 · sRGB 태그 · 의존 0 · present 36 → 2.9 ms · nexa-sql `gfx.mac_present`로 선택 · 남은 것 = `Surface` stride) → [journal](journal/2026-09-18.md)

## 2026-09-20 (49차 · mac) — TextBox 한글 **앱 조합**(전역 스위치 · macOS winit IME 결함 회피) · nexa-sys `input_source`(한글 입력 소스 감지 + 바뀜 알림) → [journal](journal/2026-09-18.md)

## 2026-09-20 (48차 · win) — 📘 CLAUDE.md §3-1 세션 공통 규칙 · 편집기 코어 불변식(다른 PC에서 그대로 · nexa-sql docs/61) → [journal](journal/2026-09-18.md)

## 2026-09-20 (47차 · win) — ★ **`TextBuf`**(UTF-8 갭 버퍼 + 줄 표 + 줄 변경 기록 · 좌표 = 글자 인덱스) = `EditState`의 저장소 · 그리기 = 보이는 줄만 + 줄별 캐시를 변경 기록으로(70만 줄 입력 157 → 3 ms · 메모리 348 → 86 MB) · `chars()` → `buf()` · `merge3::line_edits` · 쉬었다 치면 새 묶음 · 거대 편집 확인 · 되돌리기 기록 내보내기/들이기 → [journal](journal/2026-09-18.md)

## 2026-09-20 (46차 · win) — ★ **되돌리기 = 연산 기록**(저장 = 지운 글자만 · 단일 변경 통로 · 저장 지점 O(1) · 바이트 예산 · 읽기 전용 — 붙여넣기 100 KB 34초 → 1 ms · 모두 바꾸기 2,000건 15초 → 3.4 ms) · **`PreparedText`/`set_prepared`**(큰 본문을 스레드에서 준비 → UI 0 ms) · **행 폭 고정폭 지름길**(70만 줄 첫 페인트 3.8 → 0.2초) → [journal](journal/2026-09-18.md)

## 2026-09-19 (45차 · win) — ★ **TextBox 큰 파일 성능**(세대+줄 해시 캐시 넷 · 키 경로의 본문 복사 제거 — 4만 줄 유휴 그리기 78 → 3.3 ms · 입력 209 → 16 ms) · **되돌리기 = 차이 저장**(`SnapBuf`) · `release_caches`/`clear_history`/`approx_bytes` · nexa-dlg **덮어쓰기 = 타임아웃 버튼** → [journal](journal/2026-09-18.md)

## 2026-09-19 (44차 · win) — ★ **`merge3`**(줄 단위 3-way 병합 · Myers 정합 · 의존 0) · **`TextBox::replace_all_undoable`**(전체 교체 = 되돌리기 한 단계 · 캐럿 유지) · **nexa-fs `watch`**(`FileSig`·`content_hash`·`StatWatch` — OS 와처 없는 외부 변경 감지) → [journal](journal/2026-09-18.md)

## 2026-09-19 (43차 · win) — `SyntaxSpec.numbers`(Plain Text = 숫자 리터럴도 안 칠함 · `.nexa-syntax` `numbers = off`) → [journal](journal/2026-09-18.md)

## 2026-09-19 (42차 · win) — **색 대비 순서** `contrast_order`/`color_contrast`/`is_warm`(순환 팔레트를 이웃끼리 가장 잘 구별되게 · 보색·색 온도·밝기) · `Theme.rainbow` 순서를 그 결과로(어두움·밝음) · 테스트 2 → [journal](journal/2026-09-18.md)

## 2026-09-19 (41차 · win) — 🔧 Toolbar 글리프 항목 비활성 흐림(`enabled`·`tone` 존중 · nexa-sql 결과 도구줄) · **TreeControl ← 키**(펼쳐진 행 = 접기 · 아니면 상위로) · ★ **타입어헤드 부품**(`typeahead`/`hangul` · nexa-beep 이식) · Switch/Checkbox 빈 bounds 가드 · **`PositionDropdown`**(위치 이미지 드롭다운) → [journal](journal/2026-09-18.md)

## 2026-09-17 (26차 · win) — ★ `ToolDock` 툴바 그룹 도크 ✅ · Toolbar 구분자/권장 폭/게터 ✅ · Toolbar 배지/툴팁/Custom 색조 ✅ · 풀다운 Disabled ✅ · TextBox 글자 세로 중앙 · `OccurrenceStyle`(1px 상자 · 인접 행 선 공유) · 미니맵 선택/출현 색 구분 ✅ · nexa-fs `reveal_in_file_manager` ✅ · Sublime 커서 규칙(단어/서브워드/스마트 Home/Ctrl+클릭/열 선택) ✅ · Auto Indent ✅ · **레인보우 괄호 코어**(`PairTable` · `BracketOpts` · 형제/상위/하위 이동 · 자동 닫기/감싸기 · 깊이 색 · `Theme.rainbow`) ✅

목적별 그룹(아이콘·구분자) · 그립 드래그 순서 이동 · 세로 드래그 = `DockAction::Float`(창은 호스트) · `DockLayout` 문자열 저장/복원 · 테스트 3. nexa-sql 52차가 상단 툴바에 배선(플로팅 창 `toolfloat.rs`). → [journal](journal/2026-09-17.md)

## 2026-09-16 (25차 · mac) — 미니맵 ✅ · 찾기 범위/전부 선택 ✅ · 자원 상한 세터 ✅ · ★ `nexa-sys` 크레이트 ✅(D-60)

TextBox 미니맵(창 한정 캐시 비트맵 · 클릭/드래그 · 테스트 5) · `set_find_scope`/`set_regions_pub`/`set_history_max` · glyph/icon 캐시 상한 세터 · OS 신호 크레이트 `nexa-sys`(배터리·원격·동작 줄이기 · 3-OS · 외부 crate 0). 워크스페이스 green · check-3os ✓. **다음**: F-8 · 미니맵 wrap 모드 · Performance 신호 60초 캐시 실기. → [journal](journal/2026-09-16.md)

## 2026-09-16 (24차 · mac) — TextBox 픽셀 스크롤 ✅ · `set_scroll_snap` ✅ · `EditState::command` 14종 ✅ · 찾기 일치 표시 ✅

nexa-sql 트랙패드 QA: 휠 잔여 이월 → 픽셀 스크롤(세로 클립) → 행 단위 모드는 표시만 스냅. `edit/ops.rs` 조각 편집 재매핑(되돌리기 1 · 테스트 10) · `edit_command`/`goto_line`/`set_find_marks`/여러 줄 Tab. 헤드리스 휠 시뮬로 편집기 대칭 증명. 227 green · clippy 0. **다음**: F-8 · 캡처 창 2단 코드. → [journal](journal/2026-09-16.md)

## 2026-09-16 (1~4차 · mac) — 두부 방지 폰트 폴백 ✅ · TreeGrid 가로 클립 ✅ · `Splitter` ✅ · Material 글리프/`Button::glyph` ✅ · `ToolTone` ✅ · 잉크 기준 세로 정렬 ✅

맥 실기 첫날(nexa-sql 세션). 기호 폴백은 OS별 고정 경로 + 이름 검색(재귀) 2중에 `UI_SYMBOLS` 회귀 테스트(CI 3-OS). `DrawCtx::text_center_y`(잉크 가운데)를 컨트롤 전반에 적용 — Windows 쪽 재검증 필요(F-9). 3-OS clippy 전부 green. **다음**: F-8 macOS `NSWorkspace` 아이콘. → [journal](journal/2026-09-16.md)

## 2026-09-15 (2차 · win) — `nexa-fs` + `nexa-dlg::FilePicker` ✅(docs/20 1차) · 편집기 다중 선택 ✅

워크스페이스 6크레이트(gfx · ctl · conf · font · **fs** · **dlg**). `FilePicker`는 기존 컨트롤 조립(TextBox·TreeView·TreeGrid·Combo·Checkbox·Button) + `nexa-fs` 모델 · 문자열 주입 · OS 분기 0. `EditState.extra` 다중 선택(전 구간 편집 · Esc 접기). 테스트 211 green · clippy 0. **다음**: F-2 Overlay(창 안 모달) · F-3 PathBar/FileList 가상화 · F-5 ops. → [journal](journal/2026-09-15.md)

## 2026-09-14 (3·4차 · win) — TabBar 이식 ✅ · 블록 선택 ✅ · ★ 구문 강조 엔진(highlight) ✅ · 안내선·공백 표시 ✅

`TabBar`(single ◀▶/드래그 · multiline · `TabAction`) · TextBox 선택 연결 블록 · **`highlight.rs`**(데이터 주도 `SyntaxSpec` · `.nexa-syntax` 파서 · 내장 SQL · `to_html` 서식 복사) · `Theme.syn_*` 4색 · TextBox `set_highlighter/set_rulers/set_whitespace`. 186 테스트 green · clippy 0. 정규식 엔진(자체 NFA)은 후속 — `.sublime-syntax` 호환·찾기 패널·인텔리전스가 같이 쓴다. → [journal](journal/2026-09-14.md)

## 2026-09-14 (2차 · win) — 설계: 그리드 계열(21) — 엔진 1 + 특화 4(결과·파일·접속·단축키)

**요청**(사용자 · nexa-sql 세션): 그리드들은 골격 공유·상속 활용 · dir2 grid 차용 · 결과 그리드 속도·메모리 최우선 · 단축키 설정 그리드 + 캡처 창. → [21](21-grid-family.md): `VirtualRows<S: RowSource>`(dir2 이식) + 오버라이드 지점 표 + `write_cell` 할당 0 훅 + `HotkeyCapture`(clip 단축키 행 이식). **⏳ 사용자**: D-10(별도 크레이트) + D-4~D-9. **다음**: G-1(이식 + DrawCtx 어댑터 · D-3). → [journal](journal/2026-09-14.md)

## 2026-09-14 (1차 · win) — 설계: 파일 관리 · 파일 대화상자(20) — 3-OS 동일 UI · 계층 확장

**요청**(사용자 · nexa-sql 세션): OS별 차이 없는 동일 UI · 파일 관리·파일 Dialog 대폭 개선 · 컨트롤 라이브러리 위에 계층 구조로 확장.
**설계** [20](20-file-management-and-dialogs.md): 원칙(네이티브 대화상자 0 · OS 차이는 `nexa-fs`에만 · 원자→조합→조립→앱) · `nexa-fs`(places·listing·sort·ops·watch(dir2 fsprobe 이식)·naming·kind·path) → `nexa-ctl` 파일 컨트롤 6종 + `Overlay` z 스택 → `nexa-dlg`(Dialog·MessageBox·Prompt·Progress·FilePicker 4종) → 앱. 대화상자 개선 12항(키보드 우선·경로 편집·자연 정렬·필터·저장 검증·최근/즐겨찾기·외부 변경 반영…). 모달 = 창 안 오버레이(3-OS 동일).
**보정**: `nexa-fs`는 dir2 `nexa-vfs`·`nexa-ops`·`nexa-tree`(std 전용) **추출** · beep ADR-0014(네이티브 대화상자 결정 · 구현 0)와 충돌 → **D-9**. **⏳ 사용자**: D-4(모달) · D-5(별도 크레이트) · D-6(아이콘) · D-7(휴지통 범위) · D-8(FileList=nexa-grid 공유) · **D-9(beep ADR-0014 정정)**. **다음**: D-5 답 → F-1 `nexa-fs`. → [journal](journal/2026-09-14.md)

## 2026-09-12 (2차 · mac) — ★ `nexa-font` 신설 · `Rect::intersection` · nexa-sql이 첫 소비자로 연결

**요청**(사용자): *"한글 처리와 고정폭 폰트 등을 잘 지원"* · *"쿼리·결과 파트 외에는 일반 폰트 · 영역별 폰트 구성"*.
**산출**: `crates/nexa-font`(clip `nclip-plat/font.rs` + `conf::load_ui_font` 이관 · `ui_font()` = 한글 UI + 기호 폴백 + 고정폭 폴백 · ★ `mono_font()` = **한글 고정폭(D2Coding·Sarasa·NanumGothicCoding) 우선 → OS 고정폭 → 한글 UI 폴백** · memmap2 원장 등재) · `nexa-ctl::Rect::intersection`.
**실측**: nexa-font 4 테스트(이 mac에서 mono가 '가'를 커버 ✓) · 워크스페이스 green. nexa-sql GUI(`--smoke`)가 UI/고정폭 체인을 로드.
→ [journal/2026-09-12](journal/2026-09-12.md)

## 2026-09-12 (1차 · mac) — ★ 저장소 생성 · nexa-clip에서 3 크레이트 추출 · v0.1.0

**요청**(사용자): *"지금까지 개발된 컨트롤들을 별도 라이브러리로 묶어서 재사용 가능한 구조를 먼저 만들어줘"* (nexa-sql 착수 전제).
**판단**: 계보 최신본은 `nexa-clip`(nbeep-ctl 포크 + 디자인 토큰·view_mode·listedit 추가). `nclip-ctl` 의존 = `nclip-gfx` 하나 · `nclip-gfx` = `ab_glyph` 하나 · `nexa-conf` = 0 → **이름 치환만으로 추출 가능**.
**실측**: 복사 + `nclip_gfx→nexa_gfx`·`nclip_ctl→nexa_ctl` 치환 → `cargo fmt --check` ✓ · `clippy -D warnings` ✓ · **테스트 189 green**(gfx 14 · ctl 165 · conf 10).
**미이관**: `nexa-dir2/nexa-gui/widgets`(dock·tabbar·menubar·columns·rows) — DrawCtx 세대 차로 어댑터 필요(U-2).
→ [journal/2026-09-12](journal/2026-09-12.md)

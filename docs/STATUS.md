# STATUS — 지금 상태 한 장

> 시간 역순. 상세는 [journal](journal/), 여기는 요약.

## 2026-09-16 (1차 · mac) — 두부 방지 폰트 폴백 ✅ · TreeGrid 가로 클립 ✅ · `Splitter` ✅ · FilePicker 4건 ✅

맥 실기 첫날. 기호 폴백은 OS별 고정 경로 + 이름 검색(재귀) 2중에 `UI_SYMBOLS` 회귀 테스트(CI 3-OS)로 고정. 3-OS clippy 전부 green. **다음**: F-8 macOS `NSWorkspace` 아이콘 · nexa-sql 쪽 실기 계속. → [journal](journal/2026-09-16.md)

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

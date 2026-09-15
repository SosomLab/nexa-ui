# TODO — 순차 백로그

> ID · 우선(P0~P2) · 규모(소/중/대) · 의존 · 상태(☐/🚧/✅/⏸). 목표순.

| ID | 우선 | 규모 | 항목 | 의존 | 상태 |
|---|:--:|:--:|---|---|:--:|
| **U-1** | P0 | 소 | nexa-sql에서 `nexa-ctl` path 의존 + 창 열어 컨트롤 1개 그리기(수직 슬라이스) | — | ☐ |
| **U-2** | P0 | 중 | dir2 `widgets/{dock,tabbar,menubar}` 이식 — ✅ tabbar(09-14) · ✅ menubar(pulldown) · ☐ dock — `DrawCtx` 합집합(D-3) 후 어댑터 | D-3 | 🚧 |
| **U-4** | P1 | 중 | 자체 정규식 엔진(Thompson NFA · 캡처 · 외부 crate 0) → `.sublime-syntax` 컨텍스트 호환 · 강조 행 시작 상태 캐시 · 찾기 패널 · `TokenKind::Builtin`(`syn_builtin`) | — | ☐ |
| **U-3** | P0 | 대 | `nexa-grid` — 가상화 행 · 컬럼 모델(dir2 `columns.rs`) · 셀 편집 · 클립보드 복사(TSV) | U-2 | ☐ |
| **G-1~G-6** | P0 | 대 | `nexa-grid` 계열([21](21-grid-family.md)) — G-1 dir2 rows/columns 이식+DrawCtx 어댑터 · G-2 `write_cell`/`CellPainter`/`Hierarchy` 훅 · G-3 ResultGrid(컬럼 저장소) · G-4 ConnectionGrid · G-5 FileGrid · G-6 KeymapGrid+HotkeyCapture | D-3 D-10 | ☐ |
| **U-4** | P1 | 중 | 셰이퍼 검토 — 한글 조합 중 글자 인라인 표시 · 합자 · `rustybuzz`(MIT) vs 자체 | — | ☐ |
| **U-5** | P1 | 소 | `DrawCtx`에 italic·밑줄·취소선(편집기 구문 강조 요구) | D-3 | ☐ |
| **U-6** | P2 | 소 | clip/beep 이관 제안 원장(공개 API 변경 시 영향 표기) | — | ☐ |

## 파일 관리 · 파일 대화상자([20](20-file-management-and-dialogs.md) · 사용자 09-14) — 3-OS 동일 UI · 계층 확장
| ID | 우선 | 규모 | 항목 | 의존 | 상태 |
|---|:--:|:--:|---|---|:--:|
| **F-1** | P0 | 중 | `nexa-fs` — ✅ 09-15 places · list(숨김) · sort(자연 · 폴더 먼저) · naming · path(`~`·환경변수·상대) · local_time(FFI) · 테스트 6 · 잔여 = 배치/취소 열거 · kind 아이콘 키 · watch(dir2 `fsprobe.rs`) | D-5 | 🚧 |
| **F-2** | P0 | 소 | `nexa-ctl::Overlay` z 스택(Base→Popup→Modal · 모달 입력 독점) + Combo/CtxMenu 팝업 승격 | D-4 | ☐ |
| **F-3** | P0 | 대 | 파일 컨트롤 6종 — 09-15 1차는 **기존 컨트롤 조립**(TextBox 경로/이름 · TreeView 장소 · TreeGrid 목록 · Combo 필터)으로 대체 · 잔여 = PathBar 브레드크럼 · FileList 가상화(U-3 공유) · NameBox 자동완성 · `docs/ctl` | F-1 U-2 D-8 | 🚧 |
| **F-4** | P0 | 중 | `nexa-dlg` — ✅ 09-15 `FilePicker{Open·Save}` + 하단 부가 콤보(`set_extra` · 인코딩 등)(복합 컨트롤 · 라벨 주입 `PickerLabels` · 정렬·필터·숨김·새 폴더·덮어쓰기 2단 · 테스트 3) · 잔여 = OpenMany·Folder · Dialog 프레임·MessageBox·Prompt · 크기/보기 기억 | F-2 F-3 | 🚧 |
| **F-5** | P1 | 중 | `nexa-fs::ops` + `Progress` — 복사/이동/이름변경/새 폴더/휴지통(3-OS) · 충돌 · 취소 · 워커 | F-1 F-4 D-7 | ☐ |
| **F-6** | P0 | 소 | nexa-sql 배선 — ✅ 09-15 열기/저장/다른 이름으로 · `DroppedFile` · 최근 파일(`file_win.rs` 모달 창) · 잔여 = 프로젝트 폴더 · export 경로 | F-4 | 🚧 |
| **F-7** | P2 | 소 | clip(첨부·내보내기) · beep(전송 파일 선택) · dir2(소비자 전환) 이관 제안 | F-5 | ☐ |
| **F-8** | P1 | 중 | `nexa-fs::shell` OS 아이콘 — ✅ 09-15 Windows(`SHGetFileInfoW` · 워커 스레드 · 전역 캐시 512 · 종류 이름) · 잔여 = macOS(`NSWorkspace`/UTType objc FFI) · Linux(freedesktop 테마 PNG · 자체 디코더) · 큰 아이콘(32/48) · 아이콘 보기 | F-4 D-6 | 🚧 |


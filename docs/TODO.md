# TODO — 순차 백로그

> ID · 우선(P0~P2) · 규모(소/중/대) · 의존 · 상태(☐/🚧/✅/⏸). 목표순.

| ID | 우선 | 규모 | 항목 | 의존 | 상태 |
|---|:--:|:--:|---|---|:--:|
| **U-1** | P0 | 소 | nexa-sql에서 `nexa-ctl` path 의존 + 창 열어 컨트롤 1개 그리기(수직 슬라이스) | — | ☐ |
| **U-2** | P0 | 중 | dir2 `widgets/{dock,tabbar,menubar}` 이식 — `DrawCtx` 합집합(D-3) 후 어댑터 | D-3 | ☐ |
| **U-3** | P0 | 대 | `nexa-grid` — 가상화 행 · 컬럼 모델(dir2 `columns.rs`) · 셀 편집 · 클립보드 복사(TSV) | U-2 | ☐ |
| **U-4** | P1 | 중 | 셰이퍼 검토 — 한글 조합 중 글자 인라인 표시 · 합자 · `rustybuzz`(MIT) vs 자체 | — | ☐ |
| **U-5** | P1 | 소 | `DrawCtx`에 italic·밑줄·취소선(편집기 구문 강조 요구) | D-3 | ☐ |
| **U-6** | P2 | 소 | clip/beep 이관 제안 원장(공개 API 변경 시 영향 표기) | — | ☐ |

## 파일 관리 · 파일 대화상자([20](20-file-management-and-dialogs.md) · 사용자 09-14) — 3-OS 동일 UI · 계층 확장
| ID | 우선 | 규모 | 항목 | 의존 | 상태 |
|---|:--:|:--:|---|---|:--:|
| **F-1** | P0 | 중 | `nexa-fs` — places · listing(배치·취소) · sort(자연·한글) · naming · kind · path · watch(dir2 `fsprobe.rs` 이식) · 3-OS 테스트 | D-5 | ☐ |
| **F-2** | P0 | 소 | `nexa-ctl::Overlay` z 스택(Base→Popup→Modal · 모달 입력 독점) + Combo/CtxMenu 팝업 승격 | D-4 | ☐ |
| **F-3** | P0 | 대 | 파일 컨트롤 6종 — PathBar · PlacesList · FileList(dir2 columns/rows 이식 = U-3 공유) · FileTree · NameBox · FilterCombo · `docs/ctl` 문서 | F-1 U-2 D-8 | ☐ |
| **F-4** | P0 | 중 | `nexa-dlg` — Dialog 프레임 · MessageBox · Prompt · FilePicker{Open·OpenMany·Save·Folder} · 크기/보기 기억 · i18n 어휘(`CtlMsg` 확장) | F-2 F-3 | ☐ |
| **F-5** | P1 | 중 | `nexa-fs::ops` + `Progress` — 복사/이동/이름변경/새 폴더/휴지통(3-OS) · 충돌 · 취소 · 워커 | F-1 F-4 D-7 | ☐ |
| **F-6** | P0 | 소 | nexa-sql 배선 — 열기/저장/다른 이름으로/프로젝트 폴더/export 경로 · `DroppedFile` · 최근 파일(nexa-sql 저장소) | F-4 | ☐ |
| **F-7** | P2 | 소 | clip(첨부·내보내기) · beep(전송 파일 선택) · dir2(소비자 전환) 이관 제안 | F-5 | ☐ |

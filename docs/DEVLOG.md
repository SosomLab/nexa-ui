# DEVLOG — 날짜별 요약

> 시간 역순. 상세는 journal.

- **2026-09-14 (2차 · win)** — 설계 [21](21-grid-family.md): 그리드 계열 — dir2 `VirtualRows`+`RowSource` 엔진 한 벌 + ResultGrid(속도·메모리 · `write_cell` 할당 0)/FileGrid/ConnectionGrid/KeymapGrid(+HotkeyCapture) · G-1~G-6 · D-10. → [journal](journal/2026-09-14.md)
- **2026-09-14 (1차 · win)** — 설계 [20](20-file-management-and-dialogs.md): 파일 관리·파일 대화상자 — 네이티브 대화상자 0 · `nexa-fs`(OS 차이 한 층) → 파일 컨트롤 6종 + `Overlay` → `nexa-dlg` FilePicker · 모달 = 창 안 오버레이 · D-4~8 · F-1~7. 코드 0. → [journal](journal/2026-09-14.md)
- **2026-09-12 (3차)** — GitHub `SosomLab/nexa-ui` 생성 · 첫 push.
- **2026-09-12 (2차)** — `nexa-font` 신설(한글 UI·한글 고정폭 우선·기호 폴백) · `Rect::intersection` · nexa-sql 첫 소비.
- **2026-09-12** — 저장소 생성. nexa-clip에서 `nclip-gfx`·`nclip-ctl`·`nexa-conf` 추출(이름만 변경) · 189 테스트 green · 문서 골격·CI. → [journal](journal/2026-09-12.md)

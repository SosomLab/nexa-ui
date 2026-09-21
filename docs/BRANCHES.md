# BRANCHES — 브랜치 이력

> 시간 역순. 생성/병합/삭제/커밋수/요약.

| 브랜치 | 생성 | 병합 | 커밋 | 요약 |
|---|---|---|---|---|
| feat/drag-autoscroll-tick | 2026-09-21 | 2026-09-21 → main(삭제) | 1 | 52차(win) — `TextBox` 드래그 자동 스크롤 = 틱 기반(`drag_autoscroll_active` · 포인터가 밖에 멈춰 있어도 이어진다 · 새 타이머 0) · `nexa-fs` 감시 시험 = 조건 대기(`settle`) (nexa-sql 89차 추가 24) |
| feat/popup-placement-secret-box | 2026-09-21 | 2026-09-21 → main(삭제) | 1 | 51차(win) — `geom` 팝업 배치 규칙(+ `place_popup_beside` · `ContextMenu::open_beside`) · `TabBar` 끌기 고스트 · `TextBox` 영역 밖 끌기 줄 단위 · `nexa-dlg` 폴더 고르기 · 가린 입력란 복사 금지 + `wipe`(nexa-sql 89차) |
| fix/mac-hangul-app-compose | 2026-09-20 | 2026-09-21 → main(삭제) | 4 | 49~50차(mac) — TextBox 한글 앱 조합 · nexa-sys `input_source` · nexa-sys `layer_present`(IOSurface 화면 내보내기 · nexa-sql T-139 · T-147) |
| docs/portable-rules | 2026-09-20 | 2026-09-20 → main(삭제) | 1 | 48차 — CLAUDE.md §3-1(편집기 코어 불변식 · 세션 공통 규칙 — 다른 PC에서 이어 가기 · nexa-sql docs/61) |
| feat/textbuf-undo-followups | 2026-09-20 | 2026-09-20 → main(삭제) | 1 | ★ 46~47차 — 되돌리기 = 연산 기록 · **`TextBuf`(UTF-8 갭 버퍼 + 줄 표 + 변경 기록)** · 그리기 = 보이는 줄만 · `PreparedText` · `merge3::line_edits` · 거대 편집 확인 · 기록 파일 내보내기/들이기 · 고정폭 행 폭 지름길 · `bench_undo` |
| feat/tooldock-ghost-editor-perf | 2026-09-19 | 2026-09-19 → main(삭제) | 5 | ToolDock 고스트·다중 행·Esc · CtxMenu 체크박스/with_mark · polyline 클립 · TextBox 성능(rev·폭 캐시·슬라이스 줄 · 2 MB 195→13 ms)·목표 열·경고 띠 · bench_editor(33~40차 · nexa-sql 09-19) |
| feat/tab-badge-textbox | 2026-09-18 | 2026-09-18 → main(삭제) | 3 | `TabBadge` 탭 앞 표식 · `item_rect` · TextBox 단일행 표시 규칙(32차 · nexa-sql DR-34) |
| main | 2026-09-12 | — | — | 초기 추출 |

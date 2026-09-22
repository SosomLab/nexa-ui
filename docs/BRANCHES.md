# BRANCHES — 브랜치 이력

> 시간 역순. 생성/병합/삭제/커밋수/요약.

| 브랜치 | 생성 | 병합 | 커밋 | 요약 |
|---|---|---|---|---|
| feat/win-61-soft-undo | 2026-09-22 | 2026-09-22 → main(삭제) | 1 | 61차(win): `EditState` 선택 되돌리기(Sublime soft undo/redo · `SoftStep` · `note_sel` · `key()` 이동 · Shift 합침) · `TextBox::soft_undo/soft_redo` · 다중 캐럿 ←/→ 접기 · ★ `max_regions` 다중 선택 상한(전 입구) · `selected_bytes` 공개 · nexa-dlg `set_multi`(용도별 단일 선택) · `TextBox::set_gutter_labels` · 🔧 다중 캐럿 세로 추종 유지 · 테스트 322 |
| feat/win-58-60-default-item-outside-click-submenu | 2026-09-22 | 2026-09-22 → main(삭제) | 1 | 58~60차(win): `ContextMenu::set_default` · `TreeGrid::set_caret_outline` · nexa-dlg Enter=ConfirmMany · `TextBox::set_popup_deferred` · `MenuBar::dismiss` · 하위 메뉴 비활성 행 닫힘 · ★ `ContextMenu::is_outside_click` · `skip_next_occurrence`/`remove_region` · ★ 풀다운 하위 메뉴 `MenuEntry::Sub` |
| feat/win-56-57-tree-reveal-marked-paths-ellipsis-band | 2026-09-22 | 2026-09-22 → main(삭제) | 1 | 56·57차(win): `TreeControl::reveal_row`+PageUp/Down/Home/End · `TreeGrid::set_marked_paths`(강조 = 노드 경로) · `draw::ellipsize_middle`+`set_show_full`(Alt) · `MenuBar::set_max_label_width` · `ContextMenu` 라벨 축약 · nexa-dlg Ctrl+드래그 스윕·러버밴드(빈 공간·행) |
| feat/win-55-tabbar-group-treegrid-marked-dlg-multi | 2026-09-22 | 2026-09-22 → main(삭제) | 1 | 55차(win · nexa-sql 93차 지원): `ScrollBars` 축별 가장자리 깨움 · `TextBox` 가로 범위 통일(`ml_bars_w`)+클릭 잔여 px 유지 · `TabBar` 클릭 수식키 + 묶인 탭 상단 줄(`set_group`) · `TreeGrid::set_marked` · nexa-dlg 열기 모드 다중 선택(`marks` · `ConfirmMany` · 테스트) |
| docs/ci-result-54 | 2026-09-22 | 2026-09-22 → main(삭제) | 1 | 54차 push CI 결과(e5433cb = macOS · Ubuntu · Windows ✓ — `file_type()` + 걷기 캐시가 Windows에 돌아온 상태에서 통과) (nexa-sql 92차) |
| fix/font-gdi-test-race | 2026-09-22 | 2026-09-22 → main(삭제) | 1 | 54차(win) — 53차 Windows CI `test` 실패의 원인 = nexa-font 시험끼리의 `set_text_gdi` 경주(`file_type()` 아님 · `gh` 로그) → 시험 가드 `tests::GdiOn` · 걷기 캐시 + `file_type()` 3-OS 복귀(Windows 걷기 12.6 → 1.2 ms · 한 번만) · 진단 `time_font_walk` (nexa-sql 92차) |
| perf/font-walk-symlink · fix/font-walk-windows · fix/font-walk-cfg · fix/font-walk-win-identical | 2026-09-22 | 2026-09-22 → main(삭제) | 4 | 53차 보완 1~4 — 링크 폴더 안전 · 캐시 Linux·mac 한정 · cfg dead_code · Windows `is_dir()` 종전대로 → CI 3-OS 초록(b3d8e8b) |
| perf/font-walk-cache | 2026-09-22 | 2026-09-22 → main(삭제) | 1 | 53차(linux) — nexa-font 가족 탐색 걷기 1회 캐시 + `file_type()`(Linux 기동 병목 · nexa-sql 91차) |
| feat/drag-autoscroll-tick | 2026-09-21 | 2026-09-21 → main(삭제) | 1 | 52차(win) — `TextBox` 드래그 자동 스크롤 = 틱 기반(`drag_autoscroll_active` · 포인터가 밖에 멈춰 있어도 이어진다 · 새 타이머 0) · `nexa-fs` 감시 시험 = 조건 대기(`settle`) (nexa-sql 89차 추가 24) |
| feat/popup-placement-secret-box | 2026-09-21 | 2026-09-21 → main(삭제) | 1 | 51차(win) — `geom` 팝업 배치 규칙(+ `place_popup_beside` · `ContextMenu::open_beside`) · `TabBar` 끌기 고스트 · `TextBox` 영역 밖 끌기 줄 단위 · `nexa-dlg` 폴더 고르기 · 가린 입력란 복사 금지 + `wipe`(nexa-sql 89차) |
| fix/mac-hangul-app-compose | 2026-09-20 | 2026-09-21 → main(삭제) | 4 | 49~50차(mac) — TextBox 한글 앱 조합 · nexa-sys `input_source` · nexa-sys `layer_present`(IOSurface 화면 내보내기 · nexa-sql T-139 · T-147) |
| docs/portable-rules | 2026-09-20 | 2026-09-20 → main(삭제) | 1 | 48차 — CLAUDE.md §3-1(편집기 코어 불변식 · 세션 공통 규칙 — 다른 PC에서 이어 가기 · nexa-sql docs/61) |
| feat/textbuf-undo-followups | 2026-09-20 | 2026-09-20 → main(삭제) | 1 | ★ 46~47차 — 되돌리기 = 연산 기록 · **`TextBuf`(UTF-8 갭 버퍼 + 줄 표 + 변경 기록)** · 그리기 = 보이는 줄만 · `PreparedText` · `merge3::line_edits` · 거대 편집 확인 · 기록 파일 내보내기/들이기 · 고정폭 행 폭 지름길 · `bench_undo` |
| feat/tooldock-ghost-editor-perf | 2026-09-19 | 2026-09-19 → main(삭제) | 5 | ToolDock 고스트·다중 행·Esc · CtxMenu 체크박스/with_mark · polyline 클립 · TextBox 성능(rev·폭 캐시·슬라이스 줄 · 2 MB 195→13 ms)·목표 열·경고 띠 · bench_editor(33~40차 · nexa-sql 09-19) |
| feat/tab-badge-textbox | 2026-09-18 | 2026-09-18 → main(삭제) | 3 | `TabBadge` 탭 앞 표식 · `item_rect` · TextBox 단일행 표시 규칙(32차 · nexa-sql DR-34) |
| main | 2026-09-12 | — | — | 초기 추출 |

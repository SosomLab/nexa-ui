# DEVLOG — 날짜별 요약

> 시간 역순. 상세는 journal.

- **2026-09-14 (7차 · win)** — ★ `tokens::IntentFade`(hover **의도 코얼레싱** + 페이드 · 사건은 목표 덮어쓰기만 · 70ms 머문 마지막 목표만 페이드 · 이탈 즉시) · `FadeSpeed { Fast, Slow }` 컨트롤 속성(`Button::set_fade_speed` · `ComboControl::set_fade_speed` · `HoverFade/IntentFade::with_speed`) · `set_fade_ms(speed, ms)` 전역 연계 · 콤보 드롭다운 항목 hover = IntentFade(Fast · 선택색 알파 페이드 · 키보드 `jump`) · `HoverFade::jump` · 테스트 +2.
- **2026-09-14 (6차 · win)** — `Button`: hover 진입 = 별도 전역 `button_hover_in_ms`(기본 500 · `Fade::button_hover`) · 눌림 = 선택색 위 전경색 0.10 한 겹 + 내용 1px 내려앉음(`PRESSED_EXTRA`) · `is_animating`/`is_pressed` 공개.
- **2026-09-14 (5차 · win)** — `tokens::set_hover_in_ms/hover_in_ms`(hover 진입 시간 프로세스 전역 · `Fade::hover()`가 읽음 · nexa-sql 설정 `grid.hover_fade` 1000ms → 버튼·콤보·트리·그리드 행 공통 반영).
- **2026-09-14 (4차 · win)** — `ScrollBars` **축별 표시**(nexa-sql 사용자 09-14 "상하 스크롤 때 좌우 막대는 안 보여도 된다"): 세로 휠 = 세로만 · 가로 휠 = 가로만 · 숨김 시각·활동 플래그 축별 · 보이는 축의 썸만 잡힘 · `show()`는 둘 다 · 테스트 +1(`wheel_wakes_only_its_axis`).
- **2026-09-14 (3차 · win)** — nexa-sql 요구로 컨트롤 보강: `TextBox` 캐럿/선택 인덱스 공개 · 멀티라인 붙여넣기 탭 보존 · `scrollbars_visible()` · **줄번호 거터**(`set_line_numbers`) · `ToolIcon::Glyph`. TabBar 이식(dir2 → nexa-ctl · U-2) 진행. → [journal](journal/2026-09-14.md)
- **2026-09-14 (2차 · win)** — 설계 [21](21-grid-family.md): 그리드 계열 — dir2 `VirtualRows`+`RowSource` 엔진 한 벌 + ResultGrid(속도·메모리 · `write_cell` 할당 0)/FileGrid/ConnectionGrid/KeymapGrid(+HotkeyCapture) · G-1~G-6 · D-10. → [journal](journal/2026-09-14.md)
- **2026-09-14 (1차 · win)** — 설계 [20](20-file-management-and-dialogs.md): 파일 관리·파일 대화상자 — 네이티브 대화상자 0 · `nexa-fs`(OS 차이 한 층) → 파일 컨트롤 6종 + `Overlay` → `nexa-dlg` FilePicker · 모달 = 창 안 오버레이 · D-4~8 · F-1~7. 코드 0. → [journal](journal/2026-09-14.md)
- **2026-09-12 (3차)** — GitHub `SosomLab/nexa-ui` 생성 · 첫 push.
- **2026-09-12 (2차)** — `nexa-font` 신설(한글 UI·한글 고정폭 우선·기호 폴백) · `Rect::intersection` · nexa-sql 첫 소비.
- **2026-09-12** — 저장소 생성. nexa-clip에서 `nclip-gfx`·`nclip-ctl`·`nexa-conf` 추출(이름만 변경) · 189 테스트 green · 문서 골격·CI. → [journal](journal/2026-09-12.md)

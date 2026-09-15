# DEVLOG — 날짜별 요약

> 시간 역순. 상세는 journal.

- **2026-09-15 (4차 · win)** — `ContextMenu` 아이콘(`MenuIcon`)·단축키·**하위 메뉴** · `EditMenu` 아이콘/단축키 주입 · 툴바 hover 배경 제거 · TextBox 선택 = 텍스트 범위 · `FilePicker::set_extra`(하단 부가 콤보 · 인코딩). → [journal](journal/2026-09-15.md)
- **2026-09-15 (3차 · win)** — ★ **글리프 비트맵 캐시**(`Font.cache` · `GlyphBitmap` · `Surface::blend_mask` 정수 블렌드 · 서브픽셀 1/3 · 상한 8192) · `IconImage::resized` + 같은 크기 빠른 경로 — nexa-sql 탐색기 성능 조사(A·B). → [journal](journal/2026-09-15.md)
- **2026-09-15 (2차 · win)** — ★ 새 크레이트 `nexa-fs`(파일시스템 중립 모델 · OS 분기 격리) · `nexa-dlg`(`FilePicker` 열기/저장 복합 컨트롤 · docs/20 F-1/F-4) · `EditState` 다중 선택/캐럿(Ctrl+D · 열 선택) · `TextBox` 붙여넣기 탭→공백 · 더블클릭 시간 판정 · 탭 폭 자기 주입. 소비자: nexa-sql T-74. → [journal](journal/2026-09-15.md)
- **2026-09-15 (1차 · win)** — `tokens::set_intent_ms/set_fade_out_ms`(hover 의도·나감 시간 전역 · nexa-sql 비노출 설정 연동) · `TextBox` 멀티라인 Tab 문자 삽입(단일 행은 포커스 이동 유지).
- **2026-09-14 (16차 · win)** — `Button::clear_transient`(hover·눌림·포커스 초기화 — 가려지거나 교체될 때 호스트가 부른다).
- **2026-09-14 (15차 · win)** — `ControlBase.enabled` + `Control::set_enabled/is_enabled`(전 컨트롤 상속) · `Button` 비활성 = 입력·hover·포커스 무시 · 흐린 글자 + 바탕 한 겹.
- **2026-09-14 (14차 · win)** — `TimeoutButton::with_show_remaining/set_show_remaining`(잔여 초 표시 여부 · 끄면 라벨 + 게이지만).
- **2026-09-14 (13차 · win)** — `TimeoutButton::tick`: 카운트다운 중 매 틱 재그리기(게이지 아날로그처럼 부드럽게 · 숫자는 초 단위) · 테스트 갱신.
- **2026-09-14 (12차 · win)** — `TimeoutButton::with_two_line`(라벨 / (N초) 두 줄 · 행간 0 · 글자 2px 작게).
- **2026-09-14 (11차 · win)** — `TextBox` 단일 행 hover 페이드(회색 계열 · `Slow` · `tick`/`is_animating`) · `ColorPanel::tick/is_animating`(hex 입력란) — nexa-sql 사용자 09-14 "텍스트박스 등에도 1초 진해지는 효과".
- **2026-09-14 (10차 · win)** — `ComboCore::items/selected_index` 공개(호스트의 "선택 항목/목록 복사" 메뉴용).
- **2026-09-14 (9차 · win)** — ★ **`ColorPanel`**(색 선택기 · 채도/명도 사각형 + 색상·투명도 막대 + `#RRGGBBAA` 입력 + 프리셋 12 + 최근 8 · HSV 보관 · 그라데이션은 코드 이미지 · 드래그 중 실시간 보고 · 테스트 3) · `tokens::set_hover_color/set_pressed_color`(0xRRGGBBAA · 버튼 hover/눌림 · 콤보 항목 공통) · 버튼 hover = 선택색 계열(전경색 오버레이 → hover 색 × 진행도) · `rgba_from_hex/rgba_to_hex` 재수출.
- **2026-09-14 (8차 · win)** — `TimeoutButton`: `with_warn`(대기 내내 위험색 배경·흰 글씨·흰 게이지 — 파괴적 2단 확인용) · `with_suffix`(남은 초 단위 i18n · 기본 "초").
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

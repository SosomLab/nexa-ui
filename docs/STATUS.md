# STATUS — 지금 상태 한 장

> 시간 역순. 상세는 [journal](journal/), 여기는 요약.

## 2026-09-12 (1차 · mac) — ★ 저장소 생성 · nexa-clip에서 3 크레이트 추출 · v0.1.0

**요청**(사용자): *"지금까지 개발된 컨트롤들을 별도 라이브러리로 묶어서 재사용 가능한 구조를 먼저 만들어줘"* (nexa-sql 착수 전제).
**판단**: 계보 최신본은 `nexa-clip`(nbeep-ctl 포크 + 디자인 토큰·view_mode·listedit 추가). `nclip-ctl` 의존 = `nclip-gfx` 하나 · `nclip-gfx` = `ab_glyph` 하나 · `nexa-conf` = 0 → **이름 치환만으로 추출 가능**.
**실측**: 복사 + `nclip_gfx→nexa_gfx`·`nclip_ctl→nexa_ctl` 치환 → `cargo fmt --check` ✓ · `clippy -D warnings` ✓ · **테스트 189 green**(gfx 14 · ctl 165 · conf 10).
**미이관**: `nexa-dir2/nexa-gui/widgets`(dock·tabbar·menubar·columns·rows) — DrawCtx 세대 차로 어댑터 필요(U-2).
→ [journal/2026-09-12](journal/2026-09-12.md)

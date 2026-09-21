# CLAUDE.md — Nexa UI 프로젝트 컨텍스트 (이식용 메모리)

> 이 파일은 **다른 PC에서 clone 시 즉시 컨텍스트를 공유**하기 위한 휴대용 프로젝트 메모리다.
> **먼저 읽기:** [docs/STATUS.md](docs/STATUS.md)(현황) → [docs/10-decision-record.md](docs/10-decision-record.md)(결정).

## 1. 이 프로젝트는

**Nexa UI** = **nexa 계열 공용 UI 라이브러리**(Windows · macOS · Linux). 앱이 아니라 **라이브러리 워크스페이스**다.
**올 러스트 · 자체 CPU 래스터라이저 · 프레임워크 0**(Qt·WebView·Electron 없음) — 소비 앱이 3-OS 완전 동일 화면을 그리게 하는 기반.

- 조직: **SosomLab** · 개발자: Sangyong Bae · kiros33@gmail.com
- 저장소: <https://github.com/SosomLab/nexa-ui> · 라이선스: **PolyForm Noncommercial 1.0.0**
- 현 단계: **v0.1.0 추출 완료(2026-09-12)** — `nexa-clip`에서 3 크레이트 이관 · 189 테스트 green. 첫 소비자 = **`nexa-sql`**.

### 계보 (재발명 금지 — 원본은 여기다)

| 크레이트 | 출처 | 규모 | 의존 |
|---|---|---|---|
| `nexa-gfx` | `../nexa-clip/crates/nclip-gfx` ← `nexa-beep/nbeep-gfx` | 745 LOC | `ab_glyph` |
| `nexa-ctl` | `../nexa-clip/crates/nclip-ctl` ← `nbeep-ctl` ← `nexa-dir2/nexa-gui` | 12,910 LOC · 컨트롤 17종 · 디자인 토큰 | `nexa-gfx` |
| `nexa-conf` | `../nexa-clip/crates/nexa-conf` (= `nexa-beep` 사본과 동일) | 599 LOC | 0 |
| `nexa-font` | `../nexa-clip/crates/nclip-plat/src/font.rs` + `conf::load_ui_font` | 한글 UI·한글 고정폭 우선·기호 폴백 | `nexa-gfx` · memmap2 |

> ⚠️ **아직 이관 안 한 것**: `nexa-dir2/crates/nexa-gui/widgets`(dock · tabbar · menubar · columns · rows — 7,648 LOC · Win32 앱 안에 있으나 플랫폼 중립). `DrawCtx` 어휘가 한 세대 앞서 있어(`select_font` 시그니처 · `term_text`·`draw_image`) **이식 시 어댑터 필요** → [docs/TODO](docs/TODO.md) U-2.

## 2. 확정 결정 (요약 — 전문은 [docs/10](docs/10-decision-record.md))

| # | 결정 |
|---|---|
| DR-1 | **공통 UI를 복사가 아니라 공유 라이브러리로** — `nexa-clip` DR-18의 실행. 이름은 앱 접두사 없이 `nexa-*` |
| DR-2 | **앱 도메인 의존 0** — 문자열(i18n)·크기 배율 등 앱 정책은 주입(`set_ctl_labels` · `set_control_size_mult`) |
| DR-3 | 외부 crate 기본 0 지향 — 현재 `ab_glyph` 1개. 추가는 docs/10 §3 원장 |
| DR-4 | **소비 앱 이관은 각 앱 저장소의 결정** — nexa-ui는 path 의존으로 먼저 `nexa-sql`이 쓰고, clip/beep은 안정화 후 |
| DR-5 | 라이선스 = PolyForm NC 1.0.0 (계열 동일). ⚠️ 향후 SDK성 크레이트를 MIT로 분리할지는 열린 결정(D-1) |

## 3. 작업 규약

- **문서·커밋/푸시 규약 SSOT = [docs/16](docs/16-doc-git-conventions.md)** — 4층 문서 · 커밋 규칙 · **push는 사용자 명시 요청 시에만**.
- 스테이징은 `git add <파일>`. **`git add -A`·`git add .` 금지.**
- push 전 `scripts/check-3os.sh`(fmt + 3타깃 clippy `-D warnings`).
- ★ **공개 API 변경은 소비자 영향 표기** — 커밋 본문에 `영향: nexa-sql | clip | beep`.
- `.claude/settings.json`은 덮어쓰기 금지, 병합만.

### 3-0. 팝업 배치 규칙(nexa-sql 사용자 09-21 "우클릭 메뉴·툴팁이 화면에서 잘리지 않게")

- 떠서 그려지는 것(메뉴 · 하위 메뉴 · 툴팁 · 드롭다운)은 **그리기 표면 밖으로 나가지 않는다.** 위치는 `geom::place_popup`(정방향 → 반대쪽 → 밀어 넣기) · 이미 놓인 것은 `geom::nudge_into` · 영역은 `geom::popup_host(host, ctx.surface_size())` — 컨트롤 안에서 위치를 따로 계산하지 않는다.
- **행(대상)에서 연 메뉴는 그 대상을 가리지 않는다**(nexa-sql 09-21 — 트리 우클릭): `ContextMenu::open_beside(x, y, 대상 rect, …)` → `geom::place_popup_beside`(대상 **바로 아래** → 자리가 없으면 **바로 위** → 둘 다 안 되면 일반 규칙 · 가로는 누른 x에서 일반 규칙). paint의 안전망도 같은 규칙으로 다시 놓는다. 메뉴를 담는 패널의 폭에 가두지 말고 **창 전체**를 host로 주고 창의 팝업 층에서 그린다.
- **가린 입력란(`set_masked`)은 비밀 값이다**: 복사·잘라내기를 하지 않는다 · 호스트는 값을 `take_secret_text()`로 꺼낸다(꺼내는 즉시 본문·되돌리기 기록·조합 글을 0으로 덮는다 · `TextBox::wipe` → `EditState::wipe` → `TextBuf::wipe` — `unsafe` 없이 `fill(0)` + `black_box`).
- `DrawCtx::surface_size()`(기본 `None` · `RasterCtx` = 표면 크기)가 안전망의 근거다: `ContextMenu::paint` · `draw::draw_tooltip_in` · `Combo`는 그리는 시점에 표면 크기를 배워 스스로 안으로 들어온다(호출자가 `host`를 잘못 넘겨도 잘리지 않는다). 새 떠 있는 컨트롤도 같은 식으로 만든다.
- 새 떠 있는 컨트롤 = `geom.rs popup_tests`에 배치 사례를 더하고, 호스트 앱에서 창 모서리 근처 캡처 1장.

### 3-1. 세션 공통 규칙 · 편집기 코어 불변식(09-20 · 다른 PC에서도 그대로 — 원문 = nexa-sql [docs/61](../nexa-sql/docs/61-core-design-and-working-rules.md))

- **답은 한글로.** "commit · main 병합 · push" = 작업 브랜치 → 커밋 → `main`에 `--ff-only` → 브랜치 삭제 → `docs/BRANCHES.md` → push(**nexa-ui가 nexa-sql보다 먼저** — path 의존) · 커밋 끝에 그 세션이 안내하는 `Co-Authored-By:` 줄.
- **`TextBuf`**(`nexa-ctl/src/edit/textbuf.rs`) = `EditState`의 저장소: UTF-8 갭 버퍼 + 줄 시작 표 + 줄 변경 기록. **좌표는 글자(char) 인덱스** — 바이트 오프셋은 이 파일 밖으로 나가지 않는다. 불변식: 갭 양 끝 = 글자 경계 · `lines[k]` = k번째 개행 바로 뒤 · 온전한 UTF-8 · `unsafe` 0. 통째 교체는 `set_string`/`adopt`만(세대가 이어서 오른다).
- **본문을 바꾸는 길은 셋뿐**: `splice_rec(_str)` · `replace_many_inner` · `apply_ops`. 새 편집 기능은 이 위에 — `buf.splice`를 직접 부르면 세대(`rev`) · 되돌리기 기록 · 읽기 전용 · 거대 편집 확인이 빠진다. 새 공개 편집 진입점은 `giant_refused` → 본체 → `giant_done`.
- **본문 전체를 `Vec<char>`/`String`으로 뜨는 코드를 자주 도는 길(키 · 그리기 · 틱)에 넣지 않는다** — `buf()`의 줄·구간 조회를 쓴다. `chars_vec()`은 드문 명령·테스트 전용.
- **줄별 캐시 = 변경 기록의 소비자**: `(epoch, seq)` + `changes_since` · 기록이 버려졌거나 세대가 다르면 전부 다시(본보기 `RowWidthCache` · `LineHlCache`). 접기(wrap) 모드만 종전의 본문 문자열 + 행 해시 경로.
- 되돌리기: 저장 = 지운 글자만 · 묶음 안 연산은 뒤에서 앞으로 · 저장 지점 = 상태 id · 기록 파일 형식의 좌표 뜻이 바뀌면 `HISTORY_VERSION`을 올린다.
- 자료 구조를 바꾸면 **단순 모델과 난수 대조 테스트**(자체 xorshift · 한글·이모지·개행 포함)를 같이 넣는다. 수치는 `--release` 벤치로(`examples/bench_editor` · `bench_undo`).
- **프로세스 전역 스위치를 만지는 시험은 가드로 직렬화한다**(시험은 병렬로 돈다) — 본보기 nexa-font `tests::GdiOn`(`set_text_gdi` · 정적 뮤텍스를 쥔 동안 켬 · Drop에서 끔 · 독 무시). 09-22: 가드 없이 켜고 끄던 두 시험이 CI windows-latest에서만 겹쳐 네 번 실패했고, "되돌리니 통과"가 엉뚱한 원인(`file_type()`)을 가리켰다 — **CI 실패는 로그를 본 뒤에 고친다**(`gh run view <id> --log-failed`).
- 공개 API를 바꿨으면 같은 작업 안에서 nexa-sql(`crates/nexa-sql`)과 `nexa-dlg`를 빌드·테스트한다(지금 nexa-ctl의 소비자는 이 둘).


## 4. 새 세션 오리엔테이션

1. 이 CLAUDE.md + [docs/STATUS.md](docs/STATUS.md) → 2. [DEVLOG](docs/DEVLOG.md) 최상단 → 3. [docs/TODO.md](docs/TODO.md).

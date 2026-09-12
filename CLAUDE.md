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

## 4. 새 세션 오리엔테이션

1. 이 CLAUDE.md + [docs/STATUS.md](docs/STATUS.md) → 2. [DEVLOG](docs/DEVLOG.md) 최상단 → 3. [docs/TODO.md](docs/TODO.md).

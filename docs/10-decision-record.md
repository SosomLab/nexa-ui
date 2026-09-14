# 10 · 결정 기록 (DR) · 열린 결정 (D)

> 확정은 DR 표, 미정은 D. 변경 시 과거를 지우지 않고 새 항목으로 정정. **최신 갱신 2026-09-12.**

## 1. 확정 결정 (DR)

| # | 결정 | 근거 | 확정 |
|:--:|---|---|---|
| **DR-1** | **공통 UI를 공유 라이브러리로 분리** — `nexa-clip`에서 `nclip-gfx`·`nclip-ctl`·`nexa-conf`를 이름만 `nexa-*`로 바꿔 이관(기능 무변경) | `nexa-clip` DR-18 *"다음 유사 프로젝트는 공통을 먼저 라이브러리로 분리한 뒤 시작한다"* · 사용자 요청 09-12 *"컨트롤들을 별도 라이브러리로 묶어서 재사용 가능한 구조를 먼저"* | ✅ 사용자 09-12 |
| **DR-2** | **앱 도메인 의존 0** 불변식 계승(nbeep-ctl DR-6 · nclip-ctl docs/13 §2-2) | 이 크레이트의 존재 이유 | ✅ 계승 |
| **DR-3** | 외부 crate 기본 0 지향 — 현재 `ab_glyph`(nexa-gfx) 1개 | 계열 공통 규율 | ✅ 계승 |
| **DR-4** | **소비 앱 이관은 각 저장소의 결정** — nexa-sql이 path 의존으로 먼저 소비 · clip/beep은 API 안정 뒤 | 다른 프로젝트 직접 수정은 승인 대상(clip CLAUDE.md §4) | ✅ 09-12 |
| **DR-5** | 라이선스 PolyForm NC 1.0.0 (계열 동일) | — | ✅ 09-12 |
| **DR-6** | 원본(clip)에 남아 있는 `nclip-*` 출처 주석은 **지우지 않는다**(앱 고유 이름만 `<app>-plat` 식으로 일반화) | 기록의 목적 = 재발명 방지 | ✅ 09-12 |

## 2. 열린 결정 (D)

| # | 내용 | 권장 |
|:--:|---|---|
| D-1 | 플러그인 SDK성 크레이트(향후 `nexa-edit` 패키지 API)는 MIT로 분리할 것인가 | `nexa-dir` DR-6 선례(SDK MIT · 본체 PolyForm) 따름 권장 |
| D-2 | 버전 정책 — 소비자 셋이 같은 커밋을 보게 할 것인가(path) vs 태그 고정(git dep) | 안정화 전까지 path · v0.2부터 태그 |
| D-3 | `DrawCtx` 어휘 통일 — dir2 세대(`select_font(slot,bold,italic)`)와 clip 세대(`select_font(slot,bold)`+`select_font_sized`) 중 무엇으로 | 편집기가 italic을 요구하므로 **합집합** 권장 |
| **D-4** | 모달 방식 — 창 안 오버레이 기본 + Progress만 별도 창(권장) / 전부 별도 창 — [20 §4](20-file-management-and-dialogs.md) |
| **D-5** | `nexa-fs`·`nexa-dlg` = 별도 크레이트(권장 · 의존 방향 강제 · dir2가 fs만 소비 가능) / `nexa-ctl` 안 모듈 |
| **D-6** | 파일 아이콘 = 자체 알파 마스크 세트(beep Lucide 원장 · 3-OS 동일 · 권장) / OS 아이콘 |
| **D-7** | 휴지통 1차 범위 — 3-OS 전부(권장 · shell32 · `.Trash` · XDG) / Windows만 |
| **D-8** | FileList 상세 보기 = U-3 `nexa-grid`와 컬럼 모델 공유(권장) / 분리 |
| **D-9** | ★ beep ADR-0014(네이티브 파일 대화상자 · 구현 0) 정정 — 계열 전체 자체 `FilePicker`(권장) / beep만 예외 — [20 §8](20-file-management-and-dialogs.md) · beep 저장소 결정(DR-4) |

## 3. 외부 crate 원장

| crate | 크레이트 | 사유 | 라이선스 |
|---|---|---|---|
| `ab_glyph 0.2` | nexa-gfx | TTF 파싱·글리프 래스터(계승) | Apache-2.0 |
| `memmap2 0.9` | nexa-font | 시스템 폰트 mmap(clip DR 계승 — 힙 복사 55MB 회피) | MIT/Apache-2.0 |

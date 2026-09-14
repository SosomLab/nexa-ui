# MILESTONES — 기능·목적 관점 현황

> ✅ 완료 / 🚧 진행 / 📐 설계만 / ☐ 미착수. 목표순.

## U0 — 추출 (v0.1)
| 상태 | 항목 |
|:--:|---|
| ✅ | `nexa-gfx` · `nexa-ctl` · `nexa-conf` 이관 · 189 테스트 green |
| ✅ | 문서 골격 · 라이선스 · CI(3-OS fmt/clippy/test) |
| ☐ | nexa-sql이 path 의존으로 첫 소비 |

## U1 — IDE 기반 컨트롤 (nexa-sql이 요구)
| 상태 | 항목 |
|:--:|---|
| ☐ | dock · tabbar · menubar 이식(dir2 → DrawCtx 어댑터) |
| ☐ | `nexa-grid` 가상화 데이터 그리드(10만 행 · 컬럼 리사이즈/정렬/고정) |
| 📐 | `nexa-edit` 다중행 편집기 코어(설계 = nexa-sql docs/07) |
| ☐ | 텍스트 셰이핑(한글 조합 표시 · IME 인라인) |

## U1b — 파일 관리 · 파일 대화상자([20](20-file-management-and-dialogs.md))
| 상태 | 항목 |
|:--:|---|
| 📐 | `nexa-fs`(OS 차이를 가두는 층) · `nexa-ctl` 파일 컨트롤 6종 + `Overlay` · `nexa-dlg`(FilePicker·MessageBox·Prompt·Progress) — 네이티브 대화상자 0 |

## U2 — 소비자 이관
| 상태 | 항목 |
|:--:|---|
| ☐ | nexa-clip → nexa-ui 의존 전환(각 저장소 결정) |
| ☐ | nexa-beep → nexa-ui 의존 전환 |

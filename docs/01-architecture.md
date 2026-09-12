# 01 · 아키텍처 — 크레이트 지도와 경계

> 출처 코드의 설계 문서는 원본 저장소에 있다(`nexa-clip/docs/13`·`20`·`25`, `nexa-beep/docs/14`). 여기는 **라이브러리 관점의 경계**만 적는다.

## 1. 계층

```
소비 앱 (nexa-sql · nexa-clip · nexa-beep)
   │  창·OS 이벤트·클립보드·폰트 경로·i18n 문자열 = 앱의 plat 계층이 소유
   ▼
nexa-ctl   DrawCtx(어휘) · Widget(계약) · Control/ControlBase(공통 상태) · controls/* 17종 · tokens(디자인 시스템)
   │  raster 어댑터 안에서만 nexa-gfx 타입을 만진다(공개 시그니처에 외부 타입 0)
   ▼
nexa-gfx   Surface(픽셀 버퍼) · Font/TextStyle(ab_glyph 셰이핑 없는 글리프 래스터) 
nexa-conf  (독립) 설정 포맷 · 미지 키 보존 · 원자적 쓰기 · 저장 스케줄
```

## 2. 컨트롤 17종 (`nexa-ctl::controls`)

button · timeout_button · checkbox · radio · switch · textbox(1,657 LOC · 단일행 편집 `EditState`) · combo · pulldown · listedit · tree(TreeView/TreeGrid) · toolbar · ctxmenu · editmenu · icondrop · colorpick · posgrid · carousel · scroll(ScrollBars).

## 3. 아직 없는 것 (nexa-sql이 필요로 하는 것)

| 필요 | 현재 | 계획 |
|---|---|---|
| **도킹 패널 · 탭 그룹 · 메뉴바** | `nexa-dir2/nexa-gui/widgets/{dock,tabbar,menubar}.rs` (DrawCtx 세대 차) | U-2 어댑터 이식 → `nexa-ctl::widgets` |
| **가상화 데이터 그리드**(10만 행 · 컬럼 리사이즈/정렬) | `nexa-gui/{columns,rows}.rs` + `nexa-ctl::TreeGrid` | U-3 `nexa-grid` 크레이트 |
| **다중행 코드 편집기** | `textbox`는 단일행 | ★ `nexa-edit` 크레이트(nexa-sql docs/07 설계) — rope · 다중 커서 · 구문 강조 |
| **텍스트 셰이핑**(한글 조합·합자·복합 스크립트) | `ab_glyph` 글리프 단위 | U-4 셰이퍼 검토(자체 vs `rustybuzz`) — 편집기가 요구 |

# 21 · 그리드 계열 — 한 골격(`VirtualRows` + `RowSource`) · 세 특화(파일 · 결과 · 접속 목록)

> **요청**(사용자 09-14 · nexa-sql 세션): *"파일 목록 Grid · Table 결과 Grid · 접속 목록 Grid는 목적별 세부 특징은 달라도 기본 Grid 형태 · 글리프로 계층 표현 등 골격은 동일하고 핵심 기능은 공유 — 상속 관계를 최대한 활용. 그리드 기술 구조는 nexa-dir2의 grid를 차용해서 시작. 특히 Table 조회 결과 Grid는 속도가 생명이고 실제 메모리가 적을수록 유리."*
> **원본**: `nexa-dir2/crates/nexa-gui/src/{columns.rs, widgets/rows.rs}`(3,212 LOC · 의존 0 · 147+ 테스트) — 이미 **"엔진 1개 + 데이터 트레이트"** 구조라 상속 요구에 정확히 맞는다.
> **연관**: [20 파일 관리·대화상자](20-file-management-and-dialogs.md)(FileList = 이 계열의 파일 특화) · nexa-sql [26 성능](../../nexa-sql/docs/26-performance-architecture.md)(결과 그리드의 속도·메모리 규칙) · U-3.

---

## 0. Rust에서 "상속"을 어떻게 쓰나

Rust에는 클래스 상속이 없다. 같은 효과를 **세 층으로** 얻는다 — dir2가 이미 그렇게 짜여 있다.

| 층 | 수단 | 무엇을 공유하나 |
|---|---|---|
| **엔진**(공통 구현) | 제네릭 구조체 `VirtualRows<S: RowSource>` | 가상화 · 스크롤 · 헤더(3단 정렬 · 다중 정렬 배지 · 드래그 리사이즈) · 선택(단일/범위/토글) · 키보드 탐색 · 타입어헤드 · 뷰 모드 · 페인트 루프 · 히트 테스트 — **한 벌** |
| **데이터 계약**(가상 메서드) | 트레이트 `RowSource`(기본 구현 있는 메서드 = 오버라이드 가능) | `len` · `row(i) -> RowItem{text, is_dir, depth, marker}` · `cell(i, key)` · `toggle`(접기) · `set_sort` · 선택 상태 |
| **특화**(서브클래스) | `RowSource` 구현체 + 필요하면 엔진을 감싼 뉴타입 | `FileSource` · `ResultSource` · `ProfileSource` — 각자 **자기 데이터와 정책만** |

"기본 Grid 형태 + 글리프로 계층 표현"은 엔진의 기본 기능이다(`RowItem.depth`·`is_dir`·`marker` → 들여쓰기·펼침 글리프·상태 마커). 특화는 그것을 **끄거나 채울 뿐** 다시 만들지 않는다.

---

## 1. 계층도

```
nexa-grid (크레이트 · nexa-ctl 위 · dir2 rows/columns 이식)
├─ engine     VirtualRows<S>  · Column{key,title,width,min_width,align,sortable,resizable} · ViewMode · SelectOp · Marker
├─ traits     RowSource(데이터) · CellPainter(셀 그리기 훅 · 기본 = 텍스트) · Hierarchy(depth/toggle · 기본 = 평면)
└─ 특화 (각각 RowSource 구현 + 얇은 래퍼)
   ├─ ResultGrid    ← ResultSource(컬럼 지향 저장소 · 할당 0 페인트 · 타입별 정렬 · 셀 복사 TSV)        nexa-sql 결과 · export 미리보기
   ├─ FileGrid      ← FileSource(nexa-fs Listing · 폴더 우선 · 자연 정렬 · 아이콘 키 · 계층 depth)     [20] FilePicker · 프로젝트 사이드바 · dir2 본체
   ├─ ConnectionGrid← ProfileSource(nsql-vault 목록 · 이름·Pin·사용자·DB·마지막 사용 · 더블클릭 = 접속)  nexa-sql 로그인 리스트(T-31)
   └─ KeymapGrid    ← KeymapSource(그룹·이름·설명·단축키 · 그룹 = 계층 depth · [지정] 버튼 → 캡처 대화상자)   단축키 설정 화면(사용자 09-14)
```

의존 방향: `nexa-grid` → `nexa-ctl`(DrawCtx·Widget·토큰) → `nexa-gfx`. 특화의 데이터 타입(`ResultSet`·`Listing`·프로필)은 **소비자 크레이트가 `RowSource`를 구현**한다(nexa-grid는 nsql-core·nexa-fs를 모른다 — DR-2 도메인 의존 0). `FileGrid`만 nexa-ui 안(`nexa-fs`가 형제)에서 제공.

---

## 2. 공통 골격이 제공하는 것(엔진 · 이식 그대로)

| 기능 | 원본 위치 | 비고 |
|---|---|---|
| 가상화(가시 행만 페인트·히트) | `VirtualRows::paint`/`hit` | 10만 행이든 28행이든 같은 비용 |
| 컬럼 모델(폭·정렬·리사이즈·다중 정렬 배지 `order_badge`) | `columns.rs` · `set_columns` | 헤더 클릭 3단(오름·내림·해제) |
| 선택(단일·범위·토글·전체) · 고스트 행 | `SelectOp` · `is_selected/is_ghosted` | 파일 잘라내기 = 고스트 |
| 계층(들여쓰기 · 펼침 글리프 · `toggle`) | `RowItem.depth/is_dir` · `indent_w` | 평면 그리드는 depth 0 고정 |
| 상태 마커(`Marker` — 아이콘/배지 자리) | `RowItem.marker` | 접속 상태 ● · 파일 종류 아이콘 · 결과는 없음 |
| 뷰 모드(상세/목록/…) | `ViewMode` | 파일용 · 결과/접속은 상세 고정 |
| 키보드(방향·Home/End/Page·타입어헤드) · 스크롤 정렬 | `typeahead.rs` · `ScrollAlign` | 전부 공통 |
| 테마·토큰 | `nexa-ctl::Theme`/`tokens` | 색 하드코딩 0 |

---

## 3. 특화별 차이(오버라이드 지점만)

| 지점 | ResultGrid(속도·메모리 생명) | FileGrid | ConnectionGrid |
|---|---|---|---|
| 데이터 | **컬럼 지향 저장소**(타입별 연속 배열 · 문자열 아레나+오프셋 · nexa-sql 26 §4-2) | `nexa-fs::Listing`(배치 append) | 프로필 벡터(수십 개) |
| `cell` | ★ **`write_cell(i, key, &mut String)`**(원본 `cell -> String`은 셀마다 할당 → 이식 시 **버퍼 재사용 훅 추가** · 기본 구현은 `cell`에 위임) | 이름·크기·시각·종류 포맷(캐시) | 이름·핀·사용자·DB·마지막 사용 |
| 정렬 | 인덱스 벡터(행 복제 0) · 타입별 비교(숫자·시각·문자열·NULL) | 폴더 우선 · 자연 정렬 · 한글 | 핀 우선 · 마지막 사용 |
| 계층 | 없음(depth 0) · 후속: 그룹핑 | 트리(폴더 펼침) | 없음(후속: 폴더 그룹) |
| 마커 | 없음 · NULL 흐리게 | 아이콘 키(3-OS 동일 세트) | ● 접속 상태(ok/warn/danger) · 📌 |
| 셀 페인터 | 우측 정렬 숫자 · 고정폭 폰트 · 잘림 `…` · 폭 캐시 | 아이콘 + 텍스트 | 텍스트 + 배지 |
| 편집 | 후속(데이터 편집기 T-22) | 이름 변경(F2) | 인라인 없음(패널이 편집) |
| 상호작용 | 셀 선택·복사 TSV · 컬럼 고정(후속) | 더블클릭 열기 · 드래그 | 더블클릭 = 접속 · Del · 컨텍스트 |
| 예산 | render ≤ 8 ms · 행당 ≤ 100 B + 문자열 | 1만 파일 폴더 부드럽게 | — |

### 3-1. KeymapGrid — 단축키 설정 화면(사용자 09-14 *"그룹·이름·설명·단축키를 그리드로 · 지정 버튼 → 캡처 창 → 적용하면 그리드 값으로"*)

| 지점 | 값 |
|---|---|
| 데이터 | `KeymapSource` — 앱의 명령 레지스트리(nexa-sql E3 `Command` + `.sublime-keymap` 사용자 오버레이 · clip/beep는 `hotkey::WINDOW_ACTIONS`) → 행 = 명령 · 컬럼 = 그룹 · 이름 · 설명 · 단축키 · [지정] · [초기화] |
| 계층 | **그룹 = depth 0(접기 가능) · 명령 = depth 1** — 엔진의 계층 글리프 그대로 |
| 셀 페인터 | 단축키 셀 = 키캡 배지(`⌘⇧T` · OS 관례 표기 = `PlatformConventions`) · 충돌 시 주황 배지 + 툴팁("Ctrl+L: 접속 패널과 충돌") |
| 상호작용 | 행 더블클릭 또는 [지정] → **캡처 대화상자**(`nexa-dlg::HotkeyCapture` — 아래) · [초기화] = 기본값 · 검색(타입어헤드 = 이름·설명·키) · `@modified` 필터([24](../../nexa-sql/docs/24-settings-and-vscode-analysis.md) 관례) |
| 저장 | 적용 즉시 사용자 키맵 파일(`<설정 폴더>/keymap.conf` 또는 `Default (<OS>).sublime-keymap` 오버레이 · 앱 정책) · 그리드 셀 갱신 · 되돌리기 |

**캡처 대화상자** `HotkeyCapture`(nexa-dlg · 20 §1 계층의 조립): 모달 오버레이 · "키를 누르세요" 안내 · 누르는 동안 조합을 실시간 표시(수식키만 눌린 상태는 미확정) · Esc = 취소 · Backspace = 비우기 · **충돌 검사**(같은 컨텍스트의 다른 명령 → "교체/취소") · [적용] [취소]. 캡처 컨트롤은 **nexa-clip 설정 화면의 단축키 행**(`nclip-ui/settings.rs` `wk()` 행 · 09-08 창 안 단축키 · 전역/창 컨텍스트)을 이식 원천으로 쓴다 — 키 조합 파싱·표기·수식키 판정이 이미 있다.

**결과 그리드 규칙 넷**(사용자 *"속도가 생명 · 메모리 적을수록"*): ① 페인트 경로 힙 할당 0(재사용 버퍼 · 폭 캐시) ② 행을 객체로 들지 않는다(컬럼 저장소 · 인덱스 정렬) ③ 받는 양을 정한다(상한·배치 · 26 §4-1) ④ 결과 교체 시 즉시 drop(클렌징 · 26 §4-5).

---

## 4. 이식 계획(U-3 구체화)

| 단계 | 내용 |
|---|---|
| **G-1** | `nexa-grid` 크레이트 생성 · dir2 `columns.rs`+`rows.rs`+`typeahead.rs` 복사 · **DrawCtx 세대 어댑터**(D-3 합집합 — `select_font(slot,bold,italic)` ↔ nexa-ctl `select_font(slot,bold)`+italic 확장) · 147 테스트 이전 green |
| **G-2** | `RowSource`에 `write_cell(&self, i, key, out: &mut String)`(기본 = `cell` 위임) · `CellPainter` 훅 · `Hierarchy` 기본 평면 · 마커 → 아이콘 키 |
| **G-3** | `ResultGrid`(nexa-sql `crates/nexa-sql/grid.rs` 교체 · `ResultSource` 컬럼 저장소 · 푸터 load/render/bytes 유지) — 26 §5 예산 실측 |
| **G-4** | `ConnectionGrid`(접속 패널의 프로필 콤보를 Golden 로그인 리스트 그리드로 · T-31) |
| **G-5** | `FileGrid`(20 F-3의 FileList = 이 특화) |
| **G-6** | `KeymapGrid` + `HotkeyCapture` 대화상자(clip 단축키 행 이식) — nexa-sql 단축키 설정 화면(E3 키맵 뒤) |

---

## 5. 결정(D-10)

| # | 결정 | 권장 |
|---|---|---|
| **D-10** | 크레이트 배치 — `nexa-grid` 별도 크레이트(권장 · nexa-ctl 위 · dir2 이식 단위와 일치 · 앱이 `RowSource`만 구현) / `nexa-ctl::controls::grid` 모듈 | 별도 크레이트 |

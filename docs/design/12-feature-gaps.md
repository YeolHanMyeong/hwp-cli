# 기능 격차 카탈로그 (Feature Gaps) + 난이도·의존성 로드맵

이 문서는 hwp-cli가 **아직 못 하는 것**을 한 곳에 모은 단일 카탈로그다. 포맷 지도(10·11)가
"무엇이 존재하고 우리가 그것을 어떻게 처리하는가"를 사실로 기술했다면, 12번은 그 처리 상태가
**실기·합성·렌더에서 어떤 결함으로 드러나는가**를 평가하고, 각 갭에 난이도·가치·의존성을 붙여
복원 우선순위를 세운다.

## 0. 이 문서의 위치

### 0.1 다른 문서와의 역할 분담

| 문서 | 역할 | 12와의 관계 |
|---|---|---|
| [07-hangul-compat-rules.md](07-hangul-compat-rules.md) §F | 실기에서 드러난 **미해결 이슈의 조사 서사**(F1 글상자 드롭·F2 페이지 오버플로) | 12는 **링크로 승계**한다. 서사를 재서술하지 않고 요약+포인터만 둔다(→ §7 GG) |
| [00-overview.md](00-overview.md) §5 | 현재 상태 **요약 스냅숏** | 12가 그 스냅숏을 항목 단위로 편다 |
| [10-hwp5-structure-map.md](10-hwp5-structure-map.md) §8 | hwp5 레코드 중 **미해석(Opaque)·raw보존** 목록 | 12 §2·§3의 **근거 데이터**(무손실 보존이 실제로 무엇을 잃는가) |
| [11-hwpx-structure-map.md](11-hwpx-structure-map.md) §5 | hwpx read↔write **대칭성 매트릭스**(미구현·정보소실·왕복비대칭) | 12 §2·§4·§5의 **근거 데이터** |
| **12(이 문서)** | **전 기능 갭의 단일 카탈로그 + 로드맵** | — |

상태 라벨(Opaque/raw보존/skip 등)의 정본은 **10·11**이다. 라벨을 바꿔야 하면 거기부터 고치고
12는 따라온다. 스펙 § 번호·태그 이름은 사실 인용이며 문구는 전재하지 않는다([README](../README.md)).

### 0.2 ID 규약

- 갭 ID는 `계열-번호` 형식(`GA-1`, `GB-6`). 계열은 GA~GG.
- 07§F에서 승계한 항목은 원 번호를 병기한다: `GG-1 (=07§F1)`.
- GE 중 **hwpx→hwpx 왕복에서만** 손실되는 특수 부류는 `GE-α`로 분리한다(§5.2).

### 0.3 "미구현 vs 무손실 보존" 구별 원칙 (이 문서의 핵심 판정 기준)

같은 레코드라도 **어느 경로에서 보느냐**에 따라 갭이기도 하고 아니기도 하다. 판정의 단일 기준:

> **Opaque 보존은 왕복에서는 갭이 아니다. 합성(포맷 간 변환)과 렌더에서만 갭이다.**

- hwp5의 `OpaqueRecord`(서브트리째 보존, [10](10-hwp5-structure-map.md) §0 상태표)는
  `hwp5→hwp5` 왕복에서 **바이트를 잃지 않는다** → 그 경로에선 갭 아님.
- 같은 레코드를 `hwp5→hwpx`로 **합성**하려면 의미를 해석해 OWPML로 다시 써야 하는데, 그 지식이
  없으므로 **드롭**된다 → 합성 경로에선 갭.
- 렌더러가 그 개체(차트·OLE 등)를 그리려면 페이로드 해석이 필요한데 안 되므로 **빈자리** →
  렌더 경로에선 갭.

그래서 각 항목의 **영향 경로**(읽기/왕복/합성/렌더)를 반드시 명시한다. "현 동작"이 `Opaque 보존`인데
"영향 경로"에 왕복이 없으면, 그건 결함이 아니라 **설계된 무손실**이다.

### 0.4 항목 스키마

각 갭은 아래 표 형식으로 기술한다.

| 열 | 뜻 |
|---|---|
| **ID** | `계열-번호`. 07 승계 항목은 원 번호 병기 |
| **현상** | 사용자·재구현자가 관측하는 결함 |
| **근거 코드** | `파일:줄` — 실제 파일 대조로 확인한 위치 |
| **스펙/포맷 근거** | HWP 5.0 § 또는 OWPML 요소명 |
| **현 동작** | `거부` / `Opaque 보존(왕복 무손실)` / `드롭(소실)` / `근사` |
| **영향 경로** | `읽기` / `왕복` / `합성`(포맷 간) / `렌더` 중 어디서 갭인가 |
| **난이도** | `S`=자료구조만 / `M`=정답지 필요 / `L`=실기 반복 필요 |

`crates/` 접두어는 생략한다(`hwp5/src/write.rs` = `crates/hwp5/src/write.rs`).

---

## 1. GA — 입력 게이트 (읽기 자체가 거부되는 것)

가장 앞단. 파일을 열자마자 **의도적으로 거부**하는 부류다. 이들은 "버그"가 아니라 미구현을 명시적
에러로 알리는 설계지만, 실문서에서 만나면 파이프라인 전체가 막히므로 갭으로 기록한다.

| ID | 현상 | 근거 코드 | 스펙/포맷 근거 | 현 동작 | 영향 경로 | 난이도 |
|---|---|---|---|---|---|---|
| GA-1 | 암호화 HWP5 문서를 열면 `Hwp5Error::Encrypted`로 즉시 거부 | `hwp5/src/file_header.rs:60,136`(ENCRYPTED bit1·`is_encrypted`), `container.rs:102`(`check_body_readable`), `error.rs:40` | §3.2.1 FileHeader 속성 bit1 | 거부 | 읽기 | L |
| GA-2 | 배포용(ViewText) 문서 거부 — `/ViewText/Section*`에 본문이 있어도 접근 전 차단 | `hwp5/src/file_header.rs:61,140`(DISTRIBUTION bit2·`is_distribution`), `container.rs:105`, `error.rs:43` | §3.2.1 bit2, §3.2.3 ViewText | 거부 | 읽기 | L |
| GA-3 | DRM·공인인증서 보안 문서에 **전용 거부 경로 없음** — 플래그는 인식(`info` 표시)하나 게이트는 `is_encrypted`(bit1)만 검사. DRM 전용 플래그만 선 문서는 명확한 거부 대신 하위 파싱 실패로 떨어질 수 있음 | `hwp5/src/file_header.rs:63,67,69`(DRM·CERT_ENCRYPTED·CERT_DRM 플래그), `:151`(`attribute_names`만 소비), `container.rs:101`(게이트는 bit1/bit2뿐) | §3.2.1 bit4·bit8·bit10 | 거부(불완전) | 읽기 | L |

**GA 교훈:** 이 계열은 **복호화·인증 자체가 목표**라 정품 파일과 크립토 역설계(L)가 없으면 손댈 수
없다. 실용 우선순위는 낮되, GA-3은 "명확한 거부 메시지"를 추가하는 국소 개선(S)으로 사용성만
먼저 올릴 수 있다.

---

## 2. GB — 개체 타입 (레코드·요소는 있으나 의미 미해석)

가장 큰 계열. 레코드/요소가 **존재하고 스캔·왕복은 되지만**, 페이로드를 의미로 해석하지 않아
합성·렌더에서 빈자리가 되는 개체들이다. 핵심은 **포맷별 동작 차이**다:

- **hwp5** = `OpaqueRecord`로 서브트리째 보존 → `hwp5→hwp5` 왕복 무손실([10](10-hwp5-structure-map.md) §8 Opaque 목록).
- **hwpx read** = `GenericControl` fallback → 개체 고유 속성은 버리고 **자식 subList 텍스트만** IR에 남김([11](11-hwpx-structure-map.md) §3.3).
- **hwpx write** = 그 Generic이 알려진 ctrl_id도 gso_shapes도 아니면 최종 `DROP`(`hwpx/src/write/section.rs:364`) → **텍스트까지 소실**.

따라서 같은 개체가 "hwp5 왕복=무손실 / hwpx 왕복=소실 / 합성=소실 / 렌더=빈자리"로 경로마다 다르다.

| ID | 개체(hwp5 태그 / hwpx 요소) | 근거 코드 | 스펙/포맷 근거 | 현 동작 | 영향 경로 | 난이도 |
|---|---|---|---|---|---|---|
| GB-1 | **차트**(`CHART_DATA` 0x5F / `hp:chart` ooxmlchart) | hwp5 `body_text.rs:617`(Opaque), hwpx 미구현 `write/section.rs:364`(DROP), [11](11-hwpx-structure-map.md) §5(c) | §4.3.9.6 | hwp5=Opaque 보존 / hwpx=드롭(텍스트도 없음=완전 소실) | 왕복(hwpx만)·합성·렌더 | L |
| GB-2 | **OLE 개체**(`SHAPE_COMPONENT_OLE` 0x54 / `hp:ole`) | hwp5 `body_text.rs:617`, hwpx `write/section.rs:364`, [10](10-hwp5-structure-map.md) 표 B | §4.3.9.5 | hwp5=Opaque 보존 / hwpx=드롭 | 왕복(hwpx만)·합성·렌더 | L |
| GB-3 | **동영상**(`VIDEO_DATA` 0x62 / `hp:video`) | hwp5 `body_text.rs:617`, hwpx `write/section.rs:364` | §4.3.9.8 | hwp5=Opaque 보존 / hwpx=드롭 | 왕복(hwpx만)·합성·렌더 | L |
| GB-4 | **글맵시**(`SHAPE_COMPONENT_TEXTART` 0x5A / `hp:textart`) | hwp5 `body_text.rs:617`, hwpx `read/section.rs:191`(fallback 텍스트)→`write/section.rs:364`(DROP) | §4.3.9(글맵시) | hwp5=Opaque 보존 / hwpx=텍스트만 fallback 후 드롭 | 왕복(hwpx만)·합성·렌더 | M |
| GB-5 | **양식 개체**(`FORM_OBJECT` 0x5B / `hp:formObject`) | hwp5 `body_text.rs:617`, hwpx `read/section.rs:191`→`:364` | §4.3.9(양식) | hwp5=Opaque 보존 / hwpx=텍스트만 후 드롭 | 왕복(hwpx만)·합성·렌더 | M |
| GB-6 | **묶음 개체**(`SHAPE_COMPONENT_CONTAINER` 0x56 / `hp:container`) — ★**비대칭**: hwp5는 raw보존이라 **렌더까지 됨**(자식 재귀), hwpx는 fallback 후 DROP | hwp5 렌더 `hwp-render/src/shape_draw.rs`([10](10-hwp5-structure-map.md) §8 raw보존), hwpx `read/section.rs:191`→`write/section.rs:364` | §4.3.9.7 | hwp5=raw보존(렌더 O) / hwpx=드롭 | 왕복(hwpx만)·합성 | M |
| GB-7 | **메모**(`MEMO_LIST` 0x5D 본문 + `MEMO_SHAPE` 0x5C DocInfo / hwpx `hp:` 미방출) | hwp5 `body_text.rs:617`·`doc_info.rs:148`(Opaque), hwpx 네임스페이스 선언만([11](11-hwpx-structure-map.md) §2) | §4.3(메모)·§4.2 표13 | hwp5=Opaque 보존 / hwpx=미구현 | 왕복(hwpx만)·합성·렌더 | M |
| GB-8 | **변경추적·편집이력**(`TRACKCHANGE` 0x20·`TRACK_CHANGE` 0x60·`TRACK_CHANGE_AUTHOR` 0x61·`PARA_RANGE_TAG` 0x46 / hwpx `hhs:` history) | hwp5 `doc_info.rs:148`·`body_text.rs:73`(Opaque), hwpx 미구현([11](11-hwpx-structure-map.md) §5(c)) | §4.2 표13·§4.3.5 | hwp5=Opaque 보존 / hwpx=미구현 | 왕복(hwpx만)·합성 | L |
| GB-9 | **문서 임의·배포 데이터**(`DOC_DATA` 0x1B·`DISTRIBUTE_DOC_DATA` 0x1C·`COMPATIBLE_DOCUMENT` 0x1E·`LAYOUT_COMPATIBILITY` 0x1F) | hwp5 `doc_info.rs:57`(Opaque). 단 writer는 COMPATIBLE/LAYOUT을 **별도 합성**([07](07-hangul-compat-rules.md) A4) | §4.2.12~4.2.15 | hwp5=Opaque 보존(+합성 처리 有) / hwpx=미구현 | 합성(부분 해소) | L |
| GB-10 | **바탕쪽**(hwpx `hm:` master-page — hwp5 대응 개체 없음) | hwpx read·write 모두 없음([11](11-hwpx-structure-map.md) §2·§5(c)) | OWPML master-page | 미구현 | 왕복·합성·렌더 | M |
| GB-11 | **미지 개체·금칙문자**(`SHAPE_COMPONENT_UNKNOWN` 0x73·`FORBIDDEN_CHAR` 0x5E) | hwp5 `body_text.rs:617`·`doc_info.rs:57`(Opaque) | §4.2 표13 | hwp5=Opaque 보존 / hwpx=미구현 | 왕복(hwpx만) | L |

**GB 교훈:** hwp5→hwp5 왕복만 보면 GB 전체가 "무손실"이라 갭이 안 보인다(그게 §0.3의 함정). 결함은
**hwpx 왕복·포맷 간 합성·렌더**에서만 터진다. GB-6(묶음)은 특히 미묘하다 — hwp5는 렌더까지 되는데
hwpx로만 가면 사라진다. 이 계열의 복원은 대부분 **정품 파일에 그 개체를 담아 페이로드를 역설계**
(M/L)해야 하므로 정답지 확보가 선행 조건이다([00](00-overview.md) §4).

---

## 3. GC — 레이아웃·조판

문서는 열리고 텍스트도 보이지만, **조판 속성**(방향·테두리·각주 모양·탭·다단·들여쓰기)이 미반영/
근사되는 계열이다. hwp5 Opaque(왕복 무손실)이거나 hwpx skip(왕복 소실)이거나 렌더 무시로 갈린다.

| ID | 현상 | 근거 코드 | 스펙/포맷 근거 | 현 동작 | 영향 경로 | 난이도 |
|---|---|---|---|---|---|---|
| GC-1 | **세로쓰기 미지원** — 방향이 항상 가로로 고정 방출 | hwpx `write/header.rs:335`(`textDir="LTR"` 상수), `write/section.rs:460`(`textDirection="HORIZONTAL"` 상수) | OWPML `secPr@textDirection`, `paraPr@textDir` | 근사(가로 고정) | 합성·렌더 | M |
| GC-2 | **쪽 테두리/배경 미반영** — hwp5는 Opaque, hwpx read는 skip, write는 상수 방출 | hwp5 `body_text.rs:357`(secd 자식 Opaque), hwpx `read/section.rs:353`(`_ => {}` skip), `write/section.rs:460`(`pageBorderFill` 상수) | §4.3.10.1.3 `PAGE_BORDER_FILL` / `hp:pageBorderFill` | hwp5=Opaque 보존 / hwpx=드롭+상수 | 왕복(hwpx만)·합성·렌더 | M |
| GC-3 | **각주/미주 모양 미반영**(번호형식·구분선·간격) — 각주 참조는 렌더하나 모양은 상수 | hwp5 `body_text.rs:357`(secd 자식 Opaque), hwpx `read/section.rs:353`(skip), `write/section.rs:460`(`footNotePr`·`endNotePr` 상수) | §4.3.10.1.2 `FOOTNOTE_SHAPE` / `hp:footNotePr`·`endNotePr` | hwp5=Opaque 보존 / hwpx=드롭+상수 | 왕복(hwpx만)·합성·렌더 | M |
| GC-4 | **탭 정의 손실**(사용자 탭 위치·채움문자) — hwp5 raw보존, hwpx는 빈 상수 방출 | hwp5 `doc_info.rs:112`(`TAB_DEF` raw), hwpx `read/header.rs`(tabPrIDRef만)·`write/header.rs:263`(`write_tab_properties` 빈 `tabPr`) | §4.2.7 `TAB_DEF` / `hh:tabPr` | hwp5=raw보존 / hwpx=드롭+상수 | 왕복(hwpx만)·렌더 | S |
| GC-5 | **구역 속성 skip**(grid/startNum/visibility/lineNumberShape) — read가 흔적 없이 버림 | hwpx `read/section.rs:353`(`parse_sec_pr` 미매칭 skip), `write/section.rs:460`(상수 재합성) | OWPML `secPr` 자식 | skip → 상수 | 왕복(hwpx만)·합성 | S |
| GC-6 | **글상자 다단 미지원** — 연결/다단 글상자를 단일 단으로 근사 렌더 | `hwp-render/src/layout.rs:864`(`v1 단일 단 — hwp5 arm의 다단은 미지원`), `:788` | §4.3.10.2 단 정의 | 근사(단일 단) | 렌더 | S |
| GC-7 | **홀/짝수 조정 미해석** — 별도 의미 파싱 없이 Generic 통과 | hwpx `read/section.rs:597`(미지 ctrl → 코드 21 Generic), [10](10-hwp5-structure-map.md) §6.1 각주 | §4.3.10.8 | Generic 보존(미해석) | 합성·렌더 | S |
| GC-8 | **내어쓰기(음수 들여쓰기) 렌더 무시** — 음수 first-indent를 0으로 클램프 | `hwp-render/src/layout.rs:1493`(`음수=내어쓰기 v1 무시`), `:1578`(`.max(0.0)`) | §4.2.10 문단모양 들여쓰기 | 근사(0 클램프) | 렌더 | S |
| GC-9 | **문단 배경이 페이지를 걸치면 생략** — `broke`면 배경 Rect 미삽입 | `hwp-render/src/layout.rs:1502`(주석), `:1516`(`if broke { return; }`) | §4.2.5 테두리/배경 | 근사(생략) | 렌더 | S |

**GC 교훈:** GC-2·GC-3(쪽 테두리·각주 모양)은 **공문서에 빈출**하므로 가치가 높다. 셋 다 hwp5는
이미 무손실 보존(Opaque)이라 **정보는 갖고 있고**, 막힌 지점은 "그 페이로드를 의미로 해석해
hwpx/렌더로 내보내는 것"이다 → 정답지로 레코드 레이아웃을 확정하면(M) 풀린다. GC-4~GC-9는
대부분 자료구조·렌더 국소 수정(S).

---

## 4. GD — 수식

수식은 mini-TeX 조판기로 대부분 렌더되지만([05](05-rendering.md), 커밋 `ff4184b` 이후), 다음
구성은 아직 근사·미조판이다. 근거는 조판기 헤더 주석이 명시한 **알려진 미지원 목록**이다.

| ID | 현상 | 근거 코드 | 스펙/포맷 근거 | 현 동작 | 영향 경로 | 난이도 |
|---|---|---|---|---|---|---|
| GD-1 | **행렬(matrix) 미조판** — 열 정렬 문자 `&`를 조판하지 않고 공백으로 취급 | `hwp-render/src/equation.rs:10`(미지원 명시), `:59`(`'&' => … 열 정렬(matrix) — v1은 공백 취급`) | §4.3.9.3 수식 스크립트 | 근사(공백 취급) | 렌더 | M |
| GD-2 | **큰연산자 극한 미배치** — `sum`·`int` 심볼은 나오나 아래·위 극한을 연산자에 붙여 배치하지 못함 | `hwp-render/src/equation.rs:10`(미지원 명시), `:216`(`sum`→∑), `:217`(`int`→∫) | §4.3.9.3 | 근사(첨자 배치) | 렌더 | M |
| GD-3 | **복잡 구분자 미지원**(크기 자동조절 괄호 등) | `hwp-render/src/equation.rs:10`(`복잡 구분자`) | §4.3.9.3 | 근사 | 렌더 | M |

**GD 교훈:** 세 항목 모두 **정품 수식 정답지**(정답지 α+β/2 정합처럼)로 조판 메트릭을 맞춰야
확정되므로 M. 왕복 자체는 스크립트 원문을 raw로 보존하므로([10](10-hwp5-structure-map.md) 표 B
`EQEDIT`) 갭은 **렌더 경로에 국한**된다.

---

## 5. GE — 변환 매트릭스 (방향별 손실)

포맷 간 **합성**에서만 나타나는 손실이다(왕복 아님). 두 부류로 나눈다: (§5.1) 합성 시 의도적
저하·상수 대체, (§5.2) `GE-α` — hwp5로는 보존되나 **hwpx 쓰기에서만** 손실되는 왕복 비대칭.

### 5.1 GE — 합성 방향 손실

| ID | 현상 | 근거 코드 | 스펙/포맷 근거 | 현 동작 | 영향 경로 | 난이도 |
|---|---|---|---|---|---|---|
| GE-1 | **hwpx→hwp5 도형 의도적 저하** — 글상자는 텍스트를 본문으로 hoist하고 도형 래퍼 생략, 순수 장식은 드롭(무손실 gso 재합성 미확보) | `hwp5/src/write.rs:467`(`degrade_hwpx_gso`), `:510`(경고) | §4.3.9 개체 | 드롭(안전 저하) | 합성(hwpx→hwp5) | L |
| GE-2 | **이미지 바이너리 미발견 시 그림 드롭** — bin_ref가 가리키는 스트림을 못 찾으면 그림 생략 | `hwp5/src/write.rs:726`(`DROP: 이미지 바이너리 스트림을 찾지 못해 생략`) | §4.3.9.4 그림 | 드롭(소실) | 합성 | S |
| GE-3 | **colPr 단별폭·구분선 미수집** — 등폭·구분선 없음으로 가정, 불균등 단 손실 | `hwpx/src/read/section.rs:375`(`colSz·colLine 자식은 v1 미수집`), `:392` | §4.3.10.2 / `hp:colPr` | 드롭→상수 | 합성·렌더 | S |
| GE-4 | **pgnp 쪽번호 서식 DIGIT 고정** — 아라비아 숫자만 매핑, 그 외 형식 소실 | `hwpx/src/read/section.rs:429`(`서식은 …DIGIT=0만 매핑, 그 외는 0`), `build_pgnp:415` | §4.3.10.9 / `hp:pageNum` | 근사(DIGIT 고정) | 합성 | S |
| GE-5 | **nwno 새 번호 종류 PAGE 고정** — 번호 값만 취하고 종류는 PAGE로 고정 | `hwpx/src/read/section.rs:473`(`build_nwno`, `종류(u32=0,PAGE)`) | §4.3.10.6 / `hp:newNum` | 근사(종류 고정) | S |
| GE-6 | **atno 자동번호 페이로드 상수** — 표준 12B 상수로 합성 | `hwpx/src/read/section.rs:465`(`build_atno`) | §4.3.10.5 / `hp:autoNum` | 근사(상수) | 합성 | S |

### 5.2 GE-α — hwpx 왕복 비대칭 (read는 해석, hwpx write만 손실)

특수 부류. 아래 속성은 read가 IR로 **정확히 해석**하므로 `hwp5`로는 나간다. 그러나 hwpx writer가
상수/미방출로 눌러 **`hwpx→hwpx` 왕복에서만** 사라진다([11](11-hwpx-structure-map.md) §5(b)).
공통 원인은 `write/header.rs`의 국소 상수화이므로 **한 파일 수정으로 독립 복원** 가능한 게 특징이다.

| ID | 속성 | 근거 코드 (read ↔ write) | 현 동작 | 영향 경로 | 난이도 |
|---|---|---|---|---|---|
| GE-α1 | 글자 **그림자**(charPr shadow) | read `hwpx/src/read/header.rs:245` ↔ write `write/header.rs:258`(상수 `NONE`) | 근사(상수 NONE) | 왕복(hwpx→hwpx)·합성 | S |
| GE-α2 | 글자 **외곽선**(charPr outline) | read `read/header.rs:259` ↔ write `write/header.rs:258`(상수 `NONE`) | 근사(상수 NONE) | 왕복(hwpx→hwpx) | S |
| GE-α3 | **양각·음각**(emboss/engrave) | read `read/header.rs:266,271` ↔ write **미방출** | 드롭(hwpx write) | 왕복(hwpx→hwpx) | S |
| GE-α4 | **위·아래 첨자**(supscript/subscript) | read `read/header.rs:234,239` ↔ write **미방출** | 드롭(hwpx write) | 왕복(hwpx→hwpx) | S |
| GE-α5 | **밑줄 모양**(underline shape) | read `read/header.rs:204` ↔ write `write/header.rs:246`(`shape="SOLID"` 고정) | 근사(SOLID 고정) | 왕복(hwpx→hwpx) | S |
| GE-α6 | **그러데이션 중심·step** | read `read/section.rs:1217`(`parse_gradation`, angle만) ↔ write `write/section.rs:764`(center/step 상수) | 근사(중심·단계 상수) | 왕복(hwpx→hwpx)·렌더 | M |
| GE-α7 | **번호 형식**(numbering paraHead) | read `read/header.rs:333` ↔ write `write/header.rs:283`(상수 `^{level}.`) | 근사(형식 상수) | 왕복(hwpx→hwpx) | S |

**GE 교훈:** GE-1(도형 저하)은 07§F1과 같은 뿌리(gso 무손실 재합성 미확보)라 L이다. 반면
**GE-α 전체는 정답지 없이 자료구조만으로 풀 수 있는 저비용 항목**이다 — read가 이미 해석하고
있으니 write에 대응 요소만 방출하면 된다. `write/header.rs` 국소 수정으로 독립적이며, GA~GD·GG의
어떤 것에도 의존하지 않는다(→ §8 의존 그래프에서 "즉시 착수 가능" 노드).

---

## 6. GF — 필드·양식

필드는 12종 전수 온디맨드 파싱되지만([10](10-hwp5-structure-map.md) §6.2), 생성·해석 범위에 갭이 있다.

| ID | 현상 | 근거 코드 | 스펙/포맷 근거 | 현 동작 | 영향 경로 | 난이도 |
|---|---|---|---|---|---|---|
| GF-1 | **미지 필드 %unk 폴백** — 매핑 안 되는 필드 종류·OWPML type을 `%unk`/`UNKNOWN`으로 뭉갬 | `hwp-convert/src/field.rs:69`(`_ => "UNKNOWN"`), `:87`(`_ => *b"%unk"`), `:104` | §4.3.10.15 / `fieldBegin@type` | 근사(폴백) | 왕복·합성 | S |
| GF-2 | **찾아보기 표식·덧말·글자겹침·숨은설명 미해석** — 의미 파싱 없이 Generic으로만 보존 | hwpx `read/section.rs:597`(미지 ctrl → 코드 21 Generic), [10](10-hwp5-structure-map.md) §6.1 각주 | §4.3.10.10·§4.3.10.12·§4.3.10.13 | Generic 보존(미해석) | 합성·렌더 | M |
| GF-3 | **신규 필드 생성 제약** — 기존 이름의 값만 채울 수 있고 새 필드 생성 없음. 편집 생성은 `%clk`·`%hlk`·`%bmk`/`bokm`만 | `hwp-convert/src/field.rs`(생성 지원 종류 한정), [README](../README.md) §범위와 한계(`신규 필드 생성은 없다`) | §4.3.10.15 | 미구현(생성) | 편집 | M |

**GF 교훈:** GF-1은 폴백이 있어 파일이 깨지진 않으나 종류 정보가 뭉개진다(S). GF-2의 겹침·덧말은
GB-10 계열과 접하며(제어문자 23), 의미 렌더를 하려면 정답지가 필요하다(M).

---

## 7. GG — 렌더 정밀도 (07§F 승계)

07§F가 **조사 서사**로 다룬 미해결 이슈를 여기서 카탈로그 항목으로 승계한다. **서사는 07이 정본**
이며 여기서는 요약+링크만 둔다(재서술 금지 원칙, §0.1).

| ID | 현상 | 근거 코드 | 상태·방향 | 현 동작 | 영향 경로 | 난이도 |
|---|---|---|---|---|---|---|
| GG-1 (=07§F1) | **글상자 드롭** — 왕복 hwp에서 글상자 박스 자체 소실(텍스트는 본문 hoist로 보존) | `hwp5/src/write.rs:467`(`degrade_hwpx_gso`) | [07§F1](07-hangul-compat-rules.md) 승계. 근본 해결은 SHAPE_COMPONENT 239B **속성 충실도** 확보 필요 | 드롭(안전 저하) | 합성(hwpx→hwp5) | L |
| GG-2 (=07§F2) | **페이지 오버플로** — 합성 멀티페이지 세로 넘침(md는 content_h 리셋으로 방어) | `hwp-render/src/lineseg.rs`(`synthesize_linesegs`) | [07§F2](07-hangul-compat-rules.md) 승계. 줄배치 속성 충실도가 유력 원인 | 근사 | 렌더·합성 | L |
| GG-3 (=U2) | **양쪽정렬 근사** — 잉여폭을 공백 우선 분배, 글리프↔글자 CJK 1:1 가정 | `hwp-render/src/layout.rs:386`, [05](05-rendering.md) §1.4(`justify_line`) | 공백 없으면 마지막 글리프 전 gap 균등. 비CJK 혼용 시 오차 | 근사 | 렌더 | M |
| GG-4 (=U4) | **자간 근사** — `spacing_pt = size_pt × spacings[lang]/100` 단순 적용 | [05](05-rendering.md):184(`// 자간`) | 언어별 자간을 pt 스케일로 근사 | 근사 | 렌더 | M |

**U1·U3에 대하여:** 00§5는 "U2(양쪽정렬)·U4(자간)"만 명명한다. `U1`·`U3`은 docs 전체와 git 이력
어디에도 정의가 없어(추측 금지 원칙) **의도적으로 제외**했다. U-계열이 U1~U4 완전 열거로 확정되면
이 표에 추가한다.

**GG 교훈:** GG-1·GG-2는 07§F의 관통 가설("속성 충실도가 충분히 높으면 자연 해소")을 그대로
따른다 — 정답지 확보 + 실기 반복(L)이 유일한 길. GG-3·GG-4는 렌더 국소지만 정품 렌더와의
픽셀 대조(M)가 있어야 확정된다.

---

## 8. 로드맵 — 난이도 × 가치 + 의존 그래프

### 8.1 난이도 × 가치 매트릭스

**가치**는 실문서 출현 빈도 기준(공문서·보고서에 얼마나 자주 나오는가).

| | **난이도 S**(자료구조만) | **난이도 M**(정답지 필요) | **난이도 L**(실기 반복) |
|---|---|---|---|
| **가치 高**(빈출) | **GE-α1~α5,α7**(글자효과·밑줄·번호형식 왕복), GC-4·GC-5(탭·구역속성), GC-8·GC-9(내어쓰기·문단배경) | GC-2·GC-3(쪽테두리·각주모양), GG-3·GG-4(양쪽정렬·자간), GF-2(찾아보기·겹침) | GG-1·GG-2(글상자 드롭·오버플로) |
| **가치 中** | GC-6(글상자 다단), GE-2·GE-3·GE-4~GE-6(그림 드롭·단·번호 합성), GF-1(%unk) | GB-4·GB-5(글맵시·양식), GB-6(묶음), GB-7(메모), GB-10(바탕쪽), GC-1(세로쓰기), GD-1~GD-3(수식), GE-α6(그러데이션), GF-3(필드 생성) | GB-1·GB-2·GB-3(차트·OLE·동영상) |
| **가치 低**(드묾) | GA-3(DRM 거부 메시지) | — | GA-1·GA-2(암호화·배포 복호화), GB-8(변경추적), GB-9(문서데이터), GB-11(미지·금칙) |

**읽는 법:** 좌상단(S·高)이 **가성비 최상**이다 — 특히 **GE-α**는 정답지 없이 write 국소 수정만으로
빈출 글자효과 왕복을 복원한다. 우하단(L·低)은 우선순위 최하(암호 복호화 등).

### 8.2 의존 그래프

```
[정답지 확보]  ──선행──▶  GB-1~7(개체 렌더)  ──필요──▶  10/11 레코드 구조 해석
   │                       GC-2/GC-3(쪽테두리·각주모양) ── FOOTNOTE_SHAPE/PAGE_BORDER_FILL 의미해석
   │                       GD-1~3(수식 조판)  ── 정품 수식 메트릭
   │                       GG-1/GG-2(속성 충실도) ── 실기 반복(07§F)
   │
[독립·즉시 착수] ──▶ GE-α1~α7  (write/header.rs·write/section.rs 국소 수정, 타 항목 의존 없음)
                    GC-8/GC-9  (hwp-render/layout.rs 국소, 렌더 전용)
                    GE-2       (write.rs 국소, 그림 드롭 경고→복구)
                    GA-3       (container.rs 거부 메시지 추가)
```

**의존 규칙 요약:**
- **GB 개체 렌더**는 10/11의 레코드/요소 구조 해석이 선행돼야 한다(현재 Opaque/fallback이라 의미
  필드가 IR에 없음). 또한 대부분 **정답지 확보가 선행**([00](00-overview.md) §4 정답지 방법론).
- **GC-2·GC-3**(쪽테두리·각주모양)은 hwp5가 이미 Opaque로 정보를 보존하므로, "정답지로 레코드
  레이아웃 확정 → IR 의미 필드 승격 → hwpx/렌더 방출"의 3단계다.
- **GE-α 전체**는 read가 이미 해석 완료라 **어떤 것에도 의존하지 않는 독립 노드**다. write 대응
  요소 방출만 추가하면 되는 최단 경로.
- **GG-1·GG-2**는 07§F의 미해결과 동일 뿌리(속성 충실도)라 **실기 반복 + 정답지**가 공동 선행.

### 8.3 정답지 선행 항목 (실기·정품 파일 필요)

아래는 [00](00-overview.md) §4 정답지 방법론에 따라 **정품 한글 파일 확보가 선행돼야** 착수 가능한
항목이다(추측 조판 금지). 나머지(특히 GE-α·GC 국소·렌더 국소)는 정답지 없이 자료구조/렌더만으로
진행 가능하다.

- **GB-1~GB-7, GB-10**: 차트·OLE·동영상·글맵시·양식·묶음·메모·바탕쪽 — 해당 개체를 담은 정품 파일
- **GC-1, GC-2, GC-3**: 세로쓰기·쪽테두리·각주모양 — 해당 조판을 쓴 정품 파일
- **GD-1~GD-3**: 행렬·큰연산자·복잡 구분자를 포함한 정품 수식
- **GG-1, GG-2**: 07§F 서사대로 실기 반복 필요

---

**요약:** 저비용·고가치의 진입점은 **GE-α**(글자효과 왕복 복원, write 국소·정답지 불요)와 **GC의
렌더 국소 항목**(GC-8·GC-9)이다. 고가치·고난도의 정공법은 **GC-2·GC-3**(공문서 빈출 쪽테두리·각주
모양)로, hwp5가 이미 보존 중인 Opaque 페이로드를 정답지로 해석해 의미 필드로 승격하는 것이 핵심
경로다.

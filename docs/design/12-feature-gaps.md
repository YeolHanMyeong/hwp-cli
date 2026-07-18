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
| [08-external-research.md](08-external-research.md) | 외부 근거 — 표준·오픈소스·**생태계 기능 대조**(deep-research) | §10 GJ·§14 로드맵의 수요·구현 선례 근거 |
| **12(이 문서)** | **전 기능 갭의 단일 카탈로그 + 로드맵** | — |

상태 라벨(Opaque/raw보존/skip 등)의 정본은 **10·11**이다. 라벨을 바꿔야 하면 거기부터 고치고
12는 따라온다. 스펙 § 번호·태그 이름은 사실 인용이며 문구는 전재하지 않는다([README](../README.md)).

### 0.2 ID 규약

- 갭 ID는 `계열-번호` 형식(`GA-1`, `GB-6`). 계열:
  - **GA~GG** (초판): 입력 게이트 / 개체 타입 / 레이아웃·조판 / 수식 / 변환 매트릭스 / 필드·양식 / 렌더 정밀도
  - **GH~GM** (2026-07-08 재수색 추가): 내보내기 손실 / 들여오기 한계 / 미지원 포맷·레거시 / 편집 프리미티브 / 텍스트 추출 옵션 / CLI 명령·워크플로
- 07§F에서 승계한 항목은 원 번호를 병기한다: `GG-1 (=07§F1)`.
- GE 중 **hwpx→hwpx 왕복에서만** 손실되는 특수 부류는 `GE-α`(§5.2),
  **IR 경유 되쓰기에서 부속 데이터가 상수/재생성으로 대체**되는 부류는 `GE-β`(§5.3)로 분리한다.

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

### 0.5 해소 이력

해소된 항목은 카탈로그에서 지우지 않고 해당 행에 ✅와 날짜를 남긴다(무엇이 갭이었는지가 곧 지식이므로).

- **2026-07-15**: GA-5(버전 게이트), GE-α1~α5·α7(글자효과·밑줄모양·번호형식 hwpx 왕복),
  GE-β4(요약정보 필드), GH-1·GH-2(md/html 링크·이미지), GL-1(추출 옵션 CLI 노출) —
  Opus 4.8 병렬 구현, 전체 테스트 236 통과, E2E 스모크(링크/이미지/media 디렉토리/validate) 확인.
- **2026-07-15 (2차 — 1차 실기 피드백 반영)**: 실기에서 C6 번호 미표시·C8 날짜 누락·C9 주제 누락
  발견 → **GE-α8**(문단↔번호 heading 역방출) 해소, C8 요약정보에 **PID 0x14 한국어 날짜 문자열**
  추가(정품 40종 실측 = 작성일시 KST 파생 — 한글 '날짜' 표시의 원천), C9 content.hpf 메타를
  정품 형식으로 전면 정합(subject/keyword meta 형식, CreatedDate/ModifiedDate ISO, date 한국어;
  **hwpx 날짜 방출 갭도 함께 해소**). FILETIME 변환 유틸은 `hwp-model/src/units.rs` 공용.
  전체 테스트 247 통과. **★실기 게이트 통과(2026-07-15)**: 1차 실기에서 C1~C5·C7(글자효과)
  정상, C6·C8·C9 결함 발견 → 2차 수정 후 재검에서 **C6 번호 표시·C8 날짜·C9 주제/날짜 모두
  정상 확인**. 이 단락의 해소 항목 전체가 실기 확정됐다.

---

## 1. GA — 입력 게이트 (읽기 자체가 거부되는 것)

가장 앞단. 파일을 열자마자 **의도적으로 거부**하는 부류다. 이들은 "버그"가 아니라 미구현을 명시적
에러로 알리는 설계지만, 실문서에서 만나면 파이프라인 전체가 막히므로 갭으로 기록한다.

| ID | 현상 | 근거 코드 | 스펙/포맷 근거 | 현 동작 | 영향 경로 | 난이도 |
|---|---|---|---|---|---|---|
| GA-1 | 암호화 HWP5 문서를 열면 `Hwp5Error::Encrypted`로 즉시 거부 | `hwp5/src/file_header.rs:60,136`(ENCRYPTED bit1·`is_encrypted`), `container.rs:102`(`check_body_readable`), `error.rs:40` | §3.2.1 FileHeader 속성 bit1 | 거부 | 읽기 | L |
| GA-2 | 배포용(ViewText) 문서 거부 — `/ViewText/Section*`에 본문이 있어도 접근 전 차단 | `hwp5/src/file_header.rs:61,140`(DISTRIBUTION bit2·`is_distribution`), `container.rs:105`, `error.rs:43` | §3.2.1 bit2, §3.2.3 ViewText | 거부 | 읽기 | **M**★ |
| GA-3 | DRM·공인인증서 보안 문서에 **전용 거부 경로 없음** — 플래그는 인식(`info` 표시)하나 게이트는 `is_encrypted`(bit1)만 검사. DRM 전용 플래그만 선 문서는 명확한 거부 대신 하위 파싱 실패로 떨어질 수 있음 | `hwp5/src/file_header.rs:63,67,69`(DRM·CERT_ENCRYPTED·CERT_DRM 플래그), `:151`(`attribute_names`만 소비), `container.rs:101`(게이트는 bit1/bit2뿐) | §3.2.1 bit4·bit8·bit10 | 거부(불완전) | 읽기 | L |
| GA-4 | **전자 서명 문서 미처리** — FileHeader bit7(전자서명)·bit9(예비)는 이름만 인식, `DigitalSignature`·`PublicKeyInfo` 스트림은 게이트도 카탈로그도 없어 하위 파싱 실패로 낙하 가능 | `hwp5/src/file_header.rs:66,68`(HAS_SIGNATURE·SIGNATURE_SPARE 이름만), `container.rs:101`(게이트는 bit1/bit2뿐), [10](10-hwp5-structure-map.md) §1 | §3.2.1 bit7·bit9, §3.2.8 서명 스트림 | 침묵(게이트 없음) | 읽기 | L |
| GA-5 | **버전 무검사 침묵 허용** — parse는 시그니처만 검사하고 버전 필드를 게이트하지 않아 5.1.x·미래 버전 전부 통과. 합성은 5.1.x 표본 상수 길이라 PARA_HEADER 24/22B 외 버전별 레코드 길이 차는 게이팅 안 됨 | `hwp5/src/file_header.rs:91-115`(버전 무검사), `write.rs:113`(5.0.3.2 분기 하나뿐), `:1072-1089`(파싱 실패 시 5.1.0.1 기본) | §3.2.1 버전 필드 | ✅ **해소(2026-07-15)** — major≠5는 `UnsupportedVersion` 거부, 5.x 전부 허용 | 읽기·왕복 | S |

**GA 교훈:** GA-1(암호화)·GA-3(DRM)·GA-4(서명)는 **복호화·인증 자체가 목표**라 정품 파일과 크립토
역설계(L)가 없으면 손댈 수 없다. ★단 **GA-2(배포용)는 L이 아니라 M** — 한컴 공식 스펙
「한글문서파일형식\_배포용문서\_revision1.2」가 복호화 알고리즘 전체(DISTRIBUTE_DOC_DATA 256B
레코드, 난수 배열, SHA1 유도 키, AES-128 ECB)를 공개하고 있고 pyhwp가 2014년부터 구현한 선례가
있다([08](08-external-research.md) 생태계 대조). GA-3·GA-4는 "명확한 거부 메시지" 국소 개선(S)으로
사용성만 먼저 올릴 수 있고, GA-5는 버전 비교 한 줄이면 되는 즉시 개선 항목이다.

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
| GB-1 | **차트**(`CHART_DATA` 0x5F / `hp:chart` ooxmlchart) | hwp5 `body_text.rs:617`(Opaque), hwpx 미구현 `write/section.rs:364`(DROP), [11](11-hwpx-structure-map.md) §5(c) | §4.3.9.6 | hwp5=Opaque 보존 / hwpx=드롭(텍스트도 없음=완전 소실) | 왕복(hwpx만)·합성·렌더 | L / hwpx 생성=**M**★ |
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
| GB-12 | **참고문헌(Bibliography) 스토리지 미포착** — read가 IR로 안 올리고 write가 미방출 → **IR 경유 되쓰기에서 소실**(identity 왕복은 무관) | hwp5 read/write 분기 없음([10](10-hwp5-structure-map.md) §1 트리 — 2026-07-08 보완 등재) | §3.2.12 Bibliography(.XML 저장) | 드롭(되쓰기) | 되쓰기 | S |

**GB 교훈:** hwp5→hwp5 왕복만 보면 GB 전체가 "무손실"이라 갭이 안 보인다(그게 §0.3의 함정). 결함은
**hwpx 왕복·포맷 간 합성·렌더**에서만 터진다. GB-6(묶음)은 특히 미묘하다 — hwp5는 렌더까지 되는데
hwpx로만 가면 사라진다. 이 계열의 복원은 대부분 **정품 파일에 그 개체를 담아 페이로드를 역설계**
(M/L)해야 하므로 정답지 확보가 선행 조건이다([00](00-overview.md) §4).
★예외가 **GB-1의 hwpx 경로**다: HWPX에서 차트는 OLE가 아니라 **OOXML DrawingML `chartSpace`
XML 파트**(`Chart/chartN.xml` + manifest 등재 + `hp:chart chartIDRef`)여서, 기존 hwpx 쓰기
인프라만으로 생성·해석이 가능하다(kordoc v3.16 구현 선례 — [08](08-external-research.md) 생태계 대조).

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
`EQEDIT`) 갭은 **렌더 경로에 국한**된다. 같은 언어(Rust) 구현체 rhwp가 `MATRIX`/`PMATRIX`/
`BMATRIX`/`DMATRIX` 조판을 이미 구현한 선례가 있어 참조 가능하다([08](08-external-research.md)
생태계 대조).

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
| GE-α1 | 글자 **그림자**(charPr shadow) | read `hwpx/src/read/header.rs:245` ↔ write `write_char_properties` | ✅ **해소(2026-07-15)** — IR 기반 방출 | 왕복(hwpx→hwpx)·합성 | S |
| GE-α2 | 글자 **외곽선**(charPr outline) | read `read/header.rs:259` ↔ write 동상 | ✅ **해소(2026-07-15)** | 왕복(hwpx→hwpx) | S |
| GE-α3 | **양각·음각**(emboss/engrave) | read `read/header.rs:266,271` ↔ write 동상 | ✅ **해소(2026-07-15)** | 왕복(hwpx→hwpx) | S |
| GE-α4 | **위·아래 첨자**(supscript/subscript) | read `read/header.rs:234,239` ↔ write 동상 | ✅ **해소(2026-07-15)** | 왕복(hwpx→hwpx) | S |
| GE-α5 | **밑줄 모양**(underline shape) | read `read/header.rs:204`(IR `underline_shape` 신설) ↔ write 동상 | ✅ **해소(2026-07-15)** | 왕복(hwpx→hwpx) | S |
| GE-α6 | **그러데이션 중심·step** | read `read/section.rs:1217`(`parse_gradation`, angle만) ↔ write `write/section.rs:764`(center/step 상수) | 근사(중심·단계 상수) | 왕복(hwpx→hwpx)·렌더 | M |
| GE-α7 | **번호 형식**(numbering paraHead) | read `read/header.rs:333` ↔ write `write_numberings` | ✅ **해소(2026-07-15)** — `numbering_levels` 기반, 다중 번호정의 itemCnt도 수정 | 왕복(hwpx→hwpx) | S |
| GE-α8 | **문단↔번호 연결**(paraPr heading) — read는 해석(attr1 bits23-27 + numbering_id)하나 write가 `type="NONE"` 고정이었음 | read `read/header.rs:309` ↔ write `write_para_properties` | ✅ **해소(2026-07-15 2차)** — OUTLINE/NUMBER/BULLET 역방출, 실기(C6)에서 발견된 결함 | 왕복(hwpx→hwpx)·합성 | S |

> **잔여 소갭(α5 관련):** 밑줄 모양 중 **물결(WAVE)**은 reader `line_type_code`에 매핑이 없어
> SOLID로 강등된다 — 점선·이중선 등은 정상 왕복. C 시리즈 실기 세트 제작(2026-07-15) 중 발견.

### 5.3 GE-β — IR 되쓰기 부속 데이터 손실 (2026-07-08 재수색 추가)

또 하나의 특수 부류. 본문 레코드가 아닌 **부속 스트림·메타데이터**가 IR에 올라오지 않아, 편집을
거치는 **IR 경유 되쓰기**(read→IR→write)에서 상수/재생성으로 대체되는 손실이다. §0.3의 "Opaque
무손실"과 달리 이들은 Opaque 보존조차 안 되므로 **같은 포맷 되쓰기에서도 소실**된다(무수정 identity
재직렬화는 바이트 복사라 무관). 참고: PrvText(미리보기 **텍스트**)는 매번 본문에서 재생성되므로
stale 갭이 아니다 — 갭은 아래 항목들이다.

| ID | 대상 | 근거 코드 | 현 동작 | 영향 경로 | 난이도 |
|---|---|---|---|---|---|
| GE-β1 | **미리보기 이미지(PrvImage / Preview/PrvImage.png)** — read가 IR로 미포착, 재생성기(썸네일 렌더러)도 없음 | hwp5 `write.rs:226-228`(opts 제공 시만), hwpx `write/mod.rs:113`(PrvText만), `patch.rs:3` | 드롭(되쓰기) | 되쓰기 | S(렌더러 재활용 시) |
| GE-β2 | **Scripts(매크로)** — 원본 JScript를 버리고 한글 빈 문서 표본 상수로 대체 | hwp5 `write.rs:213-221`(표본 바이트 상수), hwpx `patch.rs:4` | 드롭→상수 | 되쓰기 | S |
| GE-β3 | **DocOptions 부속 스트림** — `_LinkDoc`은 524B 0 상수, DRM·서명 6스트림은 미방출 | `write.rs:208-210`, [10](10-hwp5-structure-map.md) §1 | 드롭/상수 | 되쓰기 | M |
| GE-β4 | **요약정보 필드 소실** — 작성/수정일시·마지막저장자·설명 | `summary.rs`·`write.rs`·`hwp-model/src/document.rs`·hwpx `templates.rs` | ✅ **해소(2026-07-15)** — Metadata에 description/last_saved_by/create_time/modify_time(raw FILETIME u64) 추가, read/write 왕복. 인쇄일시·통계는 잔존(기본값 방출) | 되쓰기 | S |
| GE-β5 | **hwpx settings.xml·version.xml·`hp:switch`** — 앱 설정·캐럿·버전 메타·2016 호환 블록을 상수로 대체/소실 | `templates.rs:10-22`(상수), `write/mod.rs:81,114`, `patch.rs:3-4` | 드롭→상수 | 되쓰기 | S |
| GE-β6 | **임베디드 폰트** — `isEmbedded="0"` 하드코딩, 폰트 BinData·hwp5 typeInfo 소실 | hwpx `write/header.rs:84,98,105`, `read/header.rs:132-135`, hwp5 `doc_info.rs:201`(`type_info: None`) | 드롭(플래그·바이너리) | 되쓰기·렌더 | M |

**GE 교훈:** GE-1(도형 저하)은 07§F1과 같은 뿌리(gso 무손실 재합성 미확보)라 L이다. 반면
**GE-α 전체는 정답지 없이 자료구조만으로 풀 수 있는 저비용 항목**이다 — read가 이미 해석하고
있으니 write에 대응 요소만 방출하면 된다. `write/header.rs` 국소 수정으로 독립적이며, GA~GD·GG의
어떤 것에도 의존하지 않는다(→ §14 의존 그래프에서 "즉시 착수 가능" 노드). **GE-β는 "충실도 보존
fill"(`patch.rs`)이 이미 우회 경로**임에 유의 — hwpx 한정으로 패키지를 통째 보존하며 텍스트만
치환하므로, GE-β가 문제되는 것은 구조 편집이 필요한 IR 경유 경로뿐이다. 근본 해법은 IR에
"부속 스트림 pass-through" 슬롯을 추가하는 것(대부분 S).

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

## 7. GG — 렌더 정밀도

### 7.1 07§F 승계

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

### 7.2 렌더 속성 갭 (2026-07-08 재수색 추가)

`crates/hwp-render/` 전수 재수색으로 확정한, IR에는 있으나(또는 raw에 보존돼 있으나) 렌더가
반영하지 않는 속성들. 영향 경로는 전부 **렌더**다(별도 표기 없으면).

| ID | 현상 | 근거 코드 | 현 동작 | 난이도 |
|---|---|---|---|---|
| GG-5 | **셀 테두리 선 종류 무시** — `BorderLine.line_type`(점선·이중선) 미반영, `Item::Line`에 dash 필드 자체가 없음. 모든 셀 테두리가 실선 1줄 | `hwp-render/src/layout.rs:1080-1091`, `display.rs:32-39`, `hwp-model/src/header.rs:311` | 근사(실선 고정) | S |
| GG-6 | **문단 테두리 선 종류 무시** — GG-5와 같은 뿌리, 경로만 다름 | `layout.rs:1556-1567` | 근사(실선 고정) | S |
| GG-7 | **셀·문단 배경 무늬(hatch)·그러데이션 무시** — `BorderFill`이 단색 `bg_color`만 모델링(무늬는 tail raw). 도형 배경 그러데이션은 되는데 셀/문단은 단색뿐 | `layout.rs:1040,1536`, `hwp-model/src/header.rs:333-344` | 근사(단색만) | M |
| GG-8 | **강조점(dot emphasis) 미렌더** — `CharShape.attr` 비트는 보존되나 접근자·렌더 모두 없음 | `hwp-model/src/header.rs:86`(비트만), 렌더 전 crate 무참조 | 드롭(미표시) | S |
| GG-9 | **밑줄 모양(이중·점선·물결)·'글자 위' 밑줄 미렌더** — kind==1(아래)만 인식, 모양 비트(4~7) 접근자 없음 | `hwp-model/src/header.rs:115-121`, `layout.rs:1615-1622` | 근사(아래 실선만) | S |
| GG-10 | **취소선 모양 무시** — 이중 취소선 등 미반영, 실선 1줄 고정 | `hwp-render/src/shape.rs:34,369`, `layout.rs:1623` | 근사(실선 고정) | S |
| GG-11 | **글자 그림자 오프셋 무시** — `CharShape.shadow_gap` 미사용, 고정 대각 오프셋(0.05~0.06em) | `hwp-model/src/header.rs:91`(무참조), `png.rs:138`, `pdf.rs:206` | 근사(고정 오프셋) | S |
| GG-12 | **개요(outline) 번호 미렌더** — head_type 2(번호)·3(불릿)만 마커 생성, 1(개요)은 제외 | `hwp-render/src/list.rs:17-21` | 드롭(번호 없음) | M |
| GG-13 | **쪽번호 미렌더** — 페이지 카운터 부재, pgnp/atno 컨트롤은 skipped 집계 후 미렌더 | `layout.rs:189`(렌더 대상 목록), pgnp 무참조 | 드롭(미표시) | M |
| GG-14 | **미주(endnote) 배치 근사** — 문서/구역 끝이 아니라 **앵커 페이지 하단**에 각주와 동일 렌더(GC-3의 '모양'과 별개인 '위치' 문제) | `hwp-render/src/footnote.rs:35-72`, `layout.rs:263,598`(kind 미구분) | 근사(각주식 배치) | M |
| GG-15 | **이미지 회전·자르기(imgClip)·반전·밝기/대비·워터마크 미렌더** — `Item::Image`에 변환 필드 없음, `common_data` 내 효과 미해석 | `layout.rs:741-760`, `display.rs:41-47`, `hwp-model/src/control.rs:43` | 근사(원본 배치) | M |
| GG-16 | **머리말/꼬리말 홀수/짝수/첫쪽 구분 무시** — 최초 head/foot 하나를 모든 페이지에 반복(GC-7 구역 EVEN_ADJUST와 별개) | `layout.rs:152-165` | 근사(단일화) | S |
| GG-17 | **단 구분선 미렌더** — `ColumnDef.divider` 파싱만 하고 미사용(GE-3은 hwpx read 미수집, 이건 hwp5 렌더 경로) | `hwp-model/src/control.rs:238`, `layout.rs` 무참조 | 드롭(선 없음) | S |
| GG-18 | **줄간격 모델 근사(합성 한정)** — attr1&0x3로 판정, 고정(1)·최소(3)를 동일 처리, 여백만(2)을 비율로 오해. `line_spacing_type` 필드 미사용. 실파일은 캐시 lineseg라 무관 | `hwp-render/src/lineseg.rs:264-270`, `hwp-model/src/header.rs:195` | 근사(합성 경로) | M |
| GG-19 | **금칙처리·외톨이줄 보호·한 줄 입력 미지원(합성 한정)** — 그리디 줄바꿈만 | `lineseg.rs:301-333` | 근사(합성 경로) | M |
| GG-20 | **인라인 제어문자 폭 무시** — 고정폭 빈칸·하이픈·묶음 빈칸 등이 폭 계산에 미반영 | `hwp-render/src/shape.rs:201`(`_ => {}`) | 근사(폭 0) | S |

> 재수색에서 **갭이 아님**으로 확인된 것(오보고 방지): 장평(x_scale), 양각/음각/외곽선/글자그림자
> on-off, 셀 세로정렬·셀 여백·자동 행높이, 위/아래 첨자·글자 음영·밑줄 색 — 전부 렌더됨.
> GE-α1~α3는 hwpx **write 왕복** 전용 갭이지 렌더 미지원이 아니다.

---

## 8. GH — 내보내기(md/HTML/ODT) 손실 (2026-07-08 재수색 추가)

IR→텍스트 포맷 출력에서 잃는 것들. `hwp-convert/src/{markdown,html,odt}.rs`가 대상이다.

| ID | 현상 | 근거 코드 | 현 동작 | 영향 경로 | 난이도 |
|---|---|---|---|---|---|
| GH-1 | **하이퍼링크 URL 드롭(md/html)** | `markdown.rs`·`html.rs`·`field.rs`(`hyperlink_url` 헬퍼 신설) | ✅ **해소(2026-07-15)** — md `[표시](URL)`, html `<a href>`, md 왕복 보존 테스트 | 내보내기 | S |
| GH-2 | **이미지 드롭(md/html)** | `markdown.rs`(`MarkdownOptions.media_dir`)·`html.rs`·`image.rs`(`image_kind` 헬퍼) | ✅ **해소(2026-07-15)** — html=data URI 임베드, convert .md=`<스템>.media/` 사이드카 추출(cat stdout은 기존 유지) | 내보내기 | S |
| GH-3 | **각주/미주가 마커 없이 본문 인라인 흡수(md/html/odt 공통)** — `[^n]`·`<text:note>` 미사용 | `markdown.rs`, `html.rs:204-223`, `odt.rs:181-199` | ✅ **md 해소(2026-07-18)** — 본문 `[^N]`/`[^eN]` 마커 + 문서 끝 정의(GFM 풋노트). html/odt는 기존 근사 유지 | 내보내기 | S |
| GH-4 | **병합 셀 평탄화** — col_span/row_span을 어떤 출력도 반영 안 함(colspan/rowspan·columns-spanned 미방출) | `markdown.rs`, `html.rs:172-203`, `odt.rs:203-243` | ✅ **md 해소(2026-07-18)** — 병합 셀 있으면 HTML `<table>`(colspan/rowspan) 폴백 → 단, GFM 표 유지는 무병합 표만. html/odt는 기존 근사 유지 | 내보내기 | S |
| GH-5 | **셀 내 블록(중첩표·이미지) 드롭** — 셀은 인라인 텍스트만 취하고 블록 버퍼 폐기 | `odt.rs:215`(blk 폐기), `markdown.rs`, `html.rs:181-189` | ✅ **md 해소(2026-07-18)** — 중첩 표·블록 수식 감지 시 HTML 표 폴백, 셀 fragment를 등장 순서대로 보존하고 이미지도 안전하게 참조. html/odt는 기존 | 내보내기 | M |
| GH-6 | **리스트 평문화(md)** — 헤딩만 인식, 글머리표/번호 문단을 `- `/`1. ` 구문으로 복원 안 함 | `markdown.rs` + `hwp-model/src/list.rs`(render에서 이동, SSOT) | ✅ **해소(2026-07-18)** — `- `/`N. ` 목록 + 부모 마커 폭 기준 들여쓰기, 정의별 번호 카운터와 구역별 재시작, 번호 형식 합성(숫자 외는 리터럴 마커) | 내보내기 | S |
| GH-7 | **ODT 페이지 레이아웃 미재현** — 여백·다단·머리말 위치 생략(모듈 주석에 명시) | `odt.rs:3-5` | 근사(생략) | 내보내기 | M |
| GH-8 | **수식·글자효과 드롭(md)** — eqed 스크립트 미방출, 밑줄/취소선/위·아래첨자 평문화 | `markdown.rs` | ✅ **해소(2026-07-18)** — 수식 인라인 `$..$`/블록 `$$..$$`(HWP 스크립트 원문), `<u>`·`~~`·`<sup>`·`<sub>` 스팬 | 내보내기 | S |

## 9. GI — 들여오기(markdown/JSON) 한계 (2026-07-08 재수색 추가)

| ID | 현상 | 근거 코드 | 현 동작 | 영향 경로 | 난이도 |
|---|---|---|---|---|---|
| GI-1 | **GFM 확장 미파싱** — 취소선(`~~`)·각주·작업목록 파서 옵션 미활성(표만 켬) | `hwp-convert/src/from_markdown.rs:234-236` | 미지원(평문) | 들여오기 | S |
| GI-2 | **순서·중첩 리스트가 단일 "• " 문단으로** — `list_depth` 추적만 하고 미사용, ordered 구분 없음 | `from_markdown.rs:465-471` | 근사(불릿 고정) | 들여오기 | S |
| GI-3 | **markdown 이미지 `![alt](url)` 드롭** — `Tag::Image` 핸들러 없음 | `from_markdown.rs:390-511`(Image arm 부재) | 드롭 | 들여오기 | S |
| GI-4 | **인라인 코드 서식 소실** — `Event::Code`를 평문 삽입(모노스페이스 글자모양 없음) | `from_markdown.rs:425` | 근사 | 들여오기 | S |
| GI-5 | **from_json 이미지 바이트 조건부** — `--embed-bin` 없으면 bin `data`가 skip이라 유실 | `hwp-convert/src/lib.rs:39,68-96` | 부분(조건부) | 들여오기 | S |

## 10. GJ — 미지원 포맷·레거시 (2026-07-08 재수색 추가)

입력/출력 포맷 축의 갭. 수요·선례 근거는 [08](08-external-research.md) 생태계 대조 참조.

| ID | 현상 | 근거 | 현 동작 | 난이도 |
|---|---|---|---|---|
| GJ-1 | **DOCX 입출력 부재** — 가장 흔한 상호운용 요구. MS가 공식 배치 변환기(HwpConverter+BATCHHWPCONV)를 배포할 정도의 수요인데 OSS HWP→DOCX는 무주공산 | 코드 흔적 전무(grep), `hwp-cli/src/main.rs` ConvertFormat에 없음 | 미구현 | M~L |
| GJ-2 | **HWPML(.hml) 입출력 부재** — 한컴 공식 스펙(HWPML rev1.2 Part II)·KS 표준 존재, kordoc 구현 선례 | grep 무일치. hwpml은 네임스페이스 URI로만 등장 | 미구현 | M |
| GJ-3 | **HWP 3.x 레거시 침묵 거부** — `V3.00` 시그니처 감지 없이 generic "시그니처 불일치" 에러. 공식 스펙(3.0 rev1.2 Part I) 존재, rhwp·kordoc·LibreOffice hwpfilter 선례 | `hwp-cli/src/format.rs:22-38`(CFB/ZIP만) | 침묵 거부 | 감지=S / 파싱=M~L |
| GJ-4 | **RTF 입출력 부재** | grep 무일치 | 미구현 | M |
| GJ-5 | **표→CSV 추출 부재** — 표를 데이터로 뽑는 경로 없음(수요의 정량 근거는 미검증 — [08] caveat) | grep 무일치 | 미구현 | S |
| GJ-6 | **`.txt` 확장자 추론 실패** — `convert -o out.txt`가 에러, 평문은 `cat`→stdout뿐 | `hwp-cli/src/commands/convert.rs:195-213`(txt arm 없음) | 미지원 | S |
| GJ-7 | **HTML/ODT/PDF 역방향 입력 부재** — 입력은 hwp5/hwpx/json/markdown만(출력 전용 4포맷) | `hwp-cli/src/commands/cat.rs:18-44` | 미구현(단방향) | L |
| GJ-8 | **HWPX 배포용 문서** — 어느 구현체도 미지원(H2Orestart #42 오픈). HWP5용 공식 배포 스펙이 HWPX 변형을 커버하는지 미확인 | [08](08-external-research.md) 미해결 질문 | 미구현 | L |

## 11. GK — 편집 프리미티브 부재 (2026-07-08 재수색 추가)

`edit`/`structure`/`format` 계열에 없는 조작. 전부 "부재 확인(grep)"이며 근거는
`hwp-convert/src/{edit,structure,format}.rs`·`hwp-cli/src/main.rs:113-165`(Edit 플래그 전수).

| ID | 현상 | 비고 | 난이도 |
|---|---|---|---|
| GK-1 | **셀 병합/분할 없음** — span은 보존·복제만, 조작 API 없음 | `structure.rs`는 행 add/delete만 | M |
| GK-2 | **열(column) 추가/삭제 없음** — 행 조작만 존재 | `structure.rs:118-168` | M |
| GK-3 | **표 신규 삽입 없음** — from_markdown은 표를 만들지만 앵커 기반 삽입 프리미티브 없음 | — | S |
| GK-4 | **문단모양 편집이 정렬 한정** — 줄간격·들여쓰기·좌우 여백·문단 간격 변경 없음 | `format.rs:211-245`(attr1 정렬 비트만) | S |
| GK-5 | **머리말/꼬리말 편집 없음** — 추출 포함/제외만 가능 | `text.rs:62-66` | M |
| GK-6 | **페이지 설정 변경 없음** — 여백·용지·방향(PageDef는 new 시 상수 주입만) | `from_markdown.rs:562-573` | S |
| GK-7 | **명명 스타일 적용/생성 없음** — 직접 모양 조작만, "제목1" 스타일 링크 편집 없음 | `format.rs` 전체 | M |
| GK-8 | **개체 삭제 없음** — 이미지/필드/표/책갈피 삭제 불가(삽입·문단/행 삭제만 — 비대칭) | `edit.rs`·`field.rs`·`image.rs` | S |

## 12. GL — 텍스트 추출 옵션 (2026-07-08 재수색 추가)

| ID | 현상 | 근거 코드 | 난이도 |
|---|---|---|---|
| GL-1 | **TextOptions(머리말/숨은설명 토글)가 CLI 미노출** → ✅ **해소(2026-07-15)** — `cat --with-header-footer`·`--with-hidden` 플래그 추가(plain·markdown 적용) | `hwp-model/src/text.rs` ↔ `main.rs`·`commands/cat.rs` | S |
| GL-2 | **각주/미주 분리·제외 불가** — 항상 본문에 포함(강제), 각주만 뽑기/빼기 없음 | `text.rs:62-66`(`_ => true`) | S |
| GL-3 | **표 제외·페이지/구역 범위 추출 없음** — 전량 추출만 | `text.rs:20-40` | S |

## 13. GM — CLI 명령·워크플로 (2026-07-08 재수색 추가)

서브커맨드 전수(`main.rs`: info·cat·convert·render·new·diff·edit·fields·bookmarks·slots·fill·
validate·mcp·dump) 기준 부재 목록. 수요 근거는 [08](08-external-research.md) 생태계 대조.

| ID | 현상 | 수요·선례 근거 | 난이도 |
|---|---|---|---|
| GM-1 | **배치/glob/디렉토리 처리 없음** — 전 명령이 단일 파일 인자 | MS BATCHHWPCONV·H2Orestart headless가 배치 수요 실증 | S |
| GM-2 | **stdin 입력·stdout 파이프 미흡** — convert/edit은 출력 파일 필수, `-` 미지원(cat만 stdout) | 유닉스 CLI 관례 | S |
| GM-3 | **문서 병합 없음** — 여러 hwp를 하나로 | pyhwpx 쿡북 정식 챕터(33개→99쪽 병합), 현행 해법은 Windows COM 전용·불안정 | M |
| GM-4 | **문서 분할/페이지 추출 없음** — render `--pages`는 이미지용 | pyhwpx 쿡북(100쪽→1쪽씩 분할 저장) | M |
| GM-5 | **텍스트 검색(grep) 명령 없음** — edit `--replace`만 존재 | — | S |
| GM-6 | **메타데이터 일괄 편집/덤프 없음** — `--set-meta`는 new/edit 국소 | — | S |
| GM-7 | **도장/서명 자동 날인 없음** — "(인)" 앵커에 도장 이미지 배치 | kordoc seal 구현 선례(공공 실무 빈출). insert-image 프리미티브는 기존재 | S |
| GM-8 | **문서 내용 비교 없음** — `diff`는 렌더 픽셀 비교 전용, 텍스트/구조 비교 없음 | kordoc compare_documents 선례 | M |

## 14. 로드맵 — 난이도 × 가치 + 의존 그래프

### 14.1 난이도 × 가치 매트릭스

**가치**는 실문서 출현 빈도 + 실사용 수요([08](08-external-research.md) 생태계 대조) 기준.

| | **난이도 S**(자료구조만) | **난이도 M**(정답지 필요) | **난이도 L**(실기 반복) |
|---|---|---|---|
| **가치 高**(빈출) | GC-4·GC-5(탭·구역속성), GC-8·GC-9(내어쓰기·문단배경) — ✅해소(2026-07-15): ~~GE-α1~α5·α7, GH-1·GH-2, GL-1, GA-5, GE-β4~~ / ✅해소(2026-07-18, md): ~~GH-3·GH-4·GH-5·GH-6, GH-8~~ | GC-2·GC-3(쪽테두리·각주모양), GG-3·GG-4(양쪽정렬·자간), GF-2(찾아보기·겹침), **GA-2★**(배포용 읽기 — 공식 스펙 공개), **GJ-1**(DOCX 출력 — 수요 최상·무주공산), **GK-1·GK-2**(셀 병합·열 조작) | GG-1·GG-2(글상자 드롭·오버플로) |
| **가치 中** | GC-6(글상자 다단), GE-2~GE-6(그림 드롭·단·번호 합성), GF-1(%unk), **GB-12**(참고문헌), **GE-β1·β2·β5**(미리보기·스크립트·설정), **GG-5·GG-6·GG-8~GG-11·GG-16·GG-17·GG-20**(렌더 국소), **GH-3·GH-4·GH-5**(html/odt 각주 마커·병합셀·셀 블록 — md는 2026-07-18 해소), **GI-1~GI-4**(md 들여오기), **GJ-5·GJ-6**(csv·txt), **GK-3·GK-4·GK-6·GK-8**(표 삽입·문단모양·페이지설정·삭제), **GM-1·GM-2·GM-5~GM-7**(배치·파이프·검색·메타·날인) | GB-4~GB-7·GB-10(글맵시·양식·묶음·메모·바탕쪽), GC-1(세로쓰기), GD-1~GD-3(수식 — rhwp 선례), GE-α6(그러데이션), GF-3(필드 생성), **GB-1 hwpx 차트 생성★**(chartSpace — kordoc 선례), **GJ-2·GJ-3**(hml·HWP3.x — 공식 스펙 공개), **GG-7·GG-12~GG-15·GG-18·GG-19**(렌더 픽셀 대조), **GE-β3·β6**(DocOptions·임베디드 폰트), **GH-7**(ODT 레이아웃), **GK-5·GK-7**(머리말 편집·스타일), **GM-3·GM-4·GM-8**(병합·분할·비교) | GB-2·GB-3(OLE·동영상), **GJ-1 완전 왕복**(docx 들여오기 포함 시) |
| **가치 低**(드묾) | GA-3·GA-4(거부 메시지), **GI-5**(embed-bin), **GL-2·GL-3**(추출 세분) | **GJ-4**(rtf) | GA-1(암호화), GB-8·GB-9·GB-11(변경추적 등), **GJ-7**(역방향 입력), **GJ-8**(HWPX 배포용) |

**읽는 법:** 좌상단(S·高)이 **가성비 최상** — GE-α(글자효과 왕복)에 더해 **GH-1·GH-2**(md/html
링크·이미지 — ODT 임베드 패턴 재사용)와 **GL-1**(clap 플래그만 추가)이 새 진입점이다.
★는 2026-07-08 재평가: **GA-2 배포용은 공식 복호화 스펙 공개로 L→M**, **GB-1 차트의 hwpx
경로는 OOXML chartSpace라 L→M**. 우하단(L·低)은 우선순위 최하.

### 14.2 의존 그래프

```
[정답지 확보]  ──선행──▶  GB-1~7(개체 렌더)  ──필요──▶  10/11 레코드 구조 해석
   │                       GC-2/GC-3(쪽테두리·각주모양) ── FOOTNOTE_SHAPE/PAGE_BORDER_FILL 의미해석
   │                       GD-1~3(수식 조판)  ── 정품 수식 메트릭
   │                       GG-1/GG-2(속성 충실도) ── 실기 반복(07§F)
   │                       GG-7/GG-12~15/GG-18/19 ── 정품 렌더 픽셀 대조
   │
[공식 스펙 존재 — 역설계 불요] ──▶ GA-2(배포용 복호화 — 배포용문서 rev1.2)
   │                              GJ-2(HWPML — 스펙 Part II)   GJ-3(HWP 3.x — 스펙 Part I)
   │                              (단, 스펙-실파일 불일치 사례가 있어 실파일 코퍼스 검증은 별도)
   │
[독립·즉시 착수] ──▶ GE-α6     (그러데이션 중심·step — α1~α5·α7·α8은 ✅해소 2026-07-15)
                    GC-8/GC-9  (hwp-render/layout.rs 국소, 렌더 전용)
                    GE-2       (write.rs 국소, 그림 드롭 경고→복구)
                    GA-3/GA-4  (거부 메시지 — GA-5 버전 게이트는 ✅해소)
                    GE-β5      (settings/version pass-through — β4는 ✅해소)
                    GM-7       (도장 날인 — insert_image 프리미티브 재사용)
                    (✅해소: GH-1/GH-2, GL-1)
   │
[수요 최상] ──▶ GJ-1(DOCX 출력) ──품질 선행──▶ GH-1/GH-2/GH-4 (링크·이미지·병합셀 정리가
                                               DOCX 매핑의 기초 데이터가 됨)
```

**의존 규칙 요약:**
- **GB 개체 렌더**는 10/11의 레코드/요소 구조 해석이 선행돼야 한다(현재 Opaque/fallback이라 의미
  필드가 IR에 없음). 또한 대부분 **정답지 확보가 선행**([00](00-overview.md) §4 정답지 방법론).
- **GC-2·GC-3**(쪽테두리·각주모양)은 hwp5가 이미 Opaque로 정보를 보존하므로, "정답지로 레코드
  레이아웃 확정 → IR 의미 필드 승격 → hwpx/렌더 방출"의 3단계다.
- **GE-α 전체**는 read가 이미 해석 완료라 **어떤 것에도 의존하지 않는 독립 노드**다. write 대응
  요소 방출만 추가하면 되는 최단 경로.
- **GG-1·GG-2**는 07§F의 미해결과 동일 뿌리(속성 충실도)라 **실기 반복 + 정답지**가 공동 선행.

### 14.3 정답지 선행 항목 (실기·정품 파일 필요)

아래는 [00](00-overview.md) §4 정답지 방법론에 따라 **정품 한글 파일 확보가 선행돼야** 착수 가능한
항목이다(추측 조판 금지). 나머지(특히 GE-α·GH·GL·GC 국소·렌더 국소)는 정답지 없이 자료구조/렌더
만으로 진행 가능하다.

- **GB-1~GB-7, GB-10**: 차트·OLE·동영상·글맵시·양식·묶음·메모·바탕쪽 — 해당 개체를 담은 정품 파일
- **GC-1, GC-2, GC-3**: 세로쓰기·쪽테두리·각주모양 — 해당 조판을 쓴 정품 파일
- **GD-1~GD-3**: 행렬·큰연산자·복잡 구분자를 포함한 정품 수식
- **GG-1, GG-2**: 07§F 서사대로 실기 반복 필요
- **GG-7, GG-12~GG-15, GG-18, GG-19**: 정품 렌더와의 픽셀 대조로 확정
- **GA-2, GJ-2, GJ-3**: 공식 스펙으로 착수 가능하되, 스펙-실파일 불일치 사례가 알려져 있어
  ([08](08-external-research.md) — 단 정의 14 vs 16B) 정품 코퍼스 검증을 병행

---

**요약:** 초판의 저비용·고가치 진입점(GE-α 글자효과, GH-1·GH-2 링크·이미지, GL-1 추출 옵션,
GA-5 버전 게이트, GE-β4 요약정보)은 **2026-07-15에 일괄 해소**됐다(§0.5). 다음 진입점은
**GC-4·GC-5·GC-8·GC-9**(탭·구역속성·내어쓰기·문단배경, S)와 **GE-β5·GM-7**(설정 pass-through·
도장 날인, S)이고, 고가치·고난도의 정공법은 **GC-2·GC-3**(공문서 빈출 쪽테두리·각주모양)과
**GA-2**(배포용 읽기 — 공식 스펙 공개로 재평가된 M), 상품 관점의 최대 수요는 **GJ-1**(DOCX 출력
— OSS 무주공산)이다.

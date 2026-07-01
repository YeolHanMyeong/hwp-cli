# hwp-cli

> A clean-room Rust toolkit to read, convert, render, write and AI-edit HWP 5.0 / HWPX documents with **no Hancom or COM dependency** — runs on Linux / macOS / CI.

[![CI](https://github.com/YeolHanMyeong/hwp-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/YeolHanMyeong/hwp-cli/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#라이선스)
[![Rust](https://img.shields.io/badge/rust-edition%202024-orange.svg)](Cargo.toml)

한글 문서(`.hwp` HWP 5.0 바이너리, `.hwpx` OWPML/KS X 6101)를 **외부 HWP 라이브러리 없이**
읽고·추출하고·변환하고·렌더링하고·쓰고·AI로 편집하는 Rust 워크스페이스. CFB 컨테이너, HWP 레코드
스트림, OWPML XML, 페이지 레이아웃, 글리프 셰이핑까지 전부 스펙 기반으로 직접 구현한 클린룸
엔진이다. 한컴오피스나 Windows COM 자동화에 의존하지 않으므로 Linux/macOS 서버와 CI에서
그대로 동작한다.

## 주요 기능

- **읽기·텍스트 추출** — hwp/hwpx → plain / markdown / JSON(전체 IR). 표·이미지·머리말/꼬리말·
  미해석 레코드까지 보존하며 파싱한다.
- **포맷 변환** — hwp ↔ hwpx, hwp/hwpx ↔ markdown, hwp/hwpx ↔ JSON(IR). 공용 문서 모델(IR)을
  경유한 양방향 변환.
- **렌더링** — hwp/hwpx → **PNG / SVG / PDF**. 파일에 저장된 줄 배치(PARA_LINE_SEG)를 우선 사용하고,
  없으면 자체 줄바꿈으로 보정한다. 표·이미지·머리말/꼬리말·밑줄/취소선·글상자·**그리기 개체(선·사각형·
  타원·호·다각형)**·**연결 다단 글상자**를 그리고, **양쪽 정렬**을 포함한 정렬을 적용한다. PDF는 폰트를
  서브셋·임베드한 단일 멀티페이지 문서로, **텍스트 선택·검색·복사가 가능**하다(ToUnicode CMap).
- **문서 쓰기 (hwp 바이너리 포함)** — hwpx 패키지 쓰기와 **HWP 5.0 바이너리(CFB) 쓰기**를 모두 구현.
  hwp 출신·무수정 문서는 표·이미지·도형·책갈피를 포함해도 압축 해제 스트림 기준 **바이트 동일 왕복**까지
  보장한다(전체 fixture 게이트).
- **AI 편집** — IR을 JSON으로 내보내 고치고 되쓰는 read→edit→rewrite 왕복. 텍스트 치환, 표 셀 설정,
  누름틀/필드 채우기를 이미지·서식·미해석 레코드를 보존한 채 인메모리로 적용한다.
- **MCP 서버** — 의존성 없는(serde_json만) stdio MCP 서버로 8개 도구를 노출. 에이전트가 문서를
  읽고·렌더해서 직접 보고·편집·변환한다.

## 현재 범위와 한계

- **구현 완료**: hwp/hwpx 읽기, hwpx 쓰기, **hwp 바이너리 쓰기**(convert hwpx→hwp, new→hwp,
  edit→hwp 포함), markdown/JSON 왕복, PNG/SVG/PDF 렌더링(표·그림·글상자·도형·각주/미주·글자 효과),
  렌더 충실도 diff, 누름틀/필드 채우기, MCP 서버.
- **PDF 출력** — 폰트 임베드(서브셋, CIDFontType2(glyf)·CIDFontType0(CFF/OTF), Identity-H) + ToUnicode로
  텍스트 검색이 가능한 단일 멀티페이지 PDF. glyf는 FontFile2, CFF(OTF)는 FontFile3(OpenType)로 정식
  임베드한다. 그리기 개체(선·사각형·**둥근 사각형**·타원·호·다각형)는 **hwp·hwpx 모두** 그리고, 채움은
  **단색·그러데이션(선형/방사형)·이미지**, 선은 **점선·화살표**, 표는 **대각선 테두리**까지 렌더한다.
  글자 효과는 **음영(형광펜)·위/아래 첨자·그림자**를, 본문에는 **각주/미주**(하단 영역 + 번호 참조)를 그린다.
  양쪽 정렬은 공백 우선 분배. 수식은 상자+스크립트로 근사 렌더하며, 차트·OLE만 아직 미지원이다.
- **무손실 왕복의 범위**: hwp 출신·무수정 문서는 표·이미지·도형·책갈피를 포함해도 압축 해제 스트림
  기준 바이트 동일 왕복까지 보장한다(전체 fixture 게이트). 편집했거나 hwpx/markdown 출신인 문서는
  writer의 합성 경로를 거쳐 **의미 동등**(텍스트·구조 보존)으로 되쓴다. hwpx 쓰기는 항상 의미 동등
  (템플릿 기반 재생성)이다. JSON 이미지까지 포함한 완전 무손실 왕복은 `--embed-bin`(base64 임베드)
  경로 전용이다.
- **미지원 입력**: 암호화/배포용(DRM) HWP 5.0 문서는 읽기를 거부한다.
- **의미 모델 한계**: 표·그림·구역·머리말/꼬리말·글상자·필드·각주/미주는 의미 파싱·렌더되지만, 도형·
  수식·차트·OLE는 렌더는 되어도(도형은 hwp·hwpx 모두) **포맷 간 의미 변환**(hwp↔hwpx 도형 레코드 합성)은
  아직 안 된다 — 같은 포맷 안에서는 원형 보존(round-trip)된다. 누름틀/필드는 기존 이름의 값만 채울 수
  있고 신규 필드 생성은 없다.

## 설치와 빌드

### 사전 요구사항

- **Rust** edition 2024, `rust-version = 1.93` 이상(워크스페이스 `Cargo.toml` 기준).
- **CJK 폰트** — 렌더링과 hwp 바이너리 쓰기(미리보기 텍스트/이미지)에 한글 글리프가 필요하다.
  레포에 함초롬 폰트(`HCRBatang`, `HCRDotum`, 각 Bold 포함 4종)가 `fonts/`에 동봉되어 있다.

#### 폰트 지정 방법

| 사용처 | 폰트 지정 |
|---|---|
| `render` / `diff` / `mcp` | `--font-dir <dir>` 플래그(반복 지정 가능) |
| `convert` / 테스트 | 환경변수 `HWP_FONT_DIR`(미설정 시 프로젝트 `fonts/` 자동 사용) |

### 빌드 / 설치

```sh
git clone git@github.com:YeolHanMyeong/hwp-cli.git && cd hwp-cli
cargo build --release
cargo install --path crates/hwp-cli   # `hwp` 바이너리 설치
```

> 이후 예시에서 `<repo>`는 위에서 클론한 디렉터리의 절대 경로를 가리킨다.

### 다운로드 (사전 빌드 바이너리)

각 [릴리스](https://github.com/YeolHanMyeong/hwp-cli/releases)에 플랫폼별 `hwp` 아카이브와 `.sha256` 체크섬이 첨부된다:

| 플랫폼 | 아카이브 |
|---|---|
| Linux x86_64 | `hwp-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| macOS Apple Silicon | `hwp-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `hwp-vX.Y.Z-x86_64-apple-darwin.tar.gz` |
| Windows x86_64 | `hwp-vX.Y.Z-x86_64-pc-windows-msvc.zip` |

압축을 풀어 `hwp`를 PATH에 두면 된다(체크섬 검증: `shasum -a 256 -c hwp-*.sha256`).
렌더/PDF엔 CJK 폰트가 필요하고(위 폰트 지정 참고), 텍스트 추출·변환은 폰트 없이 동작한다.

### 릴리스 (메인테이너)

버전은 워크스페이스 `Cargo.toml`의 `[workspace.package] version`이 단일 기준(SSOT)이다.

```sh
scripts/release.sh 0.2.0                        # 버전 bump + 커밋 + 태그 (푸시는 수동)
git push origin main && git push origin v0.2.0  # 태그 푸시가 릴리스를 트리거
```

`vX.Y.Z` 태그를 푸시하면 `release.yml`이 ① 테스트(fmt/clippy/test) 통과 ② 태그↔`Cargo.toml` 버전 일치를 확인한 **뒤에만** 4개 플랫폼 바이너리를 빌드해 GitHub Release로 게시한다. 태그가 커밋된 버전과 다르면 릴리스가 차단된다.

## 빠른 시작 (Quickstart)

```sh
# 진단: 포맷/버전/속성/스트림
hwp info report.hwp

# 본문 추출
hwp cat report.hwp                       # plain text
hwp cat report.hwp --format markdown     # markdown
hwp cat report.hwp --format json         # 전체 IR(JSON)

# 변환 (출력 확장자로 포맷 추론)
hwp convert report.hwp   -o report.hwpx  # hwp → hwpx (표·이미지·머리말 보존)
hwp convert report.hwpx  -o report.hwp   # hwpx → hwp 바이너리
hwp convert report.hwp   -o report.md    # hwp → markdown
hwp convert report.hwp   -o doc.json --embed-bin   # 이미지까지 임베드한 자급식 JSON

# 렌더링 (함초롬 폰트 디렉터리 지정)
hwp render report.hwp -o page.png --dpi 150 --font-dir ./fonts
hwp render report.hwp -o page.svg --pages 1-3 --font-dir ./fonts
hwp render report.hwp -o report.pdf --font-dir ./fonts   # 단일 멀티페이지 PDF(검색 가능)
hwp convert report.hwp -o report.pdf   # convert로도 동일(.pdf는 렌더 출력) — 폰트는 시스템 글꼴 사용

# 새 문서 생성
hwp new -o out.hwpx --from notes.md
hwp new -o out.hwp  --from doc.json

# 편집 (이미지·서식·미해석 레코드 보존)
hwp fields form.hwp                        # 채울 수 있는 필드/누름틀 이름 확인
hwp edit form.hwp -o filled.hwp \
    --replace "초안=>최종" \
    --set-cell "0:1:2=12,300원" \
    --set-field "수신처=홍길동" --verify

# 구조 편집 (문단 삽입/삭제·표 행 추가/삭제 — 모양 상속)
hwp edit report.hwp -o out.hwp \
    --insert-para "개요=>추가 설명 문단입니다." \
    --insert-para-before "결론=>결론 직전에 삽입." \
    --delete-para "임시 메모" \
    --add-row "0" --delete-row "1:3" --verify

# 누름틀 생성 + 채우기 (앵커 텍스트 뒤에 %clk 필드 삽입 후 이름으로 채움)
# 출력이 .hwp든 .hwpx든 필드가 보존된다(hwpx는 OWPML fieldBegin/fieldEnd로 왕복).
hwp edit form.hwp -o out.hwpx \
    --create-field "수신:=>수신처" --set-field "수신처=홍길동" --verify

# 이미지 삽입 (앵커 뒤에 그림 — 자연 크기 또는 "@너비x높이"mm)
hwp edit report.hwp -o out.hwp \
    --insert-image "그림:=>./logo.png" \
    --insert-image "표지:=>./cover.jpg@120x80" --verify

# 렌더 충실도 비교 (한글 기준 PNG와 잉크/오프셋/픽셀 오차)
hwp diff report.hwp --ref hancom_p1.png --page 1 --dpi 150 --font-dir ./fonts

# MCP stdio 서버
hwp mcp --font-dir ./fonts
```

## 명령 레퍼런스

| 명령 | 인자 / 플래그 | 설명 |
|---|---|---|
| `info <file>` | `--json` | 포맷/버전/속성/스트림 진단 |
| `cat <file>` | `--format plain\|markdown\|json` (기본 `plain`), `--preview` | 본문 추출. `--preview`는 본문 파싱 없이 PrvText만 출력 |
| `convert <input> -o <output>` | `--to hwp\|hwpx\|md\|json`(생략 시 확장자 추론), `--strict`(예약 — 현재 미동작), `--preserve-layout`, `--embed-bin` | 포맷 변환. 출력이 `.pdf`이면 렌더 경로로 위임(시스템 글꼴 사용 — 정밀 글꼴은 `render --font-dir` 권장). `--preserve-layout`는 무수정 왕복 전용 줄 배치 보존. `--embed-bin`은 JSON에 이미지 base64 임베드. `--strict`는 향후 보존 불가 데이터 발견 시 실패 처리 예정(현재는 동작하지 않음) |
| `render <input> -o <output>` | `--pages "1"\|"1-3"\|"all"`(기본 `all`), `--dpi <f64>`(기본 96, 래스터 전용), `--format png\|svg\|pdf`(생략 시 확장자 추론), `--font-dir <dir>`(반복) | 페이지를 PNG/SVG(페이지별 파일)·PDF(단일 멀티페이지)로 렌더. 번호 목록은 형식 템플릿(`^1.`→"1.", `(^5)`→"(5)", `제^1조`→"제1조")을 적용 |
| `new -o <output>` | `--from <md\|json>`(생략 시 빈 문서) | markdown/JSON IR에서 새 문서 생성 |
| `edit <input> -o <output>` | `--replace "찾기=>바꾸기"`, `--set-cell "표:행:열=값"`(0-기반), `--set-field "이름=값"`, `--create-field "앵커=>이름"`(또는 `"앵커=>이름=값"`, %clk 누름틀 생성), `--insert-image "앵커=>경로"`(또는 `"앵커=>경로@너비x높이"`mm, png/jpg/bmp/gif 삽입), `--set-format "찾기:bold=on,size=16,color=#RRGGBB"`, `--set-align "찾기=left\|right\|center\|justify\|distribute\|divide"`, `--insert-para "앵커=>텍스트"`(앵커 문단 뒤), `--insert-para-before "앵커=>텍스트"`(앞), `--delete-para "텍스트"`, `--add-row "표"`, `--delete-row "표:행"`, `--verify` (모두 반복 가능) | 기존 문서 편집. 텍스트·서식·구조(문단/표 행) 편집. 삽입 문단·행은 앵커/템플릿 모양을 상속하고 합성 경로로 저장(불변식 적용). `--verify`는 쓰기 후 재읽기로 검증 |
| `fields <file>` | `--json` | 필드/누름틀 목록(이름·종류·값·명령) |
| `diff <input> --ref <png>` | `--page <n>`(기본 1), `--dpi <f64>`(기본 96), `-o/--out <png>`, `--font-dir <dir>`(반복), `--tolerance <u8>`(기본 16) | 렌더 결과를 한글 기준 PNG와 비교(잉크 적용률·dx/dy 오프셋·픽셀 차이율·MAE) |
| `mcp` | `--font-dir <dir>`(반복) | MCP stdio 서버 실행 |
| `dump <file>` | `--stream <name>`, `--raw`, `--json` | [개발자용] 레코드/패키지 구조 덤프 |

> 출력 포맷은 대부분 출력 파일의 확장자(`.hwp` / `.hwpx` / `.md` / `.json` / `.png` / `.svg`)에서
> 추론된다. `convert`/`render`는 `--to`/`--format`으로 명시할 수도 있다.

## MCP 서버 (AI 에이전트 연동)

`hwp mcp`는 tokio나 SDK 없이 `serde_json`만으로 동기 JSON-RPC 2.0(stdio, 줄 단위)을 구현한 **MCP
서버**다(프로토콜 버전 `2024-11-05`). stdout은 프로토콜 전용이고 로그는 stderr로 나간다.
Windows/한컴이 필요한 COM 자동화와 달리 크로스플랫폼 오픈 엔진으로 동작한다.

### 노출 도구 (8종)

| 도구 | 필수 인자 | 기능 |
|---|---|---|
| `hwp_info` | `path` | 포맷/버전/속성/스트림 진단(JSON) |
| `hwp_read` | `path` (`format` = `plain`\|`markdown`\|`json`) | 본문 추출. `json`이면 전체 IR 구조 |
| `hwp_list_fields` | `path` | 필드/누름틀 목록(이름·종류·값·명령) |
| `hwp_render` | `path` (`page`, `dpi`, `font_dir`) | 지정 페이지를 **PNG 이미지로 반환** — 에이전트가 문서를 직접 본다 |
| `hwp_edit` | `input`, `output` (`replace[]`, `set_cell[]`, `set_field[]`) | 텍스트 치환·표 셀 설정·필드 채우기 후 되쓰기(이미지·서식 보존) |
| `hwp_convert` | `input`, `output` (`embed_bin`) | 포맷 변환(확장자로 결정) |
| `hwp_new` | `output` (`markdown` 또는 `json`) | markdown/JSON IR에서 새 문서 생성 |
| `hwp_diff` | `input`, `ref` (`page`, `dpi`, `font_dir`) | 렌더 결과를 기준 PNG와 비교(잉크 적용률·오프셋·픽셀 차이율) |

### 클라이언트 설정 예

```json
{
  "mcpServers": {
    "hwp": {
      "command": "hwp",
      "args": ["mcp", "--font-dir", "<repo>/fonts"]
    }
  }
}
```

### AI read → edit → rewrite 왕복

1. **읽기** — `hwp_read`(`format=json`)가 문서 전체를 IR(JSON)로 내보낸다. 텍스트뿐 아니라 표·이미지
   참조·서식·미해석 레코드까지 구조적으로 담긴다.
2. **편집** — 에이전트는 `hwp_edit`로 텍스트 치환·표 셀 설정·누름틀 채우기를 적용한다. IR만 바꾸므로
   이미지·서식·opaque 레코드가 보존되고, 편집된 문단의 줄 배치만 무효화되어 writer가 재합성한다.
   `hwp_new`/`hwp_convert`로 JSON IR을 그대로 문서로 되쓸 수도 있다.
3. **확인** — `hwp_render`가 결과 페이지를 PNG로 돌려주어 에이전트가 변경을 **눈으로 검증**한다.
   `hwp_diff`로 한글 기준 렌더와 정량 비교할 수 있다.

편집된 hwp는 writer의 합성 경로를 거쳐 한글 문단 불변식(줄 배치·문단끝 `0x0d`·nchars 등)을 다시
세우므로 한글에서 정상 문서로 열린다.

## 워크스페이스 구성

| 크레이트 | 역할 |
|---|---|
| `hwp-model` | 공유 문서 모델(IR) — 모든 크레이트가 의존하는 단일 계약. `Document{meta,header,sections,bin_streams}`, 무손실 보존(opaque/tail), 단위 변환 |
| `hwp5` | HWP 5.0 바이너리 reader/writer (CFB 컨테이너 + 레코드 스트림 + 압축) |
| `hwpx` | HWPX reader/writer (ZIP 패키지 + OWPML XML) |
| `hwp-convert` | IR ↔ markdown / JSON, 인메모리 편집(치환·셀·필드), 필드 스캔 |
| `hwp-render` | IR → PNG / SVG / PDF 렌더러, 줄 배치 합성, 텍스트 셰이핑, 폰트 서브셋·임베드, 렌더 diff |
| `hwp-cli` | `hwp` 바이너리 (CLI + MCP 서버) |

## 개발과 테스트

```sh
cargo build --all-targets
cargo test                              # 워크스페이스 전체 테스트
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

테스트는 hwp5의 바이트 동일 왕복(identity/roundtrip/synth), hwpx 의미 동등 왕복, IR JSON·markdown
왕복, 편집/필드 보정, 렌더 레이아웃·표·diff 메트릭 등을 포함한다.

HWP 5.0 포맷 스펙은 한컴 공식 [한글 문서 파일 형식 5.0](https://store.hancom.com/etc/hwpDownload.do)
문서를 참고한다 — 저작권상 저장소에 동봉하지 않고 공식 배포처 링크만 둔다(`docs/README.md` 참고).

**CI** (`.github/workflows/ci.yml`, GitHub Actions, Ubuntu): `fonts-noto-cjk` 설치 후
`cargo fmt --check` → `cargo clippy --all-targets -- -D warnings` → `cargo test`를 실행한다.

## 기여

버그 리포트와 PR을 환영한다. 이슈는 GitHub Issues에, 변경은 PR로 제출한다.

- PR 전 로컬에서 CI와 동일한 게이트를 통과시킨다: `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- 새 포맷 기능은 가능하면 왕복/골든 테스트를 함께 추가한다.
- 스펙 참고 자료는 한컴 공식 [한글 문서 파일 형식 5.0](https://store.hancom.com/etc/hwpDownload.do)
  문서를 본다(저장소에 동봉하지 않는다).

## 고지 (Acknowledgments)

본 제품은 한글과컴퓨터의 한글 문서 파일(`.hwp`) 공개 문서를 참고하여 개발하였습니다.

> This product was developed with reference to Hancom's HWP document file format open
> specification — [한글 문서 파일 형식 5.0 / HWP Document File Formats 5.0](https://store.hancom.com/etc/hwpDownload.do)
> (© (주)한글과컴퓨터).

한컴 공개 문서의 저작권은 (주)한글과컴퓨터에 있다. 한컴 공개 문서 라이선스는 자유로운 열람·복사·
배포를 허용하되 **수정되지 않은 원본/복사본**으로 제한하므로, 이 저장소는 스펙 문서(및 그 추출본·
페이지 캡처 등 파생물)를 **동봉하지 않고** 공식 배포처 링크만 제공한다(`docs/README.md` 참고).

테스트 픽스처 일부는 [hahnlee/hwp-rs](https://github.com/hahnlee/hwp-rs)(Apache-2.0)에서 가져왔다 —
`fixtures/README.md`와 루트 `NOTICE` 참고.

## 라이선스

이 프로젝트는 듀얼 라이선스다 — [MIT](LICENSE-MIT) 또는 [Apache-2.0](LICENSE-APACHE) 중 하나를
선택할 수 있다(워크스페이스 `Cargo.toml`에 `MIT OR Apache-2.0`으로 선언). 별도 명시가 없는 한, 이
저장소에 기여한 코드는 위 두 라이선스로 동일하게 배포되는 것에 동의하는 것으로 간주한다.

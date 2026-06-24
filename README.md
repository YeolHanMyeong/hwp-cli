# hwp-cli

한글 문서(.hwp, .hwpx)를 처리하는 Rust CLI. HWP 5.0 바이너리와 HWPX(OWPML,
KS X 6101) 포맷의 읽기·쓰기·변환·렌더링을 외부 HWP 라이브러리 없이 직접 구현한다.

## 목표 기능

- **읽기·텍스트 추출** — hwp/hwpx → plain/markdown/JSON
- **포맷 변환** — hwp → hwpx, hwpx ↔ markdown/JSON
- **이미지 렌더링** — hwp/hwpx → PNG/SVG/PDF (파일에 저장된 줄 배치
  정보(PARA_LINE_SEG)를 활용해 원본에 가까운 레이아웃)
- **문서 생성·쓰기** — hwpx와 **hwp 바이너리 쓰기** (생태계 공백)

## 사용법

```sh
hwp info <file>                     # 포맷/버전/속성/스트림 진단
hwp cat <file> [--format md|json]   # 본문 텍스트 추출 (--preview: PrvText)
hwp convert <in> -o out.md          # hwp/hwpx → markdown/JSON
hwp convert <in> -o out.hwpx        # hwp/hwpx → hwpx (표·이미지·머리말 보존)
hwp convert doc.json -o out.hwp     # JSON IR → hwp/hwpx (편집 왕복; --embed-bin로 이미지 임베드)
hwp render <in> -o out.png          # 페이지 렌더링 (PNG/SVG, --dpi, --font-dir)
hwp new -o out.hwpx --from doc.md   # markdown/JSON → 새 문서
hwp edit <in> -o out.hwp \          # 기존 문서 편집 (이미지·서식 보존)
    --replace "찾기=>바꾸기" --set-cell "표:행:열=값" [--verify]
hwp mcp [--font-dir <dir>]          # MCP stdio 서버 (AI 에이전트용 도구 인터페이스)
hwp dump <file> [--raw] [--json]    # [개발자용] 레코드/패키지 구조 덤프
```

### MCP 서버 (AI 에이전트 연동)

`hwp mcp`는 의존성 없이(serde_json만) **MCP(Model Context Protocol) stdio 서버**를 띄운다.
Claude 등 에이전트가 도구 호출로 HWP를 **읽고·렌더해서 보고·편집·변환**한다 — Windows/한컴이
필요한 COM 자동화와 달리 크로스플랫폼 오픈 엔진. 노출 도구:

| 도구 | 기능 |
|---|---|
| `hwp_info` | 포맷/버전/속성/스트림 진단(JSON) |
| `hwp_read` | 본문 추출 (plain/markdown/**json**=전체 IR 구조) |
| `hwp_render` | 페이지를 **PNG 이미지로 반환** — 에이전트가 문서를 직접 봄 |
| `hwp_edit` | 텍스트 치환·표 셀 설정 후 되쓰기(이미지·서식 보존) |
| `hwp_convert` | 포맷 변환(.hwp/.hwpx/.json/.md) |
| `hwp_new` | markdown/JSON에서 새 문서 생성 |
| `hwp_diff` | 렌더 결과를 기준 PNG와 비교(잉크 적용률·위치 오프셋) |

MCP 클라이언트 설정 예:
```json
{ "mcpServers": { "hwp": { "command": "hwp", "args": ["mcp", "--font-dir", "<repo>/fonts"] } } }
```

`cat --format json`은 전체 IR을 구조적으로 내보내고, `convert *.json`/`new --from *.json`은
이를 다시 문서로 쓴다(AI가 읽고·고치고·되쓰는 왕복). `edit`은 원본을 IR로 읽어 텍스트 치환·
표 셀 설정을 인메모리로 적용하므로 이미지·서식·opaque 레코드가 모두 보존된다. 편집된 hwp는
writer의 합성 경로를 거쳐 한글 문단 불변식(줄 배치·문단끝·nchars 등)을 다시 세운다.

렌더링은 표(테두리/배경), 이미지, 머리말/꼬리말, 밑줄/취소선을 지원하며
파일에 저장된 줄 배치(lineseg)를 우선 사용하고 불완전한 파일은 자체
줄바꿈으로 보정한다. hwp 바이너리 쓰기(M6)와 PDF 출력(M7)은 진행 중.

## 워크스페이스 구성

| 크레이트 | 역할 |
|---|---|
| `hwp-model` | 공유 문서 모델(IR) — 모든 크레이트의 계약 |
| `hwp5` | HWP 5.0 바이너리 reader/writer (CFB + 레코드 스트림) |
| `hwpx` | HWPX reader/writer (ZIP + OWPML XML) |
| `hwp-convert` | IR ↔ markdown/JSON |
| `hwp-render` | IR → PNG/SVG/PDF 렌더러 |
| `hwp-cli` | `hwp` 바이너리 |

## 개발

```sh
cargo build
cargo test
cargo clippy --all-targets
```

`docs/`에 한글문서파일형식 5.0 공식 스펙 PDF와 스펙 hwp 원본(배포용 문서
테스트 겸용)이 있다. 진행 상황과 설계 결정은 계획 문서(마일스톤 M0~M7) 참조.

## 라이선스

MIT OR Apache-2.0

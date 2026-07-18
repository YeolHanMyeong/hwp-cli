# CLAUDE.md

HWP 5.0(바이너리)·HWPX(OWPML)를 외부 HWP 라이브러리 없이 **직접 구현**하는 Rust 워크스페이스.
문서·주석·커밋 메시지·경고 문구는 한국어를 기본으로 한다.

## 빌드 · 테스트

```bash
cargo build                    # 디버그 빌드 (bin: hwp)
cargo test --workspace         # 전체 테스트 (fixture 없으면 통합 테스트는 자동 skip)
cargo clippy --workspace       # 린트
HWP_FONT_DIR=$PWD/fonts python3 tools/diagnostic_corpus.py   # 진단 코퍼스 + 자체 검증 하네스
```

- Rust edition 2024, rust-version 1.93.
- 렌더 테스트는 저장소 동봉 폰트(`fonts/` HCR바탕·돋움)를 쓴다.
- `HWP_GOLDEN=1` — 한글 기준 PNG와의 골든 렌더 대조(옵트인). `HWP_CORPUS_DIR` — 대형 야생 corpus 소크 테스트.

## 데이터 정책 (중요)

- `fixtures/hwp5/*.hwp`·`fixtures/hwpx/*.hwpx`는 gitignore(로컬 전용). 없으면 테스트가 skip될 뿐 실패하지 않는다. 출처는 `fixtures/README.md`.
- `fixtures/samples/`는 **예외적으로 커밋한다** — 소유자 자작 문서를 대학명 가명 치환한 테스트 샘플만 둔다(익명화 레시피는 `fixtures/README.md`). 원본은 커밋 금지.
- **정답지 코퍼스(`~/Documents/hwp_samples` 등 정품 한글 파일)는 절대 커밋 금지.**
- **한컴 스펙 문서·파생물(추출 텍스트, 페이지 캡처) 커밋 금지** — `docs/README.md` 참조. 스펙은 섹션 번호로만 인용한다(예: `한글문서파일형식 5.0 §4.2.6`). 로컬 `docs/spec.txt`(gitignore)는 작업 참고용.

## 설계 지식은 docs/design/ 에 있다

- 시작점: [docs/design/00-overview.md](docs/design/00-overview.md) (문서 색인·설계 원칙)
- **필독**: [07-hangul-compat-rules.md](docs/design/07-hangul-compat-rules.md) — 실기로만 확정된 한글 호환 규칙 카탈로그. 이 규칙을 모르고 writer를 고치면 한글에서 파일이 깨진다.
- 포맷 전수 지도: [10-hwp5-structure-map.md](docs/design/10-hwp5-structure-map.md)(레코드/컨트롤 카탈로그), [11-hwpx-structure-map.md](docs/design/11-hwpx-structure-map.md)(OWPML 요소 카탈로그)
- 미구현 기능은 [12-feature-gaps.md](docs/design/12-feature-gaps.md)에서 먼저 확인.

## 불변식 (어기면 안 됨)

1. **hwp-model은 다른 내부 크레이트에 의존하지 않는다** (허브-스포크). `hwp5`↔`hwpx`도 서로 의존하지 않고 IR을 경유한다.
2. **무손실 왕복 게이트**: hwp5→hwp5 identity 재직렬화는 바이트 동일이어야 한다(`crates/hwp5/tests/identity.rs`). 모르는 레코드는 버리지 말고 `OpaqueRecord`로 보존.
3. **정답지 방법론 — 추측 금지**: 포맷 동작은 한글이 저장한 정품 파일 바이트와의 대조로만 확정한다. 최종 판정은 한글(한컴오피스) 실기에서 열리는지 여부다.
4. 새 HWP 관련 외부 크레이트 추가 금지(인프라 크레이트 cfb/zip/quick-xml/tiny-skia 등만 허용).

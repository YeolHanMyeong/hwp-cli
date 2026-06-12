# 테스트 픽스처

## hwp5/

[hahnlee/hwp-rs](https://github.com/hahnlee/hwp-rs) (Apache-2.0)의 통합 테스트
픽스처에서 가져옴:

- `hello_world.hwp`, `bookmark.hwp`, `color_fill.hwp`, `outline.hwp` — 기능별 최소 파일
- `annual_report.hwp`, `work_report.hwp` — 실문서에 가까운 샘플

## hwpx/

- `minimal.hwpx` — hwpx MCP 서버로 생성한 최소 문서 (한/영/숫자 혼합 3문단)

## 대형 corpus

수백 개 이상의 야생 문서 소크 테스트는 커밋하지 않고
`HWP_CORPUS_DIR` 환경변수로 외부 디렉터리를 가리켜 실행한다.

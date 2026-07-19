<!-- hwp-cli PR 체크리스트 — 지우지 말고 확인 후 체크 -->

## 요약

<!-- 변경 목적과 범위를 2~3줄로 -->

## 체크리스트

- [ ] `scripts/check.sh` 통과 (fmt → clippy --all-targets -D warnings → test, CI와 동일 게이트)
- [ ] 한글(한컴오피스) 실기 확인이 필요한 변경인가? (writer/호환 규칙 영향)
  - 필요했다면: [ ] 실기 결과를 PR 본문에 기록 (열림/손상 팝업·레이아웃 확인)
  - 아니라면: [ ] 근거 한 줄 (예: 읽기 전용, 테스트·문서 변경)
- [ ] 데이터 정책 준수 (fixtures/samples 예외 외 픽스처 미커밋, 한컴 스펙 문서·파생물 미동봉 — CLAUDE.md §데이터 정책)
- [ ] 설계 문서 갱신 (해당 시): `docs/design/12-feature-gaps.md` 상태, 구조도(10/11), README/CLAUDE.md
- [ ] 새 기능이면 테스트 동반 (왕복/골든/CLI 표면 중 해당 경로)

## 참고

- 브랜치·PR 정책: CLAUDE.md §브랜치 · PR 정책. main 직접 push 금지, CI green 후 squash 머지.

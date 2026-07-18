# 테스트 픽스처

> **이 디렉터리의 `hwp5/*.hwp`·`hwpx/*.hwpx` 문서는 저장소에 동봉하지 않는다**
> (`.gitignore`로 제외 — 로컬 전용). 아래 출처에서 받아 같은 경로에 두면 렌더/PDF 테스트가
> 동작하고, 없으면 해당 테스트는 자동으로 **skip**된다. 이 README와 `golden/README.md`만 커밋한다.
>
> **예외: `samples/`는 커밋한다** — 저장소 소유자 자신의 문서를 익명화한 테스트 샘플로,
> 테스트가 하드 의존한다(skip 없음).

## samples/ (커밋)

- `report-tables.hwpx` — 표 편집(행/열 추가·복합 표 거부)과 패키지 보존 치환의 기능
  테스트 픽스처(A4 5쪽, 표 10개: 톱레벨 병합 3종 + 중첩 단순 6종 + [별표] 단순 7x2).
  **본문은 전부 가상의 예시 문구** — 실제 사업·기관 내용이 아니다.
  - 출처: 저장소 소유자 자작 문서(로컬 전용)를 아래 과정으로 익명화.
  - 익명화 파이프라인(재현용):
    1. 대학명 가명 치환(`hwp edit --replace`, 패키지 보존 경로): 제주한라대학교→한빛대학교,
       제주대학교→미륵대학교, 관광대→다온대 + 약칭, JOY→가온
    2. `tools/anonymize_fixture.py`로 본문 전체를 구조 보존 예시 문구로 재작성
       (문서 번호·불릿 마커·가명 대학명은 유지, `Preview/PrvImage.png` 제거,
       content.hpf creator/lastsaveby→`hwp-cli` 중화, **본문 linesegarray 제거**:
       텍스트 재작성으로 줄 배치 캐시가 어긋나면 한글이 "손상/변조" 경고를 띄운다)
    3. 검사: 전 엔트리에서 실명·실내용 키워드 0건, `hwp validate` 유효,
       본문 linesegarray 0건(`table_edit.rs::fixture_has_no_body_linesegarray`)

## hwp5/

[hahnlee/hwp-rs](https://github.com/hahnlee/hwp-rs) (Apache-2.0)의 통합 테스트
픽스처에서 가져옴:

- `hello_world.hwp`, `bookmark.hwp`, `color_fill.hwp`, `outline.hwp` — 기능별 최소 파일
  (hwp-rs `integration/project/files`)
- `annual_report.hwp`, `work_report.hwp` — 실문서에 가까운 샘플
  (hwp-rs `integration/naver_documents/files`, 원출처 Naver 무료 문서 템플릿)

`annual_report.hwp`에는 템플릿에 포함된 장식용 이미지(BinData JPG/PNG), `work_report.hwp`에는
작은 비트맵(117×17 BMP)이 임베드되어 있다. 본문은 자리표시자("OOOOO", "상세 내용을 입력하세요")
뿐인 빈 템플릿으로 실제 개인정보·조직 식별 정보는 없다. 모든 픽스처는 위 Apache-2.0 저장소에서
재배포된 것을 가져왔다(루트 `NOTICE` 참고).

## hwpx/

- `minimal.hwpx` — hwpx MCP 서버로 생성한 최소 문서 (한/영/숫자 혼합 3문단)

## 대형 corpus

수백 개 이상의 야생 문서 소크 테스트는 커밋하지 않고
`HWP_CORPUS_DIR` 환경변수로 외부 디렉터리를 가리켜 실행한다.

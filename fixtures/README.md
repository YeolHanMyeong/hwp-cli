# 테스트 픽스처

> **이 디렉터리의 `hwp5/*.hwp`·`hwpx/*.hwpx` 문서는 저장소에 동봉하지 않는다**
> (`.gitignore`로 제외 — 로컬 전용). 아래 출처에서 받아 같은 경로에 두면 렌더/PDF 테스트가
> 동작하고, 없으면 해당 테스트는 자동으로 **skip**된다. 이 README와 `golden/README.md`만 커밋한다.
>
> **예외: `samples/`는 커밋한다** — 저장소 소유자 자신의 문서를 익명화한 테스트 샘플로,
> 테스트가 하드 의존한다(skip 없음).

## samples/ (커밋)

- `report-tables.hwpx` — RISE 사업계획서 스타일 보고서(한컴오피스 12.30 저장, A4 5쪽).
  표 편집(행/열 추가·복합 표 거부)과 패키지 보존 치환의 기능 테스트 픽스처.
  - 출처: 저장소 소유자 자작 문서(`inbox/drop/kakao/hwp-cli-test-sample.hwpx`, 로컬 전용).
  - 익명화 레시피(재현용, `hwp edit` 패키지 보존 치환 + zip 수술):
    1. `hwp edit <원본> -o <중간>.hwpx`에 아래 `--replace`를 **긴 이름 먼저** 순서대로 적용:
       제주한라대학교→한빛대학교, 제주한라대→한빛대, 제주대학교→미륵대학교, 제주대→미륵대,
       관광대→다온대, 한라대→한빛대, JOY→가온 (지역명 `제주` 등은 유지)
    2. `Contents/content.hpf`의 creator/lastsaveby `yj.lee`→`hwp-cli` (zip 수준 치환)
    3. `Preview/PrvImage.png` 엔트리 제거(원문이 보이는 렌더 이미지 — 정책상 미동봉,
       한글이 열 때 재생성)
    4. 검사: 전체 엔트리에서 `한라/제주대/관광대/JOY/yj.lee` 0건, `hwp validate` 유효

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

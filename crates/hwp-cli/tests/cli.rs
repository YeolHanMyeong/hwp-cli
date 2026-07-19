//! `hwp` CLI 통합 테스트 — validate 종료코드 계약 (소비자가 exit code로 판정).

use std::path::PathBuf;
use std::process::Command;

fn hwp() -> Command {
    Command::new(env!("CARGO_BIN_EXE_hwp"))
}

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel)
}

/// fixture 바이너리는 저장소에서 제외된다(로컬 전용). 없으면 `true`(스킵).
fn skip_if_no_fixtures() -> bool {
    if fixture("hwpx/minimal.hwpx").exists() {
        return false;
    }
    eprintln!("스킵: fixtures 없음 — fixtures/README.md 참고");
    true
}

#[test]
fn validate_valid_hwpx_exit_zero() {
    if skip_if_no_fixtures() {
        return;
    }
    let out = hwp()
        .arg("validate")
        .arg(fixture("hwpx/minimal.hwpx"))
        .output()
        .expect("hwp 실행");
    assert!(
        out.status.success(),
        "유효 hwpx는 exit 0 (stderr: {})",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn validate_corrupt_exit_nonzero_json() {
    let bad = std::env::temp_dir().join("hwp_cli_bad.hwpx");
    std::fs::write(&bad, b"this is not a valid hwp/hwpx file").unwrap();

    let out = hwp()
        .args(["validate", "--json"])
        .arg(&bad)
        .output()
        .expect("hwp 실행");
    assert!(!out.status.success(), "손상 파일은 비-0 종료");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\"valid\": false") || stdout.contains("\"valid\":false"),
        "JSON에 valid:false (실제: {stdout})"
    );

    let _ = std::fs::remove_file(&bad);
}

#[test]
fn slots_json_shape() {
    // 합성 템플릿을 만들고 slots --json 구조 확인 (placeholders 배열).
    let tmp = std::env::temp_dir().join("hwp_cli_slots.hwpx");
    // hwp new로 {{name}}을 본문에 담은 hwpx 생성.
    let md = std::env::temp_dir().join("hwp_cli_slots.md");
    std::fs::write(&md, "{{기관명}} 본문 {{제목}}\n").unwrap();
    let mk = hwp()
        .args(["new", "--from"])
        .arg(&md)
        .arg("-o")
        .arg(&tmp)
        .output()
        .expect("hwp new");
    assert!(
        mk.status.success(),
        "hwp new: {}",
        String::from_utf8_lossy(&mk.stderr)
    );

    let out = hwp()
        .args(["slots", "--json"])
        .arg(&tmp)
        .output()
        .expect("hwp slots");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("placeholders"), "placeholders 키");
    assert!(
        stdout.contains("기관명") && stdout.contains("제목"),
        "자리표시자 이름"
    );

    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(&md);
}

fn tmp(name: &str) -> PathBuf {
    // PID 포함 — 동시 cargo test 실행 간 산출물 충돌(플레이크) 방지.
    let dir = std::env::temp_dir().join(format!("hwp-cli-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// 실패 진단용: 파일의 cat 출력(본문+stderr)을 덤프한다 (Windows CI 전용 결함 추적).
fn dump_file(path: &PathBuf) -> String {
    let out = hwp().arg("cat").arg(path).output().unwrap();
    format!(
        "stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn cat_with_header_footer_hidden_flags() {
    // 합성 문서로 cat 텍스트 추출 옵션 플래그가 파싱되고 본문을 출력하는지(스모크).
    let md = tmp("hwp_cli_cat_flags.md");
    std::fs::write(&md, "본문 텍스트입니다\n").unwrap();
    let src = tmp("hwp_cli_cat_flags.hwpx");
    assert!(
        hwp()
            .args(["new", "--from"])
            .arg(&md)
            .arg("-o")
            .arg(&src)
            .status()
            .unwrap()
            .success()
    );
    // plain + 두 플래그.
    let out = hwp()
        .arg("cat")
        .arg(&src)
        .args(["--with-header-footer", "--with-hidden"])
        .output()
        .expect("hwp cat");
    assert!(
        out.status.success(),
        "cat 플래그 실행 성공 (stderr: {})",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("본문 텍스트입니다"),
        "본문 출력"
    );
    // markdown 경로에도 플래그가 유효해야 한다.
    let md_out = hwp()
        .arg("cat")
        .arg(&src)
        .args(["--format", "markdown", "--with-hidden"])
        .output()
        .expect("hwp cat md");
    assert!(md_out.status.success(), "cat markdown 플래그 실행");
    assert!(
        String::from_utf8_lossy(&md_out.stdout).contains("본문 텍스트입니다"),
        "markdown 본문 출력"
    );
    for f in [&md, &src] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn convert_html_has_title_from_metadata() {
    let md = tmp("hwp_cli_html.md");
    std::fs::write(&md, "# 본문 제목\n\n내용\n").unwrap();
    let src = tmp("hwp_cli_html.hwpx");
    assert!(
        hwp()
            .args(["new", "--from"])
            .arg(&md)
            .arg("-o")
            .arg(&src)
            .args(["--set-meta", "title=메타 제목"])
            .status()
            .unwrap()
            .success()
    );
    let out = tmp("hwp_cli_html.html");
    assert!(
        hwp()
            .arg("convert")
            .arg(&src)
            .arg("-o")
            .arg(&out)
            .args(["--to", "html"])
            .status()
            .unwrap()
            .success()
    );
    let html = std::fs::read_to_string(&out).unwrap();
    assert!(html.starts_with("<!DOCTYPE html>"), "html 헤더");
    assert!(
        html.contains("<title>메타 제목</title>"),
        "메타데이터 제목이 <title>에: {}",
        &html[..html.len().min(200)]
    );
    for f in [&md, &src, &out] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn convert_pdf_embeds_image_xobject() {
    if skip_if_no_fixtures() {
        return;
    }
    // 이미지 있는 fixture → PDF는 %PDF- 헤더 + Image XObject (폰트 비의존).
    let out = tmp("hwp_cli_img.pdf");
    let status = hwp()
        .arg("convert")
        .arg(fixture("hwp5/annual_report.hwp"))
        .arg("-o")
        .arg(&out)
        .args(["--to", "pdf"])
        .status()
        .unwrap();
    assert!(status.success(), "convert pdf");
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"%PDF-"), "PDF 헤더");
    assert!(
        bytes.windows(6).any(|w| w == b"/Image"),
        "Image XObject 임베드"
    );
    let _ = std::fs::remove_file(&out);
}

#[test]
fn new_metadata_then_info_json() {
    let md = tmp("hwp_cli_meta.md");
    std::fs::write(&md, "본문\n").unwrap();
    let src = tmp("hwp_cli_meta.hwp");
    assert!(
        hwp()
            .args(["new", "--from"])
            .arg(&md)
            .arg("-o")
            .arg(&src)
            .args(["--set-meta", "title=제목X", "--set-meta", "author=지은이Y"])
            .status()
            .unwrap()
            .success()
    );
    let out = hwp().args(["info", "--json"]).arg(&src).output().unwrap();
    let j = String::from_utf8_lossy(&out.stdout);
    assert!(
        j.contains("제목X") && j.contains("지은이Y"),
        "메타데이터: {j}"
    );
    for f in [&md, &src] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn convert_odt_mimetype_first() {
    if skip_if_no_fixtures() {
        return;
    }
    let out = tmp("hwp_cli.odt");
    assert!(
        hwp()
            .arg("convert")
            .arg(fixture("hwpx/minimal.hwpx"))
            .arg("-o")
            .arg(&out)
            .args(["--to", "odt"])
            .status()
            .unwrap()
            .success()
    );
    let bytes = std::fs::read(&out).unwrap();
    // ODF: 첫 엔트리는 STORED mimetype. zip local header(30B) 직후 파일명 "mimetype".
    assert_eq!(&bytes[0..2], b"PK", "zip");
    assert!(
        bytes.windows(8).take(64).any(|w| w == b"mimetype"),
        "mimetype 첫 엔트리"
    );
    assert!(
        bytes
            .windows(39)
            .any(|w| w == b"application/vnd.oasis.opendocument.text"),
        "ODT mimetype 값"
    );
    let _ = std::fs::remove_file(&out);
}

#[test]
fn strict_fails_on_dropped_controls() {
    if skip_if_no_fixtures() {
        return;
    }
    // annual_report의 hwp→hwpx는 이제 무드롭(도형 전부 지원: rect/line/ellipse/arc/polygon).
    // 드롭 발생 경로는 역방향(hwpx→hwp) — hwpx-출신 장식 도형은 hwp5 SHAPE_COMPONENT
    // 정합 역합성을 안 하고 strip으로 드롭한다. --strict면 그 드롭에서 비정상 종료.
    let mid = tmp("hwp_cli_strict.hwpx");
    let fwd = hwp()
        .arg("convert")
        .arg(fixture("hwp5/annual_report.hwp"))
        .arg("-o")
        .arg(&mid)
        .args(["--to", "hwpx"])
        .status()
        .unwrap();
    assert!(fwd.success(), "hwp→hwpx는 무드롭으로 성공");

    let dst = tmp("hwp_cli_strict.hwp");
    let strict = hwp()
        .arg("convert")
        .arg(&mid)
        .arg("-o")
        .arg(&dst)
        .arg("--strict")
        .output()
        .unwrap();
    assert!(
        !strict.status.success(),
        "역방향 장식 도형 드롭 시 --strict면 비정상 종료"
    );
    assert!(
        String::from_utf8_lossy(&strict.stderr).contains("strict"),
        "strict 사유 출력"
    );
    let _ = std::fs::remove_file(&mid);
    let _ = std::fs::remove_file(&dst);
}

#[test]
fn fill_replaces_slots() {
    let md = tmp("hwp_cli_fill.md");
    std::fs::write(&md, "{{수신}} 귀하\n").unwrap();
    let tpl = tmp("hwp_cli_fill_tpl.hwpx");
    assert!(
        hwp()
            .args(["new", "--from"])
            .arg(&md)
            .arg("-o")
            .arg(&tpl)
            .status()
            .unwrap()
            .success()
    );
    let out = tmp("hwp_cli_fill_out.hwpx");
    let r = hwp()
        .arg("fill")
        .arg(&tpl)
        .arg("-o")
        .arg(&out)
        .args(["--set", "수신=홍길동", "--json"])
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "fill: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let j = String::from_utf8_lossy(&r.stdout);
    assert!(j.contains("\"replaced\""), "replaced 키: {j}");
    let filled = hwp().arg("cat").arg(&out).output().unwrap();
    assert!(
        String::from_utf8_lossy(&filled.stdout).contains("홍길동"),
        "치환 결과"
    );
    for f in [&md, &tpl, &out] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn edit_add_row_then_fill() {
    // 양식(2행 표) → 행 3개 추가(pass 1) → 추가 행 셀 채움(pass 2) → hwp5. cat으로 확인.
    // edit 순서상 구조편집(add-row)은 set-cell 뒤에 적용되므로 두 번에 나눠 호출한다.
    let md = tmp("hwp_cli_addrow.md");
    std::fs::write(&md, "| 품목 | 수량 |\n|------|------|\n| | |\n").unwrap();
    let form = tmp("hwp_cli_addrow_form.hwp");
    assert!(
        hwp()
            .args(["new", "--from"])
            .arg(&md)
            .arg("-o")
            .arg(&form)
            .status()
            .unwrap()
            .success()
    );
    // pass 1: 행 3개 추가
    let rows = tmp("hwp_cli_addrow_rows.hwp");
    let r1 = hwp()
        .arg("edit")
        .arg(&form)
        .arg("-o")
        .arg(&rows)
        .args(["--add-row", "0", "--add-row", "0", "--add-row", "0"])
        .output()
        .unwrap();
    assert!(
        r1.status.success(),
        "edit --add-row: {} | form.hwp: {}",
        String::from_utf8_lossy(&r1.stderr),
        dump_file(&form)
    );
    // pass 2: 추가된 행 셀 채움
    let out = tmp("hwp_cli_addrow_out.hwp");
    let r2 = hwp()
        .arg("edit")
        .arg(&rows)
        .arg("-o")
        .arg(&out)
        .args([
            "--set-cell",
            "0:1:0=노트북",
            "--set-cell",
            "0:3:0=키보드",
            "--verify",
        ])
        .output()
        .unwrap();
    assert!(
        r2.status.success(),
        "edit --set-cell: {}",
        String::from_utf8_lossy(&r2.stderr)
    );
    let cat = hwp().arg("cat").arg(&out).output().unwrap();
    let text = String::from_utf8_lossy(&cat.stdout);
    assert!(
        text.contains("노트북") && text.contains("키보드"),
        "내용: {text}"
    );
    for f in [&md, &form, &rows, &out] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn fill_data_tables_grows() {
    // 데이터 구동: --data tables 로 표를 데이터 수만큼 자동 증식 + 채움.
    let md = tmp("hwp_cli_filltab.md");
    std::fs::write(&md, "| 품목 | 수량 |\n|------|------|\n| | |\n").unwrap();
    let form = tmp("hwp_cli_filltab_form.hwp");
    assert!(
        hwp()
            .args(["new", "--from"])
            .arg(&md)
            .arg("-o")
            .arg(&form)
            .status()
            .unwrap()
            .success()
    );
    let data = tmp("hwp_cli_filltab.json");
    std::fs::write(
        &data,
        r#"{"tables":[{"table":0,"start_row":1,"rows":[["사과","3"],["배","7"],["감","9"]]}]}"#,
    )
    .unwrap();
    let out = tmp("hwp_cli_filltab_out.hwp");
    let r = hwp()
        .arg("fill")
        .arg(&form)
        .arg("-o")
        .arg(&out)
        .arg("--data")
        .arg(&data)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "fill --data tables: {} | form.hwp: {}",
        String::from_utf8_lossy(&r.stderr),
        dump_file(&form)
    );
    let j = String::from_utf8_lossy(&r.stdout);
    assert!(j.contains("\"rows_added\""), "rows_added 키: {j}");
    let cat = hwp().arg("cat").arg(&out).output().unwrap();
    let text = String::from_utf8_lossy(&cat.stdout);
    assert!(
        text.contains("사과") && text.contains("배") && text.contains("감"),
        "데이터 채움: {text}"
    );
    for f in [&md, &form, &data, &out] {
        let _ = std::fs::remove_file(f);
    }
}

#[test]
fn fill_literal_tables_key_not_misrouted() {
    // 최상위 "tables"가 (표 지시 객체가 아닌) 문자열 배열이면 평문 자리표시자 치환으로
    // 라우팅돼야 한다(IR 표 채우기로 오인 → "rows 배열 필요" 오류 금지).
    let md = tmp("hwp_cli_litkey.md");
    std::fs::write(&md, "{{tables}} 목록\n").unwrap();
    let tpl = tmp("hwp_cli_litkey.hwpx");
    assert!(
        hwp()
            .args(["new", "--from"])
            .arg(&md)
            .arg("-o")
            .arg(&tpl)
            .status()
            .unwrap()
            .success()
    );
    let data = tmp("hwp_cli_litkey.json");
    std::fs::write(&data, r#"{"tables":["사과","배"]}"#).unwrap();
    let out = tmp("hwp_cli_litkey_out.hwpx");
    let r = hwp()
        .arg("fill")
        .arg(&tpl)
        .arg("-o")
        .arg(&out)
        .arg("--data")
        .arg(&data)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "flat tables 키 치환: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let j = String::from_utf8_lossy(&r.stdout);
    assert!(
        j.contains("\"replaced\""),
        "평문 fill 경로(replaced 키): {j}"
    );
    for f in [&md, &tpl, &data, &out] {
        let _ = std::fs::remove_file(f);
    }
}

/// ★글상자 보존 기함 테스트: work_report.hwp의 글상자(gso) 안 텍스트와 %hlk 하이퍼링크가
/// hwp→hwpx 변환에서 살아남는다 — 이전엔 글상자가 통째로 드롭돼 둘 다 소실(⑪의 알려진 한계).
#[test]
fn 변환_글상자_텍스트_필드_보존() {
    if skip_if_no_fixtures() {
        return;
    }
    let src = fixture("hwp5/work_report.hwp");
    if !src.exists() {
        eprintln!("스킵: work_report.hwp 없음");
        return;
    }
    let out = tmp("hwp_cli_textbox.hwpx");
    let r = hwp()
        .arg("convert")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let stderr = String::from_utf8_lossy(&r.stderr);
    assert!(!stderr.contains("DROP"), "드롭 경고가 없어야: {stderr}");

    // 글상자 안 텍스트 생존.
    let cat = hwp().arg("cat").arg(&out).output().unwrap();
    let text = String::from_utf8_lossy(&cat.stdout);
    assert!(text.contains("나눔글꼴"), "글상자 텍스트 보존: {text}");

    // 글상자 안 %hlk 하이퍼링크 생존.
    let fields = hwp().args(["fields", "--json"]).arg(&out).output().unwrap();
    let j = String::from_utf8_lossy(&fields.stdout);
    assert!(j.contains("%hlk"), "글상자 안 하이퍼링크 보존: {j}");
    assert!(j.contains("설치하기"), "하이퍼링크 표시값 보존: {j}");

    let _ = std::fs::remove_file(&out);
}

/// ★도형 보존: annual_report(디자인 문서, 도형 142개)의 hwp→hwpx 변환에서 장식 도형이
/// 보존된다 — 이전엔 76개가 통째로 드롭. 잔여 드롭(ARC/이미지채움 v1 제외)은 소수만 허용.
#[test]
fn 변환_장식_도형_보존() {
    if skip_if_no_fixtures() {
        return;
    }
    let src = fixture("hwp5/annual_report.hwp");
    if !src.exists() {
        eprintln!("스킵: annual_report.hwp 없음");
        return;
    }
    let out = tmp("hwp_cli_shapes.hwpx");
    let r = hwp()
        .arg("convert")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let stderr = String::from_utf8_lossy(&r.stderr);
    let drops = stderr.matches("DROP").count();
    assert!(
        drops <= 8,
        "도형 드롭이 소수여야(이전 76): {drops}건\n{stderr}"
    );

    // 텍스트(글상자 포함)는 원본과 동일하게 추출돼야 한다.
    let cat_hwp = hwp().arg("cat").arg(&src).output().unwrap();
    let cat_hwpx = hwp().arg("cat").arg(&out).output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&cat_hwp.stdout),
        String::from_utf8_lossy(&cat_hwpx.stdout),
        "hwpx 텍스트 추출이 원본 hwp와 동일해야"
    );

    // ★도형 z-order 보존(㉗): 예전엔 전부 zOrder="0"으로 뭉개 한글이 겹친 도형을
    // undefined 순서로 그려 표지가 빈 화면이 됐다. 원본 gso z-order(고유 1~143)를
    // 실값으로 방출하는지 확인 — zOrder 값이 다수 고유해야(전부 0 회귀 방지).
    let xml = std::process::Command::new("unzip")
        .args(["-p"])
        .arg(&out)
        .arg("Contents/section0.xml")
        .output()
        .unwrap()
        .stdout;
    let xml = String::from_utf8_lossy(&xml);
    let zorders: std::collections::HashSet<&str> = xml
        .match_indices("zOrder=\"")
        .map(|(i, _)| {
            let rest = &xml[i + 8..];
            &rest[..rest.find('"').unwrap_or(0)]
        })
        .collect();
    assert!(
        zorders.len() >= 20,
        "도형 zOrder가 다수 고유해야(전부 0 회귀 방지): 고유값 {}종 = {:?}",
        zorders.len(),
        zorders
    );

    let _ = std::fs::remove_file(&out);
}

/// ★완전 왕복 기함: work_report.hwp → hwpx → hwp — 글상자 텍스트·%hlk 하이퍼링크가
/// 양방향 변환을 모두 살아남는다. 역방향(hwpx→hwp)의 gso는 한글 실기 손상 판정으로
/// ㉕에서 안전 저하(글상자 텍스트를 본문으로 보존, 도형 래퍼 생략) — 도형 자체는
/// 왕복에서 유지되지 않으나 텍스트·필드는 보존되고 파일은 유효하다(DROP 없음).
#[test]
fn 변환_완전_왕복_hwp_hwpx_hwp() {
    if skip_if_no_fixtures() {
        return;
    }
    let src = fixture("hwp5/work_report.hwp");
    if !src.exists() {
        eprintln!("스킵: work_report.hwp 없음");
        return;
    }
    let mid = tmp("hwp_cli_rt.hwpx");
    let dst = tmp("hwp_cli_rt.hwp");
    for (i, o) in [(&src, &mid), (&mid.clone(), &dst)] {
        let r = hwp()
            .arg("convert")
            .arg(i)
            .arg("-o")
            .arg(o)
            .output()
            .unwrap();
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
        let stderr = String::from_utf8_lossy(&r.stderr);
        assert!(!stderr.contains("DROP"), "드롭 없어야: {stderr}");
    }

    // 텍스트(글상자 포함) 완전 동일.
    let cat_a = hwp().arg("cat").arg(&src).output().unwrap();
    let cat_b = hwp().arg("cat").arg(&dst).output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&cat_a.stdout),
        String::from_utf8_lossy(&cat_b.stdout),
        "왕복 후 텍스트 동일해야"
    );

    // 글상자 안 하이퍼링크 생존.
    let fields = hwp().args(["fields", "--json"]).arg(&dst).output().unwrap();
    let j = String::from_utf8_lossy(&fields.stdout);
    assert!(j.contains("%hlk"), "왕복 후 %hlk 보존: {j}");
    assert!(j.contains("설치하기"), "하이퍼링크 표시값 보존: {j}");

    let _ = std::fs::remove_file(&mid);
    let _ = std::fs::remove_file(&dst);
}

/// markdown 변환 --media-dir: 합성 hwpx에 이미지를 삽입하고 안전한 링크·충돌 거부를 검증한다.
#[test]
fn convert_md_media_dir_figs() {
    let uniq = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("hwp_cli_figs_{}_{}", std::process::id(), uniq));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let source_md = dir.join("source.md");
    let base = dir.join("base.hwpx");
    let image_doc = dir.join("image.hwpx");
    let image = dir.join("tiny.png");
    std::fs::write(&source_md, "이미지 앵커: 본문\n").unwrap();
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend([0, 0, 0, 13]);
    png.extend(b"IHDR");
    png.extend(32u32.to_be_bytes());
    png.extend(24u32.to_be_bytes());
    png.extend([0u8; 8]);
    std::fs::write(&image, &png).unwrap();

    let new = hwp()
        .args(["new", "--from"])
        .arg(&source_md)
        .arg("-o")
        .arg(&base)
        .output()
        .unwrap();
    assert!(
        new.status.success(),
        "합성 문서 생성: {}",
        String::from_utf8_lossy(&new.stderr)
    );
    let edit = hwp()
        .arg("edit")
        .arg(&base)
        .arg("-o")
        .arg(&image_doc)
        .arg("--insert-image")
        .arg(format!("이미지 앵커:=>{}", image.display()))
        .output()
        .unwrap();
    assert!(
        edit.status.success(),
        "이미지 삽입: {}",
        String::from_utf8_lossy(&edit.stderr)
    );

    let nested = dir.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    let out = nested.join("report.md");

    let r = hwp()
        .arg("convert")
        .arg(&image_doc)
        .arg("-o")
        .arg(&out)
        .args(["--media-dir", "my figs"])
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));

    let md = std::fs::read_to_string(&out).unwrap();
    assert!(
        md.contains("![image](<my figs/image1.png>)"),
        "공백 포함 media 경로 링크: {}",
        &md[..md.len().min(400)]
    );
    let figs = nested.join("my figs");
    assert!(figs.is_dir(), "figs 디렉터리가 출력 옆에 생성");
    let extracted = figs.join("image1.png");
    assert_eq!(std::fs::read(&extracted).unwrap(), png, "이미지 바이트");

    std::fs::write(&extracted, b"do not overwrite").unwrap();
    let collision = hwp()
        .arg("convert")
        .arg(&image_doc)
        .arg("-o")
        .arg(&out)
        .args(["--media-dir", "my figs"])
        .output()
        .unwrap();
    assert!(!collision.status.success(), "다른 기존 파일이면 실패");
    assert_eq!(
        std::fs::read(&extracted).unwrap(),
        b"do not overwrite",
        "기존 파일을 덮어쓰지 않음"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// 최소 유효 PNG(시그니처+IHDR)를 만든다 — image_pixel_size가 치수를 읽고
/// writer가 바이트를 그대로 임베드한다(디코딩은 하지 않음).
fn write_min_png(path: &std::path::Path, w: u32, h: u32) {
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend([0, 0, 0, 13]);
    png.extend(b"IHDR");
    png.extend(w.to_be_bytes());
    png.extend(h.to_be_bytes());
    png.extend([8, 6, 0, 0, 0]); // bit depth/color type 등
    png.extend([0, 0, 0, 0]); // CRC 자리(검증 안 함)
    std::fs::write(path, &png).unwrap();
}

/// ★도장 날인(GM-7): edit --seal 로 앵커 "(인)" 위에 부유 그림을 얹고, hwpx 저장·
/// 재읽기에서 Picture가 살아있으며 validate가 통과한다. hwp5 저장 경로도 왕복 스모크.
#[test]
fn edit_seal_floating_image_roundtrip() {
    let md = tmp("hwp_cli_seal.md");
    std::fs::write(&md, "결재 (인) 란\n").unwrap();
    let src = tmp("hwp_cli_seal_src.hwpx");
    assert!(
        hwp()
            .args(["new", "--from"])
            .arg(&md)
            .arg("-o")
            .arg(&src)
            .status()
            .unwrap()
            .success(),
        "hwp new"
    );
    let png = tmp("hwp_cli_seal.png");
    write_min_png(&png, 100, 50);

    // hwpx 경로: 앵커 위에 도장 부유 배치(18mm).
    let out_hwpx = tmp("hwp_cli_seal_out.hwpx");
    let seal_arg = format!("(인)=>{}@18mm", png.display());
    let ed = hwp()
        .arg("edit")
        .arg(&src)
        .arg("-o")
        .arg(&out_hwpx)
        .args(["--seal", &seal_arg])
        .output()
        .expect("hwp edit --seal");
    assert!(
        ed.status.success(),
        "edit --seal 성공 (stderr: {})",
        String::from_utf8_lossy(&ed.stderr)
    );

    // validate 통과(구조 유효).
    assert!(
        hwp()
            .arg("validate")
            .arg(&out_hwpx)
            .status()
            .unwrap()
            .success(),
        "도장 삽입 hwpx는 validate 통과"
    );

    // 재읽기 IR(JSON)에 Picture 컨트롤이 존재.
    let cj = hwp()
        .arg("cat")
        .args(["--format", "json"])
        .arg(&out_hwpx)
        .output()
        .expect("cat json");
    assert!(cj.status.success(), "cat json 성공");
    let j = String::from_utf8_lossy(&cj.stdout);
    assert!(j.contains("Picture"), "재읽기 IR에 Picture 존재: {j:.200}");

    // 앵커 텍스트는 유지되어야 한다.
    let ct = hwp().arg("cat").arg(&out_hwpx).output().expect("cat");
    assert!(
        String::from_utf8_lossy(&ct.stdout).contains("(인)"),
        "앵커 텍스트 유지"
    );

    // hwp5 저장 경로 왕복 스모크(합성 규격 준수 — 재읽기가 성공).
    let out_hwp = tmp("hwp_cli_seal_out.hwp");
    let seal_arg2 = format!("(인)=>{}", png.display()); // 기본 20mm
    let ed5 = hwp()
        .arg("edit")
        .arg(&src)
        .arg("-o")
        .arg(&out_hwp)
        .args(["--seal", &seal_arg2])
        .output()
        .expect("hwp edit --seal hwp");
    assert!(
        ed5.status.success(),
        "hwp5 저장 성공 (stderr: {})",
        String::from_utf8_lossy(&ed5.stderr)
    );
    let ct5 = hwp().arg("cat").arg(&out_hwp).output().expect("cat hwp");
    assert!(ct5.status.success(), "hwp5 재읽기 성공");
    assert!(
        String::from_utf8_lossy(&ct5.stdout).contains("(인)"),
        "hwp5 왕복 후 앵커 유지 | out.hwp: {}",
        dump_file(&out_hwp)
    );

    for f in [&md, &src, &png, &out_hwpx, &out_hwp] {
        let _ = std::fs::remove_file(f);
    }
}

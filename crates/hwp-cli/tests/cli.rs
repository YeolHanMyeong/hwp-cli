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

#[test]
fn validate_valid_hwpx_exit_zero() {
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
    std::env::temp_dir().join(name)
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
            .args(["--title", "메타 제목"])
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
            .args(["--title", "제목X", "--author", "지은이Y"])
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
    // annual_report는 hwpx 쓰기 시 gso 도형을 드롭 → --strict면 비정상 종료.
    let out = tmp("hwp_cli_strict.hwpx");
    let ok = hwp()
        .arg("convert")
        .arg(fixture("hwp5/annual_report.hwp"))
        .arg("-o")
        .arg(&out)
        .args(["--to", "hwpx"])
        .status()
        .unwrap();
    assert!(ok.success(), "--strict 없으면 성공");

    let strict = hwp()
        .arg("convert")
        .arg(fixture("hwp5/annual_report.hwp"))
        .arg("-o")
        .arg(&out)
        .args(["--to", "hwpx", "--strict"])
        .output()
        .unwrap();
    assert!(!strict.status.success(), "--strict면 드롭 시 비정상 종료");
    assert!(
        String::from_utf8_lossy(&strict.stderr).contains("strict"),
        "strict 사유 출력"
    );
    let _ = std::fs::remove_file(&out);
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
    // 양식(2행 표) → 행 3개 추가 + 같은 호출에서 셀 채움 → hwp5. cat으로 내용 확인.
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
    let out = tmp("hwp_cli_addrow_out.hwp");
    let r = hwp()
        .arg("edit")
        .arg(&form)
        .arg("-o")
        .arg(&out)
        .args([
            "--add-row",
            "0:3",
            "--set-cell",
            "0:1:0=노트북",
            "--set-cell",
            "0:3:0=키보드",
            "--verify",
        ])
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "edit --add-row: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let cat = hwp().arg("cat").arg(&out).output().unwrap();
    let text = String::from_utf8_lossy(&cat.stdout);
    assert!(
        text.contains("노트북") && text.contains("키보드"),
        "내용: {text}"
    );
    for f in [&md, &form, &out] {
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
        "fill --data tables: {}",
        String::from_utf8_lossy(&r.stderr)
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

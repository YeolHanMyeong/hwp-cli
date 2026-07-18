//! 표 편집 통합 테스트 — 커밋된 익명화 픽스처(fixtures/samples/report-tables.hwpx)에
//! 하드 의존한다(스킵 없음). 표 지도(재귀 깊이 우선 인덱스):
//!   #0 5x4(병합2, 깨끗한 행 0)  #1 9x6(병합6, 깨끗한 행 3~8)  #2 11x10(병합30, 깨끗한 행 없음)
//!   #3~#8 중첩 2x1 단순표(표#2 셀 안)  #9 7x2 단순표([별표 1] 전문가 등급 기준, 병합 없음)

use std::io::Read as _;
use std::path::PathBuf;
use std::process::Command;

fn hwp() -> Command {
    Command::new(env!("CARGO_BIN_EXE_hwp"))
}

fn fixture() -> PathBuf {
    let p =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/samples/report-tables.hwpx");
    assert!(p.exists(), "커밋된 픽스처가 없습니다: {}", p.display());
    p
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("hwp-cli-table-edit");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// 픽스처를 임시 사본으로 복사해 편집한다(원본 불변).
fn copy_fixture(name: &str) -> PathBuf {
    let dst = tmp(name);
    std::fs::copy(fixture(), &dst).unwrap();
    dst
}

fn cat(path: &PathBuf) -> String {
    let out = hwp().arg("cat").arg(path).output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn read_zip_entry(path: &PathBuf, name: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut buf = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut buf).unwrap();
    buf
}

/// 픽스처 자체가 유효해야 한다(익명화 후에도 한컴 규격 충족).
#[test]
fn fixture_is_valid() {
    let out = hwp().arg("validate").arg(fixture()).output().unwrap();
    assert!(
        out.status.success(),
        "validate: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// 표#0(병합2, 깨끗한 행 존재): 행 추가 성공 → 새 행에 값 채우기까지.
/// (edit는 add-row를 set-cell 뒤에 적용하므로 두 호출로 나눈다 — 기존 CLI 의미.)
#[test]
fn tbl0_add_row_then_fill() {
    let src = copy_fixture("tbl0_row.hwpx");
    let out = tmp("tbl0_row_out.hwpx");
    // pass 1: 행 추가
    let r1 = hwp()
        .arg("edit")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .args(["--add-row", "0"])
        .output()
        .unwrap();
    assert!(
        r1.status.success(),
        "add-row 성공해야: {}",
        String::from_utf8_lossy(&r1.stderr)
    );
    // pass 2: 새 행(인덱스 5) 채우기
    let out2 = tmp("tbl0_row_out2.hwpx");
    let r2 = hwp()
        .arg("edit")
        .arg(&out)
        .arg("-o")
        .arg(&out2)
        .args(["--set-cell", "0:5:0=신규행값"])
        .output()
        .unwrap();
    assert!(
        r2.status.success(),
        "set-cell: {}",
        String::from_utf8_lossy(&r2.stderr)
    );
    assert!(cat(&out2).contains("신규행값"), "새 행 값 확인");
}

/// 표#0: 열 추가는 병합 표라 거부.
#[test]
fn tbl0_add_col_refused() {
    let src = copy_fixture("tbl0_col.hwpx");
    let out = tmp("tbl0_col_out.hwpx");
    let r = hwp()
        .arg("edit")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .args(["--add-col", "0"])
        .output()
        .unwrap();
    assert!(!r.status.success(), "병합 표 열 추가는 거부돼야");
    assert!(
        String::from_utf8_lossy(&r.stderr).contains("병합"),
        "병합 안내: {}",
        String::from_utf8_lossy(&r.stderr)
    );
}

/// 표#2(11x10, 병합 30): 행·열 추가 모두 거부.
#[test]
fn tbl2_add_row_col_refused() {
    let src = copy_fixture("tbl2.hwpx");
    let out = tmp("tbl2_out.hwpx");
    for args in [["--add-row", "2"].as_slice(), ["--add-col", "2"].as_slice()] {
        let r = hwp()
            .arg("edit")
            .arg(&src)
            .arg("-o")
            .arg(&out)
            .args(args)
            .output()
            .unwrap();
        assert!(!r.status.success(), "{args:?} 거부돼야");
        assert!(
            String::from_utf8_lossy(&r.stderr).contains("병합"),
            "{args:?} 병합 안내: {}",
            String::from_utf8_lossy(&r.stderr)
        );
    }
}

/// 중첩 표(재귀 인덱스 3~8): set-cell/add-row가 재귀 로케이터로 걸린다.
#[test]
fn nested_table_recursive_indexing() {
    let src = copy_fixture("nested.hwpx");
    let out = tmp("nested_out.hwpx");
    // 표#3(2x1 단순): 값 교체 + 행 추가.
    let r = hwp()
        .arg("edit")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .args(["--set-cell", "3:0:0=중첩교체", "--add-row", "3"])
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "중첩 표 편집 성공해야: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    // 새 행(인덱스 2) 채우기 — 재귀 인덱스가 set-cell과 일치해야 한다.
    let out2 = tmp("nested_out2.hwpx");
    let r2 = hwp()
        .arg("edit")
        .arg(&out)
        .arg("-o")
        .arg(&out2)
        .args(["--set-cell", "3:2:0=중첩신규"])
        .output()
        .unwrap();
    assert!(
        r2.status.success(),
        "set-cell: {}",
        String::from_utf8_lossy(&r2.stderr)
    );
    let text = cat(&out2);
    assert!(text.contains("중첩교체"), "set-cell 재귀 인덱싱");
    assert!(
        text.contains("중첩신규"),
        "add-row 후 새 행 채우기(재귀 인덱싱)"
    );
}

/// replace 고속 경로: 미수정 엔트리(header.xml)는 입력과 바이트 동일해야 한다
/// (IR 재작성 경로였다면 재합성되어 달라진다).
#[test]
fn replace_fast_path_preserves_package() {
    let src = fixture();
    let out = tmp("replace_fast.hwpx");
    let r = hwp()
        .arg("edit")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .args(["--replace", "한빛대학교=>검증대학교", "--verify"])
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "replace: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    assert!(
        String::from_utf8_lossy(&r.stderr).contains("패키지 보존"),
        "고속 경로 사용 확인: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    // header.xml은 바이트 동일, 본문은 치환됨.
    assert_eq!(
        read_zip_entry(&src, "Contents/header.xml"),
        read_zip_entry(&out, "Contents/header.xml"),
        "header.xml 바이트 보존"
    );
    let section = String::from_utf8(read_zip_entry(&out, "Contents/section0.xml")).unwrap();
    assert!(section.contains("검증대학교"), "본문 치환");
    assert!(!section.contains("한빛대학교"), "원 이름 제거");
}

/// add-col 성공 경로: 합성 단순 표에서 열 추가 → 새 셀 채우기 (.hwpx/.hwp 양쪽).
#[test]
fn add_col_success_synthetic() {
    let md = tmp("addcol.md");
    std::fs::write(&md, "| 가 | 나 |\n|----|----|\n| 1 | 2 |\n").unwrap();
    for ext in ["hwpx", "hwp"] {
        let form = tmp(&format!("addcol_form.{ext}"));
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
        let out = tmp(&format!("addcol_out.{ext}"));
        let r = hwp()
            .arg("edit")
            .arg(&form)
            .arg("-o")
            .arg(&out)
            .args(["--add-col", "0"])
            .output()
            .unwrap();
        assert!(
            r.status.success(),
            "{ext} add-col: {}",
            String::from_utf8_lossy(&r.stderr)
        );
        // 새 열(인덱스 2) 채우기.
        let out2 = tmp(&format!("addcol_out2.{ext}"));
        let r2 = hwp()
            .arg("edit")
            .arg(&out)
            .arg("-o")
            .arg(&out2)
            .args(["--set-cell", "0:0:2=열3", "--verify"])
            .output()
            .unwrap();
        assert!(
            r2.status.success(),
            "{ext} set-cell: {}",
            String::from_utf8_lossy(&r2.stderr)
        );
        assert!(cat(&out2).contains("열3"), "{ext} 새 열 값 확인");
    }
}

/// 표#9([별표 1] 7x2 단순표): 행 추가 성공 → 새 행 채우기.
#[test]
fn tbl9_add_row_then_fill() {
    let src = copy_fixture("tbl9_row.hwpx");
    let out = tmp("tbl9_row_out.hwpx");
    let r1 = hwp()
        .arg("edit")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .args(["--add-row", "9"])
        .output()
        .unwrap();
    assert!(
        r1.status.success(),
        "단순 표 add-row 성공해야: {}",
        String::from_utf8_lossy(&r1.stderr)
    );
    let out2 = tmp("tbl9_row_out2.hwpx");
    let r2 = hwp()
        .arg("edit")
        .arg(&out)
        .arg("-o")
        .arg(&out2)
        .args(["--set-cell", "9:7:0=7급", "--set-cell", "9:7:1=신규 요건"])
        .output()
        .unwrap();
    assert!(
        r2.status.success(),
        "set-cell: {}",
        String::from_utf8_lossy(&r2.stderr)
    );
    let text = cat(&out2);
    assert!(
        text.contains("7급") && text.contains("신규 요건"),
        "새 행 값 확인"
    );
}

/// 표#9: 열 추가 성공 + 전체 표 폭이 정확히 보존(행별 총폭 동일).
#[test]
fn tbl9_add_col_width_preserved() {
    let src = copy_fixture("tbl9_col.hwpx");
    let out = tmp("tbl9_col_out.hwpx");
    let r = hwp()
        .arg("edit")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .args(["--add-col", "9"])
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "단순 표 add-col 성공해야: {}",
        String::from_utf8_lossy(&r.stderr)
    );

    // IR JSON으로 행별 총폭 비교 (입력 vs 출력).
    fn row_sums(path: &PathBuf, nth: usize) -> Vec<i64> {
        let out = hwp()
            .arg("cat")
            .arg(path)
            .args(["--format", "json"])
            .output()
            .unwrap();
        let j: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let mut tables = Vec::new();
        collect_tables(&j["sections"][0]["paragraphs"], &mut tables);
        let t = &tables[nth];
        let rows = t["rows"].as_u64().unwrap() as i64;
        (0..rows)
            .map(|r| {
                t["cells"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|c| c["row"].as_i64() == Some(r))
                    .map(|c| c["width"].as_i64().unwrap())
                    .sum()
            })
            .collect()
    }
    fn collect_tables<'a>(paras: &'a serde_json::Value, out: &mut Vec<&'a serde_json::Value>) {
        for p in paras.as_array().unwrap() {
            for c in p["controls"].as_array().unwrap() {
                if let Some(t) = c.get("Table") {
                    out.push(t);
                    for cell in t["cells"].as_array().unwrap() {
                        collect_tables(&cell["paragraphs"], out);
                    }
                } else if let Some(g) = c.get("Generic") {
                    for l in g["paragraph_lists"].as_array().unwrap() {
                        collect_tables(&l["paragraphs"], out);
                    }
                }
            }
        }
    }

    let before = row_sums(&src, 9);
    let after = row_sums(&out, 9);
    assert_eq!(before.len(), after.len(), "행 수 유지");
    assert_eq!(before, after, "행별 총폭 정확 보존");

    // 새 열(인덱스 2) 채우기.
    let out2 = tmp("tbl9_col_out2.hwpx");
    let r2 = hwp()
        .arg("edit")
        .arg(&out)
        .arg("-o")
        .arg(&out2)
        .args(["--set-cell", "9:0:2=비고", "--verify"])
        .output()
        .unwrap();
    assert!(
        r2.status.success(),
        "set-cell: {}",
        String::from_utf8_lossy(&r2.stderr)
    );
    assert!(cat(&out2).contains("비고"), "새 열 값 확인");
}

//! 충실도 보존 fill (patch::fill_placeholders) 통합 테스트.
//!
//! 합성 HWPX(미리보기 썸네일 + `hp:switch` 호환 블록 + `{{name}}`)를 만든 뒤,
//! 채우기 후에도 비대상 엔트리가 바이트 보존되고 본문 자리표시자만 치환되는지 검증.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

const PRV_IMAGE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];

fn build_fixture(path: &std::path::Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/hwp+zip").unwrap();

    zip.start_file("Preview/PrvImage.png", deflated).unwrap();
    zip.write_all(PRV_IMAGE).unwrap();

    // 2016 호환 블록(hp:switch) — IR 경유 writer가 떨어뜨리는 부분.
    zip.start_file("Contents/header.xml", deflated).unwrap();
    zip.write_all(
        b"<hh:head><hp:switch><hp:case>a</hp:case><hp:default>b</hp:default></hp:switch></hh:head>",
    )
    .unwrap();

    // 단일 런 자리표시자.
    zip.start_file("Contents/section0.xml", deflated).unwrap();
    zip.write_all(
        "<hs:sec><hp:p><hp:run><hp:t>{{기관명}} 운영 보고</hp:t></hp:run></hp:p></hs:sec>"
            .as_bytes(),
    )
    .unwrap();

    zip.finish().unwrap();
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut buf).unwrap();
    buf
}

#[test]
fn fill_preserves_preview_and_compat() {
    let dir = std::env::temp_dir();
    let src = dir.join("hwpx_patch_src.hwpx");
    let out = dir.join("hwpx_patch_out.hwpx");
    build_fixture(&src);

    let mut values = BTreeMap::new();
    values.insert("기관명".to_string(), "제주한라대학교".to_string());
    let counts = hwpx::patch::fill_placeholders(&src, &out, &values).unwrap();
    assert_eq!(counts.get("기관명"), Some(&1), "{{기관명}} 1회 치환");

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();

    // mimetype 첫 엔트리 + STORED.
    {
        let first = zip.by_index(0).unwrap();
        assert_eq!(first.name(), "mimetype");
        assert_eq!(first.compression(), CompressionMethod::Stored);
    }
    // 미리보기 썸네일 바이트 보존 (raw copy).
    assert_eq!(read_entry(&mut zip, "Preview/PrvImage.png"), PRV_IMAGE);
    // hp:switch 호환 블록 보존.
    let header = String::from_utf8(read_entry(&mut zip, "Contents/header.xml")).unwrap();
    assert!(header.contains("hp:switch"), "hp:switch 보존");
    // 본문: 자리표시자 → 값.
    let section = String::from_utf8(read_entry(&mut zip, "Contents/section0.xml")).unwrap();
    assert!(!section.contains("{{기관명}}"), "자리표시자 제거됨");
    assert!(section.contains("제주한라대학교"), "값 삽입됨");

    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn fill_reports_unfilled_as_zero() {
    let dir = std::env::temp_dir();
    let src = dir.join("hwpx_patch_src2.hwpx");
    let out = dir.join("hwpx_patch_out2.hwpx");
    build_fixture(&src);

    let mut values = BTreeMap::new();
    values.insert("없는키".to_string(), "x".to_string());
    let counts = hwpx::patch::fill_placeholders(&src, &out, &values).unwrap();
    assert_eq!(counts.get("없는키"), Some(&0), "미발견 키는 0");

    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn fill_동일_입출력_경로_거부() {
    // 제자리 치환(input==output)은 File::create(O_TRUNC)가 입력을 먼저 비워 손상되므로
    // 즉시 거부돼야 한다(입력 파일은 그대로 보존).
    let dir = std::env::temp_dir();
    let f = dir.join("hwpx_patch_inplace.hwpx");
    build_fixture(&f);
    let orig_len = std::fs::metadata(&f).unwrap().len();

    let mut values = BTreeMap::new();
    values.insert("기관명".to_string(), "x".to_string());
    let err = hwpx::patch::fill_placeholders(&f, &f, &values).unwrap_err();
    assert!(
        err.to_string().contains("같습니다"),
        "동일 경로는 거부돼야: {err}"
    );
    assert_eq!(
        std::fs::metadata(&f).unwrap().len(),
        orig_len,
        "거부는 truncate 이전 — 입력 보존"
    );

    let _ = std::fs::remove_file(&f);
}

// ---- patch::replace_texts ----

/// replace_texts용 픽스처: 섹션에 대학명 텍스트 + PrvText.txt(UTF-8) 포함.
fn build_replace_fixture(path: &std::path::Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/hwp+zip").unwrap();

    zip.start_file("Preview/PrvImage.png", deflated).unwrap();
    zip.write_all(PRV_IMAGE).unwrap();

    zip.start_file("Preview/PrvText.txt", deflated).unwrap();
    zip.write_all("한빛대학교 보고서".as_bytes()).unwrap();

    zip.start_file("Contents/header.xml", deflated).unwrap();
    zip.write_all(b"<hh:head/>").unwrap();

    // 대학명 + XML 특수문자 포함 본문.
    zip.start_file("Contents/section0.xml", deflated).unwrap();
    zip.write_all(
        "<hs:sec><hp:p><hp:run><hp:t>한빛대학교 &amp; 한빛대 협약</hp:t></hp:run></hp:p></hs:sec>"
            .as_bytes(),
    )
    .unwrap();

    zip.finish().unwrap();
}

#[test]
fn replace_texts_바이트보존_순차치환() {
    let dir = std::env::temp_dir();
    let src = dir.join("hwpx_repl_src.hwpx");
    let out = dir.join("hwpx_repl_out.hwpx");
    build_replace_fixture(&src);

    // 긴 이름 먼저(순차 치환 — 짧은 이름이 먼저면 긴 이름 안을 오염).
    let pairs = vec![
        ("한빛대학교".to_string(), "누리대학교".to_string()),
        ("한빛대".to_string(), "누리대".to_string()),
    ];
    let counts = hwpx::patch::replace_texts(&src, &out, &pairs).unwrap();
    assert_eq!(counts.get("Contents/section0.xml"), Some(&2), "본문 2건");
    assert_eq!(counts.get("Preview/PrvText.txt"), Some(&1), "미리보기 1건");

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
    // 비대상 엔트리는 바이트 보존.
    assert_eq!(read_entry(&mut zip, "Preview/PrvImage.png"), PRV_IMAGE);
    {
        let first = zip.by_index(0).unwrap();
        assert_eq!(first.name(), "mimetype");
        assert_eq!(first.compression(), CompressionMethod::Stored);
    }
    // 재오염 없음: "누리대학교" 안에 "누리대"가 다시 치환되지 않았다.
    let section = String::from_utf8(read_entry(&mut zip, "Contents/section0.xml")).unwrap();
    assert!(
        section.contains("누리대학교 &amp; 누리대 협약"),
        "순차 치환 결과: {section}"
    );
    // PrvText도 치환.
    let prv = String::from_utf8(read_entry(&mut zip, "Preview/PrvText.txt")).unwrap();
    assert_eq!(prv, "누리대학교 보고서");

    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&out);
}

#[test]
fn replace_texts_xml_이스케이프() {
    let dir = std::env::temp_dir();
    let src = dir.join("hwpx_repl_esc_src.hwpx");
    let out = dir.join("hwpx_repl_esc_out.hwpx");
    build_replace_fixture(&src);

    // from/to의 특수문자는 XML 이스케이프 후 치환돼야 한다.
    let pairs = vec![("한빛대 협약".to_string(), "A&B 제휴".to_string())];
    hwpx::patch::replace_texts(&src, &out, &pairs).unwrap();

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
    let section = String::from_utf8(read_entry(&mut zip, "Contents/section0.xml")).unwrap();
    assert!(
        section.contains("A&amp;B 제휴"),
        "이스케이프 치환: {section}"
    );
    assert!(
        !section.contains("A&B"),
        "날 ampersand 방출 금지: {section}"
    );

    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&out);
}

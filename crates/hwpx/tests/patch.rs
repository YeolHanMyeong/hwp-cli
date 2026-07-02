//! 충실도 보존 fill (patch::fill_placeholders) 통합 테스트.
//!
//! 합성 HWPX(미리보기 썸네일 + `hp:switch` 호환 블록 + `{{name}}`)를 만든 뒤,
//! 채우기 후에도 비대상 엔트리가 바이트 보존되고 본문 자리표시자만 치환되는지,
//! 그리고 치환 문단의 줄 배치 캐시 제거가 지켜지는지 검증.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

const PRV_IMAGE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];

const SECTION0: &str = concat!(
    "<hs:sec>",
    // 치환 대상 문단 — 줄 배치 캐시 보유 (치환 후 제거돼야 함)
    "<hp:p id=\"1\"><hp:run><hp:t>{{기관명}} 운영 보고</hp:t></hp:run>",
    "<hp:linesegarray><hp:lineseg textpos=\"0\" vertpos=\"0\"/></hp:linesegarray></hp:p>",
    // 무변경 문단 — 줄 배치 캐시가 그대로 남아야 함
    "<hp:p id=\"2\"><hp:run><hp:t>고정 문구</hp:t></hp:run>",
    "<hp:linesegarray><hp:lineseg textpos=\"0\" vertpos=\"100\"/></hp:linesegarray></hp:p>",
    "</hs:sec>"
);

fn utf16le_bom(text: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

fn build_fixture(path: &std::path::Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file("mimetype", stored).unwrap();
    zip.write_all(b"application/hwp+zip").unwrap();

    zip.start_file("Preview/PrvImage.png", deflated).unwrap();
    zip.write_all(PRV_IMAGE).unwrap();

    // hwp2hwpx(Java) 변환본 스타일: UTF-16LE + BOM 미리보기 텍스트.
    zip.start_file("Preview/PrvText.txt", deflated).unwrap();
    zip.write_all(&utf16le_bom("{{기관명}} 운영 보고\n고정 문구"))
        .unwrap();

    zip.start_file("Contents/content.hpf", deflated).unwrap();
    zip.write_all(
        "<opf:package><opf:title>{{기관명}} 운영 보고</opf:title></opf:package>".as_bytes(),
    )
    .unwrap();

    // 2016 호환 블록(hp:switch) — IR 경유 writer가 떨어뜨리는 부분.
    zip.start_file("Contents/header.xml", deflated).unwrap();
    zip.write_all(
        b"<hh:head><hp:switch><hp:case>a</hp:case><hp:default>b</hp:default></hp:switch></hh:head>",
    )
    .unwrap();

    // 단일 런 자리표시자.
    zip.start_file("Contents/section0.xml", deflated).unwrap();
    zip.write_all(SECTION0.as_bytes()).unwrap();

    zip.finish().unwrap();
}

fn read_entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut buf).unwrap();
    buf
}

fn fill_기관명(
    src: &std::path::Path,
    out: &std::path::Path,
    value: &str,
) -> BTreeMap<String, usize> {
    let mut values = BTreeMap::new();
    values.insert("기관명".to_string(), value.to_string());
    hwpx::patch::fill_placeholders(src, out, &values).unwrap()
}

#[test]
fn fill_preserves_preview_and_compat() {
    let dir = std::env::temp_dir();
    let src = dir.join("hwpx_patch_src.hwpx");
    let out = dir.join("hwpx_patch_out.hwpx");
    build_fixture(&src);

    let counts = fill_기관명(&src, &out, "제주한라대학교");
    assert_eq!(counts.get("기관명"), Some(&1), "{{기관명}} 본문 1회 치환");

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
fn fill_치환_문단만_lineseg_제거() {
    let dir = std::env::temp_dir();
    let src = dir.join("hwpx_patch_src_lineseg.hwpx");
    let out = dir.join("hwpx_patch_out_lineseg.hwpx");
    build_fixture(&src);

    fill_기관명(&src, &out, "제주한라대학교");

    let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
    let section = String::from_utf8(read_entry(&mut zip, "Contents/section0.xml")).unwrap();

    // 치환된 문단(id=1)의 줄 배치 캐시는 제거 — 남아 있으면 macOS 한글에서
    // 글자 겹침·변조 경고의 원인.
    let p1 =
        &section[section.find("<hp:p id=\"1\"").unwrap()..section.find("<hp:p id=\"2\"").unwrap()];
    assert!(
        !p1.contains("linesegarray"),
        "치환 문단의 lineseg 캐시 제거돼야: {p1}"
    );
    // 무변경 문단(id=2)은 줄 배치 캐시까지 그대로.
    let p2 = &section[section.find("<hp:p id=\"2\"").unwrap()..];
    assert!(
        p2.contains(r#"<hp:lineseg textpos="0" vertpos="100"/>"#),
        "무변경 문단의 lineseg 캐시는 보존돼야: {p2}"
    );

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

    // 아무것도 안 바뀌면 섹션도 raw copy — 바이트 보존.
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
    assert_eq!(
        read_entry(&mut zip, "Contents/section0.xml"),
        SECTION0.as_bytes(),
        "무변경 섹션은 바이트 보존"
    );

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

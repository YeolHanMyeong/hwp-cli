//! IR 경유 hwp→hwp 왕복 테스트.
//!
//! - 모든 fixture: 의미 동등 (재파싱 텍스트/구조 일치)
//! - 단순 컨트롤만 있는 파일(hello_world): **압축 해제 스트림 바이트 동일**
//!   — prefix+tail 보존 전략의 최종 증명.

use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/hwp5")
        .join(name)
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("hwp5-write-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// hello_world: IR 경유 재저장이 바이트 수준으로 동일해야 한다.
#[test]
fn hello_world_바이트_동일_왕복() {
    let src = fixture("hello_world.hwp");
    let doc = hwp5::read_document(&src).unwrap().document;
    let out = tmp("hello_rt.hwp");
    let opts = hwp5::WriteOptions {
        preserve_linesegs: true,
        ..Default::default()
    };
    let warnings = hwp5::write_document(&doc, &out, &opts).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let mut orig = hwp5::Hwp5Container::open(&src).unwrap();
    let mut ours = hwp5::Hwp5Container::open(&out).unwrap();
    for stream in ["/DocInfo", "/BodyText/Section0"] {
        let a = orig.read_record_stream(stream).unwrap();
        let b = ours.read_record_stream(stream).unwrap();
        assert_eq!(
            a,
            b,
            "{stream}: 압축 해제 스트림 불일치 ({} vs {} 바이트)",
            a.len(),
            b.len()
        );
    }
    // FileHeader: 버전/압축 플래그 보존
    assert_eq!(ours.file_header().version.to_string(), "5.1.0.1");
    assert!(ours.file_header().is_compressed());
}

/// 전체 fixture: 의미 동등 왕복.
#[test]
fn 전체_fixture_의미_왕복() {
    for name in [
        "hello_world.hwp",
        "bookmark.hwp",
        "color_fill.hwp",
        "outline.hwp",
        "work_report.hwp",
    ] {
        let doc = hwp5::read_document(&fixture(name)).unwrap().document;
        let out = tmp(&format!("rt_{name}"));
        let opts = hwp5::WriteOptions {
            preserve_linesegs: true,
            ..Default::default()
        };
        hwp5::write_document(&doc, &out, &opts).unwrap();

        let reread = hwp5::read_document(&out).unwrap_or_else(|e| panic!("{name}: {e}"));
        for w in &reread.warnings {
            assert!(!w.contains("불일치"), "{name}: {w}");
        }
        let doc2 = reread.document;
        assert_eq!(doc2.plain_text(), doc.plain_text(), "{name}: 텍스트 불일치");
        assert_eq!(
            doc2.header.char_shapes.len(),
            doc.header.char_shapes.len(),
            "{name}: 문자 모양 수"
        );
        assert_eq!(doc2.sections.len(), doc.sections.len(), "{name}");
        // 줄 배치 보존 (렌더링 충실도)
        let segs = |d: &hwp_model::Document| {
            d.sections
                .iter()
                .flat_map(|s| &s.paragraphs)
                .map(|p| p.line_segs.len())
                .sum::<usize>()
        };
        assert_eq!(segs(&doc2), segs(&doc), "{name}: lineseg 수");
    }
}

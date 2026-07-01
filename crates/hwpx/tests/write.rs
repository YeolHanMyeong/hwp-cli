//! HWPX writer 테스트: 왕복 + 패키지 규칙.

use std::io::Read as _;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/hwpx")
        .join(name)
}

/// fixture 바이너리는 저장소에서 제외된다(로컬 전용). 없으면 `true`(스킵).
fn skip_if_no_fixtures() -> bool {
    if fixture("minimal.hwpx").exists() {
        return false;
    }
    eprintln!("스킵: fixtures 없음 (fixtures/hwpx/) — fixtures/README.md 참고");
    true
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("hwpx-write-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// hwpx → IR → hwpx → IR 왕복: 의미 동등성.
#[test]
fn 왕복_의미_동등() {
    if skip_if_no_fixtures() {
        return;
    }
    let original = hwpx::read_document(&fixture("minimal.hwpx"))
        .unwrap()
        .document;
    let out = tmp("roundtrip.hwpx");
    let warnings = hwpx::write_document(&original, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let reread = hwpx::read_document(&out).unwrap();
    assert!(reread.warnings.is_empty(), "{:?}", reread.warnings);
    let doc = reread.document;

    assert_eq!(doc.plain_text(), original.plain_text());
    assert_eq!(doc.sections.len(), original.sections.len());
    assert_eq!(
        doc.header.char_shapes.len(),
        original.header.char_shapes.len()
    );
    assert_eq!(
        doc.header
            .styles
            .iter()
            .map(|s| &s.name)
            .collect::<Vec<_>>(),
        original
            .header
            .styles
            .iter()
            .map(|s| &s.name)
            .collect::<Vec<_>>(),
    );
    // PageDef 보존
    let (a, b) = (
        original.sections[0].section_def().unwrap().page.unwrap(),
        doc.sections[0].section_def().unwrap().page.unwrap(),
    );
    assert_eq!(
        (a.width, a.height, a.margin_left),
        (b.width, b.height, b.margin_left)
    );
}

/// 패키지 규칙: mimetype이 첫 엔트리 + 무압축.
#[test]
fn 패키지_mimetype_규칙() {
    if skip_if_no_fixtures() {
        return;
    }
    let doc = hwpx::read_document(&fixture("minimal.hwpx"))
        .unwrap()
        .document;
    let out = tmp("package.hwpx");
    hwpx::write_document(&doc, &out).unwrap();

    let file = std::fs::File::open(&out).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let first = zip.by_index(0).unwrap();
    assert_eq!(first.name(), "mimetype");
    assert_eq!(first.compression(), zip::CompressionMethod::Stored);
    drop(first);

    let mut mime = String::new();
    zip.by_name("mimetype")
        .unwrap()
        .read_to_string(&mut mime)
        .unwrap();
    assert_eq!(mime, "application/hwp+zip");
}

/// markdown → hwpx → markdown 왕복: 구조 보존.
#[test]
fn markdown_생성_왕복() {
    let md = "# 제목\n\n본문 **굵게** 그리고 *기울임*.\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n";
    let doc = hwp_convert::from_markdown(md);
    let out = tmp("from_md.hwpx");
    let warnings = hwpx::write_document(&doc, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    let reread = hwpx::read_document(&out).unwrap().document;
    let text = reread.plain_text();
    assert!(text.contains("제목"));
    assert!(text.contains("본문 굵게 그리고 기울임."));
    assert!(text.contains("1\t2"), "표 셀: {text:?}");

    // 헤딩 스타일과 서식 스팬이 md로 되돌아온다
    let md_out = hwp_convert::to_markdown(&reread);
    assert!(md_out.contains("# "), "{md_out}");
    assert!(md_out.contains("**굵게**"), "{md_out}");
    assert!(md_out.contains("*기울임*"), "{md_out}");
    assert!(md_out.contains("| 1 | 2 |"), "{md_out}");
}

/// 필드(누름틀) hwpx 왕복: create_field → write → read → list_fields로 이름·값 복원.
#[test]
fn 필드_생성_hwpx_왕복() {
    let mut doc = hwp_convert::from_markdown("수신: 부서명");
    assert!(hwp_convert::create_field(&mut doc, "수신:", "수신처", ""));
    assert_eq!(hwp_convert::set_field(&mut doc, "수신처", "홍길동"), 1);

    let out = tmp("field.hwpx");
    let warnings = hwpx::write_document(&doc, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    // 쓴 XML에 fieldBegin/fieldEnd가 있다.
    let bytes = std::fs::read(&out).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    {
        use std::io::Read as _;
        zip.by_name("Contents/section0.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
    }
    assert!(
        xml.contains(r#"type="CLICK_HERE""#),
        "fieldBegin CLICK_HERE 없음"
    );
    assert!(xml.contains(r#"name="수신처""#), "필드 이름 없음");
    assert!(xml.contains("<hp:fieldEnd"), "fieldEnd 없음");

    // 재읽기 → list_fields로 이름·종류·값 복원.
    let reread = hwpx::read_document(&out).unwrap().document;
    let fields = hwp_convert::list_fields(&reread);
    assert_eq!(fields.len(), 1, "{fields:?}");
    assert_eq!(fields[0].ctrl_id, "%clk");
    assert_eq!(fields[0].name.as_deref(), Some("수신처"));
    assert_eq!(fields[0].value, "홍길동");
}

/// 책갈피(bokm) hwpx 왕복: create_bookmark → write → `<hp:bookmark name>` → read → list_bookmarks.
#[test]
fn 책갈피_생성_hwpx_왕복() {
    let mut doc = hwp_convert::from_markdown("제목 문단\n\n본문");
    assert!(hwp_convert::create_bookmark(
        &mut doc,
        "제목",
        "책갈피테스트"
    ));

    let out = tmp("bookmark.hwpx");
    let warnings = hwpx::write_document(&doc, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    // 쓴 XML에 <hp:bookmark name="…"/>가 있다.
    let bytes = std::fs::read(&out).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    {
        use std::io::Read as _;
        zip.by_name("Contents/section0.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
    }
    assert!(
        xml.contains(r#"<hp:bookmark name="책갈피테스트""#),
        "hp:bookmark 없음: {xml}"
    );

    // 재읽기 → list_bookmarks로 이름 복원.
    let reread = hwpx::read_document(&out).unwrap().document;
    let bms = hwp_convert::list_bookmarks(&reread);
    assert_eq!(bms.len(), 1, "{bms:?}");
    assert_eq!(bms[0].name, "책갈피테스트");
}

/// 하이퍼링크(%hlk) hwpx 왕복: create_hyperlink → write → fieldBegin HYPERLINK+Command → read.
#[test]
fn 하이퍼링크_생성_hwpx_왕복() {
    let mut doc = hwp_convert::from_markdown("문서: 참고");
    assert!(hwp_convert::create_hyperlink(
        &mut doc,
        "문서:",
        "https://example.com/a",
        "여기"
    ));

    let out = tmp("hyperlink.hwpx");
    let warnings = hwpx::write_document(&doc, &out).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");

    // 쓴 XML에 fieldBegin type=HYPERLINK + Command(URL)가 있다.
    let bytes = std::fs::read(&out).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    {
        use std::io::Read as _;
        zip.by_name("Contents/section0.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
    }
    assert!(xml.contains(r#"type="HYPERLINK""#), "HYPERLINK 없음: {xml}");
    assert!(xml.contains("example.com"), "Command URL 없음: {xml}");

    // 재읽기 → list_fields로 종류·값·command 복원.
    let reread = hwpx::read_document(&out).unwrap().document;
    let fields = hwp_convert::list_fields(&reread);
    let hlk: Vec<_> = fields.iter().filter(|f| f.ctrl_id == "%hlk").collect();
    assert_eq!(hlk.len(), 1, "{fields:?}");
    assert_eq!(hlk[0].value, "여기");
    assert_eq!(
        hlk[0].command.as_deref(),
        Some("https\\://example.com/a;1;0;0;")
    );
}

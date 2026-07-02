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

/// 문단 끝에 gso 컨트롤(ExtCtrl 코드 11 + Generic)을 부착한다.
fn attach_gso(para: &mut hwp_model::Paragraph, g: hwp_model::GenericControl) {
    use hwp_model::HwpChar;
    let idx = para.controls.len() as u32;
    para.chars.push(HwpChar::ExtCtrl {
        code: 11,
        ctrl_id: g.ctrl_id,
        payload: hwp_convert::field::rev_payload(&g.ctrl_id),
        ctrl_index: Some(idx),
    });
    para.controls.push(hwp_model::Control::Generic(g));
    para.header.ctrl_mask = 0;
}

/// hwp5-출신 글상자(gso + 문단)가 hwpx `<hp:rect>+<hp:drawText>` 왕복을 통과한다 —
/// 이전엔 통째로 드롭돼 안의 텍스트가 소실됐다.
#[test]
fn 글상자_hwp5출신_hwpx_왕복() {
    use hwp_model::{CharShapeId, GenericControl, HwpChar, Paragraph, ParagraphList};

    let mut doc = hwp_convert::from_markdown("본문 문단\n\n둘째 문단");
    // hwp5형 gso: 40B 공통 헤더(attr bit0=글자처럼, 크기 4000x2000) + 글상자 문단 1개.
    let mut data = vec![0u8; 40];
    data[0] = 1; // treatAsChar
    data[12..16].copy_from_slice(&4000i32.to_le_bytes());
    data[16..20].copy_from_slice(&2000i32.to_le_bytes());
    let boxed = Paragraph {
        chars: "상자속글".chars().map(HwpChar::Text).collect(),
        char_shape_runs: vec![(0, CharShapeId(0))],
        ..Default::default()
    };
    let gso = GenericControl {
        ctrl_id: *b"gso ",
        data,
        paragraph_lists: vec![ParagraphList {
            header_data: Vec::new(),
            paragraphs: vec![boxed],
        }],
        extras: Vec::new(),
        raw_children: Vec::new(),
        gso_shapes: Vec::new(),
        equation: None,
    };
    attach_gso(&mut doc.sections[0].paragraphs[1], gso);

    let out = tmp("gso_textbox.hwpx");
    let warnings = hwpx::write_document(&doc, &out).unwrap();
    assert!(
        !warnings.iter().any(|w| w.contains("gso")),
        "gso 드롭 경고가 없어야: {warnings:?}"
    );

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
    assert!(xml.contains("<hp:rect "), "hp:rect 없음: {xml}");
    assert!(xml.contains("<hp:drawText "), "hp:drawText 없음: {xml}");
    assert!(xml.contains("상자속글"), "글상자 텍스트 없음: {xml}");
    assert!(
        xml.contains(r#"treatAsChar="1""#),
        "treatAsChar 보존: {xml}"
    );

    // 재읽기: 텍스트 보존 + 도형 기하 복원(rect 4000x2000).
    let reread = hwpx::read_document(&out).unwrap().document;
    assert!(
        reread.plain_text().contains("상자속글"),
        "재읽기 텍스트: {}",
        reread.plain_text()
    );
    let shape = reread.sections[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.controls)
        .find_map(|c| match c {
            hwp_model::Control::Generic(g) if !g.gso_shapes.is_empty() => Some(&g.gso_shapes[0]),
            _ => None,
        })
        .expect("재읽기 도형");
    assert_eq!(shape.kind, hwp_model::ShapeKind::Rect);
    assert_eq!((shape.w, shape.h), (4000, 2000));
}

/// hwpx-출신 구조화 도형(ShapeGeom)이 쓰기→읽기 왕복에서 기하·스타일을 보존한다 —
/// 이전엔 드롭. Polygon 점(pt0..)·Rect 채움/테두리 색 왕복 확인.
#[test]
fn 도형_shapegeom_hwpx_왕복() {
    use hwp_model::{GenericControl, ShapeGeom, ShapeKind};

    let mut doc = hwp_convert::from_markdown("본문\n\n둘째");
    let rect = ShapeGeom {
        kind: ShapeKind::Rect,
        x: 1000,
        y: 2000,
        w: 5000,
        h: 3000,
        points: Vec::new(),
        fill: 0x00CC8040, // BGR
        fill_gradient: None,
        border_color: 0x000000FF, // 빨강(BGR)
        border_width: 40,
        round_ratio: 10,
        border_style: 1, // DASH
        arrow_start: 0,
        arrow_end: 0,
        anchored: false,
    };
    let poly = ShapeGeom {
        kind: ShapeKind::Polygon,
        x: 0,
        y: 0,
        w: 200,
        h: 100,
        points: vec![(0, 0), (100, 50), (200, 0)],
        fill: 0xFFFF_FFFF,
        fill_gradient: None,
        border_color: 0,
        border_width: 12,
        round_ratio: 0,
        border_style: 0,
        arrow_start: 0,
        arrow_end: 0,
        anchored: false,
    };
    let gso = GenericControl {
        ctrl_id: *b"rect",
        data: Vec::new(),
        paragraph_lists: Vec::new(),
        extras: Vec::new(),
        raw_children: Vec::new(),
        gso_shapes: vec![rect.clone(), poly.clone()],
        equation: None,
    };
    attach_gso(&mut doc.sections[0].paragraphs[1], gso);

    let out = tmp("gso_shapes.hwpx");
    let warnings = hwpx::write_document(&doc, &out).unwrap();
    assert!(!warnings.iter().any(|w| w.contains("DROP")), "{warnings:?}");

    let reread = hwpx::read_document(&out).unwrap().document;
    let shapes: Vec<&ShapeGeom> = reread.sections[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.controls)
        .filter_map(|c| match c {
            hwp_model::Control::Generic(g) if !g.gso_shapes.is_empty() => Some(&g.gso_shapes[0]),
            _ => None,
        })
        .collect();
    assert_eq!(shapes.len(), 2, "도형 2개 왕복");
    let r = shapes.iter().find(|s| s.kind == ShapeKind::Rect).unwrap();
    assert_eq!((r.x, r.y, r.w, r.h), (1000, 2000, 5000, 3000));
    assert_eq!(r.fill, rect.fill);
    assert_eq!(r.border_color, rect.border_color);
    assert_eq!(r.border_width, rect.border_width);
    assert_eq!(r.border_style, rect.border_style);
    assert_eq!(r.round_ratio, rect.round_ratio);
    let p = shapes
        .iter()
        .find(|s| s.kind == ShapeKind::Polygon)
        .unwrap();
    assert_eq!(p.points, poly.points, "폴리곤 점 왕복");
}

/// hwp5-출신 장식 도형(텍스트 없는 gso)이 hwpx 도형 요소로 왕복된다 — 실쌍 바이트
/// (코퍼스 원본.hwp의 SHAPE_COMPONENT+SC_LINE; 한글 export와 lineShape width=32 등 일치 검증됨).
#[test]
fn 장식_도형_hwp5출신_hwpx_왕복() {
    use hwp_model::opaque::OpaqueRecord;
    use hwp_model::{GenericControl, ShapeKind};

    // 실쌍 SHAPE_COMPONENT(252B) + SC_LINE(20B) — hwp-convert/src/gso.rs 테스트와 동일 출처.
    const LINE_SC: &str = "6e696c246e696c240000000000000000000001006400000064000000c8c1000004000000000000000000e4600000020000000100000000000000f03f000000000000000000000000000000000000000000000000000000000000f03f0000000000000000e17a14ae47017f400000000000000000000000000000000000000000000000007b14ae47e17aa43f0000000000000000000000000000f03f000000000000008000000000000000000000000000000000000000000000f03f00000000000000000000000020000000410000c000010000000000000000000000ffffffff00000000000000000000000000000000000000000001e76b390000";
    const LINE_GEOM: &str = "0000000000000000640000006400000000000000";
    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    let mut doc = hwp_convert::from_markdown("본문\n\n둘째");
    // gso 40B 공통 헤더: attr=0x042a2211(글자처럼·PARA/COLUMN), 49608×4 — 실쌍 값.
    let mut data = vec![0u8; 40];
    data[0..4].copy_from_slice(&0x042a_2211u32.to_le_bytes());
    data[12..16].copy_from_slice(&49608i32.to_le_bytes());
    data[16..20].copy_from_slice(&4i32.to_le_bytes());
    let gso = GenericControl {
        ctrl_id: *b"gso ",
        data,
        paragraph_lists: Vec::new(), // 텍스트 없음 = 장식 도형
        extras: Vec::new(),
        raw_children: vec![OpaqueRecord {
            tag: 0x4C,
            data: hex(LINE_SC),
            children: vec![OpaqueRecord {
                tag: 0x4E,
                data: hex(LINE_GEOM),
                children: Vec::new(),
            }],
        }],
        gso_shapes: Vec::new(),
        equation: None,
    };
    attach_gso(&mut doc.sections[0].paragraphs[1], gso);

    let out = tmp("gso_deco.hwpx");
    let warnings = hwpx::write_document(&doc, &out).unwrap();
    assert!(!warnings.iter().any(|w| w.contains("DROP")), "{warnings:?}");

    // 쓴 XML이 한글 export와 동형: hp:line + lineShape width=32 + treatAsChar=1 PARA/COLUMN.
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
    assert!(xml.contains("<hp:line "), "hp:line 없음: {xml}");
    assert!(
        xml.contains(r#"width="32" style="SOLID""#),
        "lineShape 스타일: {xml}"
    );
    assert!(
        xml.contains(r#"vertRelTo="PARA" horzRelTo="COLUMN""#),
        "배치 역매핑: {xml}"
    );

    // 재읽기 → 도형 기하 복원.
    let reread = hwpx::read_document(&out).unwrap().document;
    let s = reread.sections[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.controls)
        .find_map(|c| match c {
            hwp_model::Control::Generic(g) if !g.gso_shapes.is_empty() => Some(&g.gso_shapes[0]),
            _ => None,
        })
        .expect("도형 재읽기");
    assert_eq!(s.kind, ShapeKind::Line);
    assert_eq!((s.w, s.h), (49608, 4));
    assert_eq!(s.border_width, 32);
    // 글자처럼취급(gso attr bit0=1)이 anchored로 복원 — 재렌더 시 흐름 위치 배치의 근거.
    assert!(s.anchored, "treatAsChar=1 → anchored");
}

/// 쪽 컨트롤(쪽번호/감추기/새번호/자동번호)이 hwpx 왕복에서 hwp5 페이로드를 바이트
/// 동일하게 복원한다 — writer(속성 방출)와 reader(build_*)가 정확한 역쌍임을 단정.
/// 이전엔 writer가 전부 드롭(코퍼스 87건).
#[test]
fn 쪽_컨트롤_hwpx_페이로드_왕복() {
    use hwp_model::{Control, GenericControl, HwpChar};

    // (ctrl_id, code, payload) — reader build_* 레이아웃/실측 표준값.
    let mut pgnp = vec![0u8; 12];
    pgnp[0..4].copy_from_slice(&(5u32 << 8).to_le_bytes()); // BOTTOM_CENTER
    pgnp[10..12].copy_from_slice(&(u16::from(b'-')).to_le_bytes());
    let pghd = 0x21u32.to_le_bytes().to_vec(); // 머리말+쪽번호 감춤(정품 실측 표지값)
    let mut nwno = vec![0u8; 6];
    nwno[4..6].copy_from_slice(&7u16.to_le_bytes());
    let atno = {
        let mut v = Vec::new();
        v.extend_from_slice(&0u32.to_le_bytes());
        v.extend_from_slice(&4u32.to_le_bytes());
        v.extend_from_slice(&0u32.to_le_bytes());
        v
    };
    let specs: Vec<([u8; 4], u16, Vec<u8>)> = vec![
        (*b"pgnp", 21, pgnp),
        (*b"pghd", 21, pghd),
        (*b"nwno", 21, nwno),
        (*b"atno", 18, atno),
    ];

    let mut doc = hwp_convert::from_markdown("본문\n\n둘째");
    let para = &mut doc.sections[0].paragraphs[1];
    for (cid, code, data) in &specs {
        let idx = para.controls.len() as u32;
        para.chars.push(HwpChar::ExtCtrl {
            code: *code,
            ctrl_id: *cid,
            payload: vec![0u8; 12],
            ctrl_index: Some(idx),
        });
        para.controls.push(Control::Generic(GenericControl {
            ctrl_id: *cid,
            data: data.clone(),
            paragraph_lists: Vec::new(),
            extras: Vec::new(),
            raw_children: Vec::new(),
            gso_shapes: Vec::new(),
            equation: None,
        }));
    }
    para.header.ctrl_mask = 0;

    let out = tmp("page_ctrls.hwpx");
    let warnings = hwpx::write_document(&doc, &out).unwrap();
    assert!(!warnings.iter().any(|w| w.contains("DROP")), "{warnings:?}");

    // XML 정답지 형식 확인.
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
        xml.contains(r#"<hp:pageNum pos="BOTTOM_CENTER" formatType="DIGIT" sideChar="-"/>"#),
        "pageNum: {xml}"
    );
    assert!(
        xml.contains(r#"hideHeader="1""#) && xml.contains(r#"hidePageNum="1""#),
        "pageHiding: {xml}"
    );
    assert!(
        xml.contains(r#"<hp:newNum num="7" numType="PAGE"/>"#),
        "newNum: {xml}"
    );
    assert!(xml.contains("<hp:autoNum "), "autoNum: {xml}");

    // 재읽기 → 페이로드 바이트 동일.
    let reread = hwpx::read_document(&out).unwrap().document;
    for (cid, _, want) in &specs {
        let got = reread.sections[0]
            .paragraphs
            .iter()
            .flat_map(|p| &p.controls)
            .find_map(|c| match c {
                Control::Generic(g) if g.ctrl_id == *cid => Some(&g.data),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{} 재읽기 실패", String::from_utf8_lossy(cid)));
        assert_eq!(got, want, "{} 페이로드 왕복", String::from_utf8_lossy(cid));
    }
}

//! HWPX reader 테스트: fixture 통합 + 합성 XML 단위.

use std::path::PathBuf;

use hwp_model::{Control, HwpChar};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/hwpx")
        .join(name)
}

#[test]
fn minimal_추출() {
    let result = hwpx::read_document(&fixture("minimal.hwpx")).unwrap();
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    let doc = &result.document;

    assert_eq!(doc.meta.source_format, "hwpx");
    assert_eq!(doc.sections.len(), 1);
    let text = doc.plain_text();
    assert!(text.contains("hwp-cli 테스트 픽스처입니다."));
    assert!(text.contains("첫 번째 문단: 한글 텍스트와 English text 혼합."));

    // 첫 문단: secd + cold 컨트롤 (hwp5와 동일한 IR 의미)
    let first = &doc.sections[0].paragraphs[0];
    assert_eq!(first.controls.len(), 2);
    assert_eq!(first.controls[0].ctrl_id(), *b"secd");
    assert_eq!(first.controls[1].ctrl_id(), *b"cold");

    // PageDef: A4
    let page = doc.sections[0].section_def().unwrap().page.unwrap();
    assert_eq!(page.width.0, 59528);
    assert_eq!(page.height.0, 84186);

    // lineseg 흡수 확인
    assert!(!first.line_segs.is_empty());

    // 헤더 테이블
    assert_eq!(doc.header.char_shapes.len(), 7);
    assert_eq!(doc.header.fonts[0].len(), 2);
    assert_eq!(doc.header.fonts[0][0].name, "함초롬돋움");
    assert!(doc.header.styles.iter().any(|s| s.name == "바탕글"));
}

#[test]
fn 합성_헤더_굵게_기울임() {
    let xml = r##"<?xml version="1.0"?>
<hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head">
  <hh:refList>
    <hh:charProperties itemCnt="2">
      <hh:charPr id="0" height="1000" textColor="#FF0000">
        <hh:fontRef hangul="1" latin="2" hanja="0" japanese="0" other="0" symbol="0" user="0"/>
        <hh:bold/>
      </hh:charPr>
      <hh:charPr id="1" height="1200">
        <hh:italic/>
      </hh:charPr>
    </hh:charProperties>
    <hh:styles>
      <hh:style id="0" type="PARA" name="개요 1" engName="Outline 1" paraPrIDRef="0" charPrIDRef="0"/>
    </hh:styles>
  </hh:refList>
</hh:head>"##;
    let (header, warnings) = hwpx::read::header::parse_header(xml).unwrap();
    assert!(warnings.is_empty());

    assert_eq!(header.char_shapes.len(), 2);
    let cs0 = &header.char_shapes[0];
    assert!(cs0.is_bold() && !cs0.is_italic());
    assert_eq!(cs0.base_size, 1000);
    assert_eq!(cs0.text_color, 0x0000_00FF); // #FF0000 → BGR
    assert_eq!(cs0.face_ids[0], 1);
    assert_eq!(cs0.face_ids[1], 2);
    let cs1 = &header.char_shapes[1];
    assert!(cs1.is_italic() && !cs1.is_bold());

    assert_eq!(header.styles[0].name, "개요 1");
}

#[test]
fn 합성_섹션_표와_컨트롤문자() {
    let xml = r##"<?xml version="1.0"?>
<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
  <hp:p paraPrIDRef="3" styleIDRef="1">
    <hp:run charPrIDRef="0"><hp:t>앞</hp:t><hp:tab/><hp:t>뒤</hp:t><hp:lineBreak/><hp:t>둘째 줄 &amp; 이스케이프</hp:t></hp:run>
    <hp:run charPrIDRef="1">
      <hp:tbl rowCnt="2" colCnt="2" borderFillIDRef="3">
        <hp:tr>
          <hp:tc><hp:cellAddr colAddr="0" rowAddr="0"/><hp:cellSpan colSpan="2" rowSpan="1"/><hp:cellSz width="100" height="50"/><hp:subList><hp:p><hp:run charPrIDRef="0"><hp:t>병합 셀</hp:t></hp:run></hp:p></hp:subList></hp:tc>
        </hp:tr>
        <hp:tr>
          <hp:tc><hp:cellAddr colAddr="0" rowAddr="1"/><hp:subList><hp:p><hp:run charPrIDRef="0"><hp:t>가</hp:t></hp:run></hp:p></hp:subList></hp:tc>
          <hp:tc><hp:cellAddr colAddr="1" rowAddr="1"/><hp:subList><hp:p><hp:run charPrIDRef="0"><hp:t>나</hp:t></hp:run></hp:p></hp:subList></hp:tc>
        </hp:tr>
      </hp:tbl>
    </hp:run>
  </hp:p>
</hs:sec>"##;
    let (section, warnings) = hwpx::read::section::parse_section(xml).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(section.paragraphs.len(), 1);
    let para = &section.paragraphs[0];

    // 탭(8 WCHAR)/줄나눔(1)/이스케이프 처리 + 위치 산수
    assert_eq!(
        para.plain_text().trim_end(),
        "앞\t뒤\n둘째 줄 & 이스케이프\n병합 셀\n가\t나"
    );
    assert!(para.chars.contains(&HwpChar::CharCtrl(10)));
    // run 경계: charPrIDRef 0 → 1
    assert_eq!(para.char_shape_runs.len(), 2);
    assert_eq!(para.char_shape_runs[0].0, 0);

    // 표 구조
    let Some(Control::Table(table)) = para.controls.first() else {
        panic!("표 컨트롤이 있어야 한다");
    };
    assert_eq!((table.rows, table.cols), (2, 2));
    assert_eq!(table.cells.len(), 3);
    assert_eq!(table.cells[0].col_span, 2);
    assert_eq!(table.row_cell_counts, vec![1, 2]);
}

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
        <hh:underline type="BOTTOM" shape="SOLID" color="#0000FF"/>
        <hh:strikeout shape="SOLID" color="#000000"/>
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
    assert!(cs1.has_underline() && cs1.has_strike());
    assert_eq!(cs1.underline_color, 0x00FF_0000); // #0000FF → BGR

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

/// 표 개체 공통 속성(<hp:pos>/<hp:sz>/<hp:outMargin>/zOrder)이 GsoPlacement로 보존되는지.
/// 글자처럼 취급(treatAsChar)을 잃으면 인라인 표가 떠 있는 개체가 돼 본문 흐름에서
/// 빠지고 한글이 재배치(겹침/빈 페이지)한다 — 정품 한라대 실측 기반 회귀 방지.
#[test]
fn 표_배치정보_보존() {
    let xml = r##"<?xml version="1.0"?>
<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
  <hp:p paraPrIDRef="0" styleIDRef="0">
    <hp:run charPrIDRef="0">
      <hp:tbl rowCnt="1" colCnt="1" borderFillIDRef="1" zOrder="8">
        <hp:sz width="18279" widthRelTo="ABSOLUTE" height="3931" heightRelTo="ABSOLUTE"/>
        <hp:pos treatAsChar="1" flowWithText="1" vertRelTo="PARA" horzRelTo="PARA" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/>
        <hp:outMargin left="283" right="283" top="283" bottom="283"/>
        <hp:tr><hp:tc><hp:cellAddr colAddr="0" rowAddr="0"/><hp:subList><hp:p><hp:run charPrIDRef="0"><hp:t>x</hp:t></hp:run></hp:p></hp:subList></hp:tc></hp:tr>
      </hp:tbl>
    </hp:run>
  </hp:p>
</hs:sec>"##;
    let (section, _) = hwpx::read::section::parse_section(xml).unwrap();
    let Some(Control::Table(table)) = section.paragraphs[0].controls.first() else {
        panic!("표 컨트롤이 있어야 한다");
    };
    let pl = table
        .placement
        .as_ref()
        .expect("표 배치정보가 보존돼야 한다");
    assert!(pl.treat_as_char, "글자처럼 취급(인라인) 보존");
    assert!(pl.flow_with_text);
    assert_eq!(pl.vert_rel_to, 2); // PARA
    assert_eq!(pl.horz_rel_to, 3); // PARA
    assert_eq!(pl.z_order, 8);
    assert_eq!(pl.width, 18279); // <hp:sz> — 병합 셀 합산 아님
    assert_eq!(pl.height, 3931);
    assert_eq!(pl.out_margins, [283, 283, 283, 283]);
    // 합성 attr = 정품 인라인 표 값
    assert_eq!(pl.synth_attr(), 0x082a_2311);
}

/// 그림 z-순서(<hp:pic zOrder>)가 보존되는지 — 누락하면 머리말/본문 로고 겹침 순서가
/// 어긋난다(정품 머리말 로고 z=4, 본문 로고 z=7).
#[test]
fn 그림_zorder_보존() {
    let xml = r##"<?xml version="1.0"?>
<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core">
  <hp:p paraPrIDRef="0" styleIDRef="0">
    <hp:run charPrIDRef="0">
      <hp:pic zOrder="7" reverse="0">
        <hp:sz width="12299" height="5074"/>
        <hp:pos treatAsChar="1" vertRelTo="PAGE" horzRelTo="PAPER" vertOffset="68401" horzOffset="25510"/>
        <hc:img binaryItemIDRef="image1"/>
      </hp:pic>
    </hp:run>
  </hp:p>
</hs:sec>"##;
    let (section, _) = hwpx::read::section::parse_section(xml).unwrap();
    let Some(Control::Picture(pic)) = section.paragraphs[0].controls.first() else {
        panic!("그림 컨트롤이 있어야 한다");
    };
    assert_eq!(pic.z_order, 7);
    assert!(pic.treat_as_char);
    // 글자처럼 취급이어도 오프셋은 보존돼야 한다(정품 본문 로고 voff=68401).
    assert_eq!(pic.vert_offset, 68401);
    assert_eq!(pic.horz_offset, 25510);
}

/// hwpx 여백류(HWPUNIT)는 hwp5 PARA_SHAPE 단위(2배)로 저장돼야 한다.
/// 정품 한라대 실측: hwpx left=1500 → hwp5 ml=3000. 줄간격은 2배 아님.
#[test]
fn 문단_여백_2배_단위() {
    let xml = r##"<?xml version="1.0"?>
<hh:head xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head" xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core">
  <hh:refList>
    <hh:paraProperties>
      <hh:paraPr id="0">
        <hh:align horizontal="LEFT"/>
        <hh:margin>
          <hc:intent value="-2248" unit="HWPUNIT"/>
          <hc:left value="1500" unit="HWPUNIT"/>
          <hc:right value="0" unit="HWPUNIT"/>
          <hc:prev value="1416" unit="HWPUNIT"/>
          <hc:next value="0" unit="HWPUNIT"/>
        </hh:margin>
        <hh:lineSpacing type="PERCENT" value="160" unit="HWPUNIT"/>
      </hh:paraPr>
    </hh:paraProperties>
  </hh:refList>
</hh:head>"##;
    let (header, _) = hwpx::read::header::parse_header(xml).unwrap();
    let ps = &header.para_shapes[0];
    assert_eq!(ps.margin_left, 3000, "left 1500 → ×2");
    assert_eq!(ps.indent, -4496, "intent -2248 → ×2");
    assert_eq!(ps.spacing_top, 1416 * 2, "prev → ×2");
    assert_eq!(ps.line_spacing, 160, "줄간격은 2배 아님");
}

/// 쪽번호/감추기/새번호 컨트롤이 올바른 ctrl_id로 매핑·보존돼야 한다(드롭 방지).
#[test]
fn 쪽번호_감추기_컨트롤_매핑() {
    let xml = r##"<?xml version="1.0"?>
<hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph">
  <hp:p paraPrIDRef="0">
    <hp:run charPrIDRef="0"><hp:ctrl>
      <hp:pageNum pos="BOTTOM_CENTER" formatType="DIGIT" sideChar="-"/>
      <hp:newNum num="1" numType="PAGE"/>
    </hp:ctrl></hp:run>
  </hp:p>
  <hp:p paraPrIDRef="0">
    <hp:run charPrIDRef="0"><hp:ctrl>
      <hp:pageHiding hideHeader="1" hideFooter="0" hideMasterPage="0" hideBorder="0" hideFill="0" hidePageNum="1"/>
    </hp:ctrl></hp:run>
  </hp:p>
</hs:sec>"##;
    let (section, _) = hwpx::read::section::parse_section(xml).unwrap();
    let ids0: Vec<[u8; 4]> = section.paragraphs[0]
        .controls
        .iter()
        .map(|c| c.ctrl_id())
        .collect();
    assert!(ids0.contains(b"pgnp"), "pageNum → pgnp: {ids0:?}");
    assert!(ids0.contains(b"nwno"), "newNum → nwno: {ids0:?}");
    let ids1: Vec<[u8; 4]> = section.paragraphs[1]
        .controls
        .iter()
        .map(|c| c.ctrl_id())
        .collect();
    assert!(ids1.contains(b"pghd"), "pageHiding → pghd: {ids1:?}");
    // 데이터가 비어 있지 않아야 writer가 드롭하지 않는다.
    for p in &section.paragraphs {
        for c in &p.controls {
            if let Control::Generic(g) = c
                && matches!(&g.ctrl_id, b"pgnp" | b"nwno" | b"pghd")
            {
                assert!(!g.data.is_empty(), "{:?} data 비어있음", g.ctrl_id);
            }
        }
    }
}

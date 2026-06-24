//! `Contents/sectionN.xml` → [`Section`].
//!
//! 재귀 하강 파서: `hp:p`는 표 셀(`hp:subList`) 안에 다시 나타나므로
//! 각 파서 함수가 자신의 닫는 태그까지 소비한다.
//!
//! IR 일치 규칙 (hwp5와 동일 의미):
//! - `hp:secPr` → ExtCtrl(2, "secd") + Control::SectionDef
//! - `hp:ctrl > hp:colPr` → ExtCtrl(2, "cold") + Control::Generic
//! - `hp:tbl` → ExtCtrl(11, "tbl ") + Control::Table
//! - 기타 개체(pic/rect/...) → ExtCtrl(11) + Control::Generic
//!   (`hp:subList` 문단은 텍스트 추출을 위해 재귀 수집)

use hwp_model::{
    Cell, CharShapeId, Control, GenericControl, HwpChar, HwpUnit, LineSeg, PageDef, ParaShapeId,
    Paragraph, ParagraphList, Section, SectionDef, StyleId, Table,
};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::{HwpxError, Result};
use crate::read::xml::{attr, attr_i32, attr_offset_i32, attr_u16, attr_u32};

type XmlReader<'a> = Reader<&'a [u8]>;

fn next_event<'a>(reader: &mut XmlReader<'a>) -> Result<Event<'a>> {
    reader.read_event().map_err(|e| HwpxError::Xml {
        entry: "section".to_string(),
        message: e.to_string(),
    })
}

/// 자신의 닫는 태그까지 서브트리를 소비한다 (관심 없는 요소 건너뛰기).
fn skip_subtree(reader: &mut XmlReader<'_>, name: &[u8]) -> Result<()> {
    let mut depth = 1u32;
    loop {
        match next_event(reader)? {
            Event::Start(e) if e.local_name().as_ref() == name => depth += 1,
            Event::End(e) if e.local_name().as_ref() == name => {
                depth -= 1;
                if depth == 0 {
                    return Ok(());
                }
            }
            Event::Eof => return Ok(()),
            _ => {}
        }
    }
}

pub fn parse_section(xml: &str) -> Result<(Section, Vec<String>)> {
    let mut reader = Reader::from_str(xml);
    let mut section = Section::default();
    let mut warnings = Vec::new();

    loop {
        match next_event(&mut reader)? {
            Event::Start(e) if e.local_name().as_ref() == b"p" => {
                section
                    .paragraphs
                    .push(parse_paragraph(&mut reader, &e, &mut warnings)?);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok((section, warnings))
}

/// `<hp:p>` 하나를 소비한다.
fn parse_paragraph(
    reader: &mut XmlReader<'_>,
    start: &BytesStart<'_>,
    warnings: &mut Vec<String>,
) -> Result<Paragraph> {
    let mut para = Paragraph {
        para_shape: ParaShapeId(attr_u16(start, "paraPrIDRef").unwrap_or(0)),
        style: StyleId(attr_u16(start, "styleIDRef").unwrap_or(0)),
        ..Paragraph::default()
    };
    // hwp5 break_type 비트와 동일 인코딩 (bit2 쪽, bit3 단)
    if attr(start, "pageBreak").as_deref() == Some("1") {
        para.header.break_type |= 0x04;
    }
    if attr(start, "columnBreak").as_deref() == Some("1") {
        para.header.break_type |= 0x08;
    }
    let mut wchar_pos = 0u32;
    let mut last_shape: Option<u16> = None;

    loop {
        let event = next_event(reader)?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let empty = matches!(event, Event::Empty(_));
                let name = e.local_name().as_ref().to_vec();
                match name.as_slice() {
                    b"run" => {
                        let id = attr_u16(e, "charPrIDRef").unwrap_or(0);
                        if last_shape != Some(id) {
                            // 직전 run이 글자를 안 더했으면(빈 <hp:t/>) 같은 위치에
                            // 두 글자모양이 겹친다. HWP5 PARA_CHAR_SHAPE는 위치당 1개
                            // (마지막이 유효)이므로 같은 위치면 마지막 run으로 덮어쓴다.
                            // (빈 문단의 빈 run 2개 → 첫 run의 큰 글자로 줄이 높아져
                            // 페이지가 밀리는 문제 방지.)
                            if let Some(last) = para.char_shape_runs.last_mut()
                                && last.0 == wchar_pos
                            {
                                last.1 = CharShapeId(id);
                            } else {
                                para.char_shape_runs.push((wchar_pos, CharShapeId(id)));
                            }
                            last_shape = Some(id);
                        }
                    }
                    b"t" => {
                        if !empty {
                            parse_text(reader, &mut para, &mut wchar_pos, warnings)?;
                        }
                    }
                    b"tab" => {
                        para.chars.push(HwpChar::InlineCtrl {
                            code: 9,
                            payload: vec![0; 12],
                        });
                        wchar_pos += 8;
                        if !empty {
                            skip_subtree(reader, b"tab")?;
                        }
                    }
                    b"lineBreak" => {
                        para.chars.push(HwpChar::CharCtrl(10));
                        wchar_pos += 1;
                    }
                    b"secPr" => {
                        let def = if empty {
                            SectionDef {
                                data: Vec::new(),
                                page: None,
                                extras: Vec::new(),
                            }
                        } else {
                            parse_sec_pr(reader)?
                        };
                        push_ext_ctrl(&mut para, &mut wchar_pos, 2, *b"secd");
                        para.controls.push(Control::SectionDef(def));
                    }
                    b"ctrl" => {
                        if !empty {
                            parse_ctrl(reader, &mut para, &mut wchar_pos, warnings)?;
                        }
                    }
                    b"tbl" => {
                        let table = parse_table(reader, e, warnings)?;
                        push_ext_ctrl(&mut para, &mut wchar_pos, 11, *b"tbl ");
                        para.controls.push(Control::Table(table));
                    }
                    b"linesegarray" => {
                        if !empty {
                            parse_linesegs(reader, &mut para)?;
                        }
                    }
                    b"pic" => {
                        let mut picture = if empty {
                            default_picture()
                        } else {
                            parse_picture(reader)?
                        };
                        // z-순서는 <hp:pic> 시작 태그 속성(자식 <hp:pos>가 아님).
                        // 누락하면 머리말/본문 로고 겹침 순서가 어긋난다.
                        picture.z_order = attr_i32(e, "zOrder").unwrap_or(0).max(0) as u32;
                        push_ext_ctrl(&mut para, &mut wchar_pos, 11, *b"gso ");
                        para.controls.push(Control::Picture(picture));
                    }
                    // 그 외 개체 (rect, ellipse, equation, container...)
                    _ => {
                        let mut ctrl_id = [b' '; 4];
                        for (i, b) in name.iter().take(4).enumerate() {
                            ctrl_id[i] = *b;
                        }
                        let mut generic = GenericControl {
                            ctrl_id,
                            data: Vec::new(),
                            paragraph_lists: Vec::new(),
                            extras: Vec::new(),
                            raw_children: Vec::new(),
                        };
                        if !empty {
                            collect_sub_lists(reader, &name, &mut generic, warnings)?;
                        }
                        push_ext_ctrl(&mut para, &mut wchar_pos, 11, ctrl_id);
                        para.controls.push(Control::Generic(generic));
                    }
                }
            }
            Event::End(e) if e.local_name().as_ref() == b"p" => break,
            Event::Eof => {
                warnings.push("hp:p가 닫히지 않은 채 EOF".to_string());
                break;
            }
            _ => {}
        }
    }
    Ok(para)
}

/// `<hp:t>` 내부의 텍스트를 수집한다 (중첩 마크업은 무시).
fn parse_text(
    reader: &mut XmlReader<'_>,
    para: &mut Paragraph,
    wchar_pos: &mut u32,
    _warnings: &mut [String],
) -> Result<()> {
    loop {
        match next_event(reader)? {
            Event::Text(t) => {
                let s = t.xml10_content().map_err(|e| HwpxError::Xml {
                    entry: "section".to_string(),
                    message: e.to_string(),
                })?;
                for c in s.chars() {
                    *wchar_pos += c.len_utf16() as u32;
                    para.chars.push(HwpChar::Text(c));
                }
            }
            // 엔티티 참조(&amp; &#x...;)는 별도 이벤트로 온다
            Event::GeneralRef(r) => {
                let resolved = r
                    .resolve_char_ref()
                    .ok()
                    .flatten()
                    .or_else(|| match &r[..] {
                        b"amp" => Some('&'),
                        b"lt" => Some('<'),
                        b"gt" => Some('>'),
                        b"quot" => Some('"'),
                        b"apos" => Some('\''),
                        _ => None,
                    });
                if let Some(c) = resolved {
                    *wchar_pos += c.len_utf16() as u32;
                    para.chars.push(HwpChar::Text(c));
                }
            }
            Event::End(e) if e.local_name().as_ref() == b"t" => break,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

fn push_ext_ctrl(para: &mut Paragraph, wchar_pos: &mut u32, code: u16, ctrl_id: [u8; 4]) {
    // payload 선두 4바이트 = 역순 ctrl_id (hwp5 저장 형식과 동일하게 구성)
    let mut payload = vec![0u8; 12];
    payload[..4].copy_from_slice(&{
        let mut rev = ctrl_id;
        rev.reverse();
        rev
    });
    let ctrl_index = Some(para.controls.len() as u32);
    para.chars.push(HwpChar::ExtCtrl {
        code,
        ctrl_id,
        payload,
        ctrl_index,
    });
    *wchar_pos += 8;
}

/// `<hp:secPr>` — pagePr/margin만 의미 파싱.
fn parse_sec_pr(reader: &mut XmlReader<'_>) -> Result<SectionDef> {
    let mut def = SectionDef {
        data: Vec::new(),
        page: None,
        extras: Vec::new(),
    };
    let mut page = PageDef {
        width: HwpUnit(0),
        height: HwpUnit(0),
        margin_left: HwpUnit(0),
        margin_right: HwpUnit(0),
        margin_top: HwpUnit(0),
        margin_bottom: HwpUnit(0),
        margin_header: HwpUnit(0),
        margin_footer: HwpUnit(0),
        gutter: HwpUnit(0),
        attr: 0,
    };
    let mut has_page = false;

    loop {
        match next_event(reader)? {
            Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                b"pagePr" => {
                    has_page = true;
                    page.width = HwpUnit(attr_i32(&e, "width").unwrap_or(0));
                    page.height = HwpUnit(attr_i32(&e, "height").unwrap_or(0));
                    // OWPML landscape="NARROWLY"(세로)/"WIDELY"(가로) — 가로면 bit0
                    if attr(&e, "landscape").as_deref() == Some("NARROWLY") {
                        page.attr |= 1;
                    }
                }
                b"margin" if has_page => {
                    page.margin_left = HwpUnit(attr_i32(&e, "left").unwrap_or(0));
                    page.margin_right = HwpUnit(attr_i32(&e, "right").unwrap_or(0));
                    page.margin_top = HwpUnit(attr_i32(&e, "top").unwrap_or(0));
                    page.margin_bottom = HwpUnit(attr_i32(&e, "bottom").unwrap_or(0));
                    page.margin_header = HwpUnit(attr_i32(&e, "header").unwrap_or(0));
                    page.margin_footer = HwpUnit(attr_i32(&e, "footer").unwrap_or(0));
                    page.gutter = HwpUnit(attr_i32(&e, "gutter").unwrap_or(0));
                }
                _ => {}
            },
            Event::End(e) if e.local_name().as_ref() == b"secPr" => break,
            Event::Eof => break,
            _ => {}
        }
    }
    if has_page {
        def.page = Some(page);
    }
    Ok(def)
}

/// `<hp:ctrl>` — colPr/머리말/꼬리말/각주 등 컨트롤 묶음.
///
/// 각 자식 컨트롤의 서브트리를 끝까지 소비하고, 문단 리스트(`hp:subList`)는
/// 재귀 수집한다 — 머리말 안의 텍스트·이미지가 여기로 들어온다.
/// hwpx `<hp:header/footer applyPageType id>` → hwp5 머리말/꼬리말 8B 페이로드.
/// `적용쪽(u32)` + `id(u32)`. 적용쪽: BOTH=0, EVEN=1, ODD=2. 정품 실측:
/// `<hp:header id="2" applyPageType="BOTH">` → `00000000 02000000`.
fn head_foot_data(e: &BytesStart<'_>) -> Vec<u8> {
    let apply: u32 = match attr(e, "applyPageType").as_deref() {
        Some("EVEN") => 1,
        Some("ODD") => 2,
        _ => 0, // BOTH
    };
    let id: u32 = attr(e, "id").and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut v = Vec::with_capacity(8);
    v.extend_from_slice(&apply.to_le_bytes());
    v.extend_from_slice(&id.to_le_bytes());
    v
}

/// hwpx `<hp:pageNum pos formatType sideChar>` → hwp5 pgnp(쪽 번호 위치) 12B.
/// `properties(u32: 서식 | 위치<<8)` + 예약(6B) + sideChar WCHAR. 정품 실측:
/// pos=BOTTOM_CENTER(5), sideChar='-' → `000500000000000000002d00`.
fn build_pgnp(e: &BytesStart<'_>) -> Vec<u8> {
    let position: u32 = match attr(e, "pos").as_deref() {
        Some("TOP_LEFT") => 1,
        Some("TOP_CENTER") => 2,
        Some("TOP_RIGHT") => 3,
        Some("BOTTOM_LEFT") => 4,
        Some("BOTTOM_CENTER") => 5,
        Some("BOTTOM_RIGHT") => 6,
        Some("OUTSIDE_TOP") => 7,
        Some("OUTSIDE_BOTTOM") => 8,
        Some("INSIDE_TOP") => 9,
        Some("INSIDE_BOTTOM") => 10,
        _ => 0, // NONE
    };
    // 서식은 아라비아 숫자(DIGIT=0)만 매핑, 그 외는 0으로 대체.
    let format: u32 = 0;
    let side_char: u16 = attr(e, "sideChar")
        .and_then(|s| s.chars().next())
        .map(|c| c as u16)
        .unwrap_or(0);
    let props = format | (position << 8);
    let mut v = Vec::with_capacity(12);
    v.extend_from_slice(&props.to_le_bytes());
    v.extend_from_slice(&[0u8; 6]);
    v.extend_from_slice(&side_char.to_le_bytes());
    v
}

/// hwpx `<hp:pageHiding hide.../>` → hwp5 pghd(쪽 감추기) 4B 비트맵.
/// bit0=머리말, 1=꼬리말, 2=바탕쪽, 3=테두리, 4=배경, 5=쪽번호.
/// 정품 실측: 표지=0x21(머리말+쪽번호), 목차=0x20(쪽번호).
fn build_pghd(e: &BytesStart<'_>) -> Vec<u8> {
    let bit = |name: &str, b: u32| {
        if attr(e, name).as_deref() == Some("1") {
            1u32 << b
        } else {
            0
        }
    };
    let mask = bit("hideHeader", 0)
        | bit("hideFooter", 1)
        | bit("hideMasterPage", 2)
        | bit("hideBorder", 3)
        | bit("hideFill", 4)
        | bit("hidePageNum", 5);
    mask.to_le_bytes().to_vec()
}

/// hwpx `<hp:newNum num/>` → hwp5 nwno(새 번호 지정) 6B. `종류(u32=0,PAGE)` + `번호(u16)`.
/// 정품 실측: num=1 → `000000000100`.
fn build_nwno(e: &BytesStart<'_>) -> Vec<u8> {
    let num: u16 = attr(e, "num").and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut v = Vec::with_capacity(6);
    v.extend_from_slice(&0u32.to_le_bytes());
    v.extend_from_slice(&num.to_le_bytes());
    v
}

fn parse_ctrl(
    reader: &mut XmlReader<'_>,
    para: &mut Paragraph,
    wchar_pos: &mut u32,
    warnings: &mut Vec<String>,
) -> Result<()> {
    loop {
        let event = next_event(reader)?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.local_name().as_ref().to_vec();
                // hwp5와 동일한 ctrl_id/컨트롤 문자 코드 매핑. 쪽번호·감추기·새번호는
                // 코드 21(페이지 컨트롤)이며, hwp5 페이로드를 여기서 합성해 둔다(빈
                // GenericControl이면 writer가 드롭). head/foot는 적용쪽+id를 8B로.
                let (ctrl_id, code, data): ([u8; 4], u16, Vec<u8>) = match name.as_slice() {
                    b"colPr" => (*b"cold", 2, Vec::new()),
                    b"header" => (*b"head", 16, head_foot_data(e)),
                    b"footer" => (*b"foot", 16, head_foot_data(e)),
                    b"footNote" => (*b"fn  ", 17, Vec::new()),
                    b"endNote" => (*b"en  ", 17, Vec::new()),
                    b"autoNum" => (*b"atno", 18, Vec::new()),
                    b"pageNum" => (*b"pgnp", 21, build_pgnp(e)),
                    b"pageHiding" => (*b"pghd", 21, build_pghd(e)),
                    b"newNum" => (*b"nwno", 21, build_nwno(e)),
                    other => {
                        let mut id = [b' '; 4];
                        for (i, b) in other.iter().take(4).enumerate() {
                            id[i] = *b;
                        }
                        (id, 21, Vec::new())
                    }
                };
                let mut generic = GenericControl {
                    ctrl_id,
                    data,
                    paragraph_lists: Vec::new(),
                    extras: Vec::new(),
                    raw_children: Vec::new(),
                };
                if matches!(event, Event::Start(_)) {
                    collect_sub_lists(reader, &name, &mut generic, warnings)?;
                }
                push_ext_ctrl(para, wchar_pos, code, ctrl_id);
                para.controls.push(Control::Generic(generic));
            }
            Event::End(e) if e.local_name().as_ref() == b"ctrl" => break,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

/// hwpx vertRelTo → hwp5 코드 (PAPER=0, PAGE=1, PARA=2).
fn vert_rel_to_code(s: Option<&str>) -> u8 {
    match s {
        Some("PAGE") => 1,
        Some("PARA") => 2,
        _ => 0, // PAPER
    }
}

/// hwpx horzRelTo → hwp5 코드 (PAPER=0, PAGE=1, COLUMN=2, PARA=3).
fn horz_rel_to_code(s: Option<&str>) -> u8 {
    match s {
        Some("PAGE") => 1,
        Some("COLUMN") => 2,
        Some("PARA") => 3,
        _ => 0, // PAPER
    }
}

/// hwpx vertAlign/horzAlign → hwp5 코드 (TOP/LEFT=0, CENTER=1, BOTTOM/RIGHT=2).
fn align_code(s: Option<&str>) -> u8 {
    match s {
        Some("CENTER") => 1,
        Some("BOTTOM") | Some("RIGHT") => 2,
        _ => 0, // TOP/LEFT
    }
}

/// `<hp:tbl>` — 표.
fn parse_table(
    reader: &mut XmlReader<'_>,
    start: &BytesStart<'_>,
    warnings: &mut Vec<String>,
) -> Result<Table> {
    // 표 속성(attr): bits0-1=쪽 나눔(NONE=0/TABLE=1/CELL=2), bit2=제목줄 반복,
    // bit3=자동 너비 조정 안 함. 정품 실측으로 검증. 0으로 두면(과거 버그) 표가
    // "나누지 않음"이 돼, 잔여 공간에 안 들어가는 표가 통째로 다음 쪽으로 밀린다
    // (목차 박스가 별도 쪽으로 분리되는 원인).
    let mut table_attr: u32 = match attr(start, "pageBreak").as_deref() {
        Some("TABLE") => 1,
        Some("CELL") => 2,
        _ => 0, // NONE
    };
    if attr(start, "repeatHeader").as_deref() == Some("1") {
        table_attr |= 1 << 2;
    }
    if attr(start, "noAdjust").as_deref() == Some("1") {
        table_attr |= 1 << 3;
    }
    let mut table = Table {
        common_data: Vec::new(),
        placement: None, // 루프 종료 후 GsoPlacement로 채운다
        attr: table_attr,
        rows: attr_u16(start, "rowCnt").unwrap_or(0),
        cols: attr_u16(start, "colCnt").unwrap_or(0),
        cell_spacing: attr_u16(start, "cellSpacing").unwrap_or(0),
        // 셀 안쪽 여백: hwpx <hp:inMargin>에서 읽는다(아래 루프). 기본 0.
        inner_margins: [0; 4],
        row_cell_counts: Vec::new(),
        border_fill: hwp_model::BorderFillId(attr_u16(start, "borderFillIDRef").unwrap_or(0)),
        table_tail: Vec::new(),
        cells: Vec::new(),
        extras: Vec::new(),
    };

    // 개체 공통 속성(배치) — hwp5 CTRL_HEADER 40바이트로 합성한다. 읽지 않으면
    // writer가 떠 있는(floating) 상수로 덮어써, 인라인이어야 할 표가 본문 흐름에서
    // 빠지고 한글이 재배치해 겹침/빈 페이지가 생긴다. zOrder는 시작 태그에 있다.
    let mut placement = hwp_model::GsoPlacement {
        z_order: attr_i32(start, "zOrder").unwrap_or(0),
        ..Default::default()
    };

    loop {
        match next_event(reader)? {
            Event::Start(e) => match e.local_name().as_ref() {
                b"tc" => {
                    let cell = parse_cell(reader, &e, warnings)?;
                    table.cells.push(cell);
                }
                b"tr" => {} // 행은 cellAddr로 복원되므로 컨테이너로만 취급
                _ => {
                    let name = e.local_name().as_ref().to_vec();
                    skip_subtree(reader, &name)?;
                }
            },
            // 표 자신의 셀 안쪽 여백(self-closing). 셀(tc) 안 중첩 표는 parse_cell이
            // 따로 소비하므로 여기서 보이는 건 이 표의 것뿐이다. 순서: left,right,top,bottom.
            Event::Empty(e) if e.local_name().as_ref() == b"inMargin" => {
                table.inner_margins = [
                    attr_u16(&e, "left").unwrap_or(0),
                    attr_u16(&e, "right").unwrap_or(0),
                    attr_u16(&e, "top").unwrap_or(0),
                    attr_u16(&e, "bottom").unwrap_or(0),
                ];
            }
            // 배치: <hp:pos>(글자처럼취급/위치기준/오프셋), <hp:sz>(경계 너비/높이 —
            // 병합 셀 합산보다 정확), <hp:outMargin>(바깥 여백).
            Event::Empty(e) if e.local_name().as_ref() == b"pos" => {
                placement.treat_as_char = attr(&e, "treatAsChar").as_deref() == Some("1");
                placement.affect_line_spacing = attr(&e, "affectLSpacing").as_deref() == Some("1");
                placement.flow_with_text = attr(&e, "flowWithText").as_deref() == Some("1");
                placement.hold_anchor = attr(&e, "holdAnchorAndSO").as_deref() == Some("1");
                placement.vert_rel_to = vert_rel_to_code(attr(&e, "vertRelTo").as_deref());
                placement.horz_rel_to = horz_rel_to_code(attr(&e, "horzRelTo").as_deref());
                placement.vert_align = align_code(attr(&e, "vertAlign").as_deref());
                placement.horz_align = align_code(attr(&e, "horzAlign").as_deref());
                placement.vert_offset = attr_offset_i32(&e, "vertOffset").unwrap_or(0);
                placement.horz_offset = attr_offset_i32(&e, "horzOffset").unwrap_or(0);
            }
            Event::Empty(e) if e.local_name().as_ref() == b"sz" => {
                placement.width = attr_i32(&e, "width").unwrap_or(0);
                placement.height = attr_i32(&e, "height").unwrap_or(0);
            }
            Event::Empty(e) if e.local_name().as_ref() == b"outMargin" => {
                placement.out_margins = [
                    attr_u16(&e, "left").unwrap_or(0),
                    attr_u16(&e, "right").unwrap_or(0),
                    attr_u16(&e, "top").unwrap_or(0),
                    attr_u16(&e, "bottom").unwrap_or(0),
                ];
            }
            Event::End(e) if e.local_name().as_ref() == b"tbl" => break,
            Event::Eof => break,
            _ => {}
        }
    }
    table.placement = Some(placement);
    // 행별 셀 수 재구성 (hwp5와 동일 의미 유지)
    let mut counts = vec![0u16; table.rows as usize];
    for cell in &table.cells {
        if let Some(c) = counts.get_mut(cell.row as usize) {
            *c += 1;
        }
    }
    table.row_cell_counts = counts;
    Ok(table)
}

/// `<hp:tc>` — 셀 하나.
fn parse_cell(
    reader: &mut XmlReader<'_>,
    start: &BytesStart<'_>,
    warnings: &mut Vec<String>,
) -> Result<Cell> {
    let mut cell = Cell {
        // 제목(머리) 셀이면 bit18 — 표 헤더 행 반복 대상(정품 실측). vertAlign은
        // 아래 subList에서 bits5-6에 더한다.
        list_attr: if attr(start, "header").as_deref() == Some("1") {
            1 << 18
        } else {
            0
        },
        col: 0,
        row: 0,
        col_span: 1,
        row_span: 1,
        width: HwpUnit(0),
        height: HwpUnit(0),
        margins: [0; 4],
        border_fill: hwp_model::BorderFillId(attr_u16(start, "borderFillIDRef").unwrap_or(0)),
        header_tail: Vec::new(),
        paragraphs: Vec::new(),
    };
    loop {
        match next_event(reader)? {
            Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                b"cellAddr" => {
                    cell.col = attr_u16(&e, "colAddr").unwrap_or(0);
                    cell.row = attr_u16(&e, "rowAddr").unwrap_or(0);
                }
                b"cellSpan" => {
                    cell.col_span = attr_u16(&e, "colSpan").unwrap_or(1);
                    cell.row_span = attr_u16(&e, "rowSpan").unwrap_or(1);
                }
                b"cellSz" => {
                    cell.width = HwpUnit(attr_i32(&e, "width").unwrap_or(0));
                    cell.height = HwpUnit(attr_i32(&e, "height").unwrap_or(0));
                }
                b"cellMargin" => {
                    cell.margins = [
                        attr_u16(&e, "left").unwrap_or(0),
                        attr_u16(&e, "right").unwrap_or(0),
                        attr_u16(&e, "top").unwrap_or(0),
                        attr_u16(&e, "bottom").unwrap_or(0),
                    ];
                }
                b"subList" => {
                    // 셀 세로 정렬(vertAlign)을 list_attr bits5-6에 인코딩:
                    // TOP=0, CENTER=1, BOTTOM=2. 정품 셀은 CENTER(0x20)인데 안 읽으면
                    // 0(TOP)이 돼 셀 내용이 위로 몰리고, 셀 높이가 내용보다 크면 빈
                    // 아래 영역이 다음 쪽으로 분리된다(빈 페이지 발생).
                    let va = match attr(&e, "vertAlign").as_deref() {
                        Some("CENTER") => 1u32,
                        Some("BOTTOM") => 2,
                        _ => 0, // TOP
                    };
                    cell.list_attr |= va << 5;
                }
                b"p" => {
                    cell.paragraphs.push(parse_paragraph(reader, &e, warnings)?);
                }
                _ => {}
            },
            Event::End(e) if e.local_name().as_ref() == b"tc" => break,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(cell)
}

fn default_picture() -> hwp_model::Picture {
    hwp_model::Picture {
        common_data: Vec::new(),
        width: HwpUnit(0),
        height: HwpUnit(0),
        treat_as_char: false,
        z_order: 0,
        vert_offset: 0,
        horz_offset: 0,
        bin_ref: hwp_model::BinRef::ItemRef(String::new()),
        extras: Vec::new(),
    }
}

/// `<hp:pic>` — 이미지 개체. 크기(hp:sz)/배치(hp:pos)/참조(hc:img)만 의미 파싱.
fn parse_picture(reader: &mut XmlReader<'_>) -> Result<hwp_model::Picture> {
    let mut pic = default_picture();
    let mut depth = 1u32;
    loop {
        let event = next_event(reader)?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                match e.local_name().as_ref() {
                    b"sz" => {
                        pic.width = HwpUnit(attr_i32(e, "width").unwrap_or(0));
                        pic.height = HwpUnit(attr_i32(e, "height").unwrap_or(0));
                    }
                    b"pos" => {
                        pic.treat_as_char = attr(e, "treatAsChar").as_deref() == Some("1");
                        // 떠 있는 개체 위치 오프셋(글자처럼 취급이면 무시됨). hwpx는
                        // 음수를 unsigned 2의보수 십진수로 저장(예: -77 = 4294967219)하므로
                        // u32로 파싱 후 i32로 재해석한다(i32 직접 파싱은 범위 초과로 실패).
                        pic.vert_offset = attr_offset_i32(e, "vertOffset").unwrap_or(0);
                        pic.horz_offset = attr_offset_i32(e, "horzOffset").unwrap_or(0);
                    }
                    b"img" => {
                        if let Some(item) = attr(e, "binaryItemIDRef") {
                            pic.bin_ref = hwp_model::BinRef::ItemRef(item);
                        }
                    }
                    // 중첩 pic은 여는 태그만 깊이 증가 (Empty는 닫는 태그가 없음)
                    b"pic" if matches!(event, Event::Start(_)) => depth += 1,
                    _ => {}
                }
            }
            Event::End(e) if e.local_name().as_ref() == b"pic" => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(pic)
}

/// 일반 개체에서 `hp:subList`의 문단들을 재귀 수집 (글상자 텍스트).
fn collect_sub_lists(
    reader: &mut XmlReader<'_>,
    end_name: &[u8],
    generic: &mut GenericControl,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let mut depth = 1u32;
    loop {
        match next_event(reader)? {
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_vec();
                if name == b"subList" {
                    let mut list = ParagraphList {
                        header_data: Vec::new(),
                        paragraphs: Vec::new(),
                    };
                    loop {
                        match next_event(reader)? {
                            Event::Start(inner) if inner.local_name().as_ref() == b"p" => {
                                list.paragraphs
                                    .push(parse_paragraph(reader, &inner, warnings)?);
                            }
                            Event::End(inner) if inner.local_name().as_ref() == b"subList" => {
                                break;
                            }
                            Event::Eof => break,
                            _ => {}
                        }
                    }
                    generic.paragraph_lists.push(list);
                } else if name == end_name {
                    depth += 1;
                }
            }
            Event::End(e) if e.local_name().as_ref() == end_name => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

/// `<hp:linesegarray>` — 줄 배치 정보.
fn parse_linesegs(reader: &mut XmlReader<'_>, para: &mut Paragraph) -> Result<()> {
    loop {
        match next_event(reader)? {
            Event::Empty(e) | Event::Start(e) if e.local_name().as_ref() == b"lineseg" => {
                para.line_segs.push(LineSeg {
                    text_start: attr_u32(&e, "textpos").unwrap_or(0),
                    v_pos: attr_i32(&e, "vertpos").unwrap_or(0),
                    line_height: attr_i32(&e, "vertsize").unwrap_or(0),
                    text_height: attr_i32(&e, "textheight").unwrap_or(0),
                    baseline_gap: attr_i32(&e, "baseline").unwrap_or(0),
                    line_spacing: attr_i32(&e, "spacing").unwrap_or(0),
                    col_start: attr_i32(&e, "horzpos").unwrap_or(0),
                    seg_width: attr_i32(&e, "horzsize").unwrap_or(0),
                    flags: attr_u32(&e, "flags").unwrap_or(0),
                });
            }
            Event::End(e) if e.local_name().as_ref() == b"linesegarray" => break,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod page_ctrl_tests {
    use super::*;

    fn elem(name: &str, attrs: &[(&str, &str)]) -> BytesStart<'static> {
        let mut e = BytesStart::new(name.to_string());
        for (k, v) in attrs {
            e.push_attribute((*k, *v));
        }
        e
    }

    /// 쪽 번호 위치(pgnp): 정품 한라대 = pos BOTTOM_CENTER + sideChar '-'.
    #[test]
    fn pgnp_합성() {
        let e = elem(
            "hp:pageNum",
            &[
                ("pos", "BOTTOM_CENTER"),
                ("formatType", "DIGIT"),
                ("sideChar", "-"),
            ],
        );
        // props=위치5<<8=0x500, 예약6B, sideChar '-'(0x2d) WCHAR.
        assert_eq!(
            build_pgnp(&e),
            vec![0x00, 0x05, 0, 0, 0, 0, 0, 0, 0, 0, 0x2d, 0x00]
        );
    }

    /// 쪽 감추기(pghd): 표지=머리말+쪽번호(0x21), 목차=쪽번호(0x20).
    #[test]
    fn pghd_합성() {
        let cover = elem(
            "hp:pageHiding",
            &[("hideHeader", "1"), ("hidePageNum", "1")],
        );
        assert_eq!(build_pghd(&cover), vec![0x21, 0, 0, 0]);
        let toc = elem(
            "hp:pageHiding",
            &[("hideHeader", "0"), ("hidePageNum", "1")],
        );
        assert_eq!(build_pghd(&toc), vec![0x20, 0, 0, 0]);
    }

    /// 새 번호 지정(nwno): num=1 → 종류(0) + 번호(1).
    #[test]
    fn nwno_합성() {
        let e = elem("hp:newNum", &[("num", "1"), ("numType", "PAGE")]);
        assert_eq!(build_nwno(&e), vec![0, 0, 0, 0, 0x01, 0x00]);
    }

    /// 머리말/꼬리말: 적용쪽(u32) + id(u32). BOTH(0) + id=2.
    #[test]
    fn head_foot_8바이트() {
        let e = elem("hp:header", &[("id", "2"), ("applyPageType", "BOTH")]);
        assert_eq!(head_foot_data(&e), vec![0, 0, 0, 0, 0x02, 0, 0, 0]);
        let odd = elem("hp:footer", &[("id", "3"), ("applyPageType", "ODD")]);
        assert_eq!(head_foot_data(&odd), vec![0x02, 0, 0, 0, 0x03, 0, 0, 0]);
    }
}

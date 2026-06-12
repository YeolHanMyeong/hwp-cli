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
use crate::read::xml::{attr, attr_i32, attr_u16, attr_u32};

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
                            para.char_shape_runs.push((wchar_pos, CharShapeId(id)));
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
                            parse_ctrl(reader, &mut para, &mut wchar_pos)?;
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
                    // 그 외 개체 (pic, rect, ellipse, equation, container...)
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

/// `<hp:ctrl>` — colPr 등 컨트롤 묶음.
fn parse_ctrl(reader: &mut XmlReader<'_>, para: &mut Paragraph, wchar_pos: &mut u32) -> Result<()> {
    loop {
        match next_event(reader)? {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.local_name().as_ref().to_vec();
                let ctrl_id: [u8; 4] = match name.as_slice() {
                    b"colPr" => *b"cold",
                    other => {
                        let mut id = [b' '; 4];
                        for (i, b) in other.iter().take(4).enumerate() {
                            id[i] = *b;
                        }
                        id
                    }
                };
                push_ext_ctrl(para, wchar_pos, 2, ctrl_id);
                para.controls.push(Control::Generic(GenericControl {
                    ctrl_id,
                    data: Vec::new(),
                    paragraph_lists: Vec::new(),
                    extras: Vec::new(),
                }));
            }
            Event::End(e) if e.local_name().as_ref() == b"ctrl" => break,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

/// `<hp:tbl>` — 표.
fn parse_table(
    reader: &mut XmlReader<'_>,
    start: &BytesStart<'_>,
    warnings: &mut Vec<String>,
) -> Result<Table> {
    let mut table = Table {
        common_data: Vec::new(),
        attr: 0,
        rows: attr_u16(start, "rowCnt").unwrap_or(0),
        cols: attr_u16(start, "colCnt").unwrap_or(0),
        cell_spacing: attr_u16(start, "cellSpacing").unwrap_or(0),
        inner_margins: [0; 4],
        row_cell_counts: Vec::new(),
        border_fill: hwp_model::BorderFillId(attr_u16(start, "borderFillIDRef").unwrap_or(0)),
        table_tail: Vec::new(),
        cells: Vec::new(),
        extras: Vec::new(),
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
            Event::End(e) if e.local_name().as_ref() == b"tbl" => break,
            Event::Eof => break,
            _ => {}
        }
    }
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

//! BodyText 섹션 스트림 → [`Section`] 파싱.
//!
//! 실측으로 확정한 구조 (fixtures 기준):
//! - 섹션 루트는 PARA_HEADER 트리들의 나열.
//! - CTRL_HEADER의 ctrl_id는 **역순으로 저장**된다 (b"dces" = secd).
//! - 표: CTRL_HEADER(tbl) 아래에 TABLE 레코드 + 셀마다
//!   [LIST_HEADER, PARA_HEADER...]가 **형제로** 나열된다 — LIST_HEADER가
//!   새 셀을 열고 다음 LIST_HEADER 전까지의 문단이 그 셀 소속.
//! - TABLE의 "행 크기" 배열은 행별 셀 개수다.

use hwp_model::{
    Cell, CharKind, CharShapeId, Control, GenericControl, HwpChar, HwpUnit, LineSeg, PageDef,
    ParaHeaderInfo, ParaShapeId, Paragraph, ParagraphList, Section, SectionDef, StyleId, Table,
    char_kind,
};

use crate::codec::ByteReader;
use crate::doc_info::to_opaque;
use crate::error::Result;
use crate::record::{RecordNode, tag};

/// 섹션 레코드 트리를 Section으로 변환한다.
pub fn parse_section(roots: &[RecordNode]) -> (Section, Vec<String>) {
    let mut section = Section::default();
    let mut warnings = Vec::new();
    for node in roots {
        if node.tag == tag::PARA_HEADER {
            section
                .paragraphs
                .push(parse_paragraph(node, &mut warnings));
        } else {
            warnings.push(format!(
                "섹션 루트에 문단이 아닌 레코드 0x{:03X} — 보존",
                node.tag
            ));
            section.extras.push(to_opaque(node));
        }
    }
    (section, warnings)
}

fn parse_paragraph(node: &RecordNode, warnings: &mut Vec<String>) -> Paragraph {
    let mut para = Paragraph::default();

    // PARA_HEADER 페이로드 (22바이트 prefix + 버전별 tail)
    let mut nchars = None;
    match parse_para_header(&node.data) {
        Ok((shape, style, info, n)) => {
            para.para_shape = shape;
            para.style = style;
            para.header = info;
            nchars = Some(n);
        }
        Err(e) => warnings.push(format!("PARA_HEADER 파싱 실패: {e}")),
    }

    for child in &node.children {
        match child.tag {
            tag::PARA_TEXT => para.chars = decode_para_text(&child.data, warnings),
            tag::PARA_CHAR_SHAPE => {
                let mut r = ByteReader::new(&child.data);
                while r.remaining() >= 8 {
                    let pos = r.read_u32().expect("크기 확인됨");
                    let id = r.read_u32().expect("크기 확인됨");
                    para.char_shape_runs.push((pos, CharShapeId(id as u16)));
                }
            }
            tag::PARA_LINE_SEG => match parse_line_segs(&child.data) {
                Ok(segs) => para.line_segs = segs,
                Err(e) => warnings.push(format!("PARA_LINE_SEG 파싱 실패: {e}")),
            },
            tag::CTRL_HEADER => para.controls.push(parse_control(child, warnings)),
            _ => para.extras.push(to_opaque(child)),
        }
    }

    // 위치 산수의 정합성 검증: 분류표가 틀리면 즉시 드러나는 강력한 불변식
    if let Some(n) = nchars
        && !para.chars.is_empty()
        && para.wchar_len() != n
    {
        warnings.push(format!(
            "문단 WCHAR 수 불일치: PARA_HEADER {n} vs PARA_TEXT 계산 {} — 컨트롤 분류 오류 가능성",
            para.wchar_len()
        ));
    }

    link_controls(&mut para, warnings);
    para
}

fn parse_para_header(data: &[u8]) -> Result<(ParaShapeId, StyleId, ParaHeaderInfo, u32)> {
    let mut r = ByteReader::new(data);
    let nchars_raw = r.read_u32()?;
    let ctrl_mask = r.read_u32()?;
    let para_shape = ParaShapeId(r.read_u16()?);
    let style = StyleId(u16::from(r.read_u8()?));
    let break_type = r.read_u8()?;
    let _char_shape_count = r.read_u16()?;
    let _range_tag_count = r.read_u16()?;
    let _line_seg_count = r.read_u16()?;
    let instance_id = r.read_u32()?;
    let info = ParaHeaderInfo {
        chars_flags: (nchars_raw >> 24 & 0x80) as u8,
        ctrl_mask,
        break_type,
        instance_id,
        tail: r.take_rest().to_vec(),
    };
    Ok((para_shape, style, info, nchars_raw & 0x7FFF_FFFF))
}

/// PARA_TEXT 디코딩 — 컨트롤 문자 분류표가 위치 산수의 기준.
fn decode_para_text(data: &[u8], warnings: &mut Vec<String>) -> Vec<HwpChar> {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    if !data.len().is_multiple_of(2) {
        warnings.push("PARA_TEXT 길이가 홀수 — 마지막 바이트 무시".to_string());
    }

    let mut chars = Vec::new();
    let mut i = 0usize;
    while i < units.len() {
        let u = units[i];
        if u < 32 {
            match char_kind(u) {
                CharKind::Char => {
                    chars.push(HwpChar::CharCtrl(u));
                    i += 1;
                }
                CharKind::Inline | CharKind::Extended => {
                    if i + 8 > units.len() {
                        warnings.push(format!(
                            "컨트롤 문자 {u}의 8 WCHAR가 잘림 (남은 {}개) — 중단",
                            units.len() - i
                        ));
                        break;
                    }
                    // [코드, 정보 6 WCHAR, 코드] — 정보부를 바이트로 보존
                    let payload: Vec<u8> = units[i + 1..i + 7]
                        .iter()
                        .flat_map(|w| w.to_le_bytes())
                        .collect();
                    if units[i + 7] != u {
                        warnings.push(format!(
                            "컨트롤 문자 {u}의 닫는 코드 불일치 ({})",
                            units[i + 7]
                        ));
                    }
                    if char_kind(u) == CharKind::Inline {
                        chars.push(HwpChar::InlineCtrl { code: u, payload });
                    } else {
                        // 선두 4바이트 = 역순 ctrl_id
                        let mut ctrl_id = [payload[0], payload[1], payload[2], payload[3]];
                        ctrl_id.reverse();
                        chars.push(HwpChar::ExtCtrl {
                            code: u,
                            ctrl_id,
                            payload,
                            ctrl_index: None,
                        });
                    }
                    i += 8;
                }
            }
        } else if (0xD800..0xDC00).contains(&u) {
            // 서로게이트 쌍
            if i + 1 < units.len() && (0xDC00..0xE000).contains(&units[i + 1]) {
                let c = char::decode_utf16([u, units[i + 1]])
                    .next()
                    .and_then(|r| r.ok())
                    .unwrap_or(char::REPLACEMENT_CHARACTER);
                chars.push(HwpChar::Text(c));
                i += 2;
            } else {
                warnings.push(format!("짝 없는 서로게이트 0x{u:04X}"));
                chars.push(HwpChar::Text(char::REPLACEMENT_CHARACTER));
                i += 1;
            }
        } else if (0xDC00..0xE000).contains(&u) {
            warnings.push(format!("짝 없는 서로게이트 0x{u:04X}"));
            chars.push(HwpChar::Text(char::REPLACEMENT_CHARACTER));
            i += 1;
        } else {
            chars.push(HwpChar::Text(
                char::from_u32(u32::from(u)).unwrap_or(char::REPLACEMENT_CHARACTER),
            ));
            i += 1;
        }
    }
    chars
}

fn parse_line_segs(data: &[u8]) -> Result<Vec<LineSeg>> {
    let mut r = ByteReader::new(data);
    let mut segs = Vec::with_capacity(data.len() / 36);
    while r.remaining() >= 36 {
        segs.push(LineSeg {
            text_start: r.read_u32()?,
            v_pos: r.read_i32()?,
            line_height: r.read_i32()?,
            text_height: r.read_i32()?,
            baseline_gap: r.read_i32()?,
            line_spacing: r.read_i32()?,
            col_start: r.read_i32()?,
            seg_width: r.read_i32()?,
            flags: r.read_u32()?,
        });
    }
    Ok(segs)
}

/// ExtCtrl 문자 ↔ CTRL_HEADER 레코드를 등장 순서로 연결하고 ctrl_id를 교차 검증.
fn link_controls(para: &mut Paragraph, warnings: &mut Vec<String>) {
    let mut next = 0u32;
    let control_ids: Vec<[u8; 4]> = para.controls.iter().map(Control::ctrl_id).collect();
    for ch in &mut para.chars {
        if let HwpChar::ExtCtrl {
            ctrl_id,
            ctrl_index,
            ..
        } = ch
        {
            if (next as usize) < control_ids.len() {
                let expected = control_ids[next as usize];
                if *ctrl_id != expected {
                    warnings.push(format!(
                        "ExtCtrl ctrl_id 불일치: 텍스트 {:?} vs CTRL_HEADER {:?}",
                        String::from_utf8_lossy(ctrl_id),
                        String::from_utf8_lossy(&expected),
                    ));
                }
                *ctrl_index = Some(next);
                next += 1;
            } else {
                warnings.push(format!(
                    "ExtCtrl {:?}에 대응하는 CTRL_HEADER 없음",
                    String::from_utf8_lossy(ctrl_id)
                ));
            }
        }
    }
    if (next as usize) < para.controls.len() {
        warnings.push(format!(
            "CTRL_HEADER {}개가 텍스트의 ExtCtrl과 연결되지 않음",
            para.controls.len() - next as usize
        ));
    }
}

fn parse_control(node: &RecordNode, warnings: &mut Vec<String>) -> Control {
    if node.data.len() < 4 {
        warnings.push("CTRL_HEADER 페이로드가 4바이트 미만".to_string());
        return Control::Generic(GenericControl {
            ctrl_id: *b"????",
            data: node.data.clone(),
            paragraph_lists: Vec::new(),
            extras: node.children.iter().map(to_opaque).collect(),
            raw_children: node.children.iter().map(to_opaque).collect(),
            gso_shapes: Vec::new(),
        });
    }
    let mut ctrl_id = [node.data[0], node.data[1], node.data[2], node.data[3]];
    ctrl_id.reverse(); // 역순 저장 → 정방향
    let rest = node.data[4..].to_vec();

    match &ctrl_id {
        b"secd" => Control::SectionDef(parse_section_def(rest, &node.children, warnings)),
        b"tbl " => Control::Table(parse_table(rest, &node.children, warnings)),
        // 그리기 개체: 문단(글상자)이 없고 그림 레코드가 있으면 이미지로 해석
        b"gso "
            if !node.children.iter().any(subtree_has_paragraphs)
                && find_picture_record(&node.children).is_some() =>
        {
            match parse_picture_gso(&rest, &node.children) {
                Ok(p) => Control::Picture(p),
                Err(e) => {
                    warnings.push(format!("그림 개체 파싱 실패: {e}"));
                    Control::Generic(parse_generic(ctrl_id, rest, &node.children, warnings))
                }
            }
        }
        _ => Control::Generic(parse_generic(ctrl_id, rest, &node.children, warnings)),
    }
}

/// 서브트리에서 SHAPE_COMPONENT_PICTURE 레코드를 찾는다.
fn find_picture_record(children: &[RecordNode]) -> Option<&RecordNode> {
    for child in children {
        if child.tag == tag::SHAPE_COMPONENT_PICTURE {
            return Some(child);
        }
        if let Some(found) = find_picture_record(&child.children) {
            return Some(found);
        }
    }
    None
}

/// gso 그림 개체: 개체 공통 속성(크기)과 그림 레코드의 BinItem ID를 추출한다.
///
/// 그림 개체 속성 레이아웃 (스펙 §표 91): 테두리 색(4)+굵기(4)+속성(4)
/// + 꼭지점 4점(32) + 자르기(16) + 안쪽 여백(8) + 밝기(1)+명암(1)+효과(1)
/// + **BinItem ID(2)** — 오프셋 71.
fn parse_picture_gso(common: &[u8], children: &[RecordNode]) -> Result<hwp_model::Picture> {
    // 개체 공통 속성: 속성(4) 세로(4) 가로(4) 폭(4) 높이(4)
    let mut r = ByteReader::new(common);
    let attr = r.read_u32()?;
    let _v_offset = r.read_u32()?;
    let _h_offset = r.read_u32()?;
    let width = HwpUnit(r.read_i32()?);
    let height = HwpUnit(r.read_i32()?);

    let pic_node = find_picture_record(children)
        .ok_or_else(|| crate::error::Hwp5Error::MalformedRecord("그림 레코드 없음".into()))?;
    let mut pr = ByteReader::new(&pic_node.data);
    pr.read_bytes(71)?;
    let bin_id = pr.read_u16()?;

    Ok(hwp_model::Picture {
        common_data: common.to_vec(),
        width,
        height,
        treat_as_char: attr & 1 != 0,
        // hwp5 원본은 배치를 common_data로 보존하므로 합성용 필드는 0.
        z_order: 0,
        vert_offset: 0,
        horz_offset: 0,
        bin_ref: hwp_model::BinRef::Id(hwp_model::BinDataId(bin_id)),
        extras: children.iter().map(to_opaque).collect(),
    })
}

fn parse_section_def(
    data: Vec<u8>,
    children: &[RecordNode],
    warnings: &mut Vec<String>,
) -> SectionDef {
    let mut def = SectionDef {
        data,
        page: None,
        extras: Vec::new(),
    };
    for child in children {
        if child.tag == tag::PAGE_DEF {
            match parse_page_def(&child.data) {
                Ok(p) => def.page = Some(p),
                Err(e) => {
                    warnings.push(format!("PAGE_DEF 파싱 실패: {e}"));
                    def.extras.push(to_opaque(child));
                }
            }
        } else {
            def.extras.push(to_opaque(child));
        }
    }
    def
}

fn parse_page_def(data: &[u8]) -> Result<PageDef> {
    let mut r = ByteReader::new(data);
    Ok(PageDef {
        width: HwpUnit(r.read_i32()?),
        height: HwpUnit(r.read_i32()?),
        margin_left: HwpUnit(r.read_i32()?),
        margin_right: HwpUnit(r.read_i32()?),
        margin_top: HwpUnit(r.read_i32()?),
        margin_bottom: HwpUnit(r.read_i32()?),
        margin_header: HwpUnit(r.read_i32()?),
        margin_footer: HwpUnit(r.read_i32()?),
        gutter: HwpUnit(r.read_i32()?),
        attr: r.read_u32()?,
    })
}

fn parse_table(common_data: Vec<u8>, children: &[RecordNode], warnings: &mut Vec<String>) -> Table {
    let mut table = Table {
        common_data,
        placement: None,
        attr: 0,
        rows: 0,
        cols: 0,
        cell_spacing: 0,
        inner_margins: [0; 4],
        row_cell_counts: Vec::new(),
        border_fill: hwp_model::BorderFillId(0),
        table_tail: Vec::new(),
        cells: Vec::new(),
        extras: Vec::new(),
    };
    let mut current_cell: Option<Cell> = None;

    for child in children {
        match child.tag {
            tag::TABLE => {
                if let Err(e) = parse_table_record(&child.data, &mut table) {
                    warnings.push(format!("TABLE 레코드 파싱 실패: {e}"));
                    table.extras.push(to_opaque(child));
                }
            }
            tag::LIST_HEADER => {
                if let Some(done) = current_cell.take() {
                    table.cells.push(done);
                }
                match parse_cell_header(&child.data) {
                    Ok(cell) => current_cell = Some(cell),
                    Err(e) => {
                        warnings.push(format!("셀 LIST_HEADER 파싱 실패: {e}"));
                        table.extras.push(to_opaque(child));
                    }
                }
            }
            tag::PARA_HEADER => {
                let para = parse_paragraph(child, warnings);
                match &mut current_cell {
                    Some(cell) => cell.paragraphs.push(para),
                    None => {
                        warnings.push("셀 밖의 문단 — LIST_HEADER 누락".to_string());
                        table.extras.push(to_opaque(child));
                    }
                }
            }
            _ => table.extras.push(to_opaque(child)),
        }
    }
    if let Some(done) = current_cell.take() {
        table.cells.push(done);
    }
    table
}

fn parse_table_record(data: &[u8], table: &mut Table) -> Result<()> {
    let mut r = ByteReader::new(data);
    table.attr = r.read_u32()?;
    table.rows = r.read_u16()?;
    table.cols = r.read_u16()?;
    table.cell_spacing = r.read_u16()?;
    table.inner_margins = r.read_u16_array::<4>()?;
    table.row_cell_counts = (0..table.rows)
        .map(|_| r.read_u16())
        .collect::<Result<_>>()?;
    table.border_fill = hwp_model::BorderFillId(r.read_u16()?);
    table.table_tail = r.take_rest().to_vec();
    Ok(())
}

/// 셀 LIST_HEADER: 문단 수 i32 + 속성 u32 + 셀 속성 (실측 46바이트 레이아웃).
fn parse_cell_header(data: &[u8]) -> Result<Cell> {
    let mut r = ByteReader::new(data);
    let _para_count = r.read_i32()?;
    let list_attr = r.read_u32()?;
    let col = r.read_u16()?;
    let row = r.read_u16()?;
    let col_span = r.read_u16()?;
    let row_span = r.read_u16()?;
    let width = HwpUnit(r.read_i32()?);
    let height = HwpUnit(r.read_i32()?);
    let margins = r.read_u16_array::<4>()?;
    let border_fill = hwp_model::BorderFillId(r.read_u16()?);
    Ok(Cell {
        list_attr,
        col,
        row,
        col_span,
        row_span,
        width,
        height,
        margins,
        border_fill,
        header_tail: r.take_rest().to_vec(),
        paragraphs: Vec::new(),
    })
}

fn parse_generic(
    ctrl_id: [u8; 4],
    data: Vec<u8>,
    children: &[RecordNode],
    warnings: &mut Vec<String>,
) -> GenericControl {
    let mut g = GenericControl {
        ctrl_id,
        data,
        paragraph_lists: Vec::new(),
        extras: Vec::new(),
        // 원본 자식 서브트리를 중첩 그대로 보존 → 무손실 재직렬화.
        raw_children: children.iter().map(to_opaque).collect(),
        gso_shapes: Vec::new(),
    };
    collect_paragraph_lists(children, &mut g, warnings);
    g
}

/// 문단 리스트를 재귀 수집한다.
///
/// 글상자/도형은 CTRL_HEADER(gso) → SHAPE_COMPONENT → LIST_HEADER처럼
/// 컨테이너 레코드 한 단계 아래에 문단을 두므로(실측), 문단을 포함하는
/// 서브트리는 내려가며 수집한다. 이때 GenericControl의 IR은 원본 중첩
/// 구조를 평탄화한다 — 정확한 재직렬화는 L0 바이패스 경로의 몫이다.
fn collect_paragraph_lists(
    children: &[RecordNode],
    g: &mut GenericControl,
    warnings: &mut Vec<String>,
) {
    for child in children {
        match child.tag {
            tag::LIST_HEADER => {
                g.paragraph_lists.push(ParagraphList {
                    header_data: child.data.clone(),
                    paragraphs: Vec::new(),
                });
                // LIST_HEADER가 자식으로 문단을 갖는 변형도 방어
                collect_paragraph_lists(&child.children, g, warnings);
            }
            tag::PARA_HEADER => {
                let para = parse_paragraph(child, warnings);
                if g.paragraph_lists.is_empty() {
                    // LIST_HEADER 없이 문단이 오는 변형 방어
                    g.paragraph_lists.push(ParagraphList {
                        header_data: Vec::new(),
                        paragraphs: Vec::new(),
                    });
                }
                g.paragraph_lists
                    .last_mut()
                    .expect("위에서 보장")
                    .paragraphs
                    .push(para);
            }
            _ if subtree_has_paragraphs(child) => {
                // 컨테이너(SHAPE_COMPONENT 등): 페이로드는 보존하고 자식으로 재귀
                g.extras.push(hwp_model::OpaqueRecord {
                    tag: child.tag,
                    data: child.data.clone(),
                    children: Vec::new(),
                });
                collect_paragraph_lists(&child.children, g, warnings);
            }
            _ => g.extras.push(to_opaque(child)),
        }
    }
}

fn subtree_has_paragraphs(node: &RecordNode) -> bool {
    node.children.iter().any(|c| {
        c.tag == tag::PARA_HEADER || c.tag == tag::LIST_HEADER || subtree_has_paragraphs(c)
    })
}

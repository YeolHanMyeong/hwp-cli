//! DocInfo 스트림 → [`DocHeader`] 파싱.
//!
//! 모든 레코드 파싱은 "알려진 prefix를 구조체로 + 남은 바이트를 tail로"
//! 규칙을 따른다 — 버전이 올라가며 필드가 뒤에 추가되는 HWP의 전방
//! 호환 구조에 대응하고, 왕복 시 그대로 덧붙여 보존한다.

use hwp_model::{
    BinDataItem, CharShape, CharShapeId, DocHeader, DocumentProperties, FaceName, LANG_COUNT,
    OpaqueRecord, ParaShape, ParaShapeId, RawEntry, Style,
};

use crate::codec::ByteReader;
use crate::error::Result;
use crate::record::{RecordNode, tag};

/// DocInfo 레코드 트리를 DocHeader로 변환한다.
/// 해석 실패는 가능한 한 opaque 보존 + 경고로 흡수한다.
pub fn parse_doc_info(roots: &[RecordNode]) -> (DocHeader, Vec<String>) {
    let mut header = DocHeader::default();
    let mut warnings = Vec::new();
    // ID_MAPPINGS의 언어별 글꼴 카운트 — FACE_NAME의 언어 슬롯 배정에 사용
    let mut font_counts: [u32; LANG_COUNT] = [0; LANG_COUNT];
    let mut font_cursor = 0usize; // 현재 채우는 언어 슬롯

    for node in roots {
        match node.tag {
            tag::DOCUMENT_PROPERTIES => match parse_document_properties(&node.data) {
                Ok(p) => header.properties = p,
                Err(e) => {
                    warnings.push(format!("DOCUMENT_PROPERTIES 파싱 실패: {e}"));
                    header.extras.push(to_opaque(node));
                }
            },
            tag::ID_MAPPINGS => {
                // 카운트 배열: binData, 글꼴×7, 테두리채움, 글자모양, 탭, 번호,
                // 글머리표, 문단모양, 스타일, [메모모양, 변경추적, 변경추적사용자…]
                let mut r = ByteReader::new(&node.data);
                let mut counts = Vec::new();
                while let Ok(v) = r.read_u32() {
                    counts.push(v);
                }
                for (i, slot) in font_counts.iter_mut().enumerate() {
                    *slot = counts.get(1 + i).copied().unwrap_or(0);
                }
                header.id_mappings_counts = counts.clone();
                // 자식 레코드들이 실제 테이블 항목
                for child in &node.children {
                    parse_id_mapping_child(
                        child,
                        &mut header,
                        &font_counts,
                        &mut font_cursor,
                        &mut warnings,
                    );
                }
            }
            _ => header.extras.push(to_opaque(node)),
        }
    }

    (header, warnings)
}

fn parse_id_mapping_child(
    node: &RecordNode,
    header: &mut DocHeader,
    font_counts: &[u32; LANG_COUNT],
    font_cursor: &mut usize,
    warnings: &mut Vec<String>,
) {
    match node.tag {
        tag::BIN_DATA => match parse_bin_data(&node.data) {
            Ok(item) => header.bin_data.push(item),
            Err(e) => {
                warnings.push(format!("BIN_DATA 파싱 실패: {e}"));
                header.extras.push(to_opaque(node));
            }
        },
        tag::FACE_NAME => {
            // 언어 슬롯 배정: 현재 슬롯의 카운트가 차면 다음 슬롯으로
            while *font_cursor < LANG_COUNT
                && header.fonts[*font_cursor].len() as u32 >= font_counts[*font_cursor]
            {
                *font_cursor += 1;
            }
            let slot = (*font_cursor).min(LANG_COUNT - 1);
            match parse_face_name(&node.data) {
                Ok(f) => header.fonts[slot].push(f),
                Err(e) => {
                    warnings.push(format!("FACE_NAME 파싱 실패: {e}"));
                    header.fonts[slot].push(FaceName::default());
                    header.extras.push(to_opaque(node));
                }
            }
        }
        tag::BORDER_FILL => match parse_border_fill(&node.data) {
            Ok(bf) => header.border_fills.push(bf),
            Err(e) => {
                warnings.push(format!("BORDER_FILL 파싱 실패: {e}"));
                header.border_fills.push(hwp_model::BorderFill::default());
                header.extras.push(to_opaque(node));
            }
        },
        tag::CHAR_SHAPE => match parse_char_shape(&node.data) {
            Ok(cs) => header.char_shapes.push(cs),
            Err(e) => {
                warnings.push(format!("CHAR_SHAPE 파싱 실패: {e}"));
                header.char_shapes.push(CharShape::default());
                header.extras.push(to_opaque(node));
            }
        },
        tag::TAB_DEF => header.tab_defs.push(raw_entry(node)),
        tag::NUMBERING => {
            header.numberings.push(raw_entry(node));
            // 렌더 전용: 수준별 형식 템플릿("^1." "(^5)" "제^1조")을 파싱한다.
            header
                .numbering_levels
                .push(parse_numbering_levels(&node.data));
        }
        tag::BULLET => {
            // 글머리 문자 = BULLET offset+8의 WCHAR(UTF-16LE).
            let ch = node
                .data
                .get(8..10)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .and_then(|u| char::from_u32(u32::from(u)))
                .filter(|c| !c.is_control())
                .unwrap_or('•');
            header.bullets.push(raw_entry(node));
            header.bullet_chars.push(ch);
        }
        tag::PARA_SHAPE => match parse_para_shape(&node.data) {
            Ok(ps) => header.para_shapes.push(ps),
            Err(e) => {
                warnings.push(format!("PARA_SHAPE 파싱 실패: {e}"));
                header.para_shapes.push(ParaShape::default());
                header.extras.push(to_opaque(node));
            }
        },
        tag::STYLE => match parse_style(&node.data) {
            Ok(s) => header.styles.push(s),
            Err(e) => {
                warnings.push(format!("STYLE 파싱 실패: {e}"));
                header.styles.push(Style::default());
                header.extras.push(to_opaque(node));
            }
        },
        _ => header.id_extras.push(to_opaque(node)),
    }
}

fn parse_document_properties(data: &[u8]) -> Result<DocumentProperties> {
    let mut r = ByteReader::new(data);
    let section_count = r.read_u16()?;
    let mut start_numbers = [0u16; 6];
    for n in &mut start_numbers {
        *n = r.read_u16()?;
    }
    let caret = (r.read_u32()?, r.read_u32()?, r.read_u32()?);
    Ok(DocumentProperties {
        section_count,
        start_numbers,
        caret,
    })
}

fn parse_face_name(data: &[u8]) -> Result<FaceName> {
    let mut r = ByteReader::new(data);
    let attr = r.read_u8()?;
    let name = r.read_hwp_string()?;
    let has_alt = attr & 0x80 != 0;
    let has_panose = attr & 0x40 != 0;
    let has_default = attr & 0x20 != 0;

    let (alt_kind, alt_name) = if has_alt {
        (Some(r.read_u8()?), Some(r.read_hwp_string()?))
    } else {
        (None, None)
    };
    let panose = if has_panose {
        let b = r.read_bytes(10)?;
        let mut p = [0u8; 10];
        p.copy_from_slice(b);
        Some(p)
    } else {
        None
    };
    let default_name = if has_default {
        Some(r.read_hwp_string()?)
    } else {
        None
    };

    Ok(FaceName {
        attr,
        name,
        alt_kind,
        alt_name,
        panose,
        default_name,
        type_info: None,
        tail: r.take_rest().to_vec(),
    })
}

fn parse_char_shape(data: &[u8]) -> Result<CharShape> {
    let mut r = ByteReader::new(data);
    let face_ids = r.read_u16_array::<LANG_COUNT>()?;
    let mut ratios = [0u8; LANG_COUNT];
    for v in &mut ratios {
        *v = r.read_u8()?;
    }
    let mut spacings = [0i8; LANG_COUNT];
    for v in &mut spacings {
        *v = r.read_i8()?;
    }
    let mut rel_sizes = [0u8; LANG_COUNT];
    for v in &mut rel_sizes {
        *v = r.read_u8()?;
    }
    let mut offsets = [0i8; LANG_COUNT];
    for v in &mut offsets {
        *v = r.read_i8()?;
    }
    let base_size = r.read_i32()?;
    let attr = r.read_u32()?;
    let shadow_gap = (r.read_i8()?, r.read_i8()?);
    let text_color = r.read_u32()?;
    let underline_color = r.read_u32()?;
    let shade_color = r.read_u32()?;
    let shadow_color = r.read_u32()?;
    // 5.0.2.1+ tail 선두 = 글자 테두리/배경 ID (tail 자체는 그대로 보존)
    let tail = r.take_rest().to_vec();
    let border_fill_id = if tail.len() >= 2 {
        u16::from_le_bytes([tail[0], tail[1]])
    } else {
        0
    };

    Ok(CharShape {
        face_ids,
        ratios,
        spacings,
        rel_sizes,
        offsets,
        base_size,
        attr,
        // HWP5 raw 비트(18~20)는 DIFFSPEC라 취소선으로 신뢰하지 않음 (가짜 취소선 방지).
        strike: false,
        shadow_gap,
        text_color,
        underline_color,
        // HWP5는 밑줄 모양을 별도 IR 필드로 두지 않는다(hwpx 왕복 전용) → 미지정(0).
        underline_shape: 0,
        shade_color,
        shadow_color,
        border_fill_id,
        tail,
    })
}

fn parse_para_shape(data: &[u8]) -> Result<ParaShape> {
    let mut r = ByteReader::new(data);
    let attr1 = r.read_u32()?;
    let margin_left = r.read_i32()?;
    let margin_right = r.read_i32()?;
    let indent = r.read_i32()?;
    let spacing_top = r.read_i32()?;
    let spacing_bottom = r.read_i32()?;
    let line_spacing_old = r.read_i32()?;
    let tab_def_id = r.read_u16()?;
    let numbering_id = r.read_u16()?;
    let border_fill_id = r.read_u16()?;
    let mut border_offsets = [0i16; 4];
    for v in &mut border_offsets {
        *v = r.read_u16()? as i16;
    }
    // 줄간격: 종류는 attr1 bits 0~1, 값은 5.0.2.5+ tail(attr2+attr3+줄간격) 또는 구버전 필드
    let tail = r.take_rest().to_vec();
    let line_spacing_type = (attr1 & 0x3) as u8;
    let line_spacing = if tail.len() >= 12 {
        i32::from_le_bytes([tail[8], tail[9], tail[10], tail[11]])
    } else {
        line_spacing_old
    };
    Ok(ParaShape {
        attr1,
        indent,
        margin_left,
        margin_right,
        spacing_top,
        spacing_bottom,
        line_spacing_old,
        tab_def_id,
        numbering_id,
        border_fill_id,
        border_offsets,
        line_spacing_type,
        line_spacing,
        tail,
    })
}

fn parse_style(data: &[u8]) -> Result<Style> {
    let mut r = ByteReader::new(data);
    let name = r.read_hwp_string()?;
    let english_name = r.read_hwp_string()?;
    let attr = r.read_u8()?;
    let next_style = r.read_u8()?;
    let lang_id = r.read_u16()? as i16;
    let para_shape = ParaShapeId(r.read_u16()?);
    let char_shape = CharShapeId(r.read_u16()?);
    Ok(Style {
        name,
        english_name,
        attr,
        next_style,
        lang_id,
        para_shape,
        char_shape,
        tail: r.take_rest().to_vec(),
    })
}

/// BORDER_FILL (실측 레이아웃): attr u16 + 4변×(종류 u8, 굵기 u8, 색 u32)
/// + 대각선 6B + 채우기 종류 u32 + [단색이면 배경색 u32 …].
fn parse_border_fill(data: &[u8]) -> Result<hwp_model::BorderFill> {
    use hwp_model::BorderLine;
    let mut r = ByteReader::new(data);
    let attr = r.read_u16()?;
    let read_line = |r: &mut ByteReader<'_>| -> Result<BorderLine> {
        Ok(BorderLine {
            line_type: r.read_u8()?,
            width: r.read_u8()?,
            color: r.read_u32()?,
        })
    };
    let mut sides = [BorderLine::default(); 4];
    for side in &mut sides {
        *side = read_line(&mut r)?;
    }
    let diagonal = read_line(&mut r)?;
    let fill_type = r.read_u32()?;
    let bg_color = if fill_type & 0x1 != 0 {
        Some(r.read_u32()?)
    } else {
        None
    };
    Ok(hwp_model::BorderFill {
        attr,
        sides,
        diagonal,
        fill_type,
        bg_color,
        tail: r.take_rest().to_vec(),
    })
}

fn parse_bin_data(data: &[u8]) -> Result<BinDataItem> {
    let mut r = ByteReader::new(data);
    let attr = r.read_u16()?;
    let kind = attr & 0xF; // 0: 링크, 1: 임베딩, 2: 스토리지
    let (mut link_abs, mut link_rel, mut storage_id, mut extension) = (None, None, None, None);
    if kind == 0 {
        link_abs = Some(r.read_hwp_string()?);
        link_rel = Some(r.read_hwp_string()?);
    } else {
        storage_id = Some(r.read_u16()?);
        if kind == 1 {
            extension = Some(r.read_hwp_string()?);
        }
    }
    Ok(BinDataItem {
        attr,
        link_abs,
        link_rel,
        storage_id,
        extension,
        tail: r.take_rest().to_vec(),
    })
}

/// RecordNode → OpaqueRecord 변환 (서브트리 통째 보존).
pub fn to_opaque(node: &RecordNode) -> OpaqueRecord {
    OpaqueRecord {
        tag: node.tag,
        data: node.data.clone(),
        children: node.children.iter().map(to_opaque).collect(),
    }
}

fn raw_entry(node: &RecordNode) -> RawEntry {
    RawEntry {
        data: node.data.clone(),
        children: node.children.iter().map(to_opaque).collect(),
    }
}

/// NUMBERING 레코드에서 7수준 형식 템플릿을 파싱한다(렌더 전용).
/// 수준마다 `[속성 u32, 너비보정 u16, 본문거리 u16, 글자모양ref u32(=0xFFFFFFFF), 템플릿(HWP string)]`.
/// 구조가 어긋나면 그 수준부터 기본값(빈 템플릿)으로 폴백한다(회귀 없음). 시작번호(대개 1)는
/// 템플릿 뒤 오프셋이 유동적이라 v1은 start=1 유지(문서화).
fn parse_numbering_levels(data: &[u8]) -> Vec<hwp_model::NumLevel> {
    fn read_level(r: &mut ByteReader) -> Option<String> {
        let _attr = r.read_u32().ok()?;
        let _width = r.read_u16().ok()?; // 너비 보정
        let _dist = r.read_u16().ok()?; // 본문과의 거리
        // 글자모양 참조 u32 — 정품 번호 수준은 0xFFFFFFFF(없음). 아니면 미지 구조.
        if r.read_u32().ok()? != 0xFFFF_FFFF {
            return None;
        }
        r.read_hwp_string().ok() // 템플릿 = len u16 + UTF-16LE
    }
    let mut levels: Vec<hwp_model::NumLevel> = Vec::with_capacity(7);
    let mut r = ByteReader::new(data);
    for _ in 0..7 {
        match read_level(&mut r) {
            Some(template) => levels.push(hwp_model::NumLevel {
                start: 1,
                fmt: hwp_model::NumFmt::Digit,
                template,
            }),
            None => break,
        }
    }
    while levels.len() < 7 {
        levels.push(hwp_model::NumLevel::default());
    }
    levels
}

#[cfg(test)]
mod numbering_tests {
    use super::parse_numbering_levels;

    #[test]
    fn 기본_번호정의_템플릿_7개() {
        // DEFAULT_NUMBERING_DATA(write.rs)와 동일 바이트 — 수준별 템플릿 검증.
        let data: &[u8] = &[
            0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x32, 0x00, 0xff, 0xff, 0xff, 0xff, 0x03, 0x00,
            0x5e, 0x00, 0x31, 0x00, 0x2e, 0x00, 0x0c, 0x01, 0x00, 0x00, 0x00, 0x00, 0x32, 0x00,
            0xff, 0xff, 0xff, 0xff, 0x03, 0x00, 0x5e, 0x00, 0x32, 0x00, 0x2e, 0x00, 0x0c, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x32, 0x00, 0xff, 0xff, 0xff, 0xff, 0x03, 0x00, 0x5e, 0x00,
            0x33, 0x00, 0x29, 0x00, 0x0c, 0x01, 0x00, 0x00, 0x00, 0x00, 0x32, 0x00, 0xff, 0xff,
            0xff, 0xff, 0x03, 0x00, 0x5e, 0x00, 0x34, 0x00, 0x29, 0x00, 0x0c, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x32, 0x00, 0xff, 0xff, 0xff, 0xff, 0x04, 0x00, 0x28, 0x00, 0x5e, 0x00,
            0x35, 0x00, 0x29, 0x00, 0x0c, 0x01, 0x00, 0x00, 0x00, 0x00, 0x32, 0x00, 0xff, 0xff,
            0xff, 0xff, 0x04, 0x00, 0x28, 0x00, 0x5e, 0x00, 0x36, 0x00, 0x29, 0x00, 0x2c, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x32, 0x00, 0xff, 0xff, 0xff, 0xff, 0x02, 0x00, 0x5e, 0x00,
            0x37, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00,
            0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x00, 0x00,
        ];
        let levels = parse_numbering_levels(data);
        let tmpls: Vec<&str> = levels.iter().map(|l| l.template.as_str()).collect();
        assert_eq!(tmpls, ["^1.", "^2.", "^3)", "^4)", "(^5)", "(^6)", "^7"]);
        assert!(levels.iter().all(|l| l.start == 1));
    }

    #[test]
    fn 미지_구조는_기본값_폴백() {
        // 글자모양ref가 0xFFFFFFFF 아님 → 첫 수준부터 폴백.
        let data: &[u8] = &[0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 0, 0];
        let levels = parse_numbering_levels(data);
        assert_eq!(levels.len(), 7);
        assert!(levels.iter().all(|l| l.template.is_empty()));
    }
}

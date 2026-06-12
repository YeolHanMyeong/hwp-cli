//! `Contents/header.xml` → [`DocHeader`].
//!
//! M2 범위: 글꼴(fontfaces), 문자 모양(charPr), 문단 모양(paraPr — 정렬),
//! 스타일(style). 테두리/번호 등은 추후 마일스톤에서 채운다.

use hwp_model::{
    CharShape, CharShapeId, DocHeader, FaceName, LANG_COUNT, ParaShape, ParaShapeId, Style,
};
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::error::{HwpxError, Result};
use crate::read::xml::{attr, attr_i32, attr_u16, attr_u32, parse_color};

/// OWPML 언어 이름 → 7언어 슬롯 인덱스.
fn lang_slot(name: &str) -> Option<usize> {
    Some(match name {
        "HANGUL" => 0,
        "LATIN" => 1,
        "HANJA" => 2,
        "JAPANESE" => 3,
        "OTHER" => 4,
        "SYMBOL" => 5,
        "USER" => 6,
        _ => return None,
    })
}

/// hwp5 ParaShape::alignment()과 같은 인코딩으로 정렬 매핑.
fn alignment_code(s: &str) -> u32 {
    match s {
        "JUSTIFY" => 0,
        "LEFT" => 1,
        "RIGHT" => 2,
        "CENTER" => 3,
        "DISTRIBUTE" => 4,
        "DISTRIBUTE_SPACE" => 5,
        _ => 0,
    }
}

pub fn parse_header(xml: &str) -> Result<(DocHeader, Vec<String>)> {
    let mut header = DocHeader::default();
    let mut warnings = Vec::new();
    let mut reader = Reader::from_str(xml);

    // 현재 컨텍스트
    let mut current_lang: Option<usize> = None;
    let mut current_char: Option<CharShape> = None;
    let mut current_para: Option<ParaShape> = None;

    loop {
        let event = reader.read_event().map_err(|e| HwpxError::Xml {
            entry: "Contents/header.xml".to_string(),
            message: e.to_string(),
        })?;
        match event {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(event, Event::Empty(_));
                match e.local_name().as_ref() {
                    b"fontface" => {
                        current_lang = attr(e, "lang").as_deref().and_then(lang_slot);
                        if current_lang.is_none() {
                            warnings
                                .push(format!("알 수 없는 fontface lang: {:?}", attr(e, "lang")));
                        }
                    }
                    b"font" => {
                        if let Some(slot) = current_lang {
                            header.fonts[slot].push(FaceName {
                                name: attr(e, "face").unwrap_or_default(),
                                ..FaceName::default()
                            });
                        }
                    }
                    b"charPr" => {
                        let cs = CharShape {
                            base_size: attr_i32(e, "height").unwrap_or(1000),
                            text_color: attr(e, "textColor").map_or(0, |c| parse_color(&c)),
                            shade_color: attr(e, "shadeColor")
                                .map_or(0xFFFF_FFFF, |c| parse_color(&c)),
                            ratios: [100; LANG_COUNT],
                            rel_sizes: [100; LANG_COUNT],
                            ..CharShape::default()
                        };
                        if empty {
                            header.char_shapes.push(cs);
                        } else {
                            current_char = Some(cs);
                        }
                    }
                    // charPr 자식들
                    b"fontRef" => {
                        if let Some(cs) = &mut current_char {
                            for (i, name) in [
                                "hangul", "latin", "hanja", "japanese", "other", "symbol", "user",
                            ]
                            .iter()
                            .enumerate()
                            {
                                cs.face_ids[i] = attr_u16(e, name).unwrap_or(0);
                            }
                        }
                    }
                    b"bold" => {
                        if let Some(cs) = &mut current_char {
                            cs.attr |= 1 << 1;
                        }
                    }
                    b"italic" => {
                        if let Some(cs) = &mut current_char {
                            cs.attr |= 1;
                        }
                    }
                    b"paraPr" => {
                        current_para = Some(ParaShape::default());
                        if empty {
                            header
                                .para_shapes
                                .push(current_para.take().expect("방금 생성"));
                        }
                    }
                    b"align" => {
                        if let Some(ps) = &mut current_para
                            && let Some(h) = attr(e, "horizontal")
                        {
                            ps.attr1 |= alignment_code(&h) << 2;
                        }
                    }
                    b"style" => {
                        header.styles.push(Style {
                            name: attr(e, "name").unwrap_or_default(),
                            english_name: attr(e, "engName").unwrap_or_default(),
                            para_shape: ParaShapeId(attr_u16(e, "paraPrIDRef").unwrap_or(0)),
                            char_shape: CharShapeId(attr_u16(e, "charPrIDRef").unwrap_or(0)),
                            next_style: attr_u32(e, "nextStyleIDRef").unwrap_or(0) as u8,
                            lang_id: attr_i32(e, "langID").unwrap_or(0) as i16,
                            ..Style::default()
                        });
                    }
                    _ => {}
                }
            }
            Event::End(ref e) => match e.local_name().as_ref() {
                b"fontface" => current_lang = None,
                b"charPr" => {
                    if let Some(cs) = current_char.take() {
                        header.char_shapes.push(cs);
                    }
                }
                b"paraPr" => {
                    if let Some(ps) = current_para.take() {
                        header.para_shapes.push(ps);
                    }
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }

    Ok((header, warnings))
}

//! `Contents/header.xml` → [`DocHeader`].
//!
//! M2 범위: 글꼴(fontfaces), 문자 모양(charPr), 문단 모양(paraPr — 정렬),
//! 스타일(style). 테두리/번호 등은 추후 마일스톤에서 채운다.

use hwp_model::{
    BorderFill, BorderLine, CharShape, CharShapeId, DocHeader, FaceName, LANG_COUNT, ParaShape,
    ParaShapeId, Style,
};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::error::{HwpxError, Result};
use crate::read::xml::{attr, attr_i32, attr_u16, attr_u32, parse_color};

/// OWPML 테두리선 종류 → hwp5 코드.
fn line_type_code(s: &str) -> u8 {
    match s {
        "NONE" => 0,
        "SOLID" => 1,
        "DASH" => 2,
        "DOT" => 3,
        "DASH_DOT" => 4,
        "DASH_DOT_DOT" => 5,
        "LONG_DASH" => 6,
        "CIRCLE" => 7,
        "DOUBLE_SLIM" => 8,
        "SLIM_THICK" => 9,
        "THICK_SLIM" => 10,
        "SLIM_THICK_SLIM" => 11,
        _ => 1,
    }
}

/// "0.12 mm" → 굵기 인덱스 (가장 가까운 값).
fn width_index(s: &str) -> u8 {
    const TABLE: [f32; 16] = [
        0.1, 0.12, 0.15, 0.2, 0.25, 0.3, 0.4, 0.5, 0.6, 0.7, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0,
    ];
    let mm: f32 = s.trim_end_matches("mm").trim().parse().unwrap_or(0.12);
    TABLE
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - mm).abs().total_cmp(&(b.1 - mm).abs()))
        .map(|(i, _)| i as u8)
        .unwrap_or(1)
}

fn parse_border_line(e: &BytesStart<'_>) -> BorderLine {
    BorderLine {
        line_type: attr(e, "type").map_or(1, |t| line_type_code(&t)),
        width: attr(e, "width").map_or(1, |w| width_index(&w)),
        color: attr(e, "color").map_or(0, |c| parse_color(&c)),
    }
}

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
    let mut current_border: Option<BorderFill> = None;
    // hp:switch의 case/default 중복 — 첫 분기(신형 한글이 읽는 값)만 취한다
    let mut para_margin_done = false;
    let mut para_ls_done = false;

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
                        let mut attr_bits = 0u32;
                        if attr(e, "useFontSpace").as_deref() == Some("1") {
                            attr_bits |= 1 << 25;
                        }
                        if attr(e, "useKerning").as_deref() == Some("1") {
                            attr_bits |= 1 << 30;
                        }
                        let cs = CharShape {
                            base_size: attr_i32(e, "height").unwrap_or(1000),
                            text_color: attr(e, "textColor").map_or(0, |c| parse_color(&c)),
                            shade_color: attr(e, "shadeColor")
                                .map_or(0xFFFF_FFFF, |c| parse_color(&c)),
                            ratios: [100; LANG_COUNT],
                            rel_sizes: [100; LANG_COUNT],
                            attr: attr_bits,
                            border_fill_id: attr_u16(e, "borderFillIDRef").unwrap_or(0),
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
                    b"ratio" | b"spacing" | b"relSz" | b"offset" => {
                        if let Some(cs) = &mut current_char {
                            let names = [
                                "hangul", "latin", "hanja", "japanese", "other", "symbol", "user",
                            ];
                            for (i, n) in names.iter().enumerate() {
                                let Some(v) = attr_i32(e, n) else { continue };
                                match e.local_name().as_ref() {
                                    b"ratio" => cs.ratios[i] = v.clamp(1, 255) as u8,
                                    b"spacing" => cs.spacings[i] = v.clamp(-128, 127) as i8,
                                    b"relSz" => cs.rel_sizes[i] = v.clamp(1, 255) as u8,
                                    _ => cs.offsets[i] = v.clamp(-128, 127) as i8,
                                }
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
                    b"underline" => {
                        if let Some(cs) = &mut current_char {
                            // BOTTOM→1, TOP→3 (hwp5 bits 2~3과 동일 인코딩)
                            let kind = match attr(e, "type").as_deref() {
                                Some("NONE") | None => 0u32,
                                Some("TOP") => 3,
                                _ => 1,
                            };
                            cs.attr |= kind << 2;
                            if let Some(c) = attr(e, "color") {
                                cs.underline_color = parse_color(&c);
                            }
                        }
                    }
                    b"strikeout" => {
                        // 취소선 비트(18)는 한글이 "보이는 취소선 선"으로 렌더하는 shape에만
                        // 켠다. shape="3D" 계열은 정품/한글에서 보이지 않게 렌더되는데(사용자
                        // 확인), 우리가 비트18(종류=1 실선)로 매핑하면 인라인 표 폭에 걸친
                        // 실선 취소선이 합성돼 목차에 가로선이 나타난다. NONE/3D는 비취소선
                        // 으로 둔다(시각 결과 동일, SOLID 등 실제 취소선은 보존).
                        if let Some(cs) = &mut current_char {
                            let shape = attr(e, "shape");
                            let visible = matches!(shape.as_deref(), Some(s) if s != "NONE" && !s.contains("3D"));
                            if visible {
                                cs.attr |= 1 << 18;
                                cs.strike = true;
                            }
                        }
                    }
                    b"paraPr" => {
                        // paraPr 자체 속성을 attr1에 인코딩(정품 한글과 일치 — 강제
                        // 0x180 대신 실제 값). snapToGrid=bit8(줄 격자 사용),
                        // condense=bits9-15(공백 최소값), fontLineHeight=bit22.
                        // tabPrIDRef → tab_def_id.
                        let mut ps = ParaShape::default();
                        if attr(e, "snapToGrid").as_deref() == Some("1") {
                            ps.attr1 |= 1 << 8;
                        }
                        if let Some(c) = attr_u32(e, "condense") {
                            ps.attr1 |= (c & 0x7f) << 9;
                        }
                        if attr(e, "fontLineHeight").as_deref() == Some("1") {
                            ps.attr1 |= 1 << 22;
                        }
                        ps.tab_def_id = attr_u16(e, "tabPrIDRef").unwrap_or(0);
                        current_para = Some(ps);
                        para_margin_done = false;
                        para_ls_done = false;
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
                    b"intent" | b"left" | b"right" | b"prev" | b"next" => {
                        if let Some(ps) = &mut current_para
                            && !para_margin_done
                            && let Some(v) = attr_i32(e, "value")
                        {
                            // HWP5 PARA_SHAPE의 여백류 필드는 hwpx HWPUNIT의 2배 단위다
                            // (정품 한글 변환 실측: hwpx left=1500 → hwp5 ml=3000). ×2를
                            // 누락하면 간격이 절반이라 본문이 빽빽해지고 페이지네이션이
                            // 어긋난다. line_spacing(아래)은 2배 아님. IR은 hwp5 단위로 통일.
                            let v2 = v.saturating_mul(2);
                            match e.local_name().as_ref() {
                                b"intent" => ps.indent = v2,
                                b"left" => ps.margin_left = v2,
                                b"right" => ps.margin_right = v2,
                                b"prev" => ps.spacing_top = v2,
                                _ => ps.spacing_bottom = v2,
                            }
                        }
                    }
                    b"lineSpacing" => {
                        if let Some(ps) = &mut current_para
                            && !para_ls_done
                        {
                            para_ls_done = true;
                            let lstype: u8 = match attr(e, "type").as_deref() {
                                Some("FIXED") => 1,
                                Some("BETWEEN_LINES") => 2,
                                Some("AT_LEAST") => 3,
                                _ => 0, // PERCENT
                            };
                            ps.line_spacing_type = lstype;
                            let value = attr_i32(e, "value").unwrap_or(160);
                            // PERCENT(0)는 비율(%) 그대로. 길이 종류(고정/여백만/최소)는
                            // 여백류처럼 hwp5에서 HWPUNIT의 2배 단위(실측: 고정 1432→2864).
                            let stored = if lstype == 0 {
                                value
                            } else {
                                value.saturating_mul(2)
                            };
                            ps.line_spacing = stored;
                            // line_spacing_old(@24)에도 넣는다 — 한글은 이 필드로 문단
                            // 줄간격을 읽으며, 0이면 ensure_para_shape_defaults가 160으로
                            // 덮어써 문단별 줄간격(170/130 등)을 잃고 페이지가 밀린다.
                            ps.line_spacing_old = stored;
                            // 줄간격 종류를 attr1 bits0-1에 인코딩(PERCENT면 0).
                            ps.attr1 |= u32::from(lstype) & 0x3;
                        }
                    }
                    b"breakSetting" => {
                        if let Some(ps) = &mut current_para {
                            // 한글 줄나눔: KEEP_WORD(어절)=bit7=1, BREAK_WORD(글자)=0.
                            // 정품 실측: BREAK_WORD 문단은 글자 단위로 빽빽하게 줄바꿈.
                            if attr(e, "breakNonLatinWord").as_deref() != Some("BREAK_WORD") {
                                ps.attr1 |= 1 << 7;
                            }
                            // 영어 줄나눔: bits5-6 (KEEP_WORD=0, HYPHENATION=1, BREAK_WORD=2).
                            let blat = match attr(e, "breakLatinWord").as_deref() {
                                Some("BREAK_WORD") => 2u32,
                                Some("HYPHENATION") => 1,
                                _ => 0,
                            };
                            ps.attr1 |= blat << 5;
                            if attr(e, "widowOrphan").as_deref() == Some("1") {
                                ps.attr1 |= 1 << 16;
                            }
                            if attr(e, "keepWithNext").as_deref() == Some("1") {
                                ps.attr1 |= 1 << 17;
                            }
                            if attr(e, "keepLines").as_deref() == Some("1") {
                                ps.attr1 |= 1 << 18;
                            }
                            if attr(e, "pageBreakBefore").as_deref() == Some("1") {
                                ps.attr1 |= 1 << 19;
                            }
                        }
                    }
                    b"border" => {
                        // 문단 테두리/배경 참조(borderFillIDRef). 강제값(2) 대신 실제 ID.
                        if let Some(ps) = &mut current_para
                            && let Some(bf) = attr_u16(e, "borderFillIDRef")
                        {
                            ps.border_fill_id = bf;
                        }
                    }
                    b"typeInfo" => {
                        if let Some(slot) = current_lang
                            && let Some(font) = header.fonts[slot].last_mut()
                        {
                            let mut attrs = String::new();
                            for a in e.attributes().flatten() {
                                let k = String::from_utf8_lossy(a.key.local_name().as_ref())
                                    .into_owned();
                                let v = String::from_utf8_lossy(&a.value).into_owned();
                                attrs.push_str(&format!(r#" {k}="{v}""#));
                            }
                            font.type_info = Some(attrs);
                        }
                    }
                    b"borderFill" => {
                        current_border = Some(BorderFill::default());
                        if empty {
                            header
                                .border_fills
                                .push(current_border.take().expect("방금 생성"));
                        }
                    }
                    b"leftBorder" | b"rightBorder" | b"topBorder" | b"bottomBorder" => {
                        if let Some(bf) = &mut current_border {
                            let idx = match e.local_name().as_ref() {
                                b"leftBorder" => 0,
                                b"rightBorder" => 1,
                                b"topBorder" => 2,
                                _ => 3,
                            };
                            bf.sides[idx] = parse_border_line(e);
                        }
                    }
                    b"diagonal" => {
                        if let Some(bf) = &mut current_border {
                            bf.diagonal = parse_border_line(e);
                        }
                    }
                    b"winBrush" => {
                        if let Some(bf) = &mut current_border
                            && let Some(c) = attr(e, "faceColor")
                        {
                            bf.fill_type |= 0x1;
                            bf.bg_color = Some(parse_color(&c));
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
                b"margin" => {
                    if current_para.is_some() {
                        para_margin_done = true;
                    }
                }
                b"charPr" => {
                    if let Some(cs) = current_char.take() {
                        header.char_shapes.push(cs);
                    }
                }
                b"borderFill" => {
                    if let Some(bf) = current_border.take() {
                        header.border_fills.push(bf);
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

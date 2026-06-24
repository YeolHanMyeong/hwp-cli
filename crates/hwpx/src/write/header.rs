//! [`DocHeader`] → `Contents/header.xml`.
//!
//! 기준 표본의 요소 구조를 따른다. 알 수 없는/미보존 속성은 한글 빈 문서
//! 기본값으로 채운다 — 의미 무손실이 아니라 "유효한 문서" 우선.

use std::fmt::Write as _;

use hwp_model::{BorderLine, CharShape, DocHeader, LANG_COUNT, ParaShape};

use crate::write::templates::{FULL_XMLNS, color_attr, esc};

const LANG_NAMES: [&str; LANG_COUNT] = [
    "HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER",
];

fn line_type_name(code: u8) -> &'static str {
    match code {
        0 => "NONE",
        1 => "SOLID",
        2 => "DASH",
        3 => "DOT",
        4 => "DASH_DOT",
        5 => "DASH_DOT_DOT",
        6 => "LONG_DASH",
        7 => "CIRCLE",
        8 => "DOUBLE_SLIM",
        9 => "SLIM_THICK",
        10 => "THICK_SLIM",
        11 => "SLIM_THICK_SLIM",
        _ => "SOLID",
    }
}

fn width_mm_attr(line: &BorderLine) -> String {
    // 0.12 같은 값은 그대로, 정수는 "0.1"이 아닌 표기 유지
    let mm = line.width_mm();
    if (mm - mm.round()).abs() < f32::EPSILON {
        format!("{} mm", mm.round() as i32)
    } else {
        format!("{mm} mm")
    }
}

pub fn write_header(header: &DocHeader, section_count: usize) -> String {
    let mut out = String::with_capacity(32 * 1024);
    let _ = write!(
        out,
        r##"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><hh:head {FULL_XMLNS} version="1.5" secCnt="{section_count}">"##
    );
    let p = &header.properties;
    let _ = write!(
        out,
        r##"<hh:beginNum page="{}" footnote="{}" endnote="{}" pic="{}" tbl="{}" equation="{}"/>"##,
        p.start_numbers[0].max(1),
        p.start_numbers[1].max(1),
        p.start_numbers[2].max(1),
        p.start_numbers[3].max(1),
        p.start_numbers[4].max(1),
        p.start_numbers[5].max(1),
    );
    out.push_str("<hh:refList>");
    write_fontfaces(&mut out, header);
    write_border_fills(&mut out, header);
    write_char_properties(&mut out, header);
    write_tab_properties(&mut out, header);
    write_numberings(&mut out, header);
    write_para_properties(&mut out, header);
    write_styles(&mut out, header);
    out.push_str("</hh:refList>");
    out.push_str(
        r##"<hh:compatibleDocument targetProgram="HWP201X"><hh:layoutCompatibility/></hh:compatibleDocument><hh:docOption><hh:linkinfo path="" pageInherit="0" footnoteInherit="0"/></hh:docOption></hh:head>"##,
    );
    out
}

fn write_fontfaces(out: &mut String, header: &DocHeader) {
    let _ = write!(out, r##"<hh:fontfaces itemCnt="{LANG_COUNT}">"##);
    for (slot, lang) in LANG_NAMES.iter().enumerate() {
        let fonts = &header.fonts[slot];
        if fonts.is_empty() {
            // 빈 슬롯 방어: 기본 글꼴 1종
            let _ = write!(
                out,
                r##"<hh:fontface lang="{lang}" fontCnt="1"><hh:font id="0" face="함초롬바탕" type="TTF" isEmbedded="0"/></hh:fontface>"##
            );
            continue;
        }
        let _ = write!(
            out,
            r##"<hh:fontface lang="{lang}" fontCnt="{}">"##,
            fonts.len()
        );
        for (i, f) in fonts.iter().enumerate() {
            match &f.type_info {
                Some(attrs) => {
                    let _ = write!(
                        out,
                        r##"<hh:font id="{i}" face="{}" type="TTF" isEmbedded="0"><hh:typeInfo{attrs}/></hh:font>"##,
                        esc(&f.name)
                    );
                }
                None => {
                    let _ = write!(
                        out,
                        r##"<hh:font id="{i}" face="{}" type="TTF" isEmbedded="0"/>"##,
                        esc(&f.name)
                    );
                }
            }
        }
        out.push_str("</hh:fontface>");
    }
    out.push_str("</hh:fontfaces>");
}

fn write_border_line(out: &mut String, el: &str, line: &BorderLine) {
    let _ = write!(
        out,
        r##"<hh:{el} type="{}" width="{}" color="{}"/>"##,
        line_type_name(line.line_type),
        width_mm_attr(line),
        if line.color == 0xFFFF_FFFF {
            "#000000".to_string()
        } else {
            color_attr(line.color)
        },
    );
}

fn write_border_fills(out: &mut String, header: &DocHeader) {
    // 참조는 1-기반이므로 최소 2개(기본 무테두리)를 보장
    let count = header.border_fills.len().max(2);
    let _ = write!(out, r##"<hh:borderFills itemCnt="{count}">"##);
    let default_bf = hwp_model::BorderFill {
        diagonal: BorderLine {
            line_type: 1,
            width: 0,
            color: 0,
        },
        ..hwp_model::BorderFill::default()
    };
    for i in 0..count {
        let bf = header.border_fills.get(i).unwrap_or(&default_bf);
        let _ = write!(
            out,
            r##"<hh:borderFill id="{}" threeD="{}" shadow="{}" centerLine="NONE" breakCellSeparateLine="0">"##,
            i + 1,
            bf.attr & 1,
            (bf.attr >> 1) & 1,
        );
        out.push_str(r##"<hh:slash type="NONE" Crooked="0" isCounter="0"/><hh:backSlash type="NONE" Crooked="0" isCounter="0"/>"##);
        for (el, line) in ["leftBorder", "rightBorder", "topBorder", "bottomBorder"]
            .iter()
            .zip(&bf.sides)
        {
            write_border_line(out, el, line);
        }
        write_border_line(out, "diagonal", &bf.diagonal);
        if let Some(bg) = bf.visible_bg() {
            let _ = write!(
                out,
                r##"<hc:fillBrush><hc:winBrush faceColor="{}" hatchColor="#999999" alpha="0"/></hc:fillBrush>"##,
                color_attr(bg)
            );
        }
        out.push_str("</hh:borderFill>");
    }
    out.push_str("</hh:borderFills>");
}

fn per_lang_attrs(values: impl IntoIterator<Item = i32>) -> String {
    let mut s = String::new();
    for (name, v) in [
        "hangul", "latin", "hanja", "japanese", "other", "symbol", "user",
    ]
    .iter()
    .zip(values)
    {
        let _ = write!(s, r##" {name}="{v}""##);
    }
    s
}

fn write_char_properties(out: &mut String, header: &DocHeader) {
    let shapes: Vec<&CharShape> = header.char_shapes.iter().collect();
    let count = shapes.len().max(1);
    let _ = write!(out, r##"<hh:charProperties itemCnt="{count}">"##);
    let default_cs = CharShape {
        base_size: 1000,
        ..CharShape::default()
    };
    for i in 0..count {
        let cs = shapes.get(i).copied().unwrap_or(&default_cs);
        let bf_ref = if cs.border_fill_id > 0 {
            cs.border_fill_id
        } else {
            2
        };
        let _ = write!(
            out,
            r##"<hh:charPr id="{i}" height="{}" textColor="{}" shadeColor="{}" useFontSpace="{}" useKerning="{}" symMark="NONE" borderFillIDRef="{bf_ref}">"##,
            cs.base_size,
            color_attr(cs.text_color),
            color_attr(cs.shade_color),
            (cs.attr >> 25) & 1,
            (cs.attr >> 30) & 1,
        );
        let _ = write!(
            out,
            "<hh:fontRef{}/>",
            per_lang_attrs(cs.face_ids.iter().map(|&v| i32::from(v)))
        );
        let _ = write!(
            out,
            "<hh:ratio{}/>",
            per_lang_attrs(cs.ratios.iter().map(|&v| i32::from(v.max(1))))
        );
        let _ = write!(
            out,
            "<hh:spacing{}/>",
            per_lang_attrs(cs.spacings.iter().map(|&v| i32::from(v)))
        );
        let _ = write!(
            out,
            "<hh:relSz{}/>",
            per_lang_attrs(cs.rel_sizes.iter().map(|&v| i32::from(v.max(1))))
        );
        let _ = write!(
            out,
            "<hh:offset{}/>",
            per_lang_attrs(cs.offsets.iter().map(|&v| i32::from(v)))
        );
        if cs.is_italic() {
            out.push_str("<hh:italic/>");
        }
        if cs.is_bold() {
            out.push_str("<hh:bold/>");
        }
        let ul_type = match cs.underline_kind() {
            1 => "BOTTOM",
            3 => "TOP",
            _ => "NONE",
        };
        let _ = write!(
            out,
            r##"<hh:underline type="{ul_type}" shape="SOLID" color="{}"/>"##,
            if cs.underline_color == 0xFFFF_FFFF {
                "#000000".to_string()
            } else {
                color_attr(cs.underline_color)
            }
        );
        let _ = write!(
            out,
            r##"<hh:strikeout shape="{}" color="#000000"/>"##,
            if cs.has_strike() { "SOLID" } else { "NONE" }
        );
        out.push_str(r##"<hh:outline type="NONE"/><hh:shadow type="NONE" color="#C0C0C0" offsetX="10" offsetY="10"/></hh:charPr>"##);
    }
    out.push_str("</hh:charProperties>");
}

fn write_tab_properties(out: &mut String, header: &DocHeader) {
    let count = header.tab_defs.len().max(1);
    let _ = write!(out, r##"<hh:tabProperties itemCnt="{count}">"##);
    for i in 0..count {
        let _ = write!(
            out,
            r##"<hh:tabPr id="{i}" autoTabLeft="0" autoTabRight="0"/>"##
        );
    }
    out.push_str("</hh:tabProperties>");
}

fn write_numberings(out: &mut String, header: &DocHeader) {
    let count = header.numberings.len().max(1);
    let _ = write!(out, r##"<hh:numberings itemCnt="{count}">"##);
    for i in 0..count {
        let _ = write!(out, r##"<hh:numbering id="{}" start="0">"##, i + 1);
        for level in 1..=7 {
            let _ = write!(
                out,
                r##"<hh:paraHead start="1" level="{level}" align="LEFT" useInstWidth="1" autoIndent="1" widthAdjust="0" textOffsetType="PERCENT" textOffset="50" numFormat="DIGIT" charPrIDRef="4294967295" checkable="0">^{level}.</hh:paraHead>"##
            );
        }
        out.push_str("</hh:numbering>");
    }
    out.push_str("</hh:numberings>");
}

fn write_para_properties(out: &mut String, header: &DocHeader) {
    let count = header.para_shapes.len().max(1);
    let _ = write!(out, r##"<hh:paraProperties itemCnt="{count}">"##);
    let default_ps = ParaShape::default();
    let tab_count = header.tab_defs.len().max(1);
    for i in 0..count {
        let ps = header.para_shapes.get(i).unwrap_or(&default_ps);
        let align = match ps.alignment() {
            1 => "LEFT",
            2 => "RIGHT",
            3 => "CENTER",
            4 => "DISTRIBUTE",
            5 => "DISTRIBUTE_SPACE",
            _ => "JUSTIFY",
        };
        let tab_ref = (ps.tab_def_id as usize).min(tab_count - 1);
        let ls_type = match ps.line_spacing_type {
            1 => "FIXED",
            2 => "BETWEEN_LINES",
            3 => "AT_LEAST",
            _ => "PERCENT",
        };
        // IR 줄간격: PERCENT는 비율 그대로, 길이 종류(고정/여백만/최소)는 2배 단위라
        // ÷2 해서 HWPUNIT로 환원(hwpx 읽기의 ×2와 대칭).
        let ls_raw = if ps.line_spacing > 0 {
            ps.line_spacing
        } else {
            160
        };
        let ls_value = if ps.line_spacing_type == 0 {
            ls_raw
        } else {
            ls_raw / 2
        };
        // IR 여백류는 hwp5 PARA_SHAPE 단위(HWPUNIT의 2배)다. hwpx는 HWPUNIT이므로
        // ÷2 해서 내보낸다(hwpx 읽기의 ×2와 대칭 — hwpx 왕복 보존).
        let _ = write!(
            out,
            r##"<hh:paraPr id="{i}" tabPrIDRef="{tab_ref}" condense="0" fontLineHeight="0" snapToGrid="1" suppressLineNumbers="0" checked="0" textDir="LTR"><hh:align horizontal="{align}" vertical="BASELINE"/><hh:heading type="NONE" idRef="0" level="0"/><hh:breakSetting breakLatinWord="KEEP_WORD" breakNonLatinWord="BREAK_WORD" widowOrphan="0" keepWithNext="0" keepLines="0" pageBreakBefore="0" lineWrap="BREAK"/><hh:autoSpacing eAsianEng="0" eAsianNum="0"/><hh:margin><hc:intent value="{}" unit="HWPUNIT"/><hc:left value="{}" unit="HWPUNIT"/><hc:right value="{}" unit="HWPUNIT"/><hc:prev value="{}" unit="HWPUNIT"/><hc:next value="{}" unit="HWPUNIT"/></hh:margin><hh:lineSpacing type="{ls_type}" value="{ls_value}" unit="HWPUNIT"/><hh:border borderFillIDRef="2" offsetLeft="0" offsetRight="0" offsetTop="0" offsetBottom="0" connect="0" ignoreMargin="0"/></hh:paraPr>"##,
            ps.indent / 2,
            ps.margin_left / 2,
            ps.margin_right / 2,
            ps.spacing_top / 2,
            ps.spacing_bottom / 2,
        );
    }
    out.push_str("</hh:paraProperties>");
}

fn write_styles(out: &mut String, header: &DocHeader) {
    let para_count = header.para_shapes.len().max(1);
    let char_count = header.char_shapes.len().max(1);
    if header.styles.is_empty() {
        out.push_str(
            r##"<hh:styles itemCnt="1"><hh:style id="0" type="PARA" name="바탕글" engName="Normal" paraPrIDRef="0" charPrIDRef="0" nextStyleIDRef="0" langID="1042" lockForm="0"/></hh:styles>"##,
        );
        return;
    }
    let _ = write!(out, r##"<hh:styles itemCnt="{}">"##, header.styles.len());
    for (i, s) in header.styles.iter().enumerate() {
        let _ = write!(
            out,
            r##"<hh:style id="{i}" type="PARA" name="{}" engName="{}" paraPrIDRef="{}" charPrIDRef="{}" nextStyleIDRef="{}" langID="{}" lockForm="0"/>"##,
            esc(&s.name),
            esc(&s.english_name),
            (s.para_shape.0 as usize).min(para_count - 1),
            (s.char_shape.0 as usize).min(char_count - 1),
            s.next_style,
            if s.lang_id > 0 { s.lang_id } else { 1042 },
        );
    }
    out.push_str("</hh:styles>");
}

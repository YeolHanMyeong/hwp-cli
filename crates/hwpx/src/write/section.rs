//! [`Section`] → `Contents/sectionN.xml`.
//!
//! 런 상태 기계: 문자 모양 경계에서 `<hp:run>`을 전환하며 텍스트를
//! 흘려보내고, 확장 컨트롤 위치에서 표/그림/머리말 등을 직렬화한다.
//! 미지원 컨트롤(글상자 등)은 드롭하되 경고로 집계한다.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use hwp_model::{
    BinRef, Cell, Control, Document, GenericControl, HwpChar, PageDef, Paragraph, Picture, Section,
    Table,
};

use crate::write::templates::esc;

/// 동봉할 바이너리(이미지) 수집기.
#[derive(Default)]
pub struct BinCollector {
    /// (item id, href, mime, bytes)
    pub items: Vec<(String, String, String, Vec<u8>)>,
}

impl BinCollector {
    /// BinRef를 해석해 패키지 항목으로 등록하고 item id를 돌려준다.
    fn register(&mut self, doc: &Document, bin_ref: &BinRef) -> Option<String> {
        let bytes = doc.resolve_bin(bin_ref)?.to_vec();
        // 같은 바이트는 재사용
        if let Some((id, ..)) = self.items.iter().find(|(.., b)| *b == bytes) {
            return Some(id.clone());
        }
        let (ext, mime) = sniff(&bytes);
        let id = format!("image{}", self.items.len() + 1);
        let href = format!("BinData/{id}.{ext}");
        self.items.push((id.clone(), href, mime.to_string(), bytes));
        Some(id)
    }
}

fn sniff(data: &[u8]) -> (&'static str, &'static str) {
    match data {
        [0x89, b'P', b'N', b'G', ..] => ("png", "image/png"),
        [0xFF, 0xD8, ..] => ("jpg", "image/jpeg"),
        [b'G', b'I', b'F', b'8', ..] => ("gif", "image/gif"),
        [b'B', b'M', ..] => ("bmp", "image/bmp"),
        _ => ("bin", "application/octet-stream"),
    }
}

pub fn write_section(
    doc: &Document,
    section: &Section,
    preserve_linesegs: bool,
    bins: &mut BinCollector,
    warnings: &mut Vec<String>,
) -> String {
    let mut out = String::with_capacity(16 * 1024);
    out.push_str(
        r##"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><hs:sec xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core">"##,
    );
    let mut ids = IdSeq::default();
    for (pi, para) in section.paragraphs.iter().enumerate() {
        // 첫 문단에 구역 정의가 없으면 기본 secPr 주입
        let inject = pi == 0
            && !para
                .controls
                .iter()
                .any(|c| matches!(c, Control::SectionDef(_)));
        write_paragraph(
            &mut out,
            doc,
            para,
            &mut ids,
            bins,
            inject,
            preserve_linesegs,
            warnings,
        );
    }
    out.push_str("</hs:sec>");
    out
}

#[derive(Default)]
struct IdSeq(u32);

impl IdSeq {
    fn next(&mut self) -> u32 {
        self.0 += 1;
        self.0
    }
}

/// 문단 하나를 직렬화한다. `inject_secpr`이면 첫 런에 기본 구역 정의를 넣는다.
#[allow(clippy::too_many_arguments)]
fn write_paragraph(
    out: &mut String,
    doc: &Document,
    para: &Paragraph,
    ids: &mut IdSeq,
    bins: &mut BinCollector,
    inject_secpr: bool,
    preserve_linesegs: bool,
    warnings: &mut Vec<String>,
) {
    let _ = write!(
        out,
        r##"<hp:p id="{}" paraPrIDRef="{}" styleIDRef="{}" pageBreak="{}" columnBreak="{}" merged="0">"##,
        ids.next(),
        para.para_shape.0,
        para.style.0,
        u8::from(para.header.break_type & 0x04 != 0),
        u8::from(para.header.break_type & 0x08 != 0),
    );

    let first_shape = para.char_shape_runs.first().map_or(0, |(_, id)| id.0);
    let mut run_open = false;
    let mut cur_shape = first_shape;
    let mut text_buf = String::new();
    let mut wchar_pos = 0u32;
    let mut emitted_any_run = false;

    macro_rules! open_run {
        ($shape:expr) => {
            if !run_open || cur_shape != $shape {
                if run_open {
                    flush_text(out, &mut text_buf);
                    out.push_str("</hp:run>");
                }
                let _ = write!(out, r##"<hp:run charPrIDRef="{}">"##, $shape);
                run_open = true;
                emitted_any_run = true;
                cur_shape = $shape;
            }
        };
    }

    if inject_secpr {
        open_run!(first_shape);
        write_default_sec_pr(out, None);
        write_col_ctrl(out);
    }

    for ch in &para.chars {
        match ch {
            HwpChar::Text(c) => {
                let shape = shape_id_at(para, wchar_pos);
                open_run!(shape);
                text_buf.push(*c);
            }
            HwpChar::CharCtrl(code) => match *code {
                10 => {
                    open_run!(cur_shape);
                    flush_text(out, &mut text_buf);
                    out.push_str("<hp:lineBreak/>");
                }
                24 => text_buf.push('-'),
                30 => text_buf.push('\u{00A0}'),
                31 => text_buf.push(' '),
                _ => {}
            },
            HwpChar::InlineCtrl { code, .. } => {
                if *code == 9 {
                    open_run!(cur_shape);
                    flush_text(out, &mut text_buf);
                    out.push_str("<hp:tab/>");
                }
            }
            HwpChar::ExtCtrl { ctrl_index, .. } => {
                let Some(control) = ctrl_index.and_then(|i| para.controls.get(i as usize)) else {
                    wchar_pos += ch.wchar_width();
                    continue;
                };
                match control {
                    Control::SectionDef(def) => {
                        open_run!(cur_shape);
                        flush_text(out, &mut text_buf);
                        write_default_sec_pr(out, def.page.as_ref());
                    }
                    Control::Generic(g) if g.ctrl_id == *b"cold" => {
                        open_run!(cur_shape);
                        flush_text(out, &mut text_buf);
                        write_col_ctrl(out);
                    }
                    Control::Generic(g) if g.ctrl_id == *b"head" || g.ctrl_id == *b"foot" => {
                        open_run!(cur_shape);
                        flush_text(out, &mut text_buf);
                        write_header_footer(out, doc, g, ids, bins, preserve_linesegs, warnings);
                    }
                    Control::Table(table) => {
                        open_run!(cur_shape);
                        flush_text(out, &mut text_buf);
                        write_table(out, doc, table, ids, bins, preserve_linesegs, warnings);
                    }
                    Control::Picture(pic) => {
                        open_run!(cur_shape);
                        flush_text(out, &mut text_buf);
                        write_picture(out, doc, pic, ids, bins, warnings);
                    }
                    Control::Generic(g) => {
                        warnings.push(format!(
                            "hwpx 쓰기 미지원 컨트롤 드롭: {:?}",
                            String::from_utf8_lossy(&g.ctrl_id)
                        ));
                    }
                }
            }
        }
        wchar_pos += ch.wchar_width();
    }

    if run_open {
        flush_text(out, &mut text_buf);
        out.push_str("</hp:run>");
    }
    // 줄 배치 정보 보존 (무수정 왕복 전용 — 기본은 제거, 한글이 재계산)
    if preserve_linesegs && !para.line_segs.is_empty() {
        out.push_str("<hp:linesegarray>");
        for seg in &para.line_segs {
            let _ = write!(
                out,
                r##"<hp:lineseg textpos="{}" vertpos="{}" vertsize="{}" textheight="{}" baseline="{}" spacing="{}" horzpos="{}" horzsize="{}" flags="{}"/>"##,
                seg.text_start,
                seg.v_pos,
                seg.line_height,
                seg.text_height,
                seg.baseline_gap,
                seg.line_spacing,
                seg.col_start,
                seg.seg_width,
                seg.flags,
            );
        }
        out.push_str("</hp:linesegarray>");
    }
    if !emitted_any_run {
        // 빈 문단도 런 하나는 가져야 한다 (기준 표본 패턴)
        let _ = write!(
            out,
            r##"<hp:run charPrIDRef="{first_shape}"><hp:t/></hp:run>"##
        );
    }
    out.push_str("</hp:p>");
}

fn flush_text(out: &mut String, buf: &mut String) {
    if !buf.is_empty() {
        let _ = write!(out, r##"<hp:t xml:space="preserve">{}</hp:t>"##, esc(buf));
        buf.clear();
    }
}

fn shape_id_at(para: &Paragraph, pos: u32) -> u16 {
    para.char_shape_runs
        .iter()
        .rev()
        .find(|(start, _)| *start <= pos)
        .map(|(_, id)| id.0)
        .unwrap_or(0)
}

/// 기본 A4 PageDef (구역 정의가 없는 문서 방어).
fn default_page() -> PageDef {
    PageDef {
        width: hwp_model::HwpUnit(59528),
        height: hwp_model::HwpUnit(84186),
        margin_left: hwp_model::HwpUnit(8504),
        margin_right: hwp_model::HwpUnit(8504),
        margin_top: hwp_model::HwpUnit(5668),
        margin_bottom: hwp_model::HwpUnit(4252),
        margin_header: hwp_model::HwpUnit(4252),
        margin_footer: hwp_model::HwpUnit(4252),
        gutter: hwp_model::HwpUnit(0),
        attr: 0,
    }
}

fn write_default_sec_pr(out: &mut String, page: Option<&PageDef>) {
    let fallback = default_page();
    let p = page.unwrap_or(&fallback);
    let landscape = if p.attr & 1 != 0 {
        "NARROWLY"
    } else {
        "WIDELY"
    };
    let _ = write!(
        out,
        r##"<hp:secPr id="" textDirection="HORIZONTAL" spaceColumns="1134" tabStop="8000" tabStopVal="4000" tabStopUnit="HWPUNIT" outlineShapeIDRef="1" memoShapeIDRef="0" textVerticalWidthHead="0" masterPageCnt="0"><hp:grid lineGrid="0" charGrid="0" wonggojiFormat="0"/><hp:startNum pageStartsOn="BOTH" page="0" pic="0" tbl="0" equation="0"/><hp:visibility hideFirstHeader="0" hideFirstFooter="0" hideFirstMasterPage="0" border="SHOW_ALL" fill="SHOW_ALL" hideFirstPageNum="0" hideFirstEmptyLine="0" showLineNumber="0"/><hp:lineNumberShape restartType="0" countBy="0" distance="0" startNumber="0"/><hp:pagePr landscape="{landscape}" width="{}" height="{}" gutterType="LEFT_ONLY"><hp:margin header="{}" footer="{}" gutter="{}" left="{}" right="{}" top="{}" bottom="{}"/></hp:pagePr><hp:footNotePr><hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar=")" supscript="0"/><hp:noteLine length="-1" type="SOLID" width="0.12 mm" color="#000000"/><hp:noteSpacing betweenNotes="283" belowLine="567" aboveLine="850"/><hp:numbering type="CONTINUOUS" newNum="1"/><hp:placement place="EACH_COLUMN" beneathText="0"/></hp:footNotePr><hp:endNotePr><hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar=")" supscript="0"/><hp:noteLine length="14692344" type="SOLID" width="0.12 mm" color="#000000"/><hp:noteSpacing betweenNotes="0" belowLine="567" aboveLine="850"/><hp:numbering type="CONTINUOUS" newNum="1"/><hp:placement place="END_OF_DOCUMENT" beneathText="0"/></hp:endNotePr><hp:pageBorderFill type="BOTH" borderFillIDRef="1" textBorder="PAPER" headerInside="0" footerInside="0" fillArea="PAPER"><hp:offset left="1417" right="1417" top="1417" bottom="1417"/></hp:pageBorderFill><hp:pageBorderFill type="EVEN" borderFillIDRef="1" textBorder="PAPER" headerInside="0" footerInside="0" fillArea="PAPER"><hp:offset left="1417" right="1417" top="1417" bottom="1417"/></hp:pageBorderFill><hp:pageBorderFill type="ODD" borderFillIDRef="1" textBorder="PAPER" headerInside="0" footerInside="0" fillArea="PAPER"><hp:offset left="1417" right="1417" top="1417" bottom="1417"/></hp:pageBorderFill></hp:secPr>"##,
        p.width.0,
        p.height.0,
        p.margin_header.0,
        p.margin_footer.0,
        p.gutter.0,
        p.margin_left.0,
        p.margin_right.0,
        p.margin_top.0,
        p.margin_bottom.0,
    );
}

fn write_col_ctrl(out: &mut String) {
    out.push_str(
        r##"<hp:ctrl><hp:colPr id="" type="NEWSPAPER" layout="LEFT" colCount="1" sameSz="1" sameGap="0"/></hp:ctrl>"##,
    );
}

#[allow(clippy::too_many_arguments)]
fn write_header_footer(
    out: &mut String,
    doc: &Document,
    g: &GenericControl,
    ids: &mut IdSeq,
    bins: &mut BinCollector,
    preserve_linesegs: bool,
    warnings: &mut Vec<String>,
) {
    let el = if g.ctrl_id == *b"head" {
        "header"
    } else {
        "footer"
    };
    let _ = write!(
        out,
        r##"<hp:ctrl><hp:{el} id="{}" applyPageType="BOTH">"##,
        ids.next()
    );
    for list in &g.paragraph_lists {
        out.push_str(
            r##"<hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0">"##,
        );
        for para in &list.paragraphs {
            write_paragraph(
                out,
                doc,
                para,
                ids,
                bins,
                false,
                preserve_linesegs,
                warnings,
            );
        }
        out.push_str("</hp:subList>");
    }
    let _ = write!(out, "</hp:{el}></hp:ctrl>");
}

#[allow(clippy::too_many_arguments)]
fn write_table(
    out: &mut String,
    doc: &Document,
    table: &Table,
    ids: &mut IdSeq,
    bins: &mut BinCollector,
    preserve_linesegs: bool,
    warnings: &mut Vec<String>,
) {
    let cols = table.cols.max(1) as usize;
    let rows = table.rows.max(1) as usize;
    // 그리드 추정 (렌더러와 동일 규칙)
    let mut col_w = vec![0i64; cols];
    let mut row_h = vec![0i64; rows];
    for cell in &table.cells {
        let (c, r) = (cell.col as usize, cell.row as usize);
        if cell.col_span == 1 && c < cols {
            col_w[c] = col_w[c].max(i64::from(cell.width.0));
        }
        if cell.row_span == 1 && r < rows {
            row_h[r] = row_h[r].max(i64::from(cell.height.0));
        }
    }
    let total_w: i64 = col_w.iter().sum();
    let total_h: i64 = row_h.iter().sum();

    let m = table.inner_margins;
    let _ = write!(
        out,
        r##"<hp:tbl id="{}" zOrder="0" numberingType="TABLE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" pageBreak="CELL" repeatHeader="1" rowCnt="{}" colCnt="{}" cellSpacing="{}" borderFillIDRef="{}" noAdjust="0"><hp:sz width="{total_w}" widthRelTo="ABSOLUTE" height="{total_h}" heightRelTo="ABSOLUTE" protect="0"/><hp:pos treatAsChar="1" affectLSpacing="0" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="PARA" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/><hp:outMargin left="283" right="283" top="283" bottom="283"/><hp:inMargin left="{}" right="{}" top="{}" bottom="{}"/>"##,
        ids.next(),
        table.rows,
        table.cols,
        table.cell_spacing,
        table.border_fill.0.max(1),
        m[0],
        m[1],
        m[2],
        m[3],
    );

    // 행별 그룹화 (셀은 행 우선 순서로 보존되어 있음)
    let mut by_row: BTreeMap<u16, Vec<&Cell>> = BTreeMap::new();
    for cell in &table.cells {
        by_row.entry(cell.row).or_default().push(cell);
    }
    for (_, cells) in by_row {
        out.push_str("<hp:tr>");
        for cell in cells {
            let _ = write!(
                out,
                r##"<hp:tc name="" header="0" hasMargin="0" protect="0" editable="0" dirty="0" borderFillIDRef="{}"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="CENTER" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0">"##,
                cell.border_fill.0.max(1),
            );
            for para in &cell.paragraphs {
                write_paragraph(
                    out,
                    doc,
                    para,
                    ids,
                    bins,
                    false,
                    preserve_linesegs,
                    warnings,
                );
            }
            let cm = cell.margins;
            let _ = write!(
                out,
                r##"</hp:subList><hp:cellAddr colAddr="{}" rowAddr="{}"/><hp:cellSpan colSpan="{}" rowSpan="{}"/><hp:cellSz width="{}" height="{}"/><hp:cellMargin left="{}" right="{}" top="{}" bottom="{}"/></hp:tc>"##,
                cell.col,
                cell.row,
                cell.col_span,
                cell.row_span,
                cell.width.0,
                cell.height.0,
                cm[0],
                cm[1],
                cm[2],
                cm[3],
            );
        }
        out.push_str("</hp:tr>");
    }
    out.push_str("</hp:tbl>");
}

fn write_picture(
    out: &mut String,
    doc: &Document,
    pic: &Picture,
    ids: &mut IdSeq,
    bins: &mut BinCollector,
    warnings: &mut Vec<String>,
) {
    let Some(item) = bins.register(doc, &pic.bin_ref) else {
        warnings.push(format!("그림 데이터를 찾지 못해 드롭: {:?}", pic.bin_ref));
        return;
    };
    let (w, h) = (pic.width.0.max(1), pic.height.0.max(1));
    let id = ids.next();
    let _ = write!(
        out,
        r##"<hp:pic id="{id}" zOrder="0" numberingType="PICTURE" textWrap="SQUARE" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="0" instid="{id}" reverse="0"><hp:offset x="0" y="0"/><hp:orgSz width="{w}" height="{h}"/><hp:curSz width="{w}" height="{h}"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="{}" centerY="{}" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo><hc:img binaryItemIDRef="{item}" bright="0" contrast="0" effect="REAL_PIC" alpha="0"/><hp:imgRect><hc:pt0 x="0" y="0"/><hc:pt1 x="{w}" y="0"/><hc:pt2 x="{w}" y="{h}"/><hc:pt3 x="0" y="{h}"/></hp:imgRect><hp:imgClip left="0" right="{w}" top="0" bottom="{h}"/><hp:inMargin left="0" right="0" top="0" bottom="0"/><hp:imgDim dimwidth="{w}" dimheight="{h}"/><hp:sz width="{w}" widthRelTo="ABSOLUTE" height="{h}" heightRelTo="ABSOLUTE" protect="0"/><hp:pos treatAsChar="{}" affectLSpacing="0" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="PARA" vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/><hp:outMargin left="0" right="0" top="0" bottom="0"/></hp:pic>"##,
        w / 2,
        h / 2,
        u8::from(pic.treat_as_char),
    );
}

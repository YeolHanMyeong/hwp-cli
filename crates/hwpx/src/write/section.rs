//! [`Section`] → `Contents/sectionN.xml`.
//!
//! 런 상태 기계: 문자 모양 경계에서 `<hp:run>`을 전환하며 텍스트를
//! 흘려보내고, 확장 컨트롤 위치에서 표/그림/머리말 등을 직렬화한다.
//! 미지원 컨트롤(글상자 등)은 드롭하되 경고로 집계한다.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use hwp_model::{
    BinRef, Cell, Control, Document, GenericControl, HwpChar, PageDef, Paragraph, Picture, Section,
    ShapeKind, Table,
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

/// 한 run에 방출할 수 있는 그리기 도형의 최대 수. 한글은 run당 앞쪽 ~21개 도형만
/// 렌더하고 나머지를 버리므로(실기 확정), 여유를 두고 이 수를 넘으면 run을 분할한다.
const SHAPE_RUN_LIMIT: usize = 12;

/// 방출된 XML 조각에서 최상위 그리기 도형 요소 수를 센다. `<hp:line `은 뒤에 공백을 둬
/// `<hp:lineShape`·`<hp:lineseg`·`<hp:lineBreak`와 구분한다(도형 요소는 항상 속성이 따름).
fn count_shape_tags(s: &str) -> usize {
    const OPENS: [&str; 8] = [
        "<hp:rect ",
        "<hp:ellipse ",
        "<hp:line ",
        "<hp:arc ",
        "<hp:polygon ",
        "<hp:curve ",
        "<hp:pic ",
        "<hp:connectLine ",
    ];
    OPENS.iter().map(|t| s.matches(t).count()).sum()
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
    // 열려 있는 필드(FIELD_START)의 id — FIELD_END의 beginIDRef로 연결(필드 비중첩 가정).
    let mut current_field_id: Option<u32> = None;
    // 현재 run에 방출한 그리기 도형 수 — 한글은 run당 앞쪽 ~21개만 그리고 나머지를 버린다
    // (annual 6쪽 링 미렌더 실기 확정: 도형 35개/run → 22번째 이후 타원 전부 누락). 한계
    // 전에 run을 강제 분할해 모든 도형이 렌더되게 한다.
    let mut run_shapes = 0usize;

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
                run_shapes = 0;
            }
        };
    }

    // 도형 방출 전 호출: 현재 run이 도형 한계에 다다르면 같은 char_shape로 run을 새로 연다.
    macro_rules! shape_break {
        () => {
            if run_open && run_shapes >= SHAPE_RUN_LIMIT {
                out.push_str("</hp:run>");
                let _ = write!(out, r##"<hp:run charPrIDRef="{}">"##, cur_shape);
                run_shapes = 0;
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
            HwpChar::InlineCtrl { code, .. } => match *code {
                9 => {
                    open_run!(cur_shape);
                    flush_text(out, &mut text_buf);
                    out.push_str("<hp:tab/>");
                }
                4 => {
                    // FIELD_END — 앞의 fieldBegin과 beginIDRef로 연결.
                    if let Some(fid) = current_field_id.take() {
                        open_run!(cur_shape);
                        flush_text(out, &mut text_buf);
                        let _ = write!(
                            out,
                            r##"<hp:ctrl><hp:fieldEnd beginIDRef="{fid}" fieldid="{fid}"/></hp:ctrl>"##,
                        );
                    }
                }
                _ => {}
            },
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
                        shape_break!();
                        let before = out.len();
                        write_picture(out, doc, pic, ids, bins, warnings);
                        run_shapes += count_shape_tags(&out[before..]);
                    }
                    Control::Generic(g) if hwp_convert::field::is_field_ctrl_id(&g.ctrl_id) => {
                        // 필드(누름틀·계산식·하이퍼링크 등) — fieldBegin 방출. 값 텍스트는
                        // 뒤따르는 Text가 <hp:t>로, FIELD_END(InlineCtrl 4)가 fieldEnd로 닫는다.
                        open_run!(cur_shape);
                        flush_text(out, &mut text_buf);
                        let (name, command) = hwp_convert::field::field_meta(control);
                        let fid = ids.next();
                        current_field_id = Some(fid);
                        let ty = hwp_convert::field::owpml_field_type(&g.ctrl_id);
                        let _ = write!(
                            out,
                            r##"<hp:ctrl><hp:fieldBegin id="{fid}" type="{ty}" name="{}" editable="1" dirty="0" zorder="-1" fieldid="{fid}" metaTag="""##,
                            esc(name.as_deref().unwrap_or("")),
                        );
                        if let Some(cmd) = &command {
                            let _ = write!(
                                out,
                                r##"><hp:parameters cnt="1" name=""><hp:stringParam name="Command">{}</hp:stringParam></hp:parameters></hp:fieldBegin></hp:ctrl>"##,
                                esc(cmd),
                            );
                        } else {
                            out.push_str("/></hp:ctrl>");
                        }
                    }
                    Control::Generic(g) if g.ctrl_id == *b"bokm" => {
                        // 책갈피(지점 표식) — <hp:bookmark name="…"/>. 필드와 달리 END 없음.
                        open_run!(cur_shape);
                        flush_text(out, &mut text_buf);
                        let name =
                            hwp_convert::bookmark::bookmark_name(control).unwrap_or_default();
                        let _ = write!(
                            out,
                            r##"<hp:ctrl><hp:bookmark name="{}"/></hp:ctrl>"##,
                            esc(&name)
                        );
                    }
                    Control::Generic(g) if g.ctrl_id == *b"pgnp" && g.data.len() >= 12 => {
                        // 쪽번호 위치 — reader build_pgnp의 역(12B: props[format|pos<<8] +
                        // 6B 0 + side_char u16). 정답지: 한글 export <hp:pageNum>.
                        open_run!(cur_shape);
                        flush_text(out, &mut text_buf);
                        let props =
                            u32::from_le_bytes([g.data[0], g.data[1], g.data[2], g.data[3]]);
                        let side = u16::from_le_bytes([g.data[10], g.data[11]]);
                        let side_s = char::from_u32(u32::from(side))
                            .filter(|c| *c != '\0')
                            .map(String::from)
                            .unwrap_or_default();
                        let _ = write!(
                            out,
                            r##"<hp:ctrl><hp:pageNum pos="{}" formatType="DIGIT" sideChar="{}"/></hp:ctrl>"##,
                            page_num_pos_name(((props >> 8) & 0xFF) as u8),
                            esc(&side_s),
                        );
                    }
                    Control::Generic(g) if g.ctrl_id == *b"pghd" && g.data.len() >= 4 => {
                        // 쪽 감추기 — reader build_pghd의 역(4B 비트맵).
                        open_run!(cur_shape);
                        flush_text(out, &mut text_buf);
                        let mask = u32::from_le_bytes([g.data[0], g.data[1], g.data[2], g.data[3]]);
                        let b = |bit: u32| u8::from(mask & (1 << bit) != 0);
                        let _ = write!(
                            out,
                            r##"<hp:ctrl><hp:pageHiding hideHeader="{}" hideFooter="{}" hideMasterPage="{}" hideBorder="{}" hideFill="{}" hidePageNum="{}"/></hp:ctrl>"##,
                            b(0),
                            b(1),
                            b(2),
                            b(3),
                            b(4),
                            b(5),
                        );
                    }
                    Control::Generic(g) if g.ctrl_id == *b"nwno" && g.data.len() >= 6 => {
                        // 새 번호 지정 — reader build_nwno의 역(종류 u32 + num u16).
                        open_run!(cur_shape);
                        flush_text(out, &mut text_buf);
                        let num = u16::from_le_bytes([g.data[4], g.data[5]]);
                        let _ = write!(
                            out,
                            r##"<hp:ctrl><hp:newNum num="{num}" numType="PAGE"/></hp:ctrl>"##,
                        );
                    }
                    Control::Generic(g) if g.ctrl_id == *b"atno" => {
                        // 자동 번호(쪽) — 코퍼스 export에 인라인 정답지가 없어 표준형으로
                        // 방출(v1). 페이로드는 reader build_atno가 실측 표준 12B로 복원.
                        open_run!(cur_shape);
                        flush_text(out, &mut text_buf);
                        out.push_str(r##"<hp:ctrl><hp:autoNum numType="PAGE"/></hp:ctrl>"##);
                    }
                    Control::Generic(g) if !g.gso_shapes.is_empty() => {
                        // hwpx-출신 구조화 도형(rect/ellipse/line/…) — ShapeGeom 재직렬화.
                        open_run!(cur_shape);
                        flush_text(out, &mut text_buf);
                        shape_break!();
                        let before = out.len();
                        write_ir_shapes(out, doc, g, ids, bins, preserve_linesegs, warnings);
                        run_shapes += count_shape_tags(&out[before..]);
                    }
                    Control::Generic(g) if g.ctrl_id == *b"gso " => {
                        // hwp5-출신 gso: 글상자(rect+drawText — 텍스트/필드/책갈피 보존)와
                        // 장식 도형(SHAPE_COMPONENT → 도형 요소) 모두 방출.
                        open_run!(cur_shape);
                        flush_text(out, &mut text_buf);
                        shape_break!();
                        let before = out.len();
                        write_gso(out, doc, g, ids, bins, preserve_linesegs, warnings);
                        run_shapes += count_shape_tags(&out[before..]);
                    }
                    Control::Generic(g) => {
                        warnings.push(format!(
                            "DROP: hwpx 쓰기 미지원 컨트롤 드롭: {:?}",
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

/// hwp5 gso 공통 개체 헤더(20B+): attr(u32)@0, 세로 오프셋@4, 가로 오프셋@8, 폭@12, 높이@16,
/// **z-order@20**. hwp5 `parse_picture_gso`/hwp-render `parse_gso_box`와 동일 레이아웃(역의존
/// 불가라 로컬 복제). z-order는 도형 겹침 순서 — 이를 `zOrder="0"`로 뭉개면 한글이 다중 도형을
/// undefined 순서로 그려 덮개 도형이 내용을 가린다(annual 표지 빈 화면 원인). 헤더가 짧아
/// z-order가 없으면 0.
fn parse_gso_header(data: &[u8]) -> Option<(u32, i32, i32, i32, i32, i32)> {
    if data.len() < 20 {
        return None;
    }
    let rd = |o: usize| i32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
    let zorder = if data.len() >= 24 { rd(20) } else { 0 };
    Some((rd(0) as u32, rd(4), rd(8), rd(12), rd(16), zorder))
}

/// COLORREF(0x00BBGGRR) → "#RRGGBB" (reader `parse_color`의 역).
fn color_hex(c: u32) -> String {
    format!(
        "#{:02X}{:02X}{:02X}",
        c & 0xFF,
        (c >> 8) & 0xFF,
        (c >> 16) & 0xFF
    )
}

// gso 배치/선 스타일 코드 → OWPML 이름 (reader의 vert_rel_to_code/line_style_code 등의 역).
fn vert_rel_to_name(code: u8) -> &'static str {
    match code {
        1 => "PAGE",
        2 => "PARA",
        _ => "PAPER",
    }
}
fn horz_rel_to_name(code: u8) -> &'static str {
    match code {
        1 => "PAGE",
        2 => "COLUMN",
        3 => "PARA",
        _ => "PAPER",
    }
}
fn vert_align_name(code: u8) -> &'static str {
    match code {
        1 => "CENTER",
        2 => "BOTTOM",
        _ => "TOP",
    }
}
fn horz_align_name(code: u8) -> &'static str {
    match code {
        1 => "CENTER",
        2 => "RIGHT",
        _ => "LEFT",
    }
}
fn line_style_name(code: u8) -> &'static str {
    match code {
        1 => "DASH",
        2 => "DOT",
        3 => "DASH_DOT",
        4 => "DASH_DOT_DOT",
        5 => "LONG_DASH",
        _ => "SOLID",
    }
}
fn arrow_name(code: u8) -> &'static str {
    if code == 0 { "NORMAL" } else { "ARROW" }
}

/// 개체 공통 자식(offset/orgSz/curSz/flip/rotationInfo/단위행렬) — 정품 line/pic 스캐폴드 복제.
fn write_obj_scaffold(out: &mut String, w: i32, h: i32, cur_w: i32, cur_h: i32) {
    let _ = write!(
        out,
        r##"<hp:offset x="0" y="0"/><hp:orgSz width="{w}" height="{h}"/><hp:curSz width="{cur_w}" height="{cur_h}"/><hp:flip horizontal="0" vertical="0"/><hp:rotationInfo angle="0" centerX="{}" centerY="{}" rotateimage="1"/><hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:scaMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo>"##,
        w / 2,
        h / 2,
    );
}

/// 글상자 텍스트: `<hp:drawText><hp:subList>문단들</hp:subList></hp:drawText>`.
/// 모든 paragraph_lists를 하나의 subList로 병합(다단 글상자 v1 근사 — 텍스트 무손실).
/// 필드/책갈피는 write_paragraph 안의 arm이 fieldBegin/bookmark로 함께 방출한다.
#[allow(clippy::too_many_arguments)]
fn write_draw_text(
    out: &mut String,
    doc: &Document,
    g: &GenericControl,
    ids: &mut IdSeq,
    bins: &mut BinCollector,
    width: i32,
    _preserve_linesegs: bool,
    warnings: &mut Vec<String>,
) {
    if g.paragraph_lists.is_empty() {
        return;
    }
    // lastWidth=박스 폭, vertAlign=CENTER(정품 실측). 안쪽 여백(textMargin)도 정품 필수.
    let _ = write!(
        out,
        r##"<hp:drawText lastWidth="{width}" name="" editable="0"><hp:subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="CENTER" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0" hasTextRef="0" hasNumRef="0">"##,
    );
    for list in &g.paragraph_lists {
        for para in &list.paragraphs {
            // 도형 텍스트는 항상 linesegarray를 방출한다(정품 실측 — 한글은 글상자 문단에
            // 줄배치를 항상 담는다). line_segs가 없으면 no-op이라 안전. 본문(전역
            // preserve_linesegs)과 무관하게 강제.
            write_paragraph(out, doc, para, ids, bins, false, true, warnings);
        }
    }
    out.push_str(
        r##"</hp:subList><hp:textMargin left="283" right="283" top="283" bottom="283"/></hp:drawText>"##,
    );
}

/// hwp5 쪽번호 위치 코드 → OWPML pos 속성(reader `build_pgnp` 역매핑).
fn page_num_pos_name(code: u8) -> &'static str {
    match code {
        1 => "TOP_LEFT",
        2 => "TOP_CENTER",
        3 => "TOP_RIGHT",
        4 => "BOTTOM_LEFT",
        5 => "BOTTOM_CENTER",
        6 => "BOTTOM_RIGHT",
        7 => "OUTSIDE_TOP",
        8 => "OUTSIDE_BOTTOM",
        9 => "INSIDE_TOP",
        10 => "INSIDE_BOTTOM",
        _ => "NONE",
    }
}

/// gso 공통 헤더의 attr 비트 + 오프셋으로 `<hp:pos …/>` 를 만든다(⑱ 역매핑 — 쌍 대조 검증).
fn gso_pos_xml(attr: u32, voff: i32, hoff: i32) -> String {
    let treat = attr & 1;
    let vrel = ((attr >> 3) & 0x3) as u8;
    let valign = ((attr >> 5) & 0x7) as u8;
    let hrel = ((attr >> 8) & 0x3) as u8;
    let halign = ((attr >> 10) & 0x7) as u8;
    // 부유 도형(treatAsChar=0)은 flowWithText=0·allowOverlap=1(정품 실측). 본문흐름(=1/0)
    // 이면 한글이 다수 도형을 배치 못 해 빈 화면. 인라인(treatAsChar=1)은 1/0 유지.
    let (flow, overlap) = if treat == 1 { (1, 0) } else { (0, 1) };
    format!(
        r##"<hp:pos treatAsChar="{treat}" affectLSpacing="0" flowWithText="{flow}" allowOverlap="{overlap}" holdAnchorAndSO="0" vertRelTo="{}" horzRelTo="{}" vertAlign="{}" horzAlign="{}" vertOffset="{voff}" horzOffset="{hoff}"/>"##,
        vert_rel_to_name(vrel),
        horz_rel_to_name(hrel),
        vert_align_name(valign),
        horz_align_name(halign),
    )
}

/// 도형 하나를 OWPML 요소로 방출한다(스캐폴드+lineShape+채움+점+선택 drawText+sz/pos).
/// hwpx-출신(Arm A)과 hwp5-출신(write_gso) 모두 이 함수를 거친다.
#[allow(clippy::too_many_arguments)]
fn write_shape_element(
    out: &mut String,
    doc: &Document,
    s: &hwp_model::ShapeGeom,
    ids: &mut IdSeq,
    bins: &mut BinCollector,
    sz: (i32, i32),
    pos_xml: &str,
    zorder: i32,
    text: Option<&GenericControl>,
    preserve_linesegs: bool,
    warnings: &mut Vec<String>,
) {
    let el = match s.kind {
        ShapeKind::Rect => "rect",
        ShapeKind::Ellipse => "ellipse",
        ShapeKind::Line => "line",
        ShapeKind::Polygon => "polygon",
        ShapeKind::Curve => "curve",
        ShapeKind::Arc => "arc",
    };
    // textWrap=IN_FRONT_OF_TEXT: 정품(테스트2.hwpx) 부유 도형 실측. TOP_AND_BOTTOM(본문
    // 흐름 삽입)이면 한글이 다수 도형을 배치 못 해 빈 화면이 된다(실기 확정).
    let _ = write!(
        out,
        r##"<hp:{el} id="{}" zOrder="{zorder}" numberingType="PICTURE" textWrap="IN_FRONT_OF_TEXT" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="0" instid="{}""##,
        ids.next(),
        ids.next(),
    );
    // 도형별 여는태그 추가 속성(정품 실측): Rect=ratio, Ellipse=호속성 3종, Arc=type.
    match s.kind {
        ShapeKind::Rect => {
            let _ = write!(out, r##" ratio="{}""##, s.round_ratio);
        }
        ShapeKind::Ellipse => {
            out.push_str(r##" intervalDirty="0" hasArcPr="0" arcType="NORMAL""##);
        }
        ShapeKind::Arc => {
            out.push_str(r##" type="NORMAL""##);
        }
        _ => {}
    }
    out.push('>');
    // curSz: 타원/호는 정품이 (0,0)(미리사이즈 없음 표식). 사각형 등은 (w,h) 유지.
    let (cur_w, cur_h) = match s.kind {
        ShapeKind::Ellipse | ShapeKind::Arc => (0, 0),
        _ => (sz.0, sz.1),
    };
    write_obj_scaffold(out, sz.0, sz.1, cur_w, cur_h);
    if s.border_width <= 0 {
        out.push_str(
            r##"<hp:lineShape color="#000000" width="0" style="NONE" endCap="FLAT" headStyle="NORMAL" tailStyle="NORMAL" headfill="1" tailfill="1" headSz="SMALL_SMALL" tailSz="SMALL_SMALL" outlineStyle="NORMAL" alpha="0"/>"##,
        );
    } else {
        let _ = write!(
            out,
            r##"<hp:lineShape color="{}" width="{}" style="{}" endCap="FLAT" headStyle="{}" tailStyle="{}" headfill="1" tailfill="1" headSz="SMALL_SMALL" tailSz="SMALL_SMALL" outlineStyle="NORMAL" alpha="0"/>"##,
            color_hex(s.border_color),
            s.border_width,
            line_style_name(s.border_style),
            arrow_name(s.arrow_start),
            arrow_name(s.arrow_end),
        );
    }
    // fillBrush는 **채움이 있을 때만** 방출한다. 무채움(s.fill=0xFFFF_FFFF)을 불투명
    // 흰색으로 내보내면(㉙ 버그) 투명이어야 할 가이드 도형이 불투명 흰 원반이 되어 한글
    // 에서 뒤 내용을 덮는다(annual 6쪽 링 다이어그램 미렌더 원인 — fill 플래그 대조 확정).
    // 도넛 구멍은 solid 흰색(0x00FFFFFF)이라 fillBrush 유지, 가이드원(무채움)만 투명.
    if let Some(gr) = &s.fill_gradient {
        // reader parse_gradation의 역: type/angle 속성 + color 자식들.
        let _ = write!(
            out,
            r##"<hc:fillBrush><hc:gradation type="{}" angle="{}" centerX="0" centerY="0" step="255" colorNum="{}" stepCenter="50" alpha="0">"##,
            if gr.radial { "RADIAL" } else { "LINEAR" },
            gr.angle_deg.round() as i32,
            gr.stops.len(),
        );
        for (_, c) in &gr.stops {
            let _ = write!(out, r##"<hc:color value="{}"/>"##, color_hex(*c));
        }
        out.push_str("</hc:gradation></hc:fillBrush>");
    } else if s.fill != 0xFFFF_FFFF {
        let _ = write!(
            out,
            r##"<hc:fillBrush><hc:winBrush faceColor="{}" hatchColor="#000000" alpha="0"/></hc:fillBrush>"##,
            color_hex(s.fill),
        );
    }
    // shadow(type=NONE)도 정품 실측 필수 요소.
    out.push_str(r##"<hp:shadow type="NONE" color="#B2B2B2" offsetX="0" offsetY="0" alpha="0"/>"##);
    if let Some(g) = text {
        write_draw_text(out, doc, g, ids, bins, sz.0, preserve_linesegs, warnings);
    }
    // 기하 좌표점은 drawText 뒤(정품 순서). Rect/Ellipse는 bbox 4모서리 pt0~3 —
    // 이 점이 없으면 한글이 도형 외곽을 몰라 렌더하지 않는다(빈 화면 원인).
    match s.kind {
        ShapeKind::Line => {
            let (p0, p1) = if s.points.len() >= 2 {
                (s.points[0], s.points[1])
            } else {
                ((0, 0), (s.w, s.h))
            };
            let _ = write!(
                out,
                r##"<hc:startPt x="{}" y="{}"/><hc:endPt x="{}" y="{}"/>"##,
                p0.0, p0.1, p1.0, p1.1,
            );
        }
        ShapeKind::Polygon | ShapeKind::Curve => {
            for (pi, (px, py)) in s.points.iter().enumerate() {
                let _ = write!(out, r##"<hc:pt{pi} x="{px}" y="{py}"/>"##);
            }
        }
        ShapeKind::Rect => {
            // 사각형은 bbox 4모서리 pt0~3(정품 실측).
            let (w, h) = (sz.0, sz.1);
            let _ = write!(
                out,
                r##"<hc:pt0 x="0" y="0"/><hc:pt1 x="{w}" y="0"/><hc:pt2 x="{w}" y="{h}"/><hc:pt3 x="0" y="{h}"/>"##,
            );
        }
        ShapeKind::Ellipse => {
            // 타원은 중심+축끝점+호각(정품 실측 — pt0~3가 아님). 완전 타원이라 start/end=0.
            let (w, h) = (sz.0, sz.1);
            let (cx, cy) = (w / 2, h / 2);
            let _ = write!(
                out,
                r##"<hc:center x="{cx}" y="{cy}"/><hc:ax1 x="{w}" y="{cy}"/><hc:ax2 x="{cx}" y="0"/><hc:start1 x="0" y="0"/><hc:end1 x="0" y="0"/><hc:start2 x="0" y="0"/><hc:end2 x="0" y="0"/>"##,
            );
        }
        ShapeKind::Arc => {
            // 호는 중심+축끝점 2개(정품 실측). 파싱된 3점(center,ax1,ax2) 사용, 없으면 bbox 근사.
            if s.points.len() >= 3 {
                let (c, a1, a2) = (s.points[0], s.points[1], s.points[2]);
                let _ = write!(
                    out,
                    r##"<hc:center x="{}" y="{}"/><hc:ax1 x="{}" y="{}"/><hc:ax2 x="{}" y="{}"/>"##,
                    c.0, c.1, a1.0, a1.1, a2.0, a2.1,
                );
            } else {
                let (w, h) = (sz.0, sz.1);
                let _ = write!(
                    out,
                    r##"<hc:center x="0" y="0"/><hc:ax1 x="0" y="{h}"/><hc:ax2 x="{w}" y="0"/>"##,
                );
            }
        }
    }
    let _ = write!(
        out,
        r##"<hp:sz width="{}" widthRelTo="ABSOLUTE" height="{}" heightRelTo="ABSOLUTE" protect="0"/>{pos_xml}<hp:outMargin left="0" right="0" top="0" bottom="0"/></hp:{el}>"##,
        sz.0, sz.1,
    );
}

/// hwp5-출신 gso를 방출한다. 텍스트가 있으면 글상자(`<hp:rect>`+drawText, 테두리/채움은
/// SHAPE_COMPONENT 첫 도형 스타일에서 복원), 없으면 장식 도형들을 도형 요소로 방출.
/// 기하/배치는 gso 공통 헤더 + shapes_from_raw(실쌍 대조 검증) — 도형 해석 실패 시 드롭 경고.
#[allow(clippy::too_many_arguments)]
fn write_gso(
    out: &mut String,
    doc: &Document,
    g: &GenericControl,
    ids: &mut IdSeq,
    bins: &mut BinCollector,
    preserve_linesegs: bool,
    warnings: &mut Vec<String>,
) {
    let Some((attr, voff, hoff, w, h, zorder)) = parse_gso_header(&g.data) else {
        warnings.push("DROP: gso 공통 헤더 파싱 실패 — 드롭".to_string());
        return;
    };
    let shapes = hwp_convert::gso::shapes_from_raw(&g.raw_children);
    let has_text = !g.paragraph_lists.is_empty();
    if has_text {
        // 글상자: rect 하나 + 첫 도형의 테두리/채움 스타일(없으면 무테두리).
        let style = shapes.first();
        let rect = hwp_model::ShapeGeom {
            kind: ShapeKind::Rect,
            x: 0,
            y: 0,
            w,
            h,
            points: Vec::new(),
            fill: style.map_or(0xFFFF_FFFF, |s| s.fill),
            fill_gradient: style.and_then(|s| s.fill_gradient.clone()),
            border_color: style.map_or(0xFFFF_FFFF, |s| s.border_color),
            border_width: style.map_or(0, |s| s.border_width),
            round_ratio: style.map_or(0, |s| s.round_ratio),
            border_style: style.map_or(0, |s| s.border_style),
            arrow_start: 0,
            arrow_end: 0,
            anchored: attr & 1 != 0,
        };
        let pos = gso_pos_xml(attr, voff, hoff);
        write_shape_element(
            out,
            doc,
            &rect,
            ids,
            bins,
            (w, h),
            &pos,
            zorder * Z_SCALE,
            Some(g),
            preserve_linesegs,
            warnings,
        );
    } else if shapes.is_empty() {
        warnings.push("DROP: gso 도형 해석 실패(ARC/이미지채움 등) — 드롭".to_string());
    } else {
        // 장식 도형: 도형별 요소. 배치 = gso 오프셋 + 박스 내 도형 오프셋.
        // ★그룹 도형(도넛=회색+흰 구멍 등, 한 gso 다중 도형)은 gso z-order를 공유하면
        // z 충돌 → 한글이 하나만 그리고 나머지를 스킵(도넛 미렌더 원인, 실기 확정).
        // 전체 z를 Z_SCALE 배로 늘리고 도형 인덱스를 더해 고유화(상대 순서 보존).
        for (i, s) in shapes.iter().enumerate() {
            let pos = gso_pos_xml(attr, voff + s.y, hoff + s.x);
            write_shape_element(
                out,
                doc,
                s,
                ids,
                bins,
                (s.w.max(1), s.h.max(1)),
                &pos,
                zorder * Z_SCALE + i as i32,
                None,
                preserve_linesegs,
                warnings,
            );
        }
    }
}

/// gso z-order 스케일 배수 — 그룹 내 도형에 고유 z를 주면서(base*Z_SCALE+index) gso 간
/// 상대 순서를 보존한다. 한 gso 최대 도형 수 여유(<64)로 인접 gso와 충돌 없음.
const Z_SCALE: i32 = 64;

/// hwpx-출신 구조화 도형(ShapeGeom) → OWPML 요소(reader `collect_shape`의 역).
/// 텍스트(paragraph_lists)는 첫 도형에 drawText로 부착한다. 배치(relTo 등)는 ShapeGeom이
/// 보존하지 않아 PAPER 절대 좌표로 근사(x/y는 reader가 pos 오프셋으로 왕복).
#[allow(clippy::too_many_arguments)]
fn write_ir_shapes(
    out: &mut String,
    doc: &Document,
    g: &GenericControl,
    ids: &mut IdSeq,
    bins: &mut BinCollector,
    preserve_linesegs: bool,
    warnings: &mut Vec<String>,
) {
    for (i, s) in g.gso_shapes.iter().enumerate() {
        // 글자처럼(anchored)이면 정품 인라인 관례(PARA/COLUMN), 아니면 PAPER 절대 좌표.
        let (treat, vrel, hrel) = if s.anchored {
            (1, "PARA", "COLUMN")
        } else {
            (0, "PAPER", "PAPER")
        };
        let pos = format!(
            r##"<hp:pos treatAsChar="{treat}" affectLSpacing="0" flowWithText="1" allowOverlap="0" holdAnchorAndSO="0" vertRelTo="{vrel}" horzRelTo="{hrel}" vertAlign="TOP" horzAlign="LEFT" vertOffset="{}" horzOffset="{}"/>"##,
            s.y, s.x,
        );
        let text = if i == 0 { Some(g) } else { None };
        // hwpx-출신 ShapeGeom엔 z-order가 없어 도형 순서로 증가 부여(전부 0보다 개선).
        write_shape_element(
            out,
            doc,
            s,
            ids,
            bins,
            (s.w, s.h),
            &pos,
            i as i32,
            text,
            preserve_linesegs,
            warnings,
        );
    }
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
        warnings.push(format!(
            "DROP: 그림 데이터를 찾지 못해 드롭: {:?}",
            pic.bin_ref
        ));
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

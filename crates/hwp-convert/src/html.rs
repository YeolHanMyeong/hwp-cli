//! IR → 독립 실행형 HTML.
//!
//! `markdown.rs`의 매핑을 1:1로 미러링하되 HTML 시맨틱 태그를 쓴다:
//! - "개요 N" 스타일 문단 → `<h1>`..`<h6>`
//! - 문자 모양 → `<strong>`/`<em>`/`<u>`/`<s>` (markdown은 굵게·기울임만, HTML은 밑줄·취소선도 보존)
//! - 표 → `<table>`/`<tr>`/`<th>`/`<td>`
//! - 줄나눔(10) → `<br>`, 탭 → 공백
//!
//! 임베드된 CJK 폰트 CSS가 포함된 standalone 문서를 생성한다.

use hwp_model::{CharShape, Control, Document, HwpChar, Paragraph, ctrl_char};

const CSS: &str = "\
body { font-family: \"함초롬바탕\",\"HCR Batang\",\"Noto Serif CJK KR\",serif;\
 max-width: 50rem; margin: 2rem auto; padding: 0 1rem; line-height: 1.7; }\n\
table { border-collapse: collapse; width: 100%; margin: 1rem 0; }\n\
th, td { border: 1px solid #999; padding: 0.35rem 0.6rem; }\n\
th { background: #f2f2f2; }\n\
h1,h2,h3,h4,h5,h6 { font-family: \"함초롬돋움\",\"HCR Dotum\",\"Noto Sans CJK KR\",sans-serif; }\n";

/// IR 전체를 standalone HTML 문서로 직렬화한다.
pub fn to_html(doc: &Document) -> String {
    let mut body = String::new();
    for section in &doc.sections {
        for para in &section.paragraphs {
            render_paragraph(doc, para, &mut body);
        }
    }
    // 문서 메타데이터 제목 우선, 없으면 첫 개요 단락으로 폴백.
    let title_text = doc
        .metadata
        .title
        .clone()
        .filter(|t| !t.trim().is_empty())
        .or_else(|| first_heading(doc))
        .unwrap_or_default();
    let title = escape(&title_text);
    let mut out = String::with_capacity(body.len() + CSS.len() + 256);
    out.push_str("<!DOCTYPE html>\n<html lang=\"ko\"><head><meta charset=\"utf-8\">\n<title>");
    out.push_str(&title);
    out.push_str("</title>\n<style>\n");
    out.push_str(CSS);
    out.push_str("</style></head>\n<body>\n");
    out.push_str(&body);
    out.push_str("</body></html>\n");
    out
}

/// 본문 fragment만 (head/style 없이) 반환한다.
pub fn to_html_fragment(doc: &Document) -> String {
    let mut body = String::new();
    for section in &doc.sections {
        for para in &section.paragraphs {
            render_paragraph(doc, para, &mut body);
        }
    }
    body
}

fn first_heading(doc: &Document) -> Option<String> {
    for section in &doc.sections {
        for para in &section.paragraphs {
            let is_heading = doc
                .header
                .styles
                .get(para.style.0 as usize)
                .and_then(|s| s.name.strip_prefix("개요 "))
                .and_then(|n| n.trim().parse::<usize>().ok())
                .is_some();
            if is_heading {
                let mut sink = String::new();
                let text = render_inline(doc, para, &mut sink);
                let text = strip_tags(&text);
                if !text.trim().is_empty() {
                    return Some(text.trim().to_string());
                }
            }
        }
    }
    None
}

fn render_paragraph(doc: &Document, para: &Paragraph, out: &mut String) {
    let heading = doc
        .header
        .styles
        .get(para.style.0 as usize)
        .and_then(|s| s.name.strip_prefix("개요 "))
        .and_then(|n| n.trim().parse::<usize>().ok())
        .filter(|n| (1..=6).contains(n));

    let body = render_inline(doc, para, out);
    let body = body.trim_end();
    if body.is_empty() {
        return;
    }
    if let Some(level) = heading {
        out.push_str(&format!("<h{level}>"));
        out.push_str(body);
        out.push_str(&format!("</h{level}>\n"));
    } else {
        out.push_str("<p>");
        out.push_str(body);
        out.push_str("</p>\n");
    }
}

/// 문단의 인라인 내용을 HTML 문자열로 반환한다.
/// 표 등 블록 컨트롤은 `out`에 직접 쓴다 (문단과 분리).
fn render_inline(doc: &Document, para: &Paragraph, out: &mut String) -> String {
    let mut body = String::new();
    let mut wchar_pos = 0u32;
    let mut style = Style::default();

    for ch in &para.chars {
        if let HwpChar::Text(_) = ch {
            let want = shape_at(doc, para, wchar_pos)
                .map(Style::from_shape)
                .unwrap_or_default();
            if want != style {
                close_marks(&mut body, &mut style);
                open_marks(&mut body, want);
                style = want;
            }
        }
        match ch {
            HwpChar::Text(c) => push_escaped(&mut body, *c),
            HwpChar::CharCtrl(code) => match *code {
                ctrl_char::LINE_BREAK => {
                    close_marks(&mut body, &mut style);
                    body.push_str("<br>\n");
                }
                ctrl_char::HYPHEN => body.push('-'),
                ctrl_char::NB_SPACE | ctrl_char::FW_SPACE => body.push(' '),
                _ => {}
            },
            HwpChar::InlineCtrl { code, .. } => {
                if *code == ctrl_char::TAB {
                    body.push(' ');
                }
            }
            HwpChar::ExtCtrl {
                code, ctrl_index, ..
            } => {
                if let Some(idx) = ctrl_index
                    && let Some(control) = para.controls.get(*idx as usize)
                {
                    render_control(doc, control, *code, &mut body, out);
                }
            }
        }
        wchar_pos += ch.wchar_width();
    }
    close_marks(&mut body, &mut style);
    body
}

fn render_control(
    doc: &Document,
    control: &Control,
    code: u16,
    body: &mut String,
    out: &mut String,
) {
    match control {
        Control::SectionDef(_) => {}
        Control::Picture(_) => body.push_str("<img alt=\"image\">"),
        Control::Table(table) => {
            let cols = table.cols.max(1) as usize;
            let mut grid: Vec<Vec<String>> = Vec::new();
            for cell in &table.cells {
                let row = cell.row as usize;
                while grid.len() <= row {
                    grid.push(vec![String::new(); cols]);
                }
                let mut text = String::new();
                for p in &cell.paragraphs {
                    let mut cell_out = String::new();
                    let inline = render_inline(doc, p, &mut cell_out);
                    if !text.is_empty() && !inline.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(inline.trim());
                }
                if let Some(slot) = grid[row].get_mut(cell.col as usize) {
                    *slot = text;
                }
            }
            out.push_str("<table>\n");
            for (i, row) in grid.iter().enumerate() {
                let tag = if i == 0 { "th" } else { "td" };
                out.push_str("<tr>");
                for cellv in row {
                    out.push_str(&format!("<{tag}>{cellv}</{tag}>"));
                }
                out.push_str("</tr>\n");
            }
            out.push_str("</table>\n");
        }
        Control::Generic(g) => {
            if code == ctrl_char::HEADER_FOOTER || code == ctrl_char::HIDDEN_COMMENT {
                return;
            }
            for list in &g.paragraph_lists {
                for p in &list.paragraphs {
                    let mut sub_out = String::new();
                    let inline = render_inline(doc, p, &mut sub_out);
                    let inline = inline.trim();
                    if !inline.is_empty() {
                        if !body.is_empty() && !body.ends_with([' ', '>']) {
                            body.push(' ');
                        }
                        body.push_str(inline);
                    }
                    out.push_str(&sub_out);
                }
            }
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Style {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
}

impl Style {
    fn from_shape(s: &CharShape) -> Self {
        Style {
            bold: s.is_bold(),
            italic: s.is_italic(),
            underline: s.has_underline(),
            strike: s.has_strike(),
        }
    }
}

fn open_marks(body: &mut String, s: Style) {
    if s.bold {
        body.push_str("<strong>");
    }
    if s.italic {
        body.push_str("<em>");
    }
    if s.underline {
        body.push_str("<u>");
    }
    if s.strike {
        body.push_str("<s>");
    }
}

fn close_marks(body: &mut String, s: &mut Style) {
    // 여는 순서(strong→em→u→s)의 역순으로 닫는다.
    if s.strike {
        body.push_str("</s>");
    }
    if s.underline {
        body.push_str("</u>");
    }
    if s.italic {
        body.push_str("</em>");
    }
    if s.bold {
        body.push_str("</strong>");
    }
    *s = Style::default();
}

fn shape_at<'d>(doc: &'d Document, para: &Paragraph, pos: u32) -> Option<&'d CharShape> {
    let id = para
        .char_shape_runs
        .iter()
        .rev()
        .find(|(start, _)| *start <= pos)
        .map(|(_, id)| *id)?;
    doc.header.char_shapes.get(id.0 as usize)
}

fn push_escaped(out: &mut String, c: char) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        _ => out.push(c),
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// HTML 태그를 제거해 평문만 남긴다 (제목 추출용, 단순 처리).
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::from_markdown::from_markdown;

    #[test]
    fn 제목_본문_표_렌더() {
        let doc =
            from_markdown("# 제목\n\n본문 문단입니다.\n\n| 가 | 나 |\n|----|----|\n| 1 | 2 |\n");
        let html = to_html(&doc);
        assert!(html.contains("<!DOCTYPE html>"));
        // 헤딩 char-shape이 굵게라 본문이 <strong>으로 감싸일 수 있음 — 구조만 확인.
        assert!(html.contains("<h1>") && html.contains("제목"));
        assert!(html.contains("<p>본문 문단입니다.</p>"));
        assert!(html.contains("<table>"));
        assert!(html.contains("<th>") && html.contains("가"));
        assert!(html.contains("<td>") && html.contains("1</td>"));
    }

    #[test]
    fn 특수문자_이스케이프() {
        let doc = from_markdown("a < b & c > d\n");
        let html = to_html(&doc);
        assert!(html.contains("a &lt; b &amp; c &gt; d"));
    }

    #[test]
    fn fragment_헤드없음() {
        let doc = from_markdown("# 제목\n\n본문\n");
        let frag = to_html_fragment(&doc);
        assert!(!frag.contains("<!DOCTYPE"));
        assert!(!frag.contains("<head>"));
        assert!(!frag.contains("<style>"));
        assert!(frag.contains("제목"));
        // standalone에는 head/style이 있어야 한다 (대조).
        let full = to_html(&doc);
        assert!(full.contains("<head>") && full.contains("<style>"));
    }
}

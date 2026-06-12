//! IR → GFM markdown.
//!
//! 매핑 규칙:
//! - "개요 N" 스타일 문단 → `#` × N 헤딩
//! - 문자 모양의 굵게/기울임 → `**`/`*` 스팬 (char_shape_runs 기반)
//! - 표 → GFM 표 (첫 행을 헤더로; 병합은 평탄화)
//! - 줄나눔(10) → 강제 줄바꿈, 탭 → 공백

use hwp_model::{CharShape, Control, Document, HwpChar, Paragraph, ctrl_char};

pub fn to_markdown(doc: &Document) -> String {
    let mut out = String::new();
    for section in &doc.sections {
        for para in &section.paragraphs {
            render_paragraph(doc, para, &mut out);
        }
    }
    // 과도한 빈 줄 정리
    let mut cleaned = String::with_capacity(out.len());
    let mut blank_run = 0;
    for line in out.lines() {
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run > 1 {
                continue;
            }
        } else {
            blank_run = 0;
        }
        cleaned.push_str(line);
        cleaned.push('\n');
    }
    cleaned
}

fn render_paragraph(doc: &Document, para: &Paragraph, out: &mut String) {
    // 개요 스타일 → 헤딩
    let heading = doc
        .header
        .styles
        .get(para.style.0 as usize)
        .and_then(|s| s.name.strip_prefix("개요 "))
        .and_then(|n| n.trim().parse::<usize>().ok())
        .filter(|n| (1..=6).contains(n));

    let body = render_inline(doc, para, out);
    let body = body.trim_end();

    if let Some(level) = heading {
        if !body.is_empty() {
            out.push_str(&"#".repeat(level));
            out.push(' ');
            out.push_str(body);
            out.push_str("\n\n");
        }
    } else if !body.is_empty() {
        out.push_str(body);
        out.push_str("\n\n");
    }
}

/// 문단의 인라인 내용을 렌더링해 반환한다.
/// 표 등 블록 컨트롤은 out에 직접 쓴다 (문단 텍스트와 분리).
fn render_inline(doc: &Document, para: &Paragraph, out: &mut String) -> String {
    let mut body = String::new();
    let mut wchar_pos = 0u32;
    let mut bold = false;
    let mut italic = false;

    for ch in &para.chars {
        // 현재 위치의 문자 모양으로 굵게/기울임 전환
        // (중첩 정합성을 위해 변경 시 전부 닫고 다시 연다)
        if let HwpChar::Text(_) = ch {
            let shape = shape_at(doc, para, wchar_pos);
            let (want_bold, want_italic) =
                shape.map_or((false, false), |s| (s.is_bold(), s.is_italic()));
            if want_bold != bold || want_italic != italic {
                close_marks(&mut body, &mut bold, &mut italic);
                if want_bold {
                    body.push_str("**");
                    bold = true;
                }
                if want_italic {
                    body.push('*');
                    italic = true;
                }
            }
        }
        match ch {
            HwpChar::Text(c) => body.push(*c),
            HwpChar::CharCtrl(code) => match *code {
                ctrl_char::LINE_BREAK => {
                    close_marks(&mut body, &mut bold, &mut italic);
                    body.push_str("  \n");
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
    close_marks(&mut body, &mut bold, &mut italic);
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
        Control::Table(table) => {
            // 표는 블록 요소로 out에 직접
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
                    *slot = text.replace('|', "\\|").replace('\n', " ");
                }
            }
            out.push('\n');
            for (i, row) in grid.iter().enumerate() {
                out.push_str("| ");
                out.push_str(&row.join(" | "));
                out.push_str(" |\n");
                if i == 0 {
                    out.push_str(&format!("|{}\n", " --- |".repeat(cols)));
                }
            }
            out.push('\n');
        }
        Control::Generic(g) => {
            // 머리말/꼬리말·숨은설명 제외 (텍스트 추출 정책과 동일)
            if code == ctrl_char::HEADER_FOOTER || code == ctrl_char::HIDDEN_COMMENT {
                return;
            }
            for list in &g.paragraph_lists {
                for p in &list.paragraphs {
                    let mut sub_out = String::new();
                    let inline = render_inline(doc, p, &mut sub_out);
                    let inline = inline.trim();
                    if !inline.is_empty() {
                        if !body.is_empty() && !body.ends_with([' ', '\n']) {
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

/// 주어진 WCHAR 위치의 문자 모양.
fn shape_at<'d>(doc: &'d Document, para: &Paragraph, pos: u32) -> Option<&'d CharShape> {
    let id = para
        .char_shape_runs
        .iter()
        .rev()
        .find(|(start, _)| *start <= pos)
        .map(|(_, id)| *id)?;
    doc.header.char_shapes.get(id.0 as usize)
}

fn close_marks(body: &mut String, bold: &mut bool, italic: &mut bool) {
    // 닫는 순서: 기울임 → 굵게 (여는 순서의 역)
    if *italic {
        body.push('*');
        *italic = false;
    }
    if *bold {
        body.push_str("**");
        *bold = false;
    }
}

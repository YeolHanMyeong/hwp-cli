//! IR → GFM markdown.
//!
//! 매핑 규칙:
//! - "개요 N" 스타일 문단 → `#` × N 헤딩
//! - 문자 모양의 굵게/기울임 → `**`/`*` 스팬 (char_shape_runs 기반)
//! - 하이퍼링크(%hlk 필드) → `[표시텍스트](URL)`
//! - 이미지(Picture) → `![image]()` (또는 media_dir 지정 시 추출·상대참조)
//! - 표 → GFM 표 (첫 행을 헤더로; 병합은 평탄화)
//! - 줄나눔(10) → 강제 줄바꿈, 탭 → 공백

use std::path::Path;

use hwp_model::{CharShape, Control, Document, HwpChar, Paragraph, TextOptions, ctrl_char};

/// markdown 출력 옵션.
#[derive(Default)]
pub struct MarkdownOptions<'a> {
    /// 이미지 바이너리를 추출할 디렉터리. `Some`이면 이미지를 `image1.png` 식으로
    /// 그 디렉터리에 뽑고 `![image](디렉터리명/image1.png)`로 참조한다(디렉터리는
    /// 첫 이미지에서 지연 생성 — 이미지가 없으면 만들지 않는다). `None`이면 기존처럼
    /// 빈 참조 `![image]()`를 유지한다(동작 불변).
    pub media_dir: Option<&'a Path>,
    /// 텍스트 추출 옵션(머리말/꼬리말·숨은 설명 포함 여부). 기본은 제외.
    pub text: TextOptions,
}

/// IR 전체를 GFM markdown으로 직렬화한다(기존 시그니처 유지 — 이미지 미추출).
pub fn to_markdown(doc: &Document) -> String {
    // media_dir 미지정 → IO가 없어 실패할 수 없다.
    to_markdown_with(doc, &MarkdownOptions::default())
        .expect("media_dir 미지정 시 IO가 없어 실패할 수 없다")
}

/// 옵션을 받는 변형. `media_dir` 지정 시 이미지를 추출하며, 추출 IO 실패는 `Err`.
pub fn to_markdown_with(doc: &Document, opts: &MarkdownOptions) -> std::io::Result<String> {
    let mut ctx = Ctx {
        media_dir: opts.media_dir,
        dir_name: opts
            .media_dir
            .and_then(|d| d.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        img_no: 0,
        error: None,
        include_header_footer: opts.text.include_header_footer,
        include_hidden: opts.text.include_hidden,
    };

    let mut out = String::new();
    for section in &doc.sections {
        for para in &section.paragraphs {
            render_paragraph(doc, para, &mut ctx, &mut out);
        }
    }
    if let Some(e) = ctx.error {
        return Err(e);
    }
    Ok(cleanup(&out))
}

/// 렌더 중 상태(이미지 추출 진행·텍스트 포함 정책).
struct Ctx<'a> {
    media_dir: Option<&'a Path>,
    /// 참조 경로 접두사(디렉터리명).
    dir_name: String,
    /// 다음 이미지 번호(1-기반 카운터).
    img_no: usize,
    /// 첫 IO 오류(있으면 to_markdown_with가 Err 반환).
    error: Option<std::io::Error>,
    include_header_footer: bool,
    include_hidden: bool,
}

impl Ctx<'_> {
    /// Picture 바이트를 media_dir에 뽑고 markdown 이미지 참조를 만든다.
    /// media_dir이 없거나 추출에 실패하면 빈 참조 `![image]()`를 유지한다.
    fn image_ref(&mut self, data: &[u8]) -> String {
        let Some(dir) = self.media_dir else {
            return "![image]()".to_string();
        };
        self.img_no += 1;
        let (ext, _) = crate::image::image_kind(data);
        let file = format!("image{}.{ext}", self.img_no);
        // 첫 이미지에서 디렉터리 지연 생성(이미지 없으면 만들지 않음).
        if let Err(e) = std::fs::create_dir_all(dir) {
            self.record_err(e);
            return "![image]()".to_string();
        }
        if let Err(e) = std::fs::write(dir.join(&file), data) {
            self.record_err(e);
            return "![image]()".to_string();
        }
        format!("![image]({}/{})", self.dir_name, file)
    }

    fn record_err(&mut self, e: std::io::Error) {
        if self.error.is_none() {
            self.error = Some(e);
        }
    }
}

/// 과도한 빈 줄을 정리한다.
fn cleanup(out: &str) -> String {
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

fn render_paragraph(doc: &Document, para: &Paragraph, ctx: &mut Ctx, out: &mut String) {
    // 개요 스타일 → 헤딩
    let heading = doc
        .header
        .styles
        .get(para.style.0 as usize)
        .and_then(|s| s.name.strip_prefix("개요 "))
        .and_then(|n| n.trim().parse::<usize>().ok())
        .filter(|n| (1..=6).contains(n));

    let body = render_inline(doc, para, ctx, out);
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
fn render_inline(doc: &Document, para: &Paragraph, ctx: &mut Ctx, out: &mut String) -> String {
    let mut body = String::new();
    let mut wchar_pos = 0u32;
    let mut bold = false;
    let mut italic = false;
    // 하이퍼링크 필드 열림 상태(대상 URL). FIELD_START에서 채우고 FIELD_END에서 닫는다.
    let mut link_url: Option<String> = None;

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
                if *code == ctrl_char::FIELD_END {
                    // 하이퍼링크 표시 텍스트 종료 → `](URL)`로 닫는다.
                    if let Some(url) = link_url.take() {
                        close_marks(&mut body, &mut bold, &mut italic);
                        body.push_str("](");
                        body.push_str(&md_link_dest(&url));
                        body.push(')');
                    }
                } else if *code == ctrl_char::TAB {
                    body.push(' ');
                }
            }
            HwpChar::ExtCtrl {
                code, ctrl_index, ..
            } => {
                if let Some(idx) = ctrl_index
                    && let Some(control) = para.controls.get(*idx as usize)
                {
                    if *code == ctrl_char::FIELD_START
                        && let Some(url) = crate::field::hyperlink_url(control)
                    {
                        // 하이퍼링크 필드 시작 → `[` 방출, 이후 표시 텍스트를 링크로 묶는다.
                        close_marks(&mut body, &mut bold, &mut italic);
                        body.push('[');
                        link_url = Some(url);
                    } else {
                        render_control(doc, control, *code, ctx, &mut body, out);
                    }
                }
            }
        }
        wchar_pos += ch.wchar_width();
    }
    close_marks(&mut body, &mut bold, &mut italic);
    body
}

/// markdown 링크 대상 포맷: 공백·괄호가 있으면 `<...>`로 감싼다.
fn md_link_dest(url: &str) -> String {
    if url.chars().any(|c| c.is_whitespace() || c == '(' || c == ')') {
        format!("<{}>", url.replace('<', "%3C").replace('>', "%3E"))
    } else {
        url.to_string()
    }
}

fn render_control(
    doc: &Document,
    control: &Control,
    code: u16,
    ctx: &mut Ctx,
    body: &mut String,
    out: &mut String,
) {
    match control {
        Control::SectionDef(_) => {}
        Control::Picture(pic) => match doc.resolve_bin(&pic.bin_ref) {
            Some(data) => {
                let r = ctx.image_ref(data);
                body.push_str(&r);
            }
            None => body.push_str("![image]()"),
        },
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
                    let inline = render_inline(doc, p, ctx, &mut cell_out);
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
            // 머리말/꼬리말·숨은설명은 옵션에 따라 제외 (텍스트 추출 정책과 동일).
            if (code == ctrl_char::HEADER_FOOTER && !ctx.include_header_footer)
                || (code == ctrl_char::HIDDEN_COMMENT && !ctx.include_hidden)
            {
                return;
            }
            for list in &g.paragraph_lists {
                for p in &list.paragraphs {
                    let mut sub_out = String::new();
                    let inline = render_inline(doc, p, ctx, &mut sub_out);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::from_markdown::from_markdown;

    /// 하이퍼링크가 md→IR→md 왕복에서 `[표시](URL)`로 보존된다.
    #[test]
    fn 하이퍼링크_왕복_보존() {
        let doc = from_markdown("자세히는 [여기](https://example.com/path)를 보라\n");
        let md = to_markdown(&doc);
        assert!(
            md.contains("[여기](https://example.com/path)"),
            "링크 왕복: {md}"
        );
    }

    /// media_dir 미지정이면 이미지 참조는 빈 참조를 유지한다(동작 불변).
    #[test]
    fn 이미지_기본은_빈참조() {
        let mut doc = from_markdown("사진: 여기");
        let png = png_bytes();
        crate::image::insert_image(
            &mut doc,
            "사진:",
            &write_temp("md_img_none.png", &png),
            crate::image::ImageSize::Natural,
        )
        .unwrap();
        let md = to_markdown(&doc);
        assert!(md.contains("![image]()"), "빈 참조 유지: {md}");
    }

    /// media_dir 지정 시 이미지가 디렉터리에 추출되고 상대경로로 참조된다.
    #[test]
    fn 이미지_media_dir_추출() {
        let mut doc = from_markdown("사진: 여기");
        let png = png_bytes();
        crate::image::insert_image(
            &mut doc,
            "사진:",
            &write_temp("md_img_extract.png", &png),
            crate::image::ImageSize::Natural,
        )
        .unwrap();

        let dir = unique_dir("md_media_extract");
        // 추출 전에는 디렉터리가 없어야 한다(지연 생성 확인).
        assert!(!dir.exists());
        let md = to_markdown_with(
            &doc,
            &MarkdownOptions {
                media_dir: Some(&dir),
                ..Default::default()
            },
        )
        .unwrap();
        let name = dir.file_name().unwrap().to_string_lossy();
        assert!(
            md.contains(&format!("![image]({name}/image1.png)")),
            "상대경로 참조: {md}"
        );
        let extracted = dir.join("image1.png");
        assert!(extracted.exists(), "이미지 파일 추출");
        assert_eq!(std::fs::read(&extracted).unwrap(), png, "추출 바이트 일치");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 이미지가 없으면 media_dir 지정이어도 디렉터리를 만들지 않는다.
    #[test]
    fn 이미지_없으면_디렉터리_미생성() {
        let doc = from_markdown("본문만 있는 문단\n");
        let dir = unique_dir("md_media_empty");
        let _ = to_markdown_with(
            &doc,
            &MarkdownOptions {
                media_dir: Some(&dir),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!dir.exists(), "이미지 없으면 디렉터리 미생성");
    }

    fn png_bytes() -> Vec<u8> {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend([0, 0, 0, 13]);
        png.extend(b"IHDR");
        png.extend(96u32.to_be_bytes());
        png.extend(96u32.to_be_bytes());
        png.extend([0u8; 8]);
        png
    }

    fn write_temp(name: &str, data: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(name);
        std::fs::write(&p, data).unwrap();
        p
    }

    fn unique_dir(stem: &str) -> std::path::PathBuf {
        let uniq = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{stem}_{uniq}"))
    }

    /// 이미지 여러 개가 등장 순서대로 image1/image2로 번호 매겨진다.
    #[test]
    fn 이미지_카운터_증가() {
        // 두 이미지가 순서대로 image1/image2로 번호 매겨진다.
        let mut doc = from_markdown("첫 사진: 여기\n\n둘 사진: 저기");
        let png = png_bytes();
        crate::image::insert_image(
            &mut doc,
            "첫 사진:",
            &write_temp("md_cnt1.png", &png),
            crate::image::ImageSize::Natural,
        )
        .unwrap();
        crate::image::insert_image(
            &mut doc,
            "둘 사진:",
            &write_temp("md_cnt2.png", &png),
            crate::image::ImageSize::Natural,
        )
        .unwrap();
        let dir = unique_dir("md_media_counter");
        let md = to_markdown_with(
            &doc,
            &MarkdownOptions {
                media_dir: Some(&dir),
                ..Default::default()
            },
        )
        .unwrap();
        let name = dir.file_name().unwrap().to_string_lossy();
        assert!(md.contains(&format!("{name}/image1.png")), "첫 이미지");
        assert!(md.contains(&format!("{name}/image2.png")), "둘째 이미지");
        assert!(dir.join("image2.png").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

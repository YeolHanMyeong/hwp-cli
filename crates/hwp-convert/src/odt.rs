//! IR → ODT (OpenDocument Text) 단방향 내보내기.
//!
//! 단락/개요(제목)/표/이미지/굵게·기울임·밑줄·취소선 + 문서 메타데이터를 옮긴다.
//! **내용·구조 충실도**이며 페이지 레이아웃(여백·단·머리말 위치)은 재현하지 않는다.
//! 패키징은 hwpx 작성기와 같은 mimetype-우선 STORED 규칙을 따른다.

use std::io::Write as _;

use hwp_model::{CharShape, Control, Document, HwpChar, Paragraph, ctrl_char};

/// 문서를 ODT 바이트(zip)로 직렬화한다.
pub fn to_odt(doc: &Document) -> std::io::Result<Vec<u8>> {
    let mut b = Builder {
        doc,
        body: String::new(),
        images: Vec::new(),
    };
    for section in &doc.sections {
        for para in &section.paragraphs {
            b.paragraph(para);
        }
    }
    let content = content_xml(&b.body);
    let meta = meta_xml(doc);
    let manifest = manifest_xml(&b.images);

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let map_err = |e: zip::result::ZipError| std::io::Error::other(e);

    // mimetype은 반드시 첫 엔트리 + 무압축.
    zip.start_file("mimetype", stored).map_err(map_err)?;
    zip.write_all(b"application/vnd.oasis.opendocument.text")?;

    for (name, data) in [
        ("content.xml", content.as_bytes()),
        ("styles.xml", STYLES_XML.as_bytes()),
        ("meta.xml", meta.as_bytes()),
        ("META-INF/manifest.xml", manifest.as_bytes()),
    ] {
        zip.start_file(name, deflated).map_err(map_err)?;
        zip.write_all(data)?;
    }
    for (i, img) in b.images.iter().enumerate() {
        zip.start_file(format!("Pictures/image{i}.{}", img.ext), deflated)
            .map_err(map_err)?;
        zip.write_all(&img.data)?;
    }
    let cursor = zip.finish().map_err(map_err)?;
    Ok(cursor.into_inner())
}

struct ImageItem {
    data: Vec<u8>,
    ext: &'static str,
    mime: &'static str,
    /// cm 단위 표시 크기 (0이면 생략).
    w_cm: f64,
    h_cm: f64,
}

struct Builder<'d> {
    doc: &'d Document,
    body: String,
    images: Vec<ImageItem>,
}

impl Builder<'_> {
    fn paragraph(&mut self, para: &Paragraph) {
        let heading = self
            .doc
            .header
            .styles
            .get(para.style.0 as usize)
            .and_then(|s| s.name.strip_prefix("개요 "))
            .and_then(|n| n.trim().parse::<usize>().ok())
            .filter(|n| (1..=6).contains(n));

        let mut inline = String::new();
        let mut blocks = String::new();
        self.inline(para, &mut inline, &mut blocks);
        let inline = inline.trim_end();
        if inline.is_empty() && blocks.is_empty() {
            return;
        }
        if !inline.is_empty() {
            match heading {
                Some(level) => {
                    self.body.push_str(&format!(
                        "<text:h text:style-name=\"H{level}\" text:outline-level=\"{level}\">{inline}</text:h>"
                    ));
                }
                None => self.body.push_str(&format!(
                    "<text:p text:style-name=\"Body\">{inline}</text:p>"
                )),
            }
        }
        self.body.push_str(&blocks);
    }

    /// 문단의 인라인 내용을 ODF로 만든다. 표 등 블록은 `blocks`로 분리.
    fn inline(&mut self, para: &Paragraph, out: &mut String, blocks: &mut String) {
        let mut wchar_pos = 0u32;
        let mut style = Style::default();
        for ch in &para.chars {
            if let HwpChar::Text(_) = ch {
                let want = shape_at(self.doc, para, wchar_pos)
                    .map(Style::from_shape)
                    .unwrap_or_default();
                if want != style {
                    close_spans(out, &mut style);
                    open_spans(out, want);
                    style = want;
                }
            }
            match ch {
                HwpChar::Text(c) => push_escaped(out, *c),
                HwpChar::CharCtrl(code) => match *code {
                    ctrl_char::LINE_BREAK => {
                        close_spans(out, &mut style);
                        out.push_str("<text:line-break/>");
                    }
                    ctrl_char::HYPHEN => out.push('-'),
                    ctrl_char::NB_SPACE | ctrl_char::FW_SPACE => out.push(' '),
                    _ => {}
                },
                HwpChar::InlineCtrl { code, .. } => {
                    if *code == ctrl_char::TAB {
                        out.push_str("<text:tab/>");
                    }
                }
                HwpChar::ExtCtrl {
                    code, ctrl_index, ..
                } => {
                    if let Some(idx) = ctrl_index
                        && let Some(control) = para.controls.get(*idx as usize)
                    {
                        close_spans(out, &mut style);
                        self.control(control, *code, out, blocks);
                    }
                }
            }
            wchar_pos += ch.wchar_width();
        }
        close_spans(out, &mut style);
    }

    fn control(&mut self, control: &Control, code: u16, out: &mut String, blocks: &mut String) {
        match control {
            Control::SectionDef(_) => {}
            Control::Picture(pic) => {
                if let Some(data) = self.doc.resolve_bin(&pic.bin_ref) {
                    let (ext, mime) = image_kind(data);
                    let idx = self.images.len();
                    self.images.push(ImageItem {
                        data: data.to_vec(),
                        ext,
                        mime,
                        w_cm: pic.width.to_mm() / 10.0,
                        h_cm: pic.height.to_mm() / 10.0,
                    });
                    let size = if self.images[idx].w_cm > 0.0 && self.images[idx].h_cm > 0.0 {
                        format!(
                            " svg:width=\"{:.3}cm\" svg:height=\"{:.3}cm\"",
                            self.images[idx].w_cm, self.images[idx].h_cm
                        )
                    } else {
                        String::new()
                    };
                    out.push_str(&format!(
                        "<draw:frame draw:style-name=\"Img\" text:anchor-type=\"as-char\"{size}>\
                         <draw:image xlink:href=\"Pictures/image{idx}.{ext}\" xlink:type=\"simple\" \
                         xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>"
                    ));
                }
            }
            Control::Table(table) => self.table(table, blocks),
            Control::Generic(g) => {
                if code == ctrl_char::HEADER_FOOTER || code == ctrl_char::HIDDEN_COMMENT {
                    return;
                }
                // 글상자 등: 내부 문단 텍스트를 인라인으로 흡수.
                for list in &g.paragraph_lists {
                    for p in &list.paragraphs {
                        let mut sub = String::new();
                        self.inline(p, &mut sub, blocks);
                        let sub = sub.trim();
                        if !sub.is_empty() {
                            if !out.is_empty() && !out.ends_with([' ', '>']) {
                                out.push(' ');
                            }
                            out.push_str(sub);
                        }
                    }
                }
            }
        }
    }

    fn table(&mut self, table: &hwp_model::Table, blocks: &mut String) {
        let cols = table.cols.max(1) as usize;
        let mut grid: Vec<Vec<String>> = Vec::new();
        for cell in &table.cells {
            let row = cell.row as usize;
            while grid.len() <= row {
                grid.push(vec![String::new(); cols]);
            }
            let mut text = String::new();
            for p in &cell.paragraphs {
                let mut inl = String::new();
                let mut blk = String::new();
                self.inline(p, &mut inl, &mut blk);
                let inl = inl.trim();
                if !inl.is_empty() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(inl);
                }
            }
            if let Some(slot) = grid[row].get_mut(cell.col as usize) {
                *slot = text;
            }
        }
        blocks.push_str("<table:table table:style-name=\"Tbl\">");
        blocks.push_str(&format!(
            "<table:table-column table:number-columns-repeated=\"{cols}\"/>"
        ));
        for row in &grid {
            blocks.push_str("<table:table-row>");
            for cellv in row {
                blocks.push_str(&format!(
                    "<table:table-cell office:value-type=\"string\">\
                     <text:p text:style-name=\"Body\">{cellv}</text:p></table:table-cell>"
                ));
            }
            blocks.push_str("</table:table-row>");
        }
        blocks.push_str("</table:table>");
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
    /// 적용할 ODF 텍스트 스타일 이름들 (중첩 span).
    fn span_styles(self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.bold {
            v.push("TB");
        }
        if self.italic {
            v.push("TI");
        }
        if self.underline {
            v.push("TU");
        }
        if self.strike {
            v.push("TS");
        }
        v
    }
}

fn open_spans(out: &mut String, s: Style) {
    for name in s.span_styles() {
        out.push_str(&format!("<text:span text:style-name=\"{name}\">"));
    }
}

fn close_spans(out: &mut String, s: &mut Style) {
    for _ in s.span_styles() {
        out.push_str("</text:span>");
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

fn image_kind(data: &[u8]) -> (&'static str, &'static str) {
    match data {
        [0x89, b'P', b'N', b'G', ..] => ("png", "image/png"),
        [0xFF, 0xD8, ..] => ("jpg", "image/jpeg"),
        [b'G', b'I', b'F', ..] => ("gif", "image/gif"),
        [b'B', b'M', ..] => ("bmp", "image/bmp"),
        _ => ("png", "image/png"),
    }
}

fn push_escaped(out: &mut String, c: char) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        _ => out.push(c),
    }
}

fn esc(s: &str) -> String {
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

const CONTENT_NS: &str = "xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" \
xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" \
xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" \
xmlns:xlink=\"http://www.w3.org/1999/xlink\"";

/// content.xml — 자동 스타일 + 본문.
fn content_xml(body: &str) -> String {
    let heading_sizes = [20, 18, 16, 14, 13, 12];
    let mut styles = String::from("<style:style style:name=\"Body\" style:family=\"paragraph\"/>");
    for (i, sz) in heading_sizes.iter().enumerate() {
        styles.push_str(&format!(
            "<style:style style:name=\"H{}\" style:family=\"paragraph\"><style:text-properties fo:font-size=\"{sz}pt\" fo:font-weight=\"bold\"/></style:style>",
            i + 1
        ));
    }
    styles.push_str(
        "<style:style style:name=\"TB\" style:family=\"text\"><style:text-properties fo:font-weight=\"bold\"/></style:style>\
         <style:style style:name=\"TI\" style:family=\"text\"><style:text-properties fo:font-style=\"italic\"/></style:style>\
         <style:style style:name=\"TU\" style:family=\"text\"><style:text-properties style:text-underline-style=\"solid\" style:text-underline-width=\"auto\" style:text-underline-color=\"font-color\"/></style:style>\
         <style:style style:name=\"TS\" style:family=\"text\"><style:text-properties style:text-line-through-style=\"solid\"/></style:style>\
         <style:style style:name=\"Img\" style:family=\"graphic\"><style:graphic-properties text:anchor-type=\"as-char\"/></style:style>\
         <style:style style:name=\"Tbl\" style:family=\"table\"><style:table-properties table:align=\"margins\"/></style:style>",
    );
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <office:document-content {CONTENT_NS} office:version=\"1.2\">\
         <office:automatic-styles>{styles}</office:automatic-styles>\
         <office:body><office:text>{body}</office:text></office:body>\
         </office:document-content>"
    )
}

const STYLES_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-styles xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" office:version=\"1.2\"><office:styles><style:default-style style:family=\"paragraph\"><style:text-properties style:font-name-asian=\"함초롬바탕\"/></style:default-style></office:styles></office:document-styles>";

fn meta_xml(doc: &Document) -> String {
    let mut m = String::new();
    if let Some(t) = doc.metadata.title.as_deref().filter(|s| !s.is_empty()) {
        m.push_str(&format!("<dc:title>{}</dc:title>", esc(t)));
    }
    if let Some(a) = doc.metadata.author.as_deref().filter(|s| !s.is_empty()) {
        m.push_str(&format!("<dc:creator>{}</dc:creator>", esc(a)));
    }
    if let Some(s) = doc.metadata.subject.as_deref().filter(|s| !s.is_empty()) {
        m.push_str(&format!("<dc:subject>{}</dc:subject>", esc(s)));
    }
    if let Some(k) = doc.metadata.keywords.as_deref().filter(|s| !s.is_empty()) {
        m.push_str(&format!("<meta:keyword>{}</meta:keyword>", esc(k)));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <office:document-meta xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
         xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" office:version=\"1.2\">\
         <office:meta><meta:generator>hwp-cli</meta:generator>{m}</office:meta></office:document-meta>"
    )
}

fn manifest_xml(images: &[ImageItem]) -> String {
    let mut entries = String::from(
        "<manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.text\"/>\
         <manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\
         <manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>\
         <manifest:file-entry manifest:full-path=\"meta.xml\" manifest:media-type=\"text/xml\"/>",
    );
    for (i, img) in images.iter().enumerate() {
        entries.push_str(&format!(
            "<manifest:file-entry manifest:full-path=\"Pictures/image{i}.{}\" manifest:media-type=\"{}\"/>",
            img.ext, img.mime
        ));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.2\">{entries}</manifest:manifest>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::from_markdown;
    use std::io::Read as _;

    fn unzip(bytes: &[u8], name: &str) -> Option<String> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
        let mut f = zip.by_name(name).ok()?;
        let mut s = String::new();
        f.read_to_string(&mut s).ok()?;
        Some(s)
    }

    #[test]
    fn produces_valid_odt_structure() {
        let doc = from_markdown::from_markdown(
            "# 제목\n\n본문 단락\n\n| a | b |\n| - | - |\n| 1 | 2 |\n",
        );
        let bytes = to_odt(&doc).unwrap();
        // mimetype이 첫 엔트리 + STORED.
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
        let first = zip.by_index(0).unwrap();
        assert_eq!(first.name(), "mimetype");
        assert_eq!(first.compression(), zip::CompressionMethod::Stored);
        drop(first);
        let content = unzip(&bytes, "content.xml").unwrap();
        assert!(content.contains("<text:h"));
        assert!(content.contains("<table:table"));
        assert!(content.contains("text:outline-level"));
    }

    #[test]
    fn writes_metadata_to_meta_xml() {
        let mut doc = from_markdown::from_markdown("본문\n");
        doc.metadata.title = Some("제목 X".into());
        doc.metadata.author = Some("이영준".into());
        let bytes = to_odt(&doc).unwrap();
        let meta = unzip(&bytes, "meta.xml").unwrap();
        assert!(meta.contains("<dc:title>제목 X</dc:title>"));
        assert!(meta.contains("<dc:creator>이영준</dc:creator>"));
    }
}

//! Markdown → IR.
//!
//! 매핑: 헤딩 → "개요 N" 스타일, 굵게/기울임 → 문자 모양 변형,
//! GFM 표 → Table 컨트롤, 목록 → "• " 접두 문단, 줄바꿈 → CharCtrl(10).

use hwp_model::{
    BorderFill, BorderFillId, BorderLine, Cell, CharShape, CharShapeId, Control, DocMeta, Document,
    FaceName, HwpChar, HwpUnit, LANG_COUNT, ParaShape, ParaShapeId, Paragraph, Section, Style,
    StyleId, Table,
};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// 문자 모양 ID 배치 (default_header와 일치해야 함).
mod shapes {
    pub const NORMAL: u16 = 0;
    pub const BOLD: u16 = 1;
    pub const ITALIC: u16 = 2;
    pub const BOLD_ITALIC: u16 = 3;
    /// H1~H6 → 4~9
    pub const HEADING_BASE: u16 = 4;
}

/// 테두리/배경 ID 배치: 1·2 = 무테두리(기본/참조용), 3 = 실선 0.12mm.
const TABLE_BORDER_FILL: u16 = 3;

/// 본문 영역 폭 (A4 기본 여백 기준, HWPUNIT).
const BODY_WIDTH: i32 = 42520;

/// `hwp new`용 기본 문서 헤더 — 한글 빈 문서에 준하는 최소 구성.
pub fn default_header() -> hwp_model::DocHeader {
    let mut header = hwp_model::DocHeader::default();
    for slot in 0..LANG_COUNT {
        header.fonts[slot] = vec![FaceName {
            name: "함초롬바탕".to_string(),
            ..FaceName::default()
        }];
    }

    let base = CharShape {
        base_size: 1000,
        ratios: [100; LANG_COUNT],
        rel_sizes: [100; LANG_COUNT],
        ..CharShape::default()
    };
    let cs = |size: i32, bold: bool, italic: bool| CharShape {
        base_size: size,
        attr: u32::from(bold) << 1 | u32::from(italic),
        ..base.clone()
    };
    header.char_shapes = vec![
        cs(1000, false, false), // 0 본문
        cs(1000, true, false),  // 1 굵게
        cs(1000, false, true),  // 2 기울임
        cs(1000, true, true),   // 3 굵게+기울임
        cs(1800, true, false),  // 4 H1
        cs(1500, true, false),  // 5 H2
        cs(1300, true, false),  // 6 H3
        cs(1200, true, false),  // 7 H4
        cs(1100, true, false),  // 8 H5
        cs(1100, true, false),  // 9 H6
    ];

    // 0 본문(양쪽), 1 헤딩(왼쪽 + 위 간격)
    header.para_shapes = vec![
        ParaShape::default(),
        ParaShape {
            attr1: 1 << 2,
            spacing_top: 600,
            spacing_bottom: 300,
            ..ParaShape::default()
        },
    ];

    header.styles = vec![Style {
        name: "바탕글".to_string(),
        english_name: "Normal".to_string(),
        ..Style::default()
    }];
    for n in 1..=6u16 {
        header.styles.push(Style {
            name: format!("개요 {n}"),
            english_name: format!("Outline {n}"),
            para_shape: ParaShapeId(1),
            char_shape: CharShapeId(shapes::HEADING_BASE + n - 1),
            ..Style::default()
        });
    }

    let none = BorderFill {
        diagonal: BorderLine {
            line_type: 1,
            width: 0,
            color: 0,
        },
        ..BorderFill::default()
    };
    let solid_line = BorderLine {
        line_type: 1,
        width: 1,
        color: 0,
    }; // 실선 0.12mm 검정
    header.border_fills = vec![
        none.clone(),
        none,
        BorderFill {
            sides: [solid_line; 4],
            diagonal: BorderLine {
                line_type: 1,
                width: 0,
                color: 0,
            },
            ..BorderFill::default()
        },
    ];
    header
}

/// markdown 텍스트를 문서로 변환한다.
pub fn from_markdown(md: &str) -> Document {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(md, options);

    let mut b = Builder::default();
    for event in parser {
        b.event(event);
    }
    b.flush_paragraph();

    if b.paragraphs.is_empty() {
        b.paragraphs.push(Paragraph::default());
    }
    Document {
        meta: DocMeta {
            source_format: "markdown".to_string(),
            source_version: String::new(),
        },
        header: default_header(),
        sections: vec![Section {
            paragraphs: b.paragraphs,
            extras: Vec::new(),
        }],
        bin_streams: Vec::new(),
    }
}

#[derive(Default)]
struct Builder {
    paragraphs: Vec<Paragraph>,
    // 현재 문단 상태
    chars: Vec<HwpChar>,
    runs: Vec<(u32, CharShapeId)>,
    wchar_pos: u32,
    style: u16,
    bold: bool,
    italic: bool,
    heading: Option<u16>, // 1..=6
    // 표 수집 상태
    table: Option<TableBuilder>,
    list_depth: usize,
    pending_bullet: bool,
}

#[derive(Default)]
struct TableBuilder {
    rows: Vec<Vec<Paragraph>>,
    current_row: Vec<Paragraph>,
    in_head: bool,
}

impl Builder {
    fn current_shape(&self) -> u16 {
        if let Some(level) = self.heading {
            return shapes::HEADING_BASE + level - 1;
        }
        match (self.bold, self.italic) {
            (false, false) => shapes::NORMAL,
            (true, false) => shapes::BOLD,
            (false, true) => shapes::ITALIC,
            (true, true) => shapes::BOLD_ITALIC,
        }
    }

    fn push_text(&mut self, text: &str) {
        let shape = CharShapeId(self.current_shape());
        if self.runs.last().map(|(_, s)| *s) != Some(shape) {
            self.runs.push((self.wchar_pos, shape));
        }
        for c in text.chars() {
            self.wchar_pos += c.len_utf16() as u32;
            self.chars.push(HwpChar::Text(c));
        }
    }

    fn flush_paragraph(&mut self) {
        if self.chars.is_empty() && self.runs.is_empty() {
            return;
        }
        let para = Paragraph {
            para_shape: ParaShapeId(if self.heading.is_some() { 1 } else { 0 }),
            style: StyleId(self.style),
            chars: std::mem::take(&mut self.chars),
            char_shape_runs: std::mem::take(&mut self.runs),
            ..Paragraph::default()
        };
        self.wchar_pos = 0;
        match &mut self.table {
            Some(tb) => tb.current_row.push(para),
            None => self.paragraphs.push(para),
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                self.flush_paragraph();
                let n = heading_level(level);
                self.heading = Some(n);
                self.style = n; // 개요 N 스타일
            }
            Event::End(TagEnd::Heading(_)) => {
                self.flush_paragraph();
                self.heading = None;
                self.style = 0;
            }
            Event::Start(Tag::Paragraph) => {
                if self.pending_bullet {
                    self.push_text("• ");
                    self.pending_bullet = false;
                }
            }
            Event::End(TagEnd::Paragraph) => self.flush_paragraph(),
            Event::Start(Tag::Strong) => self.bold = true,
            Event::End(TagEnd::Strong) => self.bold = false,
            Event::Start(Tag::Emphasis) => self.italic = true,
            Event::End(TagEnd::Emphasis) => self.italic = false,
            Event::Text(t) => {
                if self.pending_bullet {
                    self.push_text("• ");
                    self.pending_bullet = false;
                }
                self.push_text(&t);
            }
            Event::Code(t) => self.push_text(&t),
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => {
                self.chars.push(HwpChar::CharCtrl(10));
                self.wchar_pos += 1;
            }
            Event::Start(Tag::List(_)) => self.list_depth += 1,
            Event::End(TagEnd::List(_)) => self.list_depth -= 1,
            Event::Start(Tag::Item) => self.pending_bullet = true,
            Event::End(TagEnd::Item) => {
                self.flush_paragraph();
                self.pending_bullet = false;
            }
            // ── GFM 표 ──
            Event::Start(Tag::Table(_)) => {
                self.flush_paragraph();
                self.table = Some(TableBuilder::default());
            }
            Event::Start(Tag::TableHead) => {
                if let Some(tb) = &mut self.table {
                    tb.in_head = true;
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(tb) = &mut self.table {
                    let row = std::mem::take(&mut tb.current_row);
                    tb.rows.push(row);
                    tb.in_head = false;
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(tb) = &mut self.table {
                    let row = std::mem::take(&mut tb.current_row);
                    tb.rows.push(row);
                }
            }
            Event::Start(Tag::TableCell) => {
                if self.table.as_ref().is_some_and(|tb| tb.in_head) {
                    self.bold = true;
                }
            }
            Event::End(TagEnd::TableCell) => {
                self.flush_paragraph();
                self.bold = false;
            }
            Event::End(TagEnd::Table) => {
                if let Some(tb) = self.table.take() {
                    self.paragraphs.push(table_paragraph(tb));
                }
            }
            _ => {}
        }
    }
}

fn heading_level(level: HeadingLevel) -> u16 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// 수집한 표를 앵커 문단(확장 컨트롤 1개)으로 만든다.
fn table_paragraph(tb: TableBuilder) -> Paragraph {
    let rows = tb.rows.len().max(1);
    let cols = tb.rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let col_w = BODY_WIDTH / cols as i32;
    let row_h = 1700i32; // 10pt 텍스트 + 셀 위아래 여백

    let mut cells = Vec::new();
    for (r, row) in tb.rows.iter().enumerate() {
        for c in 0..cols {
            cells.push(Cell {
                col: c as u16,
                row: r as u16,
                col_span: 1,
                row_span: 1,
                width: HwpUnit(col_w),
                height: HwpUnit(row_h),
                margins: [510, 510, 141, 141],
                border_fill: BorderFillId(TABLE_BORDER_FILL),
                header_tail: Vec::new(),
                paragraphs: row.get(c).cloned().map(|p| vec![p]).unwrap_or_default(),
            });
        }
    }
    let table = Table {
        common_data: Vec::new(),
        attr: 0,
        rows: rows as u16,
        cols: cols as u16,
        cell_spacing: 0,
        inner_margins: [510, 510, 141, 141],
        row_cell_counts: vec![cols as u16; rows],
        border_fill: BorderFillId(TABLE_BORDER_FILL),
        table_tail: Vec::new(),
        cells,
        extras: Vec::new(),
    };

    let mut payload = vec![0u8; 12];
    payload[..4].copy_from_slice(b" lbt"); // 역순 ctrl_id
    Paragraph {
        chars: vec![
            HwpChar::ExtCtrl {
                code: 11,
                ctrl_id: *b"tbl ",
                payload,
                ctrl_index: Some(0),
            },
            HwpChar::CharCtrl(13),
        ],
        char_shape_runs: vec![(0, CharShapeId(0))],
        controls: vec![Control::Table(table)],
        ..Paragraph::default()
    }
}

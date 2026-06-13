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
            // 한글 무결성 검사는 글꼴 대체를 위해 기본 글꼴 이름(attr bit5, 0x20)을 기대한다.
            // 정상 표본 hello_world.hwp 의 '함초롬바탕'은 default_name="HCR Batang", attr=0x21.
            // attr 하위 0x01 = 글꼴 유형 TTF(표 20). emit_face_name 이 0x20 비트를 자동 OR 한다.
            attr: 0x01,
            default_name: Some("HCR Batang".to_string()),
            ..FaceName::default()
        }];
    }

    let base = CharShape {
        base_size: 1000,
        ratios: [100; LANG_COUNT],
        rel_sizes: [100; LANG_COUNT],
        // 음영 색(shade_color)은 0xFFFFFFFF = '없음' 표식이어야 한다. 기본값 0은
        // 한글이 '불투명 검정 음영(글자 배경 하이라이트)'으로 해석해, 글자 칸마다
        // 검은 막대를 그리고 (검정) 글자가 그 위에서 안 보이게 된다 — 14차 실기의
        // '검은 바' 원인. 정상 표본(가나다.hwp 5.1.1.0, hello_world.hwp 5.1.0.1)은
        // 모두 shade_color=0xFFFFFFFF, shadow_gap=(10,10), shadow_color≈0xC0C0C0.
        // (face_id=0은 무해 — hello_world도 char_shape[0].face_ids=0이고 정상 렌더.)
        shade_color: 0xFFFF_FFFF,
        shadow_color: 0x00C0_C0C0,
        shadow_gap: (10, 10),
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

    // 탭 정의 — 한글 기본 좌/중/우 자동 탭 3개. 정상 표본(hello_world 등
    // 5.1.0.1)은 전부 이 3개를 가지며, 모든 PARA_SHAPE가 tab_def_id=0 을
    // 참조한다. 비우면 dangling reference가 되어 한글이 '손상/변조'로 거부.
    // 각 8바이트: 속성 u32(0/1/2) + count i16=0 + 예약 u16 (spec 표36, count=0→8B).
    header.tab_defs = vec![
        hwp_model::RawEntry {
            data: vec![0, 0, 0, 0, 0, 0, 0, 0],
            children: Vec::new(),
        },
        hwp_model::RawEntry {
            data: vec![1, 0, 0, 0, 0, 0, 0, 0],
            children: Vec::new(),
        },
        hwp_model::RawEntry {
            data: vec![2, 0, 0, 0, 0, 0, 0, 0],
            children: Vec::new(),
        },
    ];

    // 0 본문(양쪽), 1 헤딩(왼쪽 + 위 간격).
    //
    // 정상 표본(가나다.hwp 5.1.1.0, hello_world.hwp 5.1.0.1)의 PARA_SHAPE[0]은
    // attr1=0x180(bit7 한글 줄나눔=글자 + bit8 줄 격자 사용), line_spacing_old=160,
    // border_fill_id=2 다. 이는 본문 줄 배치를 한글이 재계산할 때의 기준값으로,
    // 0(우리 기존값)이면 줄 격자·줄나눔 기준이 정상 표본과 어긋난다. 검은 바의
    // 직접 원인은 char_shape 음영색이지만, 한글이 줄 배치를 다시 잡을 때 안전하도록
    // 정상 표본 바이트에 맞춘다. (BodyText의 PARA_LINE_SEG 캐시는 합성기가 채운다.)
    let base_para = ParaShape {
        attr1: 0x180,
        line_spacing_old: 160,
        border_fill_id: 2,
        line_spacing: 160,
        ..ParaShape::default()
    };
    header.para_shapes = vec![
        base_para.clone(),
        ParaShape {
            attr1: 0x180 | (1 << 2), // 정상 attr1 + 왼쪽 정렬
            spacing_top: 600,
            spacing_bottom: 300,
            ..base_para
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
        // 빈 문서도 문단 하나로 닫는다. 문단끝 문자는 writer가 보장한다.
        b.paragraphs.push(Paragraph::default());
    }
    // 첫 문단에 구역/단 정의 주입 — hwp5/한글 호환의 전제 조건
    inject_section_controls(&mut b.paragraphs[0]);
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
        self.flush_paragraph_inner(false);
    }

    /// 문단을 닫는다. `force`면 내용이 없어도 빈 문단을 만든다.
    ///
    /// 표 셀은 반드시 문단을 1개 이상 가져야 한다(LIST_HEADER nparas≥1).
    /// 빈 markdown 셀(`| |`)을 그냥 흘리면 셀에 PARA_HEADER가 하나도 안 붙어
    /// nparas=0 셀이 되고, 한글이 이를 '손상'으로 거부한다. 셀 종료 시 force=true로
    /// 호출해 빈 셀도 빈 문단을 갖게 한다.
    fn flush_paragraph_inner(&mut self, force: bool) {
        if self.chars.is_empty() && self.runs.is_empty() && !force {
            return;
        }
        // 문단끝 문자(0x0d)·nchars bit31·char_shape run 병합 등 한글 문단 불변식은
        // hwp5 writer(emit_paragraph)가 합성 경로 전체(md+hwpx)에 일원 적용한다.
        // 단, 모든 문단은 PARA_CHAR_SHAPE를 1개 이상 가져야 한다(정품 전수:
        // PARA_HEADER 수 == PARA_CHAR_SHAPE 수, 빈 셀 문단도 (0,id) run 1개 보유).
        // writer는 char_shape_runs가 비면 PARA_CHAR_SHAPE를 아예 방출하지 않으므로,
        // 빈 문단(force로 만든 빈 셀 등)은 여기서 (0, 본문모양) run 1개를 채운다.
        // 누락 시 한글이 '손상'으로 거부하고 pyhwp 파서도 크래시한다.
        let mut runs = std::mem::take(&mut self.runs);
        if runs.is_empty() {
            runs.push((0, CharShapeId(self.current_shape())));
        }
        let para = Paragraph {
            para_shape: ParaShapeId(if self.heading.is_some() { 1 } else { 0 }),
            style: StyleId(self.style),
            chars: std::mem::take(&mut self.chars),
            char_shape_runs: runs,
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
                // 빈 셀도 문단 1개를 반드시 만든다(nparas≥1 보장 + 열 수 정합).
                self.flush_paragraph_inner(true);
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

/// 첫 문단 앞에 secd/cold 확장 컨트롤을 삽입한다 (16 WCHAR 시프트 포함).
fn inject_section_controls(para: &mut Paragraph) {
    use hwp_model::{Control, GenericControl, HwpUnit, PageDef, SectionDef};
    if para
        .controls
        .iter()
        .any(|c| matches!(c, Control::SectionDef(_)))
    {
        return;
    }
    // 기존 참조들 시프트
    for ch in &mut para.chars {
        if let HwpChar::ExtCtrl {
            ctrl_index: Some(i),
            ..
        } = ch
        {
            *i += 2;
        }
    }
    for (pos, _) in &mut para.char_shape_runs {
        *pos += 16;
    }
    for seg in &mut para.line_segs {
        seg.text_start += 16;
    }
    let first_shape = para
        .char_shape_runs
        .first()
        .map_or(CharShapeId(0), |(_, id)| *id);
    if para.char_shape_runs.first().map(|(p, _)| *p) != Some(0) {
        para.char_shape_runs.insert(0, (0, first_shape));
    }
    // 연속 동일 id run 병합(secd/cold 삽입으로 생기는 [(0,0),(16,0)] 중복 등)은
    // writer가 합성 경로 전체에 적용한다.

    let page = PageDef {
        width: HwpUnit(59528),
        height: HwpUnit(84186),
        margin_left: HwpUnit(8504),
        margin_right: HwpUnit(8504),
        margin_top: HwpUnit(5668),
        margin_bottom: HwpUnit(4252),
        margin_header: HwpUnit(4252),
        margin_footer: HwpUnit(4252),
        gutter: HwpUnit(0),
        attr: 0,
    };
    para.controls.insert(
        0,
        Control::SectionDef(SectionDef {
            data: Vec::new(),
            page: Some(page),
            extras: Vec::new(),
        }),
    );
    para.controls.insert(
        1,
        Control::Generic(GenericControl {
            ctrl_id: *b"cold",
            data: Vec::new(),
            paragraph_lists: Vec::new(),
            extras: Vec::new(),
            raw_children: Vec::new(),
        }),
    );
    let ext = |ctrl_id: [u8; 4], idx: u32| {
        let mut payload = vec![0u8; 12];
        let mut rev = ctrl_id;
        rev.reverse();
        payload[..4].copy_from_slice(&rev);
        HwpChar::ExtCtrl {
            code: 2,
            ctrl_id,
            payload,
            ctrl_index: Some(idx),
        }
    };
    para.chars.insert(0, ext(*b"secd", 0));
    para.chars.insert(1, ext(*b"cold", 1));
    // 구역 첫 문단의 break_type — 한글이 직접 저장한 단일 문단 표본 전수
    // (가나다·hello_world·outline·bookmark)가 모두 0x03(bit0 구역나눔 +
    // bit1 다단나눔)이다. secd/cold ExtCtrl를 품은 '구역 첫 문단'에 한글이
    // 항상 쓰는 값으로, 0x00이면 헤더-컨트롤 정합이 깨져 손상 판정된다.
    // (hwp5 왕복 경로는 body_text.rs에서 원본 break_type를 보존하며 이
    // 함수를 거치지 않으므로 바이트동일 게이트에 영향 없음.)
    para.header.break_type = 0x03;
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
                list_attr: 0,
                col: c as u16,
                row: r as u16,
                col_span: 1,
                row_span: 1,
                width: HwpUnit(col_w),
                height: HwpUnit(row_h),
                margins: [510, 510, 141, 141],
                border_fill: BorderFillId(TABLE_BORDER_FILL),
                header_tail: Vec::new(),
                // 셀은 문단 1개 이상 필수(nparas≥1). 짧은 행에서 누락된 칸은
                // 빈 문단으로 채운다 — nparas=0 셀은 한글이 손상 처리한다. 채움
                // 문단도 PARA_CHAR_SHAPE run 1개를 가져야 한다(정품 전수 불변식,
                // writer는 char_shape_runs가 비면 레코드를 방출하지 않음).
                paragraphs: row.get(c).cloned().map_or_else(
                    || {
                        vec![Paragraph {
                            char_shape_runs: vec![(0, CharShapeId(0))],
                            ..Paragraph::default()
                        }]
                    },
                    |p| vec![p],
                ),
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

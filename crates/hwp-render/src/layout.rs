//! LineSegLayouter — 파일에 저장된 줄 배치(PARA_LINE_SEG)를 복원해
//! DisplayList를 만든다.
//!
//! 실측으로 확정한 좌표 해석 (U1):
//! - `v_pos`: 페이지 본문 영역 상단 기준, 페이지마다 0으로 리셋
//! - 베이스라인 y = body_top + v_pos + baseline_gap
//! - `col_start`/`seg_width`: 본문 영역 왼쪽 기준
//! - 페이지 경계: v_pos가 직전 줄보다 작아지면 새 페이지 (v1 휴리스틱)
//!
//! 불완전한 파일 대응 (실무 hwpx에서 실측):
//! - 도구 생성 파일은 문단당 lineseg 1개 + 문단당 1줄 가정의 v_pos를
//!   기록한다 → seg 폭에서 그리디 줄바꿈 + **흐름 커서**로 보정한다.
//!   베이스라인 = max(저장된 v_pos 기반, 직전 콘텐츠 하단 기반) —
//!   완전한 파일에서는 저장값이 항상 크므로 무손실, 불완전 파일에서는
//!   겹침만 아래로 밀어낸다.
//! - lineseg가 아예 없는 문단은 본문 폭 기준 폴백 배치.

use hwp_model::{Control, Document, HwpUnit, PageDef, Paragraph, Table};

use crate::display::{DisplayList, Item, PageList};
use crate::fonts::FontStore;
use crate::shape::{InlineItem, shape_range};

/// 기본 탭 간격 (40pt = 4000 HWPUNIT).
const TAB_INTERVAL_PT: f32 = 40.0;

/// A4 기본값 (PAGE_DEF가 없는 비정상 문서 방어).
fn default_page() -> PageDef {
    PageDef {
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
    }
}

pub fn layout_document(
    doc: &Document,
    store: &mut FontStore,
    warnings: &mut Vec<String>,
) -> DisplayList {
    let mut pages = Vec::new();

    for section in &doc.sections {
        let page_def = section
            .section_def()
            .and_then(|d| d.page)
            .unwrap_or_else(|| {
                warnings.push("PAGE_DEF 없음 — A4 기본값 사용".to_string());
                default_page()
            });
        let (w, h) = (
            page_def.width.to_pt() as f32,
            page_def.height.to_pt() as f32,
        );
        let body_left = page_def.margin_left.to_pt() as f32;
        let body_top = (page_def.margin_top.0 + page_def.margin_header.0) as f32 / 100.0;
        let body_width =
            (page_def.width.0 - page_def.margin_left.0 - page_def.margin_right.0) as f32 / 100.0;

        let mut page = PageList {
            width_pt: w,
            height_pt: h,
            items: Vec::new(),
        };
        let mut prev_v_pos = -1i32;
        // 흐름 커서: 이 페이지에 실제 배치된 콘텐츠의 하단 y (page 좌표)
        let mut content_bottom = body_top;
        let mut skipped_controls = 0usize;
        let mut paras_on_page = 0usize;

        // 머리말/꼬리말: 구역에서 처음 정의된 것을 모든 페이지에 반복
        let mut header_ctrl = None;
        let mut footer_ctrl = None;
        for para in &section.paragraphs {
            for c in &para.controls {
                if let Control::Generic(g) = c {
                    match &g.ctrl_id {
                        b"head" if header_ctrl.is_none() => header_ctrl = Some(g),
                        b"foot" if footer_ctrl.is_none() => footer_ctrl = Some(g),
                        _ => {}
                    }
                }
            }
        }
        let furniture = Furniture {
            header: header_ctrl,
            footer: footer_ctrl,
            page_def: &page_def,
            body_left,
            body_width,
        };

        for para in &section.paragraphs {
            skipped_controls += para
                .controls
                .iter()
                .filter(|c| {
                    !matches!(
                        c,
                        Control::SectionDef(_) | Control::Table(_) | Control::Picture(_)
                    ) && ![*b"cold", *b"head", *b"foot"].contains(&c.ctrl_id())
                })
                .count();

            // 쪽 나누기 (PARA_HEADER break_type bit2 / hp:p pageBreak)
            // — 글상자만 있어 items가 비어도 문단을 거쳤으면 분할한다
            if para.header.break_type & 0x04 != 0 && paras_on_page > 0 {
                furniture.render(doc, store, &mut page, warnings);
                pages.push(std::mem::replace(
                    &mut page,
                    PageList {
                        width_pt: w,
                        height_pt: h,
                        items: Vec::new(),
                    },
                ));
                content_bottom = body_top;
                prev_v_pos = -1;
                paras_on_page = 0;
            }
            paras_on_page += 1;

            // 이 문단의 첫 줄 상단 (표 앵커 위치)
            let mut para_top: Option<f32> = None;

            if para.line_segs.is_empty() {
                // 폴백: 본문 폭에서 그리디 줄바꿈
                if para.chars.is_empty() {
                    content_bottom += 16.0; // 빈 문단 높이 근사
                } else {
                    let end = para.wchar_len();
                    let items = shape_range(store, doc, para, (0, end), warnings);
                    let max_size = items_max_size(&items).unwrap_or(10.0);
                    let baseline_y = content_bottom + max_size * 1.2;
                    para_top = Some(content_bottom);
                    let last_y = place_wrapped(
                        &mut page,
                        items,
                        body_left,
                        baseline_y,
                        body_width,
                        max_size * 1.6,
                    );
                    content_bottom = last_y + max_size * 0.4;
                }
                content_bottom = layout_para_objects(
                    doc,
                    store,
                    &mut page,
                    para,
                    body_left,
                    para_top.unwrap_or(content_bottom),
                    content_bottom,
                    warnings,
                );
                continue;
            }

            for (i, seg) in para.line_segs.iter().enumerate() {
                // 페이지 경계: v_pos 리셋 감지
                if seg.v_pos < prev_v_pos && !page.items.is_empty() {
                    furniture.render(doc, store, &mut page, warnings);
                    pages.push(std::mem::replace(
                        &mut page,
                        PageList {
                            width_pt: w,
                            height_pt: h,
                            items: Vec::new(),
                        },
                    ));
                    content_bottom = body_top;
                    paras_on_page = 0;
                }
                prev_v_pos = seg.v_pos;

                let line_start = seg.text_start;
                let line_end = para
                    .line_segs
                    .get(i + 1)
                    .map_or(para.wchar_len(), |next| next.text_start);
                if line_end <= line_start {
                    continue;
                }

                let items = shape_range(store, doc, para, (line_start, line_end), warnings);
                let natural_width: f32 = items_width(&items);

                // 정렬 보정 (가운데/오른쪽만 — 양쪽 정렬 잉여 분배는 U2)
                let seg_width_pt = seg.seg_width as f32 / 100.0;
                let align = doc
                    .header
                    .para_shapes
                    .get(para.para_shape.0 as usize)
                    .map_or(0, |ps| ps.alignment());
                let shift = match align {
                    2 => (seg_width_pt - natural_width).max(0.0), // 오른쪽
                    3 => ((seg_width_pt - natural_width) / 2.0).max(0.0), // 가운데
                    _ => 0.0,
                };

                let baseline_gap_pt = seg.baseline_gap as f32 / 100.0;
                let line_height_pt = seg.line_height as f32 / 100.0;
                let stored_baseline = body_top + (seg.v_pos + seg.baseline_gap) as f32 / 100.0;
                // 흐름 커서 보정: 앞 콘텐츠가 저장 위치를 이미 지났으면
                // 베이스라인을 (콘텐츠 하단 + 이 줄의 ascent) 아래로 밀어낸다
                let baseline_y = stored_baseline.max(content_bottom + baseline_gap_pt);

                // 문단에 lineseg가 1개뿐인데 텍스트가 폭을 넘으면 불완전한
                // lineseg로 보고 seg 폭에서 줄바꿈. 완전한 lineseg는 신뢰.
                let wrap_width = if para.line_segs.len() == 1 {
                    seg_width_pt.max(10.0)
                } else {
                    f32::INFINITY
                };
                let line_advance =
                    (seg.line_height + seg.line_spacing).max(seg.line_height) as f32 / 100.0;

                let x = body_left + seg.col_start as f32 / 100.0 + shift;
                if i == 0 {
                    para_top = Some(baseline_y - baseline_gap_pt);
                }
                let last_y =
                    place_wrapped(&mut page, items, x, baseline_y, wrap_width, line_advance);
                content_bottom = last_y + (line_height_pt - baseline_gap_pt).max(0.0);
            }

            content_bottom = layout_para_objects(
                doc,
                store,
                &mut page,
                para,
                body_left,
                para_top.unwrap_or(content_bottom),
                content_bottom,
                warnings,
            );
        }
        if skipped_controls > 0 {
            warnings.push(format!(
                "렌더 미지원 컨트롤 {skipped_controls}개 생략 (글상자/도형 등 — 후속 마일스톤)"
            ));
        }
        furniture.render(doc, store, &mut page, warnings);
        pages.push(page);
    }

    DisplayList { pages }
}

/// 기본 셀 안쪽 여백 (HWPUNIT — 한글 기본값).
const DEFAULT_CELL_MARGINS: [u16; 4] = [510, 510, 141, 141];

/// 페이지 가구 (머리말/꼬리말) — 페이지 마감 시마다 그린다.
struct Furniture<'a> {
    header: Option<&'a hwp_model::GenericControl>,
    footer: Option<&'a hwp_model::GenericControl>,
    page_def: &'a PageDef,
    body_left: f32,
    body_width: f32,
}

impl Furniture<'_> {
    fn render(
        &self,
        doc: &Document,
        store: &mut FontStore,
        page: &mut PageList,
        warnings: &mut Vec<String>,
    ) {
        if let Some(h) = self.header {
            let top = self.page_def.margin_top.to_pt() as f32;
            for list in &h.paragraph_lists {
                layout_box_paragraphs(
                    doc,
                    store,
                    page,
                    &list.paragraphs,
                    self.body_left,
                    top,
                    self.body_width,
                    warnings,
                );
            }
        }
        if let Some(f) = self.footer {
            let top = page.height_pt
                - self.page_def.margin_bottom.to_pt() as f32
                - self.page_def.margin_footer.to_pt() as f32;
            for list in &f.paragraph_lists {
                layout_box_paragraphs(
                    doc,
                    store,
                    page,
                    &list.paragraphs,
                    self.body_left,
                    top,
                    self.body_width,
                    warnings,
                );
            }
        }
    }
}

/// 문단에 달린 블록 개체(표/이미지)를 배치한다. 갱신된 콘텐츠 하단을 반환.
#[allow(clippy::too_many_arguments)]
fn layout_para_objects(
    doc: &Document,
    store: &mut FontStore,
    page: &mut PageList,
    para: &Paragraph,
    x: f32,
    anchor_top: f32,
    content_bottom: f32,
    warnings: &mut Vec<String>,
) -> f32 {
    let mut bottom = content_bottom;
    let mut object_y = anchor_top;
    for control in &para.controls {
        match control {
            Control::Table(table) => {
                let h = layout_table(doc, store, page, table, x, object_y, warnings);
                bottom = bottom.max(object_y + h);
                object_y += h; // 한 문단에 개체가 여럿이면 세로로 이어 배치
            }
            Control::Picture(pic) => {
                let (w, h) = (pic.width.to_pt() as f32, pic.height.to_pt() as f32);
                if w <= 0.0 || h <= 0.0 {
                    warnings.push("이미지 크기 정보 없음 — 생략".to_string());
                    continue;
                }
                match doc.resolve_bin(&pic.bin_ref) {
                    Some(bytes) => {
                        page.items.push(Item::Image {
                            x,
                            y: object_y,
                            w,
                            h,
                            data: std::sync::Arc::new(bytes.to_vec()),
                        });
                        bottom = bottom.max(object_y + h);
                        object_y += h;
                    }
                    None => warnings.push(format!("이미지 데이터를 찾지 못함: {:?}", pic.bin_ref)),
                }
            }
            _ => {}
        }
    }
    bottom
}

/// 표 하나를 (x, y)에 배치하고 높이를 반환한다.
fn layout_table(
    doc: &Document,
    store: &mut FontStore,
    page: &mut PageList,
    table: &Table,
    x: f32,
    y: f32,
    warnings: &mut Vec<String>,
) -> f32 {
    let cols = table.cols.max(1) as usize;
    let rows = table.rows.max(1) as usize;

    // 그리드 기하: span=1 셀에서 열 폭/행 높이를 확정, 모르는 칸은 평균으로
    let mut col_w = vec![0.0f32; cols];
    let mut row_h = vec![0.0f32; rows];
    for cell in &table.cells {
        let (c, r) = (cell.col as usize, cell.row as usize);
        if cell.col_span == 1 && c < cols {
            col_w[c] = col_w[c].max(cell.width.to_pt() as f32);
        }
        if cell.row_span == 1 && r < rows {
            row_h[r] = row_h[r].max(cell.height.to_pt() as f32);
        }
    }
    fill_unknown(&mut col_w, 60.0);
    fill_unknown(&mut row_h, 18.0);

    // 누적 오프셋
    let col_x: Vec<f32> = prefix_sums(&col_w, x);
    let row_y: Vec<f32> = prefix_sums(&row_h, y);

    for cell in &table.cells {
        let (c, r) = (cell.col as usize, cell.row as usize);
        if c >= cols || r >= rows {
            warnings.push(format!("셀 주소가 표 범위를 벗어남: ({r},{c})"));
            continue;
        }
        let cx = col_x[c];
        let cy = row_y[r];
        let cw: f32 = col_w[c..(c + cell.col_span as usize).min(cols)]
            .iter()
            .sum();
        let ch: f32 = row_h[r..(r + cell.row_span as usize).min(rows)]
            .iter()
            .sum();

        let border_fill = doc
            .header
            .border_fills
            .get((cell.border_fill.0 as usize).saturating_sub(1));

        // 1) 배경
        if let Some(bg) = border_fill.and_then(|bf| bf.visible_bg()) {
            page.items.push(Item::Rect {
                x: cx,
                y: cy,
                w: cw,
                h: ch,
                fill: bg,
            });
        }

        // 2) 내용 — 셀 여백(셀 지정 → 표 기본 → 한글 기본) 적용
        let margins = if cell.margins.iter().any(|&m| m > 0) {
            cell.margins
        } else if table.inner_margins.iter().any(|&m| m > 0) {
            table.inner_margins
        } else {
            DEFAULT_CELL_MARGINS
        };
        let (ml, mr, mt) = (
            margins[0] as f32 / 100.0,
            margins[1] as f32 / 100.0,
            margins[2] as f32 / 100.0,
        );
        layout_box_paragraphs(
            doc,
            store,
            page,
            &cell.paragraphs,
            cx + ml,
            cy + mt,
            (cw - ml - mr).max(4.0),
            warnings,
        );

        // 3) 테두리 (왼/오른/위/아래)
        if let Some(bf) = border_fill {
            let edges = [
                (cx, cy, cx, cy + ch),           // 왼
                (cx + cw, cy, cx + cw, cy + ch), // 오른
                (cx, cy, cx + cw, cy),           // 위
                (cx, cy + ch, cx + cw, cy + ch), // 아래
            ];
            for (side, (x1, y1, x2, y2)) in bf.sides.iter().zip(edges) {
                if side.is_visible() {
                    page.items.push(Item::Line {
                        x1,
                        y1,
                        x2,
                        y2,
                        color: side.color,
                        width: side.width_mm() * 72.0 / 25.4, // mm → pt
                    });
                }
            }
        }
    }
    row_h.iter().sum()
}

/// 상자(셀) 안 문단들을 배치한다. origin은 텍스트 영역 좌상단(pt).
/// 셀 내부 lineseg의 v_pos는 셀 텍스트 영역 상단 기준(본문과 동일 모델).
#[allow(clippy::too_many_arguments)]
fn layout_box_paragraphs(
    doc: &Document,
    store: &mut FontStore,
    page: &mut PageList,
    paras: &[Paragraph],
    origin_x: f32,
    origin_y: f32,
    width: f32,
    warnings: &mut Vec<String>,
) -> f32 {
    let mut content_bottom = origin_y;
    for para in paras {
        let mut para_top: Option<f32> = None;

        if para.line_segs.is_empty() {
            if para.chars.is_empty() {
                content_bottom += 12.0;
            } else {
                let end = para.wchar_len();
                let items = shape_range(store, doc, para, (0, end), warnings);
                let max_size = items_max_size(&items).unwrap_or(10.0);
                para_top = Some(content_bottom);
                let last_y = place_wrapped(
                    page,
                    items,
                    origin_x,
                    content_bottom + max_size * 1.2,
                    width,
                    max_size * 1.6,
                );
                content_bottom = last_y + max_size * 0.4;
            }
        } else {
            for (i, seg) in para.line_segs.iter().enumerate() {
                let line_start = seg.text_start;
                let line_end = para
                    .line_segs
                    .get(i + 1)
                    .map_or(para.wchar_len(), |next| next.text_start);
                if line_end <= line_start {
                    continue;
                }
                let items = shape_range(store, doc, para, (line_start, line_end), warnings);
                let natural_width = items_width(&items);

                let seg_width_pt = (seg.seg_width as f32 / 100.0).min(width);
                let align = doc
                    .header
                    .para_shapes
                    .get(para.para_shape.0 as usize)
                    .map_or(0, |ps| ps.alignment());
                let shift = match align {
                    2 => (seg_width_pt - natural_width).max(0.0),
                    3 => ((seg_width_pt - natural_width) / 2.0).max(0.0),
                    _ => 0.0,
                };

                let gap_pt = seg.baseline_gap as f32 / 100.0;
                let stored = origin_y + (seg.v_pos + seg.baseline_gap) as f32 / 100.0;
                let baseline_y = stored.max(content_bottom + gap_pt);
                if i == 0 {
                    para_top = Some(baseline_y - gap_pt);
                }
                let wrap_width = if para.line_segs.len() == 1 {
                    seg_width_pt.max(4.0)
                } else {
                    f32::INFINITY
                };
                let line_advance =
                    (seg.line_height + seg.line_spacing).max(seg.line_height) as f32 / 100.0;

                let last_y = place_wrapped(
                    page,
                    items,
                    origin_x + seg.col_start as f32 / 100.0 + shift,
                    baseline_y,
                    wrap_width,
                    line_advance,
                );
                content_bottom = last_y + (seg.line_height as f32 / 100.0 - gap_pt).max(0.0);
            }
        }

        // 셀 안의 중첩 표
        content_bottom = layout_para_objects(
            doc,
            store,
            page,
            para,
            origin_x,
            para_top.unwrap_or(content_bottom),
            content_bottom,
            warnings,
        );
    }
    content_bottom
}

fn fill_unknown(values: &mut [f32], fallback: f32) {
    let known: Vec<f32> = values.iter().copied().filter(|v| *v > 0.0).collect();
    let avg = if known.is_empty() {
        fallback
    } else {
        known.iter().sum::<f32>() / known.len() as f32
    };
    for v in values.iter_mut() {
        if *v <= 0.0 {
            *v = avg;
        }
    }
}

fn prefix_sums(values: &[f32], start: f32) -> Vec<f32> {
    let mut out = Vec::with_capacity(values.len() + 1);
    let mut acc = start;
    for v in values {
        out.push(acc);
        acc += v;
    }
    out.push(acc);
    out
}

fn items_width(items: &[InlineItem]) -> f32 {
    let mut x = 0.0f32;
    for item in items {
        match item {
            InlineItem::Run(run) => x += run.width_pt,
            InlineItem::Tab => {
                x = (x / TAB_INTERVAL_PT).floor() * TAB_INTERVAL_PT + TAB_INTERVAL_PT
            }
        }
    }
    x
}

fn items_max_size(items: &[InlineItem]) -> Option<f32> {
    items
        .iter()
        .filter_map(|i| match i {
            InlineItem::Run(r) => Some(r.size_pt),
            InlineItem::Tab => None,
        })
        .reduce(f32::max)
}

/// 글리프 런과 그 장식(밑줄/취소선)을 함께 배치한다.
/// 장식 상수(0.10em/0.25em/0.05em)는 U5 실측 전 초기값.
fn push_run(page: &mut PageList, x: f32, y: f32, run: crate::shape::ShapedRun) {
    let w = run.width_pt;
    let em = run.size_pt;
    let underline = run.underline.then(|| {
        let color = if run.underline_color == 0xFFFF_FFFF {
            run.color
        } else {
            run.underline_color
        };
        (y + em * 0.10, color)
    });
    let strike = run.strike.then_some((y - em * 0.25, run.color));
    page.items.push(Item::Glyphs { x, y, run });
    for (ly, color) in underline.into_iter().chain(strike) {
        page.items.push(Item::Line {
            x1: x,
            y1: ly,
            x2: x + w,
            y2: ly,
            color,
            width: em * 0.05,
        });
    }
}

/// 인라인 항목들을 배치한다. `max_width`를 넘으면 글리프 단위 그리디
/// 줄바꿈(`f32::INFINITY`면 비활성). 마지막 베이스라인 y를 반환한다.
fn place_wrapped(
    page: &mut PageList,
    items: Vec<InlineItem>,
    x0: f32,
    first_baseline_y: f32,
    max_width: f32,
    line_advance: f32,
) -> f32 {
    let limit = x0 + max_width;
    let mut x = x0;
    let mut y = first_baseline_y;

    if std::env::var_os("HWP_RENDER_TRACE").is_some() {
        let preview: String = items
            .iter()
            .filter_map(|i| match i {
                InlineItem::Run(r) => Some(r.text.as_str()),
                InlineItem::Tab => None,
            })
            .collect::<String>()
            .chars()
            .take(20)
            .collect();
        eprintln!("TRACE y={first_baseline_y:.1} x={x0:.1} wrap={max_width:.0} [{preview}]");
    }

    for item in items {
        match item {
            InlineItem::Run(run) => {
                if max_width.is_infinite() || x + run.width_pt <= limit {
                    let w = run.width_pt;
                    push_run(page, x, y, run);
                    x += w;
                    continue;
                }
                // 글리프 단위 분할 (CJK는 글자 사이 어디서나 분리 가능)
                let mut start = 0usize;
                let mut piece_x = x;
                let mut acc = 0.0f32;
                for (i, g) in run.glyphs.iter().enumerate() {
                    let over = piece_x + acc + g.x_advance > limit;
                    let line_has_content = i > start || piece_x > x0;
                    if over && line_has_content {
                        if i > start {
                            let piece = run.slice(start, i);
                            push_run(page, piece_x, y, piece);
                        }
                        y += line_advance;
                        piece_x = x0;
                        acc = 0.0;
                        start = i;
                    }
                    acc += g.x_advance;
                }
                if start < run.glyphs.len() {
                    let piece = run.slice(start, run.glyphs.len());
                    let w = piece.width_pt;
                    push_run(page, piece_x, y, piece);
                    x = piece_x + w;
                } else {
                    x = piece_x;
                }
            }
            InlineItem::Tab => {
                let rel = x - x0;
                x = x0 + (rel / TAB_INTERVAL_PT).floor() * TAB_INTERVAL_PT + TAB_INTERVAL_PT;
            }
        }
    }
    y
}

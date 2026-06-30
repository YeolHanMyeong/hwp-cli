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

use crate::display::{DisplayList, Item, PageList, PathCmd, Stroke};
use crate::fonts::FontStore;
use crate::footnote::{self, Note};
use crate::shape::{InlineItem, shape_range, shape_range_notes};

/// 기본 탭 간격 (40pt = 4000 HWPUNIT).
const TAB_INTERVAL_PT: f32 = 40.0;

/// 연결 글상자 후보가 없을 때의 단 사이 가로 간격 근사값(pt).
const COL_GAP_PT: f32 = 14.0;

/// 글상자 내부 문단을 단(컬럼)별 범위로 나눈다. 한 줄의 v_pos가 직전 줄보다 작아지면
/// (한컴이 단 나누기로 흘린 것) 새 단으로 본다. 줄 배치 없는 문단은 현재 단에 둔다.
fn split_columns(paras: &[&Paragraph]) -> Vec<std::ops::Range<usize>> {
    let mut cols = Vec::new();
    let mut start = 0usize;
    let mut prev: Option<i32> = None;
    for (i, p) in paras.iter().enumerate() {
        if let Some(v) = p.line_segs.first().map(|s| s.v_pos) {
            if prev.is_some_and(|pv| v < pv) {
                cols.push(start..i);
                start = i;
            }
            prev = Some(v);
        }
    }
    cols.push(start..paras.len());
    cols
}

/// 연결 글상자의 이음단 위치(pt). 같은 폭·높이·세로오프셋을 갖고 더 오른쪽에 있는
/// 떠 있는 gso 박스들을 가로 순으로 모은다(연결 글상자 = 다음 단).
fn continuation_columns(para: &Paragraph, base: &crate::gso::GsoBox) -> Vec<(f32, f32)> {
    const TOL: i32 = 200; // 2pt 허용
    let mut v: Vec<(f32, f32)> = para
        .controls
        .iter()
        .filter_map(|c| match c {
            Control::Generic(g) if g.ctrl_id == *b"gso " => {
                let b = crate::gso::parse_gso_box(&g.data)?;
                let same = (b.width - base.width).abs() < TOL
                    && (b.height - base.height).abs() < TOL
                    && (b.vert_offset - base.vert_offset).abs() < TOL;
                (same && !b.treat_as_char() && b.horz_offset > base.horz_offset)
                    .then_some((b.horz_offset as f32 / 100.0, b.vert_offset as f32 / 100.0))
            }
            _ => None,
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    v.dedup();
    v
}

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
        // 본문 영역 하한 (넘침 분할 기준)
        let body_bottom = h - (page_def.margin_bottom.0 + page_def.margin_footer.0) as f32 / 100.0;

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

        // 각주/미주: 구역 전체에 번호를 매기고, 페이지마다 앵커가 든 노트를 모아
        // 하단에 그린다.
        let notes = footnote::collect_notes(&section.paragraphs);
        let mut page_notes: Vec<&Note> = Vec::new();

        for para in &section.paragraphs {
            skipped_controls += para
                .controls
                .iter()
                .filter(|c| {
                    let rendered = matches!(
                        c,
                        Control::SectionDef(_) | Control::Table(_) | Control::Picture(_)
                    ) || [*b"cold", *b"head", *b"foot", *b"fn  ", *b"en  "]
                        .contains(&c.ctrl_id())
                        // 글상자(텍스트) + 도형(선/사각형/타원/호/다각형)은 렌더한다.
                        || matches!(c, Control::Generic(g)
                            if g.ctrl_id == *b"gso "
                                && (!g.paragraph_lists.is_empty()
                                    || crate::shape_draw::has_shape(&g.raw_children)))
                        // hwpx 구조화 도형(rect/ellipse/...).
                        || matches!(c, Control::Generic(g) if !g.gso_shapes.is_empty())
                        // 수식(hp:equation).
                        || matches!(c, Control::Generic(g) if g.equation.is_some());
                    !rendered
                })
                .count();

            // 본문 넘침: 직전 콘텐츠가 본문 하한을 지났으면 새 페이지
            // (lineseg 없는 생성 문서의 기본 페이지네이션)
            if content_bottom > body_bottom && paras_on_page > 0 {
                render_page_notes(
                    doc, store, &mut page, &page_notes, body_left, body_width, body_bottom,
                    warnings,
                );
                page_notes.clear();
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

            // 쪽 나누기 (PARA_HEADER break_type bit2 / hp:p pageBreak)
            // — 글상자만 있어 items가 비어도 문단을 거쳤으면 분할한다
            if para.header.break_type & 0x04 != 0 && paras_on_page > 0 {
                render_page_notes(
                    doc, store, &mut page, &page_notes, body_left, body_width, body_bottom,
                    warnings,
                );
                page_notes.clear();
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

            // 본문 각주/미주 마커(윗첨자 번호)와 이 페이지에 속할 노트 수집.
            let marks = footnote::para_marks(&notes, para);
            page_notes.extend(footnote::para_notes(&notes, para));

            // 이 문단의 첫 줄 상단 (표 앵커 위치)
            let mut para_top: Option<f32> = None;

            if para.line_segs.is_empty() {
                // 폴백: 본문 폭에서 그리디 줄바꿈
                if para.chars.is_empty() {
                    content_bottom += 16.0; // 빈 문단 높이 근사
                } else {
                    let end = para.wchar_len();
                    let items = shape_range_notes(store, doc, para, (0, end), &marks, warnings);
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

            let last_content = last_content_seg(para);
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

                let mut items =
                    shape_range_notes(store, doc, para, (line_start, line_end), &marks, warnings);
                let natural_width: f32 = items_width(&items);

                // 정렬 보정 (가운데/오른쪽 + 양쪽정렬은 마지막 줄 빼고 글자 사이로 잉여 분배).
                let seg_width_pt = seg.seg_width as f32 / 100.0;
                let align = doc
                    .header
                    .para_shapes
                    .get(para.para_shape.0 as usize)
                    .map_or(0, |ps| ps.alignment());
                let shift = align_line(
                    &mut items,
                    align,
                    seg_width_pt,
                    natural_width,
                    i == last_content,
                );

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
        render_page_notes(
            doc, store, &mut page, &page_notes, body_left, body_width, body_bottom, warnings,
        );
        page_notes.clear();
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

/// 페이지 하단에 각주/미주 영역을 그린다(구분선 + 번호 + 내용).
/// 블록 하단이 본문 하한(body_bottom)에 닿도록 위로 올려 배치한다.
#[allow(clippy::too_many_arguments)]
fn render_page_notes(
    doc: &Document,
    store: &mut FontStore,
    page: &mut PageList,
    notes: &[&Note],
    body_left: f32,
    body_width: f32,
    body_bottom: f32,
    warnings: &mut Vec<String>,
) {
    if notes.is_empty() {
        return;
    }
    // 1) 스크래치 페이지에 y=0부터 노트를 쌓아 총 높이를 잰다.
    let mut scratch = PageList {
        width_pt: page.width_pt,
        height_pt: page.height_pt,
        items: Vec::new(),
    };
    let mut y = 0.0f32;
    for note in notes {
        y = render_one_note(doc, store, &mut scratch, note, body_left, body_width, y, warnings);
        y += 3.0; // 노트 사이 간격
    }
    // 2) 블록 하단이 body_bottom에 닿도록 위로 올린다(본문과 겹치면 그대로 둠).
    let top = (body_bottom - y).max(0.0);
    let sep_gap = 5.0;
    page.items.push(Item::Line {
        x1: body_left,
        y1: top - sep_gap,
        x2: body_left + body_width * 0.34,
        y2: top - sep_gap,
        color: 0x0000_0000,
        width: 0.5,
    });
    // 3) 스크래치 아이템을 top만큼 내려 본 페이지에 합친다.
    for item in scratch.items.drain(..) {
        page.items.push(translate_item(item, 0.0, top));
    }
}

/// 노트 하나(번호 마커 + 내용 문단)를 (x, y)에 그리고 다음 y(하단)를 반환.
#[allow(clippy::too_many_arguments)]
fn render_one_note(
    doc: &Document,
    store: &mut FontStore,
    page: &mut PageList,
    note: &Note,
    x: f32,
    width: f32,
    y: f32,
    warnings: &mut Vec<String>,
) -> f32 {
    let marker_size = 8.0;
    let indent = 16.0_f32.min(width * 0.25);
    let label = format!("{})", note.number);
    let baseline = y + marker_size;
    if let Some(run) = crate::shape::shape_plain(store, doc, &label, marker_size, 0) {
        page.items.push(Item::Glyphs {
            x,
            y: baseline,
            run,
        });
    }
    // 내용 문단들(자체 char_shape 크기 사용). 여러 문단은 세로로 누적.
    let mut bottom = y;
    for list in &note.content.paragraph_lists {
        bottom = layout_box_paragraphs(
            doc,
            store,
            page,
            &list.paragraphs,
            x + indent,
            bottom,
            width - indent,
            warnings,
        );
    }
    bottom.max(baseline + marker_size * 0.3)
}

/// Item을 (dx, dy)만큼 평행이동한 사본.
fn translate_item(item: Item, dx: f32, dy: f32) -> Item {
    match item {
        Item::Glyphs { x, y, run } => Item::Glyphs {
            x: x + dx,
            y: y + dy,
            run,
        },
        Item::Rect { x, y, w, h, fill } => Item::Rect {
            x: x + dx,
            y: y + dy,
            w,
            h,
            fill,
        },
        Item::Line {
            x1,
            y1,
            x2,
            y2,
            color,
            width,
        } => Item::Line {
            x1: x1 + dx,
            y1: y1 + dy,
            x2: x2 + dx,
            y2: y2 + dy,
            color,
            width,
        },
        Item::Image { x, y, w, h, data } => Item::Image {
            x: x + dx,
            y: y + dy,
            w,
            h,
            data,
        },
        Item::Path {
            commands,
            fill,
            stroke,
        } => Item::Path {
            commands: commands
                .into_iter()
                .map(|c| translate_cmd(c, dx, dy))
                .collect(),
            fill,
            stroke,
        },
    }
}

/// PathCmd를 (dx, dy)만큼 평행이동.
fn translate_cmd(c: PathCmd, dx: f32, dy: f32) -> PathCmd {
    match c {
        PathCmd::MoveTo(x, y) => PathCmd::MoveTo(x + dx, y + dy),
        PathCmd::LineTo(x, y) => PathCmd::LineTo(x + dx, y + dy),
        PathCmd::CubicTo(a, b, c, d, e, f) => {
            PathCmd::CubicTo(a + dx, b + dy, c + dx, d + dy, e + dx, f + dy)
        }
        PathCmd::Close => PathCmd::Close,
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
            // 글상자(text box): 텍스트 있는 gso 개체의 내부 문단을 박스 영역에 배치.
            Control::Generic(g) if g.ctrl_id == *b"gso " && !g.paragraph_lists.is_empty() => {
                let Some(b) = crate::gso::parse_gso_box(&g.data) else {
                    continue;
                };
                let bw = (b.width as f32 / 100.0).max(8.0);
                let bh = b.height as f32 / 100.0;
                // 글자처럼취급=흐름 위치, 떠 있음=PAPER/PAGE 기준 페이지 절대 위치.
                let (bx, by, inline) = if b.treat_as_char() {
                    (x, object_y, true)
                } else {
                    (
                        b.horz_offset as f32 / 100.0,
                        b.vert_offset as f32 / 100.0,
                        false,
                    )
                };

                // 글상자 자체 테두리/배경(사각형 프레임)을 텍스트 뒤에 먼저 그린다.
                let frame_origin = if inline {
                    (bx as f64 * 100.0, by as f64 * 100.0)
                } else {
                    (b.horz_offset as f64, b.vert_offset as f64)
                };
                crate::shape_draw::draw_gso_shapes(g, frame_origin, doc, page, warnings);

                // 다단/연결 글상자: 내부 문단의 v_pos 리셋(단 나누기)으로 단을 분할한다.
                // 단 0은 이 박스, 단 1+는 연결 글상자(같은 크기·세로위치, 더 오른쪽
                // 떠 있는 gso 박스) 위치로 흐른다. 없으면 가로로 한 단 진행(근사).
                let flat: Vec<&Paragraph> =
                    g.paragraph_lists.iter().flat_map(|l| l.paragraphs.iter()).collect();
                let columns = split_columns(&flat);
                let cont = if columns.len() > 1 && !inline {
                    continuation_columns(para, &b)
                } else {
                    Vec::new()
                };

                let mut max_bottom = by;
                for (k, range) in columns.iter().enumerate() {
                    let (cx, cy) = if k == 0 {
                        (bx, by)
                    } else if let Some(&o) = cont.get(k - 1) {
                        o
                    } else {
                        (bx + k as f32 * (bw + COL_GAP_PT), by)
                    };
                    let inner = layout_box_para_iter(
                        doc,
                        store,
                        page,
                        flat[range.clone()].iter().copied(),
                        cx,
                        cy,
                        bw,
                        warnings,
                    );
                    max_bottom = max_bottom.max(inner);
                }

                if inline {
                    let used = (max_bottom - by).max(bh);
                    bottom = bottom.max(by + used);
                    object_y += used;
                }
            }
            // 순수 도형 (텍스트 없는 gso): 선/사각형/타원/호/다각형.
            Control::Generic(g)
                if g.ctrl_id == *b"gso "
                    && g.paragraph_lists.is_empty()
                    && crate::shape_draw::has_shape(&g.raw_children) =>
            {
                let Some(b) = crate::gso::parse_gso_box(&g.data) else {
                    continue;
                };
                let origin = if b.treat_as_char() {
                    (x as f64 * 100.0, object_y as f64 * 100.0)
                } else {
                    (b.horz_offset as f64, b.vert_offset as f64)
                };
                crate::shape_draw::draw_gso_shapes(g, origin, doc, page, warnings);
            }
            // hwpx 구조화 도형(rect/ellipse/line/polygon/curve).
            Control::Generic(g) if !g.gso_shapes.is_empty() => {
                crate::shape_draw::draw_ir_shapes(&g.gso_shapes, page);
            }
            // 수식(hp:equation) — 상자+스크립트 텍스트로 근사.
            Control::Generic(g) if g.equation.is_some() => {
                let eq = g.equation.as_ref().expect("is_some");
                let w = (eq.width as f32 / 100.0).max(24.0);
                let h = (eq.height as f32 / 100.0).max(14.0);
                let (bx, by, inline) = if eq.inline {
                    (x, object_y, true)
                } else {
                    (eq.x as f32 / 100.0, eq.y as f32 / 100.0, false)
                };
                // 옅은 회색 점선 상자(수식 영역).
                page.items.push(Item::Path {
                    commands: vec![
                        PathCmd::MoveTo(bx, by),
                        PathCmd::LineTo(bx + w, by),
                        PathCmd::LineTo(bx + w, by + h),
                        PathCmd::LineTo(bx, by + h),
                        PathCmd::Close,
                    ],
                    fill: None,
                    stroke: Some(Stroke {
                        color: 0x00C0_C0C0,
                        width: 0.5,
                        dash: vec![2.0, 2.0],
                    }),
                });
                // 스크립트를 사람이 읽을 수 있게 근사 변환해 상자 안에 배치.
                let text = prettify_equation(&eq.script);
                if !text.is_empty() {
                    let size = (h * 0.55).clamp(7.0, 14.0);
                    if let Some(run) = crate::shape::shape_plain(store, doc, &text, size, 0) {
                        let ty = by + h * 0.5 + size * 0.33;
                        push_run(page, bx + 2.0, ty, run);
                    }
                }
                if inline {
                    object_y += h;
                    bottom = bottom.max(by + h);
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

            // 4) 대각선/역대각선 — slash/backSlash 비트가 켜졌을 때만(병합 셀은 전체 영역 가로지름).
            //    diagonal 선은 스타일/색만 제공하므로 방향 비트가 없으면 그리지 않는다.
            let (slash, backslash) = diagonal_dirs(bf.attr);
            if (slash || backslash) && bf.diagonal.is_visible() {
                let dw = bf.diagonal.width_mm() * 72.0 / 25.4;
                if backslash {
                    page.items.push(Item::Line {
                        x1: cx,
                        y1: cy,
                        x2: cx + cw,
                        y2: cy + ch,
                        color: bf.diagonal.color,
                        width: dw,
                    });
                }
                if slash {
                    page.items.push(Item::Line {
                        x1: cx,
                        y1: cy + ch,
                        x2: cx + cw,
                        y2: cy,
                        color: bf.diagonal.color,
                        width: dw,
                    });
                }
            }
        }
    }
    row_h.iter().sum()
}

/// BORDER_FILL 속성 비트 → (대각선 `/`, 역대각선 `\`) 그릴지.
/// slash=bit2~4, backSlash=bit5~7. 둘 다 0이면 대각선 없음.
fn diagonal_dirs(attr: u16) -> (bool, bool) {
    let slash = (attr >> 2) & 0x7 != 0;
    let backslash = (attr >> 5) & 0x7 != 0;
    (slash, backslash)
}

/// HWP 수식 스크립트를 사람이 읽을 수 있는 근사 문자열로 변환한다.
/// 그룹 중괄호는 제거, 그리스/연산자 토큰을 유니코드 기호로 매핑한다.
/// (정밀 수식 조판이 아닌 box+text 근사 — 가독성 우선.)
fn prettify_equation(script: &str) -> String {
    let spaced = script.replace(['{', '}'], " ");
    spaced
        .split_whitespace()
        .map(map_eqn_token)
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// 수식 토큰 하나 → 유니코드 기호(모르면 원문). 빈 문자열=버림(글꼴 명령 등).
fn map_eqn_token(tok: &str) -> String {
    let mapped = match tok {
        "over" => "/",
        "times" => "×",
        "div" => "÷",
        "cdot" => "·",
        "pm" => "±",
        "mp" => "∓",
        "sqrt" | "root" => "√",
        "sum" | "SUM" => "Σ",
        "int" | "INT" => "∫",
        "prod" | "PROD" => "∏",
        "inf" | "infinity" => "∞",
        "partial" => "∂",
        "nabla" => "∇",
        "<=" | "leq" => "≤",
        ">=" | "geq" => "≥",
        "!=" | "neq" => "≠",
        "~=" | "approx" => "≈",
        "->" | "to" | "rightarrow" => "→",
        "<-" | "leftarrow" => "←",
        "in" => "∈",
        "notin" => "∉",
        "degree" => "°",
        "dot" => "·",
        "alpha" => "α",
        "beta" => "β",
        "gamma" => "γ",
        "delta" => "δ",
        "epsilon" => "ε",
        "theta" => "θ",
        "lambda" => "λ",
        "mu" => "μ",
        "nu" => "ν",
        "pi" => "π",
        "rho" => "ρ",
        "sigma" => "σ",
        "tau" => "τ",
        "phi" => "φ",
        "psi" => "ψ",
        "omega" => "ω",
        "GAMMA" => "Γ",
        "DELTA" => "Δ",
        "THETA" => "Θ",
        "LAMBDA" => "Λ",
        "PI" => "Π",
        "SIGMA" => "Σ",
        "PHI" => "Φ",
        "PSI" => "Ψ",
        "OMEGA" => "Ω",
        // 글꼴/그룹 명령은 버린다.
        "LEFT" | "RIGHT" | "rm" | "it" | "bold" | "ITALIC" => "",
        other => return other.to_string(),
    };
    mapped.to_string()
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
    layout_box_para_iter(doc, store, page, paras.iter(), origin_x, origin_y, width, warnings)
}

/// `layout_box_paragraphs`의 반복자 버전 — 단(컬럼)으로 분할된 조각도 받는다.
///
/// 캐시된 lineseg v_pos는 한컴 배치 그대로 존중한다(흐름 커서로 끌어내리지 않음 —
/// 끌어내리면 키 큰 글상자에서 줄마다 드리프트가 누적돼 페이지 밖으로 넘친다).
/// `flow_floor`는 "흐름으로 배치된 콘텐츠"(캐시 없는 폴백 문단, 표/이미지 블록 개체,
/// 우리 줄바꿈이 캐시와 어긋나 캐시 자리 아래로 넘친 줄)만 바닥을 올려, 뒤따르는
/// 캐시 문단이 그 위로 겹치지 않게 한다.
#[allow(clippy::too_many_arguments)]
fn layout_box_para_iter<'a>(
    doc: &Document,
    store: &mut FontStore,
    page: &mut PageList,
    paras: impl Iterator<Item = &'a Paragraph>,
    origin_x: f32,
    origin_y: f32,
    width: f32,
    warnings: &mut Vec<String>,
) -> f32 {
    let mut content_bottom = origin_y;
    // 흐름 하한: 캐시 줄은 올리지 않고, 흐름 배치 콘텐츠만 올린다 (함수 doc 참고).
    let mut flow_floor = origin_y;
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
            // 폴백(캐시 없는) 문단은 흐름 배치 — 이후 캐시 문단이 넘지 않게 바닥을 올린다.
            flow_floor = flow_floor.max(content_bottom);
        } else {
            let last_content = last_content_seg(para);
            for (i, seg) in para.line_segs.iter().enumerate() {
                let line_start = seg.text_start;
                let line_end = para
                    .line_segs
                    .get(i + 1)
                    .map_or(para.wchar_len(), |next| next.text_start);
                if line_end <= line_start {
                    continue;
                }
                let mut items = shape_range(store, doc, para, (line_start, line_end), warnings);
                let natural_width = items_width(&items);

                let seg_width_pt = (seg.seg_width as f32 / 100.0).min(width);
                let align = doc
                    .header
                    .para_shapes
                    .get(para.para_shape.0 as usize)
                    .map_or(0, |ps| ps.alignment());
                let shift = align_line(
                    &mut items,
                    align,
                    seg_width_pt,
                    natural_width,
                    i == last_content,
                );

                let gap_pt = seg.baseline_gap as f32 / 100.0;
                let stored = origin_y + (seg.v_pos + seg.baseline_gap) as f32 / 100.0;
                // 캐시 v_pos를 존중: 흐름 하한 위로만 보정(흐름 커서로 끌어내리지 않음).
                let baseline_y = stored.max(flow_floor + gap_pt);
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
                // 우리 줄바꿈이 캐시와 어긋나 이 줄이 캐시 자리 아래로 넘쳤다면(단일 seg
                // 문단의 추가 줄바꿈 등) 흐름 하한을 올려 다음 캐시 문단 겹침을 막는다.
                // 다중 seg 줄은 wrap_width=INFINITY → last_y == baseline_y → 올리지 않음.
                if last_y > baseline_y {
                    flow_floor = flow_floor.max(content_bottom);
                }
            }
        }

        // 셀 안의 중첩 표/이미지 — 바닥을 늘렸으면 흐름 하한도 올려 후속 캐시 문단 겹침 방지.
        let before_objects = content_bottom;
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
        if content_bottom > before_objects {
            flow_floor = flow_floor.max(content_bottom);
        }
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

/// 문단에서 텍스트가 있는(line_end>line_start) 마지막 seg 인덱스. 빈 trailing seg 방어.
fn last_content_seg(para: &Paragraph) -> usize {
    let n = para.line_segs.len();
    (0..n)
        .rev()
        .find(|&j| {
            let ls = para.line_segs[j].text_start;
            let le = para
                .line_segs
                .get(j + 1)
                .map_or(para.wchar_len(), |s| s.text_start);
            le > ls
        })
        .unwrap_or(n.saturating_sub(1))
}

/// 정렬에 따른 가로 shift(pt). 양쪽/배분/나눔(0/4/5)이고 마지막 줄이 아니면
/// items의 글리프 advance를 늘려 줄을 seg_width까지 채우고 shift 0을 반환한다.
fn align_line(items: &mut [InlineItem], align: u8, seg_width: f32, natural: f32, is_last: bool) -> f32 {
    match align {
        2 => (seg_width - natural).max(0.0),       // 오른쪽
        3 => ((seg_width - natural) / 2.0).max(0.0), // 가운데
        0 | 4 | 5 if !is_last => {
            // 잉여 폭 분배. 폰트 부재 등으로 natural이 비정상이면 캡(≤100% stretch)으로 폭주 방지.
            let slack = (seg_width - natural).max(0.0).min(natural.max(1.0));
            justify_line(items, slack);
            0.0
        }
        _ => 0.0,
    }
}

/// 양쪽 정렬: 잉여 폭 slack을 분배. 줄에 **공백이 있으면 공백에만**(단어 사이 벌림),
/// 없으면 전 글자 사이에 균등 분배한다. 후행 공백(마지막 보이는 글리프 뒤)에는 분배하지
/// 않아 보이는 텍스트가 오른쪽 끝에 닿도록 한다. 글리프↔글자는 CJK 1:1 가정.
fn justify_line(items: &mut [InlineItem], slack: f32) {
    if slack <= 0.0 {
        return;
    }
    // 줄 전체 글리프의 공백 여부(런 내 글자 순서 매핑).
    let mut is_space: Vec<bool> = Vec::new();
    for item in items.iter() {
        if let InlineItem::Run(run) = item {
            let mut chars = run.text.chars();
            for _ in 0..run.glyphs.len() {
                is_space.push(chars.next().is_some_and(|c| c.is_whitespace()));
            }
        }
    }
    let total = is_space.len();
    if total < 2 {
        return;
    }
    // 마지막 보이는(비공백) 글리프 — 그 이후엔 분배하지 않는다.
    let last_visible = is_space.iter().rposition(|&s| !s).unwrap_or(total - 1);
    let space_count = is_space[..last_visible].iter().filter(|&&s| s).count();

    // 공백 우선; 없으면 전 글자 사이(마지막 보이는 글리프 전까지의 gap).
    let use_spaces = space_count > 0;
    let denom = if use_spaces {
        space_count as f32
    } else {
        last_visible.max(1) as f32
    };
    let extra = slack / denom;

    let mut gi = 0usize;
    for item in items.iter_mut() {
        if let InlineItem::Run(run) = item {
            let mut added = 0.0;
            for g in run.glyphs.iter_mut() {
                let apply = if use_spaces {
                    is_space[gi] && gi < last_visible
                } else {
                    gi < last_visible
                };
                if apply {
                    g.x_advance += extra;
                    added += extra;
                }
                gi += 1;
            }
            run.width_pt += added;
        }
    }
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

#[cfg(test)]
mod diagonal_tests {
    use super::diagonal_dirs;

    #[test]
    fn 수식_스크립트_근사() {
        use super::prettify_equation;
        // 중괄호 제거 + 그리스/연산자/기호 매핑.
        assert_eq!(prettify_equation("x ^{2} + y"), "x ^ 2 + y");
        assert_eq!(prettify_equation("alpha + beta times gamma"), "α + β × γ");
        assert_eq!(prettify_equation("sqrt {x over y}"), "√ x / y");
        assert_eq!(prettify_equation("a <= b >= c != d"), "a ≤ b ≥ c ≠ d");
        // 글꼴/그룹 명령은 버림.
        assert_eq!(prettify_equation("LEFT ( a RIGHT )"), "( a )");
        // 모르는 토큰은 원문 유지.
        assert_eq!(prettify_equation("foo_bar"), "foo_bar");
    }

    #[test]
    fn 대각선_방향_비트() {
        // 둘 다 0 → 대각선 없음.
        assert_eq!(diagonal_dirs(0), (false, false));
        // 3D/그림자(bit0,1)만 켜져도 대각선 아님.
        assert_eq!(diagonal_dirs(0b11), (false, false));
        // slash(bit2~4) → `/`.
        assert_eq!(diagonal_dirs(0x4), (true, false));
        // backSlash(bit5~7) → `\`.
        assert_eq!(diagonal_dirs(0x20), (false, true));
        // 둘 다(X자).
        assert_eq!(diagonal_dirs(0x4 | 0x20), (true, true));
    }
}

#[cfg(test)]
mod justify_tests {
    use super::*;
    use crate::fonts::LoadedFont;
    use crate::shape::Glyph;
    use std::sync::Arc;

    fn run(advs: &[f32]) -> InlineItem {
        run_t("", advs)
    }

    fn run_t(text: &str, advs: &[f32]) -> InlineItem {
        let glyphs: Vec<Glyph> = advs
            .iter()
            .map(|&a| Glyph {
                id: 0,
                x_advance: a,
                x_offset: 0.0,
                y_offset: 0.0,
            })
            .collect();
        InlineItem::Run(crate::shape::ShapedRun {
            font: Arc::new(LoadedFont {
                data: Arc::new(Vec::new()),
                index: 0,
                family: String::new(),
            }),
            size_pt: 10.0,
            x_scale: 1.0,
            color: 0,
            bold: false,
            italic: false,
            underline: false,
            strike: false,
            underline_color: 0xFFFF_FFFF,
            shade_color: 0xFFFF_FFFF,
            shadow: None,
            glyphs,
            width_pt: advs.iter().sum(),
            text: text.to_string(),
            start_wchar: 0,
        })
    }

    fn total_adv(items: &[InlineItem]) -> f32 {
        items
            .iter()
            .map(|i| match i {
                InlineItem::Run(r) => r.glyphs.iter().map(|g| g.x_advance).sum(),
                InlineItem::Tab => 0.0,
            })
            .sum()
    }

    #[test]
    fn 양쪽정렬_잉여를_글자사이에_분배() {
        // natural 30, seg_width 45 → slack 15, 마지막 제외 2개에 7.5씩.
        let mut items = vec![run(&[10.0, 10.0, 10.0])];
        let shift = align_line(&mut items, 0, 45.0, 30.0, false);
        assert_eq!(shift, 0.0);
        assert!((total_adv(&items) - 45.0).abs() < 0.01, "줄이 seg_width를 채워야");
        if let InlineItem::Run(r) = &items[0] {
            assert!((r.glyphs[0].x_advance - 17.5).abs() < 0.01);
            assert!((r.glyphs[2].x_advance - 10.0).abs() < 0.01, "마지막 글리프는 불변");
            assert!((r.width_pt - 45.0).abs() < 0.01, "width_pt 갱신");
        }
    }

    #[test]
    fn 공백이_있으면_공백에만_분배() {
        // "ab cd" 5글자, glyph 5개. 공백(인덱스2)에만 slack 전부.
        let mut items = vec![run_t("ab cd", &[10.0, 10.0, 5.0, 10.0, 10.0])];
        align_line(&mut items, 0, 60.0, 45.0, false); // slack 15
        if let InlineItem::Run(r) = &items[0] {
            assert!((r.glyphs[2].x_advance - 20.0).abs() < 0.01, "공백 5+15=20");
            assert!((r.glyphs[0].x_advance - 10.0).abs() < 0.01, "글자 불변");
            assert!((r.glyphs[4].x_advance - 10.0).abs() < 0.01, "글자 불변");
        }
        assert!((total_adv(&items) - 60.0).abs() < 0.01);
    }

    #[test]
    fn 후행_공백엔_분배안함() {
        // "ab " 끝 공백 → 보이는 텍스트가 끝까지 닿도록 공백 없는 줄처럼 전 글자 분배.
        let mut items = vec![run_t("ab ", &[10.0, 10.0, 5.0])];
        align_line(&mut items, 0, 40.0, 25.0, false); // slack 15
        if let InlineItem::Run(r) = &items[0] {
            // 후행 공백(idx2)은 분배 제외, last_visible=1 → gap 1개(idx0)에 15.
            assert!((r.glyphs[0].x_advance - 25.0).abs() < 0.01, "{}", r.glyphs[0].x_advance);
            assert!((r.glyphs[2].x_advance - 5.0).abs() < 0.01, "후행 공백 불변");
        }
    }

    #[test]
    fn 마지막_줄은_늘리지_않음() {
        let mut items = vec![run(&[10.0, 10.0, 10.0])];
        align_line(&mut items, 0, 45.0, 30.0, true);
        assert!((total_adv(&items) - 30.0).abs() < 0.01, "마지막 줄은 ragged 유지");
    }

    #[test]
    fn 가운데_오른쪽은_shift만() {
        let mut center = vec![run(&[10.0, 10.0])];
        assert!((align_line(&mut center, 3, 40.0, 20.0, false) - 10.0).abs() < 0.01);
        assert!((total_adv(&center) - 20.0).abs() < 0.01, "가운데는 advance 불변");
        let mut right = vec![run(&[10.0, 10.0])];
        assert!((align_line(&mut right, 2, 40.0, 20.0, false) - 20.0).abs() < 0.01);
    }
}

//! 렌더링 스모크 테스트.
//!
//! 픽셀 골든 비교는 폰트 가용성에 좌우되므로(CI 폰트 고정은 M7),
//! 여기서는 구조적 불변식만 검증한다: 페이지 수/크기, 텍스트 영역에
//! 어두운 픽셀 존재, 본문 영역 밖은 흰색.

use std::path::PathBuf;

use hwp_render::{RenderOptions, render_document};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel)
}

/// fixture 문서는 저장소에 없으므로(로컬 전용 — fixtures/README.md) 없으면 건너뛴다.
fn fixture_or_skip(rel: &str) -> Option<PathBuf> {
    let p = fixture(rel);
    if !p.exists() {
        eprintln!(
            "스킵: fixture 없음 ({}) — fixtures/README.md 참고",
            p.display()
        );
        return None;
    }
    Some(p)
}

/// 어두운 픽셀(텍스트) 수를 센다.
fn dark_pixels(pixmap: &tiny_skia::Pixmap) -> usize {
    pixmap
        .pixels()
        .iter()
        .filter(|p| p.red() < 128 && p.green() < 128 && p.blue() < 128)
        .count()
}

#[test]
fn hello_world_렌더() {
    let Some(path) = fixture_or_skip("hwp5/hello_world.hwp") else {
        return;
    };
    let doc = hwp5::read_document(&path).unwrap().document;
    let out = render_document(
        &doc,
        &RenderOptions {
            dpi: 96.0,
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(out.pages.len(), 1);
    let page = &out.pages[0];
    // A4 @96dpi: 59528/7200*96 ≈ 793.7 → 794
    assert_eq!(page.width(), 794);
    assert_eq!(page.height(), 1123);

    // "Hello World!" 텍스트가 그려졌는지 (시스템에 폰트가 하나라도 있으면)
    let dark = dark_pixels(page);
    assert!(dark > 100, "텍스트 픽셀이 너무 적음: {dark}");

    // 본문 영역 밖(여백)은 흰색이어야 한다 — 좌상단 모서리
    let corner = page.pixel(5, 5).unwrap();
    assert_eq!(
        (corner.red(), corner.green(), corner.blue()),
        (255, 255, 255)
    );
}

#[test]
fn hwpx_폴백_렌더() {
    // minimal.hwpx의 문단 대부분은 lineseg가 없다 — 폴백 경로 검증
    let Some(path) = fixture_or_skip("hwpx/minimal.hwpx") else {
        return;
    };
    let doc = hwpx::read_document(&path).unwrap().document;
    let out = render_document(&doc, &RenderOptions::default()).unwrap();
    assert_eq!(out.pages.len(), 1);
    assert!(
        dark_pixels(&out.pages[0]) > 500,
        "세 문단이 모두 그려져야 한다"
    );
}

#[test]
fn 표_렌더() {
    let Some(path) = fixture_or_skip("hwp5/work_report.hwp") else {
        return;
    };
    let doc = hwp5::read_document(&path).unwrap().document;
    let out = render_document(&doc, &RenderOptions::default()).unwrap();
    assert_eq!(out.pages.len(), 1);
    let page = &out.pages[0];

    // 표 테두리 + 셀 텍스트로 어두운 픽셀이 충분해야 한다
    assert!(
        dark_pixels(page) > 5_000,
        "표 선·텍스트: {}",
        dark_pixels(page)
    );

    // 표·머리말·꼬리말은 더 이상 미지원으로 집계되지 않는다 (글상자 1개만 남음)
    let skipped: Vec<_> = out
        .report
        .iter()
        .filter(|w| w.contains("미지원 컨트롤"))
        .collect();
    assert!(
        skipped.iter().all(|w| w.contains("1개")),
        "표/머리말이 미지원으로 집계됨: {skipped:?}"
    );
}

/// 멀티페이지 문서의 합성 줄 배치는 페이지마다 v_pos 가 0 으로 리셋(페이지 상대)
/// 되어야 한다. 리셋 없이 섹션 단조 누적하면 v_pos 가 페이지 본문 높이를 한참
/// 초과해(정품은 페이지 상대) 한글이 '손상'으로 판정한다(커밋 29014b0).
/// 폰트 없이도(문단당 1줄) 페이지 분할 로직만 검증한다.
#[test]
fn 멀티페이지_lineseg_페이지_상대_v_pos() {
    let md: String = (1..=120)
        .map(|i| format!("{i}번째 문단입니다. 페이지를 넘기기 위한 본문.\n\n"))
        .collect();
    let mut doc = hwp_convert::from_markdown(&md);

    let page = doc.sections[0].section_def().unwrap().page.unwrap();
    let content_h = page.height.0 - page.margin_top.0 - page.margin_bottom.0;

    let mut store = hwp_render::FontStore::new();
    let mut warns = Vec::new();
    hwp_render::lineseg::synthesize_linesegs(&mut doc, &mut store, &mut warns);

    let vs: Vec<i32> = doc.sections[0]
        .paragraphs
        .iter()
        .flat_map(|p| p.line_segs.iter().map(|s| s.v_pos))
        .collect();

    assert!(
        vs.len() >= 120,
        "문단마다 줄 배치가 합성되어야: {}",
        vs.len()
    );
    let maxv = *vs.iter().max().unwrap();
    assert!(
        maxv <= content_h,
        "모든 v_pos 는 페이지 본문 높이({content_h}) 이내여야 한다(페이지 상대) — 최댓값 {maxv}"
    );
    let resets = vs.windows(2).filter(|w| w[1] < w[0]).count();
    assert!(
        resets >= 1,
        "한 페이지를 넘기는 문서는 v_pos 리셋이 있어야 한다 — 리셋 {resets}회"
    );
}

/// 문단 위/아래 간격(spacing_top/bottom)이 합성 줄 배치 v_pos 에 반영되어야 한다.
/// 빠지면 한글이 문단 사이 여백 없이 압축해 그린다(제목 위 여백 사라짐 등).
/// from_markdown 은 제목에 spacing_top=600, spacing_bottom=300 을 준다.
#[test]
fn 문단_간격이_v_pos에_반영() {
    let mut doc = hwp_convert::from_markdown("# 제목\n\n본문 문단.\n");
    let mut store = hwp_render::FontStore::new();
    let mut warns = Vec::new();
    hwp_render::lineseg::synthesize_linesegs(&mut doc, &mut store, &mut warns);

    let paras = &doc.sections[0].paragraphs;
    let h = &paras[0].line_segs[0]; // 제목 (한 줄)
    let b = &paras[1].line_segs[0]; // 본문 (한 줄)
    // 본문 첫 줄 v_pos = 제목 줄 v_pos + 제목 line_advance + 제목 아래간격(300).
    let heading_advance = h.line_height + h.line_spacing;
    assert_eq!(
        b.v_pos - h.v_pos,
        heading_advance + 300,
        "본문 v_pos 는 제목 advance + 제목 아래간격(300) 만큼 떨어져야"
    );
}

#[test]
fn 빈_문서_렌더() {
    let Some(path) = fixture_or_skip("hwp5/bookmark.hwp") else {
        return;
    };
    let doc = hwp5::read_document(&path).unwrap().document;
    let out = render_document(&doc, &RenderOptions::default()).unwrap();
    assert_eq!(out.pages.len(), 1);
    assert_eq!(dark_pixels(&out.pages[0]), 0, "빈 문서는 흰 페이지");
}

/// 연결 다단 글상자: annual_report "At a Glance"(5쪽)는 월 텍스트가 왼쪽→오른쪽 단으로
/// 흐른다. (1) 글자 베이스라인이 페이지 하단을 넘지 않아야 하고(흐름 드리프트/잘림 회귀
/// 방지), (2) 오른쪽 단(x≈300pt)에 본문이 배치돼야 한다(다단 흐름). 폰트 무관 — 배치는
/// 캐시 v_pos·글상자 위치가 좌우한다.
#[test]
fn 글상자_연결_다단_배치() {
    let Some(path) = fixture_or_skip("hwp5/annual_report.hwp") else {
        return;
    };
    let doc = hwp5::read_document(&path).unwrap().document;
    let mut store = hwp_render::FontStore::new();
    let mut warns = Vec::new();
    let list = hwp_render::layout::layout_document(&doc, &mut store, &mut warns);
    assert!(
        list.pages.len() >= 5,
        "annual_report 는 5쪽 이상: {}",
        list.pages.len()
    );

    let page = &list.pages[4]; // 5쪽 (0-기반)
    let glyphs: Vec<(f32, f32)> = page
        .items
        .iter()
        .filter_map(|it| match it {
            hwp_render::display::Item::Glyphs { x, y, .. } => Some((*x, *y)),
            _ => None,
        })
        .collect();
    assert!(!glyphs.is_empty(), "5쪽에 글자가 있어야 한다");

    // (1) 세로 넘침 없음
    let max_y = glyphs.iter().map(|(_, y)| *y).fold(0.0_f32, f32::max);
    assert!(
        max_y <= page.height_pt,
        "5쪽 글자 베이스라인({max_y:.1}pt)이 페이지 하단({:.1}pt)을 넘음 — 글상자 드리프트",
        page.height_pt
    );

    // (2) 오른쪽 단 배치 (연결 다단 글상자가 둘째 단을 우측으로 흘림)
    let right_col = glyphs
        .iter()
        .any(|(x, y)| (280.0..330.0).contains(x) && (200.0..800.0).contains(y));
    assert!(
        right_col,
        "오른쪽 단(x≈300pt)에 본문이 없음 — 다단 글상자 미배치"
    );
}

/// 그리기 개체(도형) 렌더: annual_report의 선/사각형/타원/호/다각형이 Item::Path로
/// 생성되고, 미지원 컨트롤로 생략되지 않아야 한다. 파이(링) 페이지엔 곡선(CubicTo)
/// 경로(타원/호)가 있어야 한다. 폰트 무관 — 배치는 도형 기하·행렬이 좌우.
#[test]
fn 도형_렌더_경로_생성() {
    use hwp_render::display::{Item, PathCmd};
    let Some(path) = fixture_or_skip("hwp5/annual_report.hwp") else {
        return;
    };
    let doc = hwp5::read_document(&path).unwrap().document;
    let mut store = hwp_render::FontStore::new();
    let mut warns = Vec::new();
    let list = hwp_render::layout::layout_document(&doc, &mut store, &mut warns);

    let paths = list
        .pages
        .iter()
        .flat_map(|p| &p.items)
        .filter(|i| matches!(i, Item::Path { .. }))
        .count();
    // 보이지 않는 글상자 프레임은 제외되므로 가시 도형(선 43·타원·호·다각형 등)만 ~80개.
    assert!(
        paths > 50,
        "도형 경로가 너무 적음: {paths} (선·사각형·타원 등 미렌더)"
    );

    // 파이(링) 페이지: 타원/호 유래 곡선(CubicTo) 경로 존재.
    let has_curve = list.pages.iter().flat_map(|p| &p.items).any(|i| {
        matches!(i, Item::Path { commands, .. }
            if commands.iter().any(|c| matches!(c, PathCmd::CubicTo(..))))
    });
    assert!(has_curve, "타원/호 유래 곡선 경로가 없음 (파이/원 미렌더)");

    // 도형이 더 이상 "미지원 컨트롤"로 집계되지 않아야 한다.
    let skipped = warns.iter().filter(|w| w.contains("미지원 컨트롤")).count();
    assert_eq!(
        skipped, 0,
        "아직 미지원으로 집계되는 도형이 있음: {warns:?}"
    );
}

/// 그러데이션 채움이 백엔드에서 실제 그러데이션으로 렌더되는지(단색 근사가 아니라).
/// 도형 fixture가 없어 합성 DisplayList로 검증한다.
#[test]
fn 그러데이션_채움_백엔드() {
    use hwp_render::display::{DisplayList, Fill, Gradient, Item, PageList, PathCmd};
    let page = PageList {
        width_pt: 100.0,
        height_pt: 100.0,
        items: vec![Item::Path {
            commands: vec![
                PathCmd::MoveTo(10.0, 10.0),
                PathCmd::LineTo(90.0, 10.0),
                PathCmd::LineTo(90.0, 90.0),
                PathCmd::LineTo(10.0, 90.0),
                PathCmd::Close,
            ],
            fill: Some(Fill::Gradient(Gradient {
                radial: false,
                angle_deg: 0.0,                                      // 가로
                stops: vec![(0.0, 0x0000_00FF), (1.0, 0x00FF_0000)], // 빨강→파랑
            })),
            stroke: None,
        }],
    };
    let list = DisplayList { pages: vec![page] };

    // SVG: <linearGradient> 정의 + url 참조
    let svg = hwp_render::svg::render_svg(&list).remove(0);
    assert!(svg.contains("<linearGradient"), "SVG 그러데이션 정의 없음");
    assert!(svg.contains("url(#grad0)"), "SVG fill url 참조 없음");

    // PNG: 좌(빨강)와 우(파랑)가 달라야 한다(실제 그러데이션).
    let pngs = hwp_render::png::render_png(&list, 96.0).unwrap();
    let px = &pngs[0];
    let mid = px.height() / 2;
    let left = px.pixel(20, mid).unwrap();
    let right = px.pixel(px.width() - 20, mid).unwrap();
    assert!(
        left.red() > right.red() && left.blue() < right.blue(),
        "좌측은 빨강, 우측은 파랑이어야 — 좌({},{}) 우({},{})",
        left.red(),
        left.blue(),
        right.red(),
        right.blue()
    );
}

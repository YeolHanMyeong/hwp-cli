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
    let doc = hwp5::read_document(&fixture("hwp5/hello_world.hwp"))
        .unwrap()
        .document;
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
    let doc = hwpx::read_document(&fixture("hwpx/minimal.hwpx"))
        .unwrap()
        .document;
    let out = render_document(&doc, &RenderOptions::default()).unwrap();
    assert_eq!(out.pages.len(), 1);
    assert!(
        dark_pixels(&out.pages[0]) > 500,
        "세 문단이 모두 그려져야 한다"
    );
}

#[test]
fn 표_렌더() {
    let doc = hwp5::read_document(&fixture("hwp5/work_report.hwp"))
        .unwrap()
        .document;
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

    assert!(vs.len() >= 120, "문단마다 줄 배치가 합성되어야: {}", vs.len());
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

#[test]
fn 빈_문서_렌더() {
    let doc = hwp5::read_document(&fixture("hwp5/bookmark.hwp"))
        .unwrap()
        .document;
    let out = render_document(&doc, &RenderOptions::default()).unwrap();
    assert_eq!(out.pages.len(), 1);
    assert_eq!(dark_pixels(&out.pages[0]), 0, "빈 문서는 흰 페이지");
}

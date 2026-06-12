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

    // 표는 더 이상 미지원으로 집계되지 않는다 (꼬리말·글상자 2개만 남음)
    let skipped: Vec<_> = out
        .report
        .iter()
        .filter(|w| w.contains("미지원 컨트롤"))
        .collect();
    assert!(
        skipped.iter().all(|w| w.contains("2개")),
        "표가 미지원으로 집계됨: {skipped:?}"
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

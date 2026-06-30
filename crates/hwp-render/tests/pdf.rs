//! PDF 백엔드 스모크 테스트.
//!
//! 폰트 가용성에 의존하지 않는 구조 불변식만 검증한다: 유효한 PDF 헤더/트레일러,
//! 페이지 수, 임베드 CID 폰트 + Identity-H + ToUnicode 존재.
//! 시각 충실도(PDF→PNG vs PNG 백엔드)는 README의 pdftoppm 교차검증으로 확인한다.

use std::path::PathBuf;

use hwp_render::{RenderOptions, count_pages, render_document_pdf};

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel)
}

/// fixture 문서는 저장소에 없으므로(로컬 전용 — fixtures/README.md) 없으면 None → 테스트 skip.
fn load_or_skip(rel: &str) -> Option<hwp_model::Document> {
    let p = fixture(rel);
    if !p.exists() {
        eprintln!(
            "스킵: fixture 없음 ({}) — fixtures/README.md 참고",
            p.display()
        );
        return None;
    }
    Some(hwp5::read_document(&p).unwrap().document)
}

/// PDF 바이트 안에서 부분 바이트열 출현 횟수.
fn count(haystack: &[u8], needle: &[u8]) -> usize {
    if needle.is_empty() || haystack.len() < needle.len() {
        return 0;
    }
    haystack
        .windows(needle.len())
        .filter(|w| *w == needle)
        .count()
}

#[test]
fn 유효한_pdf_구조() {
    let Some(doc) = load_or_skip("hwp5/hello_world.hwp") else {
        return;
    };
    let out = render_document_pdf(&doc, &RenderOptions::default(), None).unwrap();
    let pdf = &out.data;

    assert!(pdf.starts_with(b"%PDF-"), "PDF 헤더가 없음");
    assert!(
        pdf.windows(5).any(|w| w == b"%%EOF"),
        "PDF 트레일러(%%EOF)가 없음"
    );
    assert!(pdf.len() > 1000, "PDF가 너무 작음: {} bytes", pdf.len());

    // 임베드 검색 가능 텍스트의 증거.
    assert!(count(pdf, b"/Identity-H") >= 1, "Identity-H 인코딩 없음");
    assert!(
        count(pdf, b"/CIDFontType2") + count(pdf, b"/CIDFontType0") >= 1,
        "CID 폰트 없음"
    );
    assert!(
        count(pdf, b"/ToUnicode") >= 1,
        "ToUnicode CMap 없음(검색 불가)"
    );
    assert!(
        count(pdf, b"/FontFile2") + count(pdf, b"/FontFile3") >= 1,
        "임베드 폰트 프로그램 없음"
    );
}

#[test]
fn 페이지_수_일치() {
    let Some(doc) = load_or_skip("hwp5/annual_report.hwp") else {
        return;
    };
    let opts = RenderOptions::default();
    let total = count_pages(&doc, &opts);
    assert!(total > 1, "멀티페이지 문서여야 함: {total}쪽");

    let out = render_document_pdf(&doc, &opts, None).unwrap();
    // 페이지 트리 1개(/Type /Pages) + 페이지 N개(/Type /Page) = "/Type /Page" N+1회.
    let page_markers = count(&out.data, b"/Type /Page");
    assert_eq!(
        page_markers,
        total + 1,
        "페이지 수 불일치: 마커 {page_markers}, 기대 {}",
        total + 1
    );
}

#[test]
fn 페이지_선택() {
    let Some(doc) = load_or_skip("hwp5/annual_report.hwp") else {
        return;
    };
    let opts = RenderOptions::default();
    let total = count_pages(&doc, &opts);
    assert!(total >= 3);

    let out = render_document_pdf(&doc, &opts, Some(&[1, 2])).unwrap();
    // 선택한 2쪽 + 페이지 트리 1개.
    assert_eq!(count(&out.data, b"/Type /Page"), 3);
}

#[test]
fn 표_문서_렌더() {
    // 표(선·셀 채움·텍스트)를 포함해도 유효한 PDF가 나와야 한다.
    let Some(doc) = load_or_skip("hwp5/work_report.hwp") else {
        return;
    };
    let out = render_document_pdf(&doc, &RenderOptions::default(), None).unwrap();
    assert!(out.data.starts_with(b"%PDF-"));
    assert!(out.data.windows(5).any(|w| w == b"%%EOF"));
}

#[test]
fn 빈_문서_렌더() {
    let Some(doc) = load_or_skip("hwp5/bookmark.hwp") else {
        return;
    };
    let out = render_document_pdf(&doc, &RenderOptions::default(), None).unwrap();
    // 텍스트가 없어도 유효한 1쪽 PDF.
    assert!(out.data.starts_with(b"%PDF-"));
    assert_eq!(count(&out.data, b"/Type /Page"), 2); // 페이지 1 + 트리 1
}

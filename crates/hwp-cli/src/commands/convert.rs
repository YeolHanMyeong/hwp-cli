//! `hwp convert` — 포맷 변환.
//!
//! M2 범위: hwp/hwpx → markdown/JSON. hwpx 쓰기(M4)와 hwp 쓰기(M6)는
//! 이후 마일스톤.

use std::path::Path;

use crate::ConvertFormat;
use crate::commands::cat::load_document;

pub fn run(
    input: &Path,
    output: &Path,
    to: Option<ConvertFormat>,
    _strict: bool,
    preserve_layout: bool,
) -> anyhow::Result<()> {
    let target = match to {
        Some(t) => t,
        None => infer_format(output)?,
    };

    match target {
        ConvertFormat::Md => {
            let doc = load_document(input)?;
            std::fs::write(output, hwp_convert::to_markdown(&doc))?;
        }
        ConvertFormat::Json => {
            let doc = load_document(input)?;
            std::fs::write(output, hwp_convert::to_json(&doc, true)?)?;
        }
        ConvertFormat::Hwpx => {
            let doc = load_document(input)?;
            let warnings = hwpx::write::write_document_with(
                &doc,
                output,
                &hwpx::write::HwpxWriteOptions {
                    preserve_linesegs: preserve_layout,
                },
            )?;
            for w in &warnings {
                eprintln!("경고: {w}");
            }
        }
        ConvertFormat::Hwp => {
            let doc = load_document(input)?;
            write_hwp(&doc, output, preserve_layout)?;
        }
    }
    eprintln!("변환 완료: {} → {}", input.display(), output.display());
    Ok(())
}

fn infer_format(output: &Path) -> anyhow::Result<ConvertFormat> {
    match output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("md") | Some("markdown") => Ok(ConvertFormat::Md),
        Some("json") => Ok(ConvertFormat::Json),
        Some("hwpx") => Ok(ConvertFormat::Hwpx),
        Some("hwp") => Ok(ConvertFormat::Hwp),
        other => {
            anyhow::bail!("출력 포맷을 추론할 수 없습니다 (확장자: {other:?}) — --to로 지정하세요")
        }
    }
}

/// hwp 바이너리 저장 (1쪽 렌더를 PrvImage로 동봉).
///
/// 합성 문서(md/hwpx 출신)는 줄 배치(PARA_LINE_SEG)가 없으면 5.1.x 한글이
/// 본문을 못 그린다(검은 바/빈 내용/손상). 폰트 셰이핑으로 정확한 줄바꿈을
/// 계산해 IR에 채운 뒤 쓴다 — 한글과 동일한 함초롬바탕 폰트가 필요하다
/// (HWP_FONT_DIR 환경변수 또는 프로젝트 `fonts/`).
pub fn write_hwp(
    doc: &hwp_model::Document,
    output: &std::path::Path,
    preserve_layout: bool,
) -> anyhow::Result<()> {
    let font_dir = std::path::PathBuf::from(
        std::env::var("HWP_FONT_DIR").unwrap_or_else(|_| "fonts".into()),
    );
    let synthesize = doc.meta.source_format != "hwp5";

    // 출처가 이미 줄 배치를 제공하면(hwpx의 linesegarray = 한글이 저장한 정품
    // 페이지 분할) 그대로 보존한다. 합성으로 덮어쓰면 페이지 리셋이 없는 섹션
    // 단조 누적이 되어(v_pos가 페이지 높이를 한참 초과) 멀티페이지 문서가
    // 한글에서 '손상' 판정된다. 줄 배치가 없는 출처(markdown)만 합성한다.
    // (writer는 synthesize=true면 emit_lineseg=true라 IR 줄 배치를 그대로 방출.)
    let has_linesegs = doc
        .sections
        .iter()
        .flat_map(|s| &s.paragraphs)
        .any(|p| !p.line_segs.is_empty());

    // 합성 경로: 정확한 줄 배치를 폰트 셰이핑으로 계산해 IR에 채운다.
    // 무수정 왕복(--preserve-layout)·원본 줄 배치 보유 시에는 합성하지 않는다.
    let owned;
    let doc = if synthesize && !preserve_layout && !has_linesegs {
        let mut d = doc.clone();
        let mut store = hwp_render::FontStore::new();
        store.load_dir(&font_dir);
        let mut warns = Vec::new();
        hwp_render::lineseg::synthesize_linesegs(&mut d, &mut store, &mut warns);
        for w in &warns {
            eprintln!("경고: {w}");
        }
        owned = d;
        &owned
    } else {
        doc
    };

    let prv_image = hwp_render::render_document(
        doc,
        &hwp_render::RenderOptions {
            dpi: 48.0,
            font_dirs: vec![font_dir],
        },
    )
    .ok()
    .and_then(|out| out.pages.first().and_then(|p| p.encode_png().ok()));

    let warnings = hwp5::write_document(
        doc,
        output,
        &hwp5::WriteOptions {
            prv_image,
            preserve_linesegs: preserve_layout,
        },
    )?;
    for w in &warnings {
        eprintln!("경고: {w}");
    }
    Ok(())
}

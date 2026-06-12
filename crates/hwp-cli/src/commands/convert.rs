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
pub fn write_hwp(
    doc: &hwp_model::Document,
    output: &std::path::Path,
    preserve_layout: bool,
) -> anyhow::Result<()> {
    let prv_image = hwp_render::render_document(
        doc,
        &hwp_render::RenderOptions {
            dpi: 48.0,
            ..Default::default()
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

//! `hwp render` — 페이지 렌더링 (PNG/SVG).

use std::path::{Path, PathBuf};

use crate::RenderFormat;
use crate::commands::cat::load_document;

pub fn run(
    input: &Path,
    output: &Path,
    pages_spec: &str,
    dpi: f64,
    format: Option<RenderFormat>,
    font_dirs: Vec<PathBuf>,
) -> anyhow::Result<()> {
    let format = format.unwrap_or_else(|| infer_format(output));
    let doc = load_document(input)?;
    let opts = hwp_render::RenderOptions {
        dpi: dpi as f32,
        font_dirs,
    };

    match format {
        RenderFormat::Png => {
            let result = hwp_render::render_document(&doc, &opts)?;
            report(&result.report);
            let selected = parse_pages(pages_spec, result.pages.len())?;
            let multi = selected.len() > 1;
            for &page_no in &selected {
                let pixmap = &result.pages[page_no - 1];
                let path = page_path(output, page_no, multi);
                pixmap
                    .save_png(&path)
                    .map_err(|e| anyhow::anyhow!("PNG 저장 실패 ({}): {e}", path.display()))?;
                eprintln!(
                    "저장: {} ({}×{}px)",
                    path.display(),
                    pixmap.width(),
                    pixmap.height()
                );
            }
        }
        RenderFormat::Svg => {
            let result = hwp_render::render_document_svg(&doc, &opts);
            report(&result.report);
            let selected = parse_pages(pages_spec, result.pages.len())?;
            let multi = selected.len() > 1;
            for &page_no in &selected {
                let path = page_path(output, page_no, multi);
                std::fs::write(&path, &result.pages[page_no - 1])?;
                eprintln!("저장: {}", path.display());
            }
        }
    }
    Ok(())
}

fn report(lines: &[String]) {
    for line in lines {
        eprintln!("렌더: {line}");
    }
}

fn infer_format(output: &Path) -> RenderFormat {
    match output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("svg") => RenderFormat::Svg,
        _ => RenderFormat::Png,
    }
}

fn page_path(base: &Path, page: usize, multi: bool) -> PathBuf {
    if multi {
        numbered_path(base, page)
    } else {
        base.to_path_buf()
    }
}

/// "all" | "3" | "1-5" → 1-기반 페이지 번호 목록.
fn parse_pages(spec: &str, total: usize) -> anyhow::Result<Vec<usize>> {
    if total == 0 {
        anyhow::bail!("렌더링된 페이지가 없습니다");
    }
    let pages: Vec<usize> = if spec.eq_ignore_ascii_case("all") {
        (1..=total).collect()
    } else if let Some((a, b)) = spec.split_once('-') {
        let (a, b): (usize, usize) = (a.trim().parse()?, b.trim().parse()?);
        (a..=b.min(total)).collect()
    } else {
        vec![spec.trim().parse()?]
    };
    if pages.is_empty() || pages.iter().any(|&p| p == 0 || p > total) {
        anyhow::bail!("페이지 범위가 잘못되었습니다 (문서: {total}쪽, 요청: {spec})");
    }
    Ok(pages)
}

/// out.png → out-3.png 형태의 페이지별 경로.
fn numbered_path(base: &Path, page: usize) -> PathBuf {
    let stem = base.file_stem().and_then(|s| s.to_str()).unwrap_or("page");
    let ext = base.extension().and_then(|s| s.to_str()).unwrap_or("png");
    base.with_file_name(format!("{stem}-{page}.{ext}"))
}

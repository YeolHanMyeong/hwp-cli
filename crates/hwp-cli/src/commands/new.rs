//! `hwp new` — 새 문서 생성 (markdown/빈 문서 → hwpx).

use std::path::Path;

pub fn run(output: &Path, from: Option<&Path>) -> anyhow::Result<()> {
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let doc = match from {
        Some(md_path) => {
            let md = std::fs::read_to_string(md_path)?;
            hwp_convert::from_markdown(&md)
        }
        None => hwp_convert::from_markdown(""),
    };

    if ext.as_deref() == Some("hwp") {
        crate::commands::convert::write_hwp(&doc, output, false)?;
    } else {
        let warnings = hwpx::write_document(&doc, output)?;
        for w in &warnings {
            eprintln!("경고: {w}");
        }
    }
    eprintln!("생성 완료: {}", output.display());
    Ok(())
}

//! `hwp new` — 새 문서 생성 (markdown/빈 문서 → hwpx).

use std::path::Path;

pub fn run(output: &Path, from: Option<&Path>) -> anyhow::Result<()> {
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let doc = match from {
        Some(src) => {
            let text = std::fs::read_to_string(src)?;
            if src
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            {
                // JSON IR(편집 왕복) 입력
                hwp_convert::from_json(&text)
                    .map_err(|e| anyhow::anyhow!("JSON IR 파싱 실패 ({}): {e}", src.display()))?
            } else {
                hwp_convert::from_markdown(&text)
            }
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

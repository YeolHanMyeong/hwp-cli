//! `hwp new` — 새 문서 생성 (markdown/빈 문서 → hwpx).

use std::path::Path;

pub fn run(output: &Path, from: Option<&Path>) -> anyhow::Result<()> {
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    if ext.as_deref() == Some("hwp") {
        anyhow::bail!(
            "hwp 바이너리 생성은 아직 구현되지 않았습니다 (M6 예정) — .hwpx를 사용하세요"
        );
    }

    let doc = match from {
        Some(md_path) => {
            let md = std::fs::read_to_string(md_path)?;
            hwp_convert::from_markdown(&md)
        }
        None => hwp_convert::from_markdown(""),
    };

    let warnings = hwpx::write_document(&doc, output)?;
    for w in &warnings {
        eprintln!("경고: {w}");
    }
    eprintln!("생성 완료: {}", output.display());
    Ok(())
}

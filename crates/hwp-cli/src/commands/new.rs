//! `hwp new` — 새 문서 생성 (markdown/빈 문서 → hwpx).

use std::path::Path;

use hwp_convert::Preset;

#[allow(clippy::too_many_arguments)]
pub fn run(
    output: &Path,
    from: Option<&Path>,
    preset: Preset,
    title: Option<String>,
    author: Option<String>,
    subject: Option<String>,
) -> anyhow::Result<()> {
    let ext = output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let mut doc = match from {
        Some(src) => {
            let text = std::fs::read_to_string(src)?;
            if src
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
            {
                // JSON IR(편집 왕복) 입력 — 프리셋은 무시(헤더가 JSON에 포함됨)
                hwp_convert::from_json(&text)
                    .map_err(|e| anyhow::anyhow!("JSON IR 파싱 실패 ({}): {e}", src.display()))?
            } else {
                hwp_convert::from_markdown_preset(&text, preset)
            }
        }
        None => hwp_convert::from_markdown_preset("", preset),
    };

    // 메타데이터 플래그가 있으면 덮어쓴다(JSON IR에 있던 값보다 우선).
    if title.is_some() {
        doc.metadata.title = title;
    }
    if author.is_some() {
        doc.metadata.author = author;
    }
    if subject.is_some() {
        doc.metadata.subject = subject;
    }

    let warnings = if ext.as_deref() == Some("hwp") {
        crate::commands::convert::write_hwp(&doc, output, false)?
    } else {
        hwpx::write_document(&doc, output)?
    };
    crate::commands::convert::print_warnings(&warnings);
    eprintln!("생성 완료: {}", output.display());
    Ok(())
}

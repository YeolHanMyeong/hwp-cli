//! `hwp slots` — `{{name}}` 텍스트 자리표시자 스캔.
//!
//! 누름틀(form field, `hwp fields`)과 별개로, 순수 텍스트 `{{...}}` 템플릿의
//! 자리표시자를 등장 순서로 나열한다. `hwp edit --replace "{{name}}=>값"` 으로 채운다.

use std::path::Path;

use crate::commands::cat::load_document;

pub fn run(path: &Path, json: bool) -> anyhow::Result<()> {
    let doc = load_document(path)?;
    let slots = hwp_convert::scan_placeholders(&doc);

    if json {
        let items: Vec<serde_json::Value> = slots
            .iter()
            .map(|p| serde_json::json!({ "name": p.name, "occurrences": p.occurrences }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "placeholders": items }))?
        );
    } else if slots.is_empty() {
        eprintln!("자리표시자 없음");
    } else {
        for p in &slots {
            println!("{}\t{}", p.name, p.occurrences);
        }
    }
    Ok(())
}

//! `hwp bookmarks` — 책갈피(bokm) 목록.

use std::path::Path;

use crate::commands::cat::load_document;

pub fn run(file: &Path, as_json: bool) -> anyhow::Result<()> {
    let doc = load_document(file)?;
    let bookmarks = hwp_convert::list_bookmarks(&doc);

    if as_json {
        let arr: Vec<_> = bookmarks
            .iter()
            .map(|b| serde_json::json!({ "name": b.name }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&arr)?);
        return Ok(());
    }

    if bookmarks.is_empty() {
        println!("책갈피 없음");
        return Ok(());
    }
    println!("책갈피 {}개:", bookmarks.len());
    for (i, b) in bookmarks.iter().enumerate() {
        println!("  [{i}] {:?}", b.name);
    }
    Ok(())
}

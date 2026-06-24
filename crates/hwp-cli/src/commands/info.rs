//! `hwp info` — 컨테이너 계층만으로 동작하는 파일 진단.
//!
//! 본문 파싱에 실패하는(손상/미지원) 파일도 진단할 수 있어야 하므로
//! 레코드 해석에 의존하지 않는다.

use std::path::Path;

use serde_json::{Value, json};

use crate::format::{FileFormat, detect};

/// 컨테이너 계층 진단을 JSON으로 만든다 (CLI `--json`과 MCP가 공유).
pub fn info_json(path: &Path) -> anyhow::Result<Value> {
    match detect(path)? {
        FileFormat::Hwp5 => {
            let container = hwp5::Hwp5Container::open(path)?;
            let header = container.file_header();
            let streams = container.list_streams();
            Ok(json!({
                "file": path.display().to_string(),
                "format": "hwp5",
                "version": header.version.to_string(),
                "attributes": header.attribute_names(),
                "compressed": header.is_compressed(),
                "encrypted": header.is_encrypted(),
                "distribution": header.is_distribution(),
                "sections": container.body_sections().len(),
                "streams": streams.iter().map(|s| json!({
                    "path": s.path,
                    "size": s.size,
                })).collect::<Vec<_>>(),
            }))
        }
        FileFormat::Hwpx => {
            let mut pkg = hwpx::HwpxPackage::open(path)?;
            let version = pkg.version_info()?;
            let entries = pkg.entries()?;
            let sections = pkg.section_entries()?;
            Ok(json!({
                "file": path.display().to_string(),
                "format": "hwpx",
                "version": version.iter().cloned().collect::<std::collections::BTreeMap<_, _>>(),
                "sections": sections.len(),
                "entries": entries.iter().map(|e| json!({
                    "name": e.name,
                    "size": e.size,
                    "compressed_size": e.compressed_size,
                })).collect::<Vec<_>>(),
            }))
        }
    }
}

pub fn run(path: &Path, as_json: bool) -> anyhow::Result<()> {
    if as_json {
        println!("{}", serde_json::to_string_pretty(&info_json(path)?)?);
        return Ok(());
    }
    match detect(path)? {
        FileFormat::Hwp5 => info_hwp5_text(path),
        FileFormat::Hwpx => info_hwpx_text(path),
    }
}

fn info_hwp5_text(path: &Path) -> anyhow::Result<()> {
    let container = hwp5::Hwp5Container::open(path)?;
    let header = container.file_header();
    let streams = container.list_streams();

    println!("파일:   {}", path.display());
    println!("포맷:   HWP 5.0 (바이너리)");
    println!("버전:   {}", header.version);
    let attrs = header.attribute_names();
    println!(
        "속성:   {}",
        if attrs.is_empty() {
            "(없음)".to_string()
        } else {
            attrs.join(", ")
        }
    );
    println!("섹션:   {}개", container.body_sections().len());
    println!("스트림: {}개", streams.len());
    for s in &streams {
        println!("  {:<40} {:>10} B", printable(&s.path), s.size);
    }
    Ok(())
}

fn info_hwpx_text(path: &Path) -> anyhow::Result<()> {
    let mut pkg = hwpx::HwpxPackage::open(path)?;
    let version = pkg.version_info()?;
    let entries = pkg.entries()?;
    let sections = pkg.section_entries()?;

    println!("파일:   {}", path.display());
    println!("포맷:   HWPX (OWPML)");
    if !version.is_empty() {
        let pairs: Vec<String> = version.iter().map(|(k, v)| format!("{k}={v}")).collect();
        println!("버전:   {}", pairs.join(" "));
    }
    println!("섹션:   {}개", sections.len());
    println!("엔트리: {}개", entries.len());
    for e in &entries {
        println!("  {:<40} {:>10} B", e.name, e.size);
    }
    Ok(())
}

/// 제어 문자가 들어간 스트림 이름(`\x05HwpSummaryInformation`)을 표시 가능하게.
fn printable(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

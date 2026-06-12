//! HWPX → IR 읽기.
//!
//! IR 의미를 hwp5와 일치시킨다: `hp:secPr`/`hp:ctrl(colPr)`/`hp:tbl`은
//! hwp5처럼 확장 컨트롤 문자(8 WCHAR) + `Control`로 표현해 두 포맷의
//! 위치 산수와 추출 로직이 같은 코드를 타게 한다.

pub mod header;
pub mod section;
mod xml;

use std::path::Path;

use hwp_model::{DocMeta, Document};

use crate::error::Result;
use crate::package::HwpxPackage;

pub struct ReadResult {
    pub document: Document,
    pub warnings: Vec<String>,
}

/// HWPX 파일을 IR로 읽는다.
pub fn read_document(path: &Path) -> Result<ReadResult> {
    let mut pkg = HwpxPackage::open(path)?;
    let mut warnings = Vec::new();

    let header_xml = pkg.read_entry_string("Contents/header.xml")?;
    let (mut doc_header, header_warnings) = header::parse_header(&header_xml)?;
    warnings.extend(
        header_warnings
            .into_iter()
            .map(|w| format!("[header.xml] {w}")),
    );

    let mut sections = Vec::new();
    for entry in pkg.section_entries()? {
        let xml = pkg.read_entry_string(&entry)?;
        let (section, sec_warnings) = section::parse_section(&xml)?;
        warnings.extend(sec_warnings.into_iter().map(|w| format!("[{entry}] {w}")));
        sections.push(section);
    }
    doc_header.properties.section_count = sections.len() as u16;

    let version = pkg
        .version_info()?
        .into_iter()
        .filter(|(k, _)| ["major", "minor", "micro", "buildNumber"].contains(&k.as_str()))
        .map(|(_, v)| v)
        .collect::<Vec<_>>()
        .join(".");

    Ok(ReadResult {
        document: Document {
            meta: DocMeta {
                source_format: "hwpx".to_string(),
                source_version: version,
            },
            header: doc_header,
            sections,
        },
        warnings,
    })
}

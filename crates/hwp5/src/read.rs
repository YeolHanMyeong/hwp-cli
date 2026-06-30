//! 최상위: HWP 5.0 파일 → [`Document`].

use std::path::Path;

use hwp_model::{DocMeta, Document};

use crate::body_text::parse_section;
use crate::container::Hwp5Container;
use crate::doc_info::parse_doc_info;
use crate::error::Result;
use crate::record::{ScanMode, scan_stream};

pub struct ReadResult {
    pub document: Document,
    /// 파싱 중 발생한 비치명 경고 (손상/미지원 구조)
    pub warnings: Vec<String>,
}

/// HWP 5.0 파일을 IR로 읽는다. 야생 파일 대응을 위해 관용 모드로 스캔한다.
pub fn read_document(path: &Path) -> Result<ReadResult> {
    let mut container = Hwp5Container::open(path)?;
    container.check_body_readable()?;

    let mut warnings = Vec::new();

    // DocInfo
    let doc_info_data = container.read_record_stream("/DocInfo")?;
    let scan = scan_stream(&doc_info_data, ScanMode::Tolerant)?;
    warnings.extend(scan.warnings.iter().map(|w| format!("[DocInfo] {w}")));
    let (header, doc_warnings) = parse_doc_info(&scan.roots);
    warnings.extend(doc_warnings.iter().map(|w| format!("[DocInfo] {w}")));

    // 본문 섹션들
    let mut sections = Vec::new();
    for stream_path in container.body_sections() {
        let data = container.read_record_stream(&stream_path)?;
        let scan = scan_stream(&data, ScanMode::Tolerant)?;
        warnings.extend(scan.warnings.iter().map(|w| format!("[{stream_path}] {w}")));
        let (section, sec_warnings) = parse_section(&scan.roots);
        warnings.extend(sec_warnings.iter().map(|w| format!("[{stream_path}] {w}")));
        sections.push(section);
    }

    // 첨부 바이너리: 압축 플래그가 있으면 해제 시도, 실패 시 원본 사용
    // (BIN_DATA 레코드의 개별 압축 모드는 보수적으로 시도-폴백으로 흡수)
    let compressed = container.file_header().is_compressed();
    let mut bin_streams = Vec::new();
    for info in container.list_streams() {
        if let Some(name) = info.path.strip_prefix("/BinData/") {
            let raw = container.read_stream_raw(&info.path)?;
            let data = if compressed {
                crate::codec::decompress(&raw, &info.path).unwrap_or(raw)
            } else {
                raw
            };
            bin_streams.push(hwp_model::BinStream {
                name: name.to_string(),
                data,
            });
        }
    }

    // 문서 메타데이터 (요약 정보 — 최선 노력: 없거나 손상돼도 진단 계속)
    let metadata = container
        .read_stream_raw("/\u{5}HwpSummaryInformation")
        .ok()
        .map(|raw| crate::summary::parse_summary(&raw))
        .unwrap_or_default();

    let document = Document {
        meta: DocMeta {
            source_format: "hwp5".to_string(),
            source_version: container.file_header().version.to_string(),
        },
        metadata,
        header,
        sections,
        bin_streams,
    };
    Ok(ReadResult { document, warnings })
}

//! IR → HWPX 패키지 쓰기.
//!
//! 패키지 규칙: `mimetype`이 **첫 엔트리이며 무압축(stored)**, 나머지는
//! deflate. 항목 구성은 한글 저장 기준 표본을 따른다.

pub mod header;
pub mod section;
mod templates;

use std::fs::File;
use std::io::Write as _;
use std::path::Path;

use hwp_model::Document;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::error::Result;
use section::BinCollector;

#[derive(Default)]
pub struct HwpxWriteOptions {
    /// 줄 배치(linesegarray) 보존 여부. 한글은 줄 배치가 내용과 정합하지
    /// 않으면 "변조" 보안 경고를 띄우므로(한컴 공식: 수정 시 제거 권장)
    /// 기본 false. 무수정 왕복에만 true.
    pub preserve_linesegs: bool,
}

/// 문서를 HWPX 파일로 저장한다 (줄 배치 제거 — 한글이 재계산).
pub fn write_document(doc: &Document, path: &Path) -> Result<Vec<String>> {
    write_document_with(doc, path, &HwpxWriteOptions::default())
}

/// 옵션 지정 저장.
pub fn write_document_with(
    doc: &Document,
    path: &Path,
    opts: &HwpxWriteOptions,
) -> Result<Vec<String>> {
    let mut warnings = Vec::new();

    // 본문 먼저 직렬화 (BinData 수집 포함)
    let mut bins = BinCollector::default();
    let sections: Vec<String> = doc
        .sections
        .iter()
        .map(|s| section::write_section(doc, s, opts.preserve_linesegs, &mut bins, &mut warnings))
        .collect();
    let header_xml = header::write_header(&doc.header, doc.sections.len().max(1));

    // 미리보기 텍스트 (선두 1KB 근사)
    let mut preview = doc.plain_text();
    preview.truncate(
        preview
            .char_indices()
            .nth(1000)
            .map_or(preview.len(), |(i, _)| i),
    );

    let bin_meta: Vec<(String, String, String)> = bins
        .items
        .iter()
        .map(|(id, href, mime, _)| (id.clone(), href.clone(), mime.clone()))
        .collect();

    let file = File::create(path)?;
    let mut zip = zip::ZipWriter::new(file);
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    // 1. mimetype — 반드시 첫 엔트리 + 무압축
    zip.start_file("mimetype", stored)?;
    zip.write_all(templates::MIMETYPE.as_bytes())?;

    let put = |zip: &mut zip::ZipWriter<File>, name: &str, data: &[u8]| -> Result<()> {
        zip.start_file(name, deflated)?;
        zip.write_all(data)?;
        Ok(())
    };

    put(&mut zip, "version.xml", templates::VERSION_XML.as_bytes())?;
    put(
        &mut zip,
        "META-INF/container.rdf",
        templates::CONTAINER_RDF.as_bytes(),
    )?;
    put(
        &mut zip,
        "META-INF/container.xml",
        templates::CONTAINER_XML.as_bytes(),
    )?;
    put(
        &mut zip,
        "META-INF/manifest.xml",
        templates::MANIFEST_XML.as_bytes(),
    )?;
    put(
        &mut zip,
        "Contents/content.hpf",
        templates::content_hpf(sections.len(), &bin_meta, &doc.metadata).as_bytes(),
    )?;
    put(&mut zip, "Contents/header.xml", header_xml.as_bytes())?;
    for (i, xml) in sections.iter().enumerate() {
        put(
            &mut zip,
            &format!("Contents/section{i}.xml"),
            xml.as_bytes(),
        )?;
    }
    for (_, href, _, bytes) in &bins.items {
        put(&mut zip, href, bytes)?;
    }
    put(&mut zip, "Preview/PrvText.txt", preview.as_bytes())?;
    put(&mut zip, "settings.xml", templates::SETTINGS_XML.as_bytes())?;

    zip.finish()?;
    Ok(warnings)
}

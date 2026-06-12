//! ZIP 패키지 수준 접근.
//!
//! `hwp info`/`hwp dump`가 OWPML 파싱 없이도 동작하도록
//! 컨테이너 계층만으로 메타데이터를 제공한다.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use quick_xml::events::Event;

use crate::error::{HwpxError, Result};

pub const MIMETYPE: &str = "application/hwp+zip";

/// ZIP 엔트리 메타데이터 (`hwp info`용).
#[derive(Debug, Clone)]
pub struct EntryInfo {
    pub name: String,
    pub size: u64,
    pub compressed_size: u64,
}

pub struct HwpxPackage {
    zip: zip::ZipArchive<File>,
}

impl HwpxPackage {
    /// 파일을 열고 mimetype을 검증한다.
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let zip = zip::ZipArchive::new(file)?;
        let mut pkg = Self { zip };
        let mime = pkg.read_entry_string("mimetype")?;
        if mime.trim() != MIMETYPE {
            return Err(HwpxError::BadMimetype(mime.trim().to_string()));
        }
        Ok(pkg)
    }

    /// 모든 엔트리를 보관 순서대로 열거한다.
    pub fn entries(&mut self) -> Result<Vec<EntryInfo>> {
        let mut v = Vec::with_capacity(self.zip.len());
        for i in 0..self.zip.len() {
            let e = self.zip.by_index(i)?;
            v.push(EntryInfo {
                name: e.name().to_string(),
                size: e.size(),
                compressed_size: e.compressed_size(),
            });
        }
        Ok(v)
    }

    pub fn read_entry(&mut self, name: &str) -> Result<Vec<u8>> {
        let mut e = self
            .zip
            .by_name(name)
            .map_err(|_| HwpxError::EntryNotFound(name.to_string()))?;
        let mut buf = Vec::new();
        e.read_to_end(&mut buf)?;
        Ok(buf)
    }

    pub fn read_entry_string(&mut self, name: &str) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.read_entry(name)?).into_owned())
    }

    /// `version.xml` 루트 요소의 속성들을 (이름, 값) 쌍으로 반환한다.
    /// (스키마에 의존하지 않는 관용적 추출 — `hwp info` 표시용)
    pub fn version_info(&mut self) -> Result<Vec<(String, String)>> {
        let xml = self.read_entry_string("version.xml")?;
        let mut reader = quick_xml::Reader::from_str(&xml);
        loop {
            match reader.read_event().map_err(|e| HwpxError::Xml {
                entry: "version.xml".to_string(),
                message: e.to_string(),
            })? {
                Event::Start(e) | Event::Empty(e) => {
                    let mut attrs = Vec::new();
                    for attr in e.attributes().flatten() {
                        let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                        let value = attr
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .unwrap_or_default()
                            .into_owned();
                        attrs.push((key, value));
                    }
                    return Ok(attrs);
                }
                Event::Eof => return Ok(Vec::new()),
                _ => {}
            }
        }
    }

    /// 본문 섹션 엔트리 이름 목록 (`Contents/section0.xml`, …).
    pub fn section_entries(&mut self) -> Result<Vec<String>> {
        let mut v: Vec<String> = self
            .entries()?
            .into_iter()
            .map(|e| e.name)
            .filter(|n| n.starts_with("Contents/section") && n.ends_with(".xml"))
            .collect();
        v.sort_by_key(|n| {
            n.trim_start_matches("Contents/section")
                .trim_end_matches(".xml")
                .parse::<u32>()
                .unwrap_or(u32::MAX)
        });
        Ok(v)
    }
}

//! 최상위 문서 모델.

use serde::{Deserialize, Serialize};

use crate::control::{BinRef, Control, SectionDef};
use crate::header::DocHeader;
use crate::paragraph::Paragraph;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Document {
    /// 출처 정보 (원본 포맷/버전 등)
    pub meta: DocMeta,
    pub header: DocHeader,
    pub sections: Vec<Section>,
    /// 첨부 바이너리 (이미지 등). 키는 원본 컨테이너 항목 이름
    /// (hwp5: "BIN0001.png", hwpx: "BinData/image1.png").
    pub bin_streams: Vec<BinStream>,
}

/// 첨부 바이너리 하나. 바이트는 JSON 직렬화에서 제외한다 (L2 출력 비대 방지).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinStream {
    pub name: String,
    #[serde(skip)]
    pub data: Vec<u8>,
}

impl Document {
    /// Picture의 BinRef를 실제 바이트로 해석한다.
    pub fn resolve_bin(&self, bin_ref: &BinRef) -> Option<&[u8]> {
        match bin_ref {
            BinRef::Id(id) => {
                let item = self.header.bin_data.get((id.0 as usize).checked_sub(1)?)?;
                let storage_id = item.storage_id?;
                let ext = item.extension.as_deref().unwrap_or("");
                let name = format!("BIN{storage_id:04X}.{ext}");
                self.bin_streams
                    .iter()
                    .find(|s| s.name.eq_ignore_ascii_case(&name))
                    .map(|s| s.data.as_slice())
            }
            BinRef::ItemRef(item) => self
                .bin_streams
                .iter()
                .find(|s| {
                    s.name == *item
                        || s.name.ends_with(&format!("/{item}"))
                        || s.name
                            .rsplit('/')
                            .next()
                            .and_then(|f| f.split('.').next())
                            .is_some_and(|stem| stem == item)
                })
                .map(|s| s.data.as_slice()),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocMeta {
    /// "hwp5" | "hwpx"
    pub source_format: String,
    /// 원본 파일 버전 (예: "5.1.0.1")
    pub source_version: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub paragraphs: Vec<Paragraph>,
    /// 문단이 아닌 최상위 레코드 (잘 형성된 파일에서는 비어 있음)
    pub extras: Vec<crate::opaque::OpaqueRecord>,
}

impl Section {
    /// 이 구역의 구역 정의 컨트롤 (보통 첫 문단의 첫 컨트롤).
    pub fn section_def(&self) -> Option<&SectionDef> {
        self.paragraphs
            .iter()
            .flat_map(|p| &p.controls)
            .find_map(|c| match c {
                Control::SectionDef(sd) => Some(sd),
                _ => None,
            })
    }
}

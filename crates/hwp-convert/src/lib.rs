//! IR → markdown/JSON 변환.

pub mod markdown;

use hwp_model::Document;

pub use markdown::to_markdown;

/// IR 전체를 JSON으로 직렬화 (구조 검사·디버깅·기계 소비용).
pub fn to_json(doc: &Document, pretty: bool) -> serde_json::Result<String> {
    if pretty {
        serde_json::to_string_pretty(doc)
    } else {
        serde_json::to_string(doc)
    }
}

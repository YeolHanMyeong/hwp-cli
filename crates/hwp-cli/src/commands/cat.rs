//! `hwp cat` — 텍스트 추출.
//!
//! 본문 파싱 기반 추출(plain/markdown/json)과 `--preview`(PrvText)를
//! 지원한다. 미리보기는 컨테이너 계층만 사용하므로 본문 파싱이 실패하는
//! 파일의 폴백으로도 쓰인다.

use std::path::Path;

use hwp_model::Document;

use crate::TextFormat;
use crate::format::{FileFormat, detect};

/// 포맷을 감지해 IR로 읽는다 (cat/convert 공용).
pub fn load_document(path: &Path) -> anyhow::Result<Document> {
    match detect(path)? {
        FileFormat::Hwp5 => {
            let result = hwp5::read_document(path)?;
            for w in &result.warnings {
                eprintln!("경고: {w}");
            }
            Ok(result.document)
        }
        FileFormat::Hwpx => {
            let result = hwpx::read_document(path)?;
            for w in &result.warnings {
                eprintln!("경고: {w}");
            }
            Ok(result.document)
        }
    }
}

/// 본문 텍스트 추출.
pub fn run(path: &Path, format: TextFormat) -> anyhow::Result<()> {
    let doc = load_document(path)?;
    match format {
        TextFormat::Plain => print!("{}", doc.plain_text()),
        TextFormat::Markdown => print!("{}", hwp_convert::to_markdown(&doc)),
        TextFormat::Json => println!("{}", hwp_convert::to_json(&doc, true)?),
    }
    Ok(())
}

pub fn preview(path: &Path) -> anyhow::Result<()> {
    let text = match detect(path)? {
        FileFormat::Hwp5 => {
            let mut container = hwp5::Hwp5Container::open(path)?;
            let raw = container.read_stream_raw("/PrvText")?;
            decode_utf16le(&raw)
        }
        FileFormat::Hwpx => {
            let mut pkg = hwpx::HwpxPackage::open(path)?;
            let raw = pkg.read_entry("Preview/PrvText.txt")?;
            // HWPX 미리보기는 보통 UTF-8이지만 UTF-16LE인 경우도 방어
            if raw.iter().take(64).any(|&b| b == 0) {
                decode_utf16le(&raw)
            } else {
                String::from_utf8_lossy(&raw).into_owned()
            }
        }
    };
    println!("{text}");
    Ok(())
}

fn decode_utf16le(raw: &[u8]) -> String {
    let units: Vec<u16> = raw
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    // 후행 NUL 제거 후 손실 허용 디코드
    let end = units.iter().rposition(|&u| u != 0).map_or(0, |i| i + 1);
    String::from_utf16_lossy(&units[..end])
}

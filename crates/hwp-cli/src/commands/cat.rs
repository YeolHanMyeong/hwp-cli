//! `hwp cat` — 텍스트 추출.
//!
//! M0에서는 `--preview`(PrvText 미리보기)만 지원한다. 본문 파싱 기반
//! 전체 추출은 M1에서 구현한다. 미리보기는 컨테이너 계층만 사용하므로
//! 본문 파싱이 실패하는 파일의 폴백으로도 쓰인다.

use std::path::Path;

use crate::format::{FileFormat, detect};

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

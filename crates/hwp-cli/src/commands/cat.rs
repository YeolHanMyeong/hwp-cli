//! `hwp cat` — 텍스트 추출.
//!
//! 본문 파싱 기반 추출(plain/markdown/json)과 `--preview`(PrvText)를
//! 지원한다. 미리보기는 컨테이너 계층만 사용하므로 본문 파싱이 실패하는
//! 파일의 폴백으로도 쓰인다.

use std::path::Path;

use hwp_model::Document;

use crate::TextFormat;
use crate::format::{FileFormat, detect};

/// 포맷을 감지해 IR로 읽는다 (cat/convert/render 공용).
///
/// `.json` 입력은 IR 직렬화본으로 보고 역직렬화한다(편집 왕복 경로) — 그 외는
/// 매직 바이트로 hwp5/hwpx를 판별한다.
pub fn load_document(path: &Path) -> anyhow::Result<Document> {
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
    {
        let text = std::fs::read_to_string(path)?;
        return hwp_convert::from_json(&text)
            .map_err(|e| anyhow::anyhow!("JSON IR 파싱 실패 ({}): {e}", path.display()));
    }
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
///
/// `with_header_footer`/`with_hidden`은 머리말·꼬리말/숨은 설명 포함 여부(기본 제외).
/// plain·markdown 경로에 일관되게 적용된다(html/json은 옵션 미대상).
pub fn run(
    path: &Path,
    format: TextFormat,
    with_header_footer: bool,
    with_hidden: bool,
) -> anyhow::Result<()> {
    let doc = load_document(path)?;
    let opts = hwp_model::TextOptions {
        include_header_footer: with_header_footer,
        include_hidden: with_hidden,
    };
    match format {
        TextFormat::Plain => print!("{}", doc.plain_text_with(&opts)),
        TextFormat::Markdown => print!(
            "{}",
            hwp_convert::to_markdown_with(
                &doc,
                &hwp_convert::MarkdownOptions {
                    text: opts,
                    ..Default::default()
                },
            )?
        ),
        TextFormat::Html => print!("{}", hwp_convert::to_html(&doc)),
        TextFormat::Json => println!("{}", hwp_convert::to_json(&doc, true, false)?),
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

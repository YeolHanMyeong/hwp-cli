//! `hwp edit` — 기존 문서를 인메모리로 편집해 다시 쓴다.
//!
//! 원본을 IR로 읽어(이미지·opaque 보존) 텍스트 치환·표 셀 설정을 적용한 뒤
//! 출력 포맷으로 저장한다. hwp 출력은 합성 경로(`write_hwp_edited`)를 거쳐
//! 편집으로 낡은 줄 배치·문단 불변식을 다시 세운다.

use std::path::Path;

use anyhow::Context;

use crate::commands::cat::load_document;

pub fn run(
    input: &Path,
    output: &Path,
    replaces: &[String],
    set_cells: &[String],
    verify: bool,
) -> anyhow::Result<()> {
    let mut doc = load_document(input)?;
    let mut edits = 0usize;

    for spec in replaces {
        let (from, to) = spec
            .split_once("=>")
            .with_context(|| format!("--replace 형식은 \"찾기=>바꾸기\" 입니다: {spec:?}"))?;
        let n = hwp_convert::replace_text(&mut doc, from, to, true);
        eprintln!("치환: {from:?} → {to:?} ({n}건)");
        edits += n;
    }

    for spec in set_cells {
        let (loc, text) = spec
            .split_once('=')
            .with_context(|| format!("--set-cell 형식은 \"표:행:열=값\" 입니다: {spec:?}"))?;
        let parts: Vec<&str> = loc.split(':').collect();
        if parts.len() != 3 {
            anyhow::bail!("--set-cell 위치는 \"표:행:열\" 형식입니다: {loc:?}");
        }
        let ti: usize = parts[0].trim().parse().context("표 인덱스")?;
        let r: u16 = parts[1].trim().parse().context("행 번호")?;
        let c: u16 = parts[2].trim().parse().context("열 번호")?;
        hwp_convert::set_cell(&mut doc, ti, r, c, text).map_err(|e| anyhow::anyhow!(e))?;
        eprintln!("셀 설정: 표{ti} ({r},{c}) = {text:?}");
        edits += 1;
    }

    if edits == 0 {
        eprintln!("경고: 적용된 편집이 없습니다 (--replace/--set-cell 확인)");
    }

    write_output(&doc, output)?;
    if verify {
        verify_output(output)?;
    }
    eprintln!("편집 완료: {} → {}", input.display(), output.display());
    Ok(())
}

fn write_output(doc: &hwp_model::Document, output: &Path) -> anyhow::Result<()> {
    match output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("hwp") => crate::commands::convert::write_hwp_edited(doc, output)?,
        Some("hwpx") => {
            let warnings = hwpx::write_document(doc, output)?;
            for w in &warnings {
                eprintln!("경고: {w}");
            }
        }
        Some("json") => std::fs::write(output, hwp_convert::to_json(doc, true, true)?)?,
        Some("md") | Some("markdown") => {
            std::fs::write(output, hwp_convert::to_markdown(doc))?;
        }
        other => anyhow::bail!("출력 포맷을 추론할 수 없습니다 (확장자: {other:?})"),
    }
    Ok(())
}

/// 쓰기 후 재읽기로 자기 검증 — 파일이 다시 파싱되고 본문이 비지 않았는지.
fn verify_output(output: &Path) -> anyhow::Result<()> {
    let doc =
        load_document(output).with_context(|| format!("검증 재읽기 실패: {}", output.display()))?;
    let text_len = doc.plain_text().chars().count();
    let paras: usize = doc.sections.iter().map(|s| s.paragraphs.len()).sum();
    eprintln!("검증: 재읽기 OK ({paras}문단, 본문 {text_len}자)");
    Ok(())
}

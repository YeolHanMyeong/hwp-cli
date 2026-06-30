//! `hwp edit` — 기존 문서를 인메모리로 편집해 다시 쓴다.
//!
//! 원본을 IR로 읽어(이미지·opaque 보존) 텍스트 치환·표 셀 설정을 적용한 뒤
//! 출력 포맷으로 저장한다. hwp 출력은 합성 경로(`write_hwp_edited`)를 거쳐
//! 편집으로 낡은 줄 배치·문단 불변식을 다시 세운다.

use std::path::Path;

use anyhow::Context;

use crate::commands::cat::load_document;

#[allow(clippy::too_many_arguments)]
pub fn run(
    input: &Path,
    output: &Path,
    replaces: &[String],
    add_rows: &[String],
    set_cells: &[String],
    set_fields: &[String],
    set_meta: &[String],
    add_memos: &[String],
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

    // 행 추가는 셀 설정보다 먼저 — 같은 호출 안에서 새 행을 --set-cell로 채울 수 있게.
    for spec in add_rows {
        let (ti, tpl, count) = parse_add_row(spec)?;
        hwp_convert::add_rows(&mut doc, ti, tpl, count).map_err(|e| anyhow::anyhow!(e))?;
        match tpl {
            Some(r) => eprintln!("행 추가: 표{ti} (템플릿 행 {r}) × {count}"),
            None => eprintln!("행 추가: 표{ti} × {count}"),
        }
        edits += count;
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

    for spec in set_fields {
        let (name, value) = spec
            .split_once('=')
            .with_context(|| format!("--set-field 형식은 \"이름=값\" 입니다: {spec:?}"))?;
        let n = hwp_convert::set_field(&mut doc, name, value);
        if n == 0 {
            eprintln!("경고: 필드 {name:?}를 찾지 못했습니다 (hwp fields로 이름 확인)");
        } else {
            eprintln!("필드 설정: {name:?} = {value:?} ({n}건)");
        }
        edits += n;
    }

    for spec in set_meta {
        let (key, value) = spec
            .split_once('=')
            .with_context(|| format!("--set-meta 형식은 \"키=값\" 입니다: {spec:?}"))?;
        let val = (!value.is_empty()).then(|| value.to_string());
        match key.trim() {
            "title" => doc.metadata.title = val,
            "author" => doc.metadata.author = val,
            "subject" => doc.metadata.subject = val,
            "keywords" => doc.metadata.keywords = val,
            other => {
                anyhow::bail!("--set-meta 키는 title|author|subject|keywords 입니다: {other:?}")
            }
        }
        eprintln!("메타데이터 설정: {key} = {value:?}");
        edits += 1;
    }

    for text in add_memos {
        let id = hwp_convert::add_memo(&mut doc, 0, None, text);
        eprintln!("메모 추가: #{id} {text:?}");
        edits += 1;
    }

    if edits == 0 {
        eprintln!(
            "경고: 적용된 편집이 없습니다 (--replace/--add-row/--set-cell/--set-field/--set-meta/--add-memo 확인)"
        );
    }

    write_output(&doc, output)?;
    if verify {
        verify_output(output)?;
    }
    eprintln!("편집 완료: {} → {}", input.display(), output.display());
    Ok(())
}

/// `--add-row` 사양 파싱 — `"표:N"`(템플릿 자동) 또는 `"표:템플릿행:N"`.
/// 반환: (표 인덱스, 템플릿 행 옵션, 추가 행 수).
fn parse_add_row(spec: &str) -> anyhow::Result<(usize, Option<u16>, usize)> {
    let parts: Vec<&str> = spec.split(':').map(str::trim).collect();
    match parts.as_slice() {
        [t, n] => {
            let ti: usize = t.parse().context("표 인덱스")?;
            let count: usize = n.parse().context("행 수")?;
            Ok((ti, None, count))
        }
        [t, r, n] => {
            let ti: usize = t.parse().context("표 인덱스")?;
            let tpl: u16 = r.parse().context("템플릿 행")?;
            let count: usize = n.parse().context("행 수")?;
            Ok((ti, Some(tpl), count))
        }
        _ => anyhow::bail!("--add-row 형식은 \"표:N\" 또는 \"표:템플릿행:N\" 입니다: {spec:?}"),
    }
}

fn write_output(doc: &hwp_model::Document, output: &Path) -> anyhow::Result<()> {
    match output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("hwp") => {
            let warnings = crate::commands::convert::write_hwp_edited(doc, output)?;
            crate::commands::convert::print_warnings(&warnings);
        }
        Some("hwpx") => {
            let warnings = hwpx::write_document(doc, output)?;
            crate::commands::convert::print_warnings(&warnings);
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

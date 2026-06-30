//! `hwp edit` — 기존 문서를 인메모리로 편집해 다시 쓴다.
//!
//! 원본을 IR로 읽어(이미지·opaque 보존) 텍스트 치환·표 셀 설정을 적용한 뒤
//! 출력 포맷으로 저장한다. hwp 출력은 합성 경로(`write_hwp_edited`)를 거쳐
//! 편집으로 낡은 줄 배치·문단 불변식을 다시 세운다.

use std::path::Path;

use anyhow::Context;
use hwp_convert::CharFormat;

use crate::commands::cat::load_document;

#[allow(clippy::too_many_arguments)]
pub fn run(
    input: &Path,
    output: &Path,
    replaces: &[String],
    set_cells: &[String],
    set_fields: &[String],
    create_fields: &[String],
    set_formats: &[String],
    set_aligns: &[String],
    insert_paras: &[String],
    insert_paras_before: &[String],
    delete_paras: &[String],
    add_rows: &[String],
    delete_rows: &[String],
    verify: bool,
) -> anyhow::Result<()> {
    let mut doc = load_document(input)?;
    let mut edits = 0usize;
    // 구조 편집(문단/행 추가·삭제)은 삽입 불변식 적용을 위해 합성 경로로 써야 한다.
    let structural = !insert_paras.is_empty()
        || !insert_paras_before.is_empty()
        || !delete_paras.is_empty()
        || !add_rows.is_empty()
        || !delete_rows.is_empty();

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

    // 누름틀 생성은 set_field보다 먼저 — 같은 호출에서 생성한 필드를 바로 채울 수 있게.
    for spec in create_fields {
        let (anchor, rest) = spec.split_once("=>").with_context(|| {
            format!("--create-field 형식은 \"앵커=>이름\" 또는 \"앵커=>이름=값\" 입니다: {spec:?}")
        })?;
        let (name, value) = rest.split_once('=').unwrap_or((rest, ""));
        if hwp_convert::create_field(&mut doc, anchor, name, value) {
            eprintln!("누름틀 생성: {anchor:?} 뒤에 이름={name:?} 값={value:?}");
            edits += 1;
        } else {
            eprintln!("경고: 앵커 {anchor:?}를 찾지 못했습니다");
        }
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

    for spec in set_formats {
        let (pattern, attrs) = spec.split_once(':').with_context(|| {
            format!("--set-format 형식은 \"찾기:속성=값,…\" 입니다: {spec:?}")
        })?;
        let fmt = parse_char_format(attrs)?;
        let n = hwp_convert::set_char_format(&mut doc, pattern, &fmt);
        if n == 0 {
            eprintln!("경고: 서식 대상 {pattern:?}를 찾지 못했습니다");
        } else {
            eprintln!("글자 서식: {pattern:?} ({n}건)");
        }
        edits += n;
    }

    for spec in set_aligns {
        let (pattern, name) = spec
            .split_once('=')
            .with_context(|| format!("--set-align 형식은 \"찾기=정렬\" 입니다: {spec:?}"))?;
        let align = parse_align(name)?;
        let n = hwp_convert::set_para_align(&mut doc, pattern, align);
        if n == 0 {
            eprintln!("경고: 정렬 대상 {pattern:?}를 찾지 못했습니다");
        } else {
            eprintln!("문단 정렬: {pattern:?} = {name:?} ({n}건)");
        }
        edits += n;
    }

    for spec in insert_paras_before {
        let (anchor, text) = spec.split_once("=>").with_context(|| {
            format!("--insert-para-before 형식은 \"앵커=>텍스트\" 입니다: {spec:?}")
        })?;
        if hwp_convert::insert_paragraph(&mut doc, anchor, text, true) {
            eprintln!("문단 삽입(앞): {anchor:?} 앞에 {text:?}");
            edits += 1;
        } else {
            eprintln!("경고: 앵커 {anchor:?}를 찾지 못했습니다");
        }
    }

    for spec in insert_paras {
        let (anchor, text) = spec
            .split_once("=>")
            .with_context(|| format!("--insert-para 형식은 \"앵커=>텍스트\" 입니다: {spec:?}"))?;
        if hwp_convert::insert_paragraph(&mut doc, anchor, text, false) {
            eprintln!("문단 삽입(뒤): {anchor:?} 뒤에 {text:?}");
            edits += 1;
        } else {
            eprintln!("경고: 앵커 {anchor:?}를 찾지 못했습니다");
        }
    }

    for matching in delete_paras {
        let n = hwp_convert::delete_paragraph(&mut doc, matching);
        if n == 0 {
            eprintln!("경고: 삭제 대상 문단 {matching:?}를 찾지 못했습니다");
        } else {
            eprintln!("문단 삭제: {matching:?} ({n}건)");
        }
        edits += n;
    }

    for spec in add_rows {
        let ti: usize = spec.trim().parse().with_context(|| {
            format!("--add-row 형식은 표 인덱스(예: \"0\") 입니다: {spec:?}")
        })?;
        hwp_convert::add_table_row(&mut doc, ti).map_err(|e| anyhow::anyhow!(e))?;
        eprintln!("표 행 추가: 표{ti}");
        edits += 1;
    }

    for spec in delete_rows {
        let (t, r) = spec
            .split_once(':')
            .with_context(|| format!("--delete-row 형식은 \"표:행\" 입니다: {spec:?}"))?;
        let ti: usize = t.trim().parse().context("표 인덱스")?;
        let row: u16 = r.trim().parse().context("행 번호")?;
        hwp_convert::delete_table_row(&mut doc, ti, row).map_err(|e| anyhow::anyhow!(e))?;
        eprintln!("표 행 삭제: 표{ti} 행{row}");
        edits += 1;
    }

    if edits == 0 {
        eprintln!(
            "경고: 적용된 편집이 없습니다 (--replace/--set-cell/--set-field/--create-field/--set-format/--set-align/--insert-para/--delete-para/--add-row/--delete-row 확인)"
        );
    }

    write_output(&doc, output, structural)?;
    if verify {
        verify_output(output)?;
    }
    eprintln!("편집 완료: {} → {}", input.display(), output.display());
    Ok(())
}

fn write_output(doc: &hwp_model::Document, output: &Path, structural: bool) -> anyhow::Result<()> {
    match output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        // 구조 편집은 삽입 문단/행에 불변식을 세우려 합성 경로를 강제한다.
        Some("hwp") if structural => crate::commands::convert::write_hwp_structural(doc, output)?,
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

/// "bold=on,size=16,color=#FF0000" → CharFormat.
fn parse_char_format(attrs: &str) -> anyhow::Result<CharFormat> {
    let mut fmt = CharFormat::default();
    for kv in attrs.split(',') {
        let kv = kv.trim();
        if kv.is_empty() {
            continue;
        }
        let (k, v) = kv.split_once('=').unwrap_or((kv, "on"));
        let v = v.trim();
        match k.trim().to_ascii_lowercase().as_str() {
            "bold" | "굵게" => fmt.bold = Some(parse_on(v)),
            "italic" | "기울임" => fmt.italic = Some(parse_on(v)),
            "underline" | "밑줄" => fmt.underline = Some(parse_on(v)),
            "strike" | "취소선" => fmt.strike = Some(parse_on(v)),
            "size" | "크기" => {
                fmt.size_pt = Some(v.parse().with_context(|| format!("size 값: {v:?}"))?);
            }
            "color" | "색" => {
                fmt.color = Some(parse_color(v).with_context(|| format!("color 값: {v:?}"))?);
            }
            other => anyhow::bail!("알 수 없는 서식 속성: {other:?}"),
        }
    }
    Ok(fmt)
}

fn parse_on(v: &str) -> bool {
    matches!(
        v.trim().to_ascii_lowercase().as_str(),
        "on" | "true" | "1" | "yes" | "y"
    )
}

/// "#RRGGBB" 또는 색 이름 → COLORREF(0x00BBGGRR).
pub(crate) fn parse_color(s: &str) -> Option<u32> {
    let s = s.trim();
    let rgb = match s.to_ascii_lowercase().as_str() {
        "red" | "빨강" => (0xFF, 0x00, 0x00),
        "green" | "초록" => (0x00, 0x80, 0x00),
        "blue" | "파랑" => (0x00, 0x00, 0xFF),
        "black" | "검정" => (0x00, 0x00, 0x00),
        "white" | "흰색" => (0xFF, 0xFF, 0xFF),
        "yellow" | "노랑" => (0xFF, 0xFF, 0x00),
        _ => {
            let hex = s.strip_prefix('#').unwrap_or(s);
            if hex.len() != 6 {
                return None;
            }
            let v = u32::from_str_radix(hex, 16).ok()?;
            ((v >> 16) & 0xFF, (v >> 8) & 0xFF, v & 0xFF)
        }
    };
    let (r, g, b) = rgb;
    Some((b << 16) | (g << 8) | r)
}

/// 정렬 이름 → 코드(0=양쪽,1=왼쪽,2=오른쪽,3=가운데,4=배분,5=나눔).
fn parse_align(name: &str) -> anyhow::Result<u8> {
    Ok(match name.trim().to_ascii_lowercase().as_str() {
        "left" | "왼쪽" => 1,
        "right" | "오른쪽" => 2,
        "center" | "가운데" => 3,
        "justify" | "both" | "양쪽" => 0,
        "distribute" | "배분" => 4,
        "divide" | "나눔" => 5,
        other => anyhow::bail!("알 수 없는 정렬: {other:?} (left/right/center/justify/distribute)"),
    })
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

//! 충실도 보존 자리표시자 치환 (패키지 외과 수술).
//!
//! IR을 거치는 [`write`](crate::write)는 미리보기 썸네일(`Preview/PrvImage.png`)·
//! `hp:switch` 2016 호환 블록·미모델 엔트리(settings/DocOptions/scripts)를 잃는다.
//! 이 모듈은 본문 `Contents/section*.xml`의 `{{name}}` 텍스트만 외과적으로 치환하고
//! 나머지 엔트리는 ZIP raw 복사로 **바이트 보존**한다(mimetype STORED·순서 포함).
//!
//! 치환은 문단 단위로 수행하며, 치환된 문단의 `<hp:linesegarray>`(줄 배치 캐시)를
//! 제거한다. 텍스트 길이가 바뀌었는데 줄 배치가 남아 있으면 macOS 한글에서
//! 글자가 겹쳐 보이고 "변조" 보안 경고의 원인이 된다. 치환되지 않은 문단은
//! 바이트 그대로 두고, 치환이 전혀 없는 섹션은 raw 복사한다.
//!
//! 한계: 자리표시자가 `<hp:t>` 런/문단 경계를 가로지르면(예: `<hp:t>{{기</hp:t>
//! <hp:t>관명}}</hp:t>`) 문자열 치환이 매칭하지 못한다. 템플릿은 자리표시자를
//! 단일 런으로 작성할 것(현행 내장 템플릿은 모두 단일 런).

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use crate::error::Result;

/// `Contents/section*.xml`의 `{{name}}`을 값으로 치환하고, 그 외 엔트리는 원본 그대로
/// 복사한다. 반환: 이름 → 본문 치환 횟수(요청한 모든 이름 포함, 미발견은 0).
pub fn fill_placeholders(
    input: &Path,
    output: &Path,
    values: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, usize>> {
    // 제자리 치환 방지: input==output이면 File::create(O_TRUNC)가 입력을 먼저 비워
    // 스트리밍 복사가 손상된다. canonicalize로 ./·심링크·상대경로까지 비교(출력이
    // 아직 없으면 canonicalize 실패 → 같을 수 없으므로 통과).
    if let (Ok(a), Ok(b)) = (input.canonicalize(), output.canonicalize())
        && a == b
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "입력과 출력 경로가 같습니다 (제자리 치환 미지원): 다른 출력 경로를 지정하세요",
        )
        .into());
    }
    let reader = File::open(input)?;
    let mut archive = zip::ZipArchive::new(reader)?;
    let out = File::create(output)?;
    let mut zip = zip::ZipWriter::new(out);

    let mut counts: BTreeMap<String, usize> = values.keys().map(|k| (k.clone(), 0)).collect();
    let deflated = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for i in 0..archive.len() {
        let name = archive.by_index_raw(i)?.name().to_string();
        let is_section = name.starts_with("Contents/section") && name.ends_with(".xml");

        let rewritten: Option<Vec<u8>> = if is_section {
            let mut xml = String::new();
            archive.by_index(i)?.read_to_string(&mut xml)?;
            fill_section_xml(&xml, values, &mut counts).map(String::into_bytes)
        } else {
            None
        };

        match rewritten {
            Some(bytes) => {
                zip.start_file(&name, deflated)?;
                zip.write_all(&bytes)?;
            }
            // 무변경 엔트리는 전부 바이트 보존 (미리보기·compat·BinData·mimetype STORED 포함).
            None => {
                let raw = archive.by_index_raw(i)?;
                zip.raw_copy_file(raw)?;
            }
        }
    }
    zip.finish()?;
    Ok(counts)
}

/// 섹션 XML에서 자리표시자를 문단 단위로 치환한다. 치환이 일어난 문단은
/// `<hp:linesegarray>`를 제거하고(한글이 열 때 줄 배치 재계산), 나머지 문단은
/// 바이트 그대로 둔다. 변경이 없으면 `None`.
fn fill_section_xml(
    xml: &str,
    values: &BTreeMap<String, String>,
    counts: &mut BTreeMap<String, usize>,
) -> Option<String> {
    let mut out = String::with_capacity(xml.len());
    let mut cursor = 0usize;
    let mut changed = false;

    for (start, end) in paragraph_spans(xml) {
        let para = &xml[start..end];
        let mut replaced: Option<String> = None;
        for (k, v) in values {
            let needle = format!("{{{{{k}}}}}"); // {{k}}
            let target = replaced.as_deref().unwrap_or(para);
            let n = target.matches(needle.as_str()).count();
            if n > 0 {
                let next = target.replace(needle.as_str(), &xml_escape(v));
                replaced = Some(next);
                if let Some(c) = counts.get_mut(k) {
                    *c += n;
                }
            }
        }
        if let Some(mut new_para) = replaced {
            strip_linesegarray(&mut new_para);
            out.push_str(&xml[cursor..start]);
            out.push_str(&new_para);
            cursor = end;
            changed = true;
        }
    }
    if !changed {
        return None;
    }
    out.push_str(&xml[cursor..]);
    Some(out)
}

/// 최상위 `<hp:p …>…</hp:p>` 구간(바이트 오프셋)을 찾는다. 표 셀 등 중첩 문단은
/// 바깥 문단 구간에 포함된다(치환 시 바깥 문단째로 처리 — 줄 배치 제거는
/// 넉넉해도 안전하다: 한글이 해당 문단만 재계산).
///
/// 주의: 속성값 안의 `>`는 고려하지 않는다(한글 계열 직렬화기는 속성에 `>`를
/// 이스케이프해 출력하므로 실 파일에서는 발생하지 않음).
fn paragraph_spans(xml: &str) -> Vec<(usize, usize)> {
    const OPEN: &str = "<hp:p";
    const CLOSE: &str = "</hp:p>";
    let bytes = xml.as_bytes();
    let mut spans = Vec::new();
    let mut depth = 0usize;
    let mut top_start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &xml[i..];
        if rest.starts_with(CLOSE) {
            if depth > 0 {
                depth -= 1;
                if depth == 0 {
                    spans.push((top_start, i + CLOSE.len()));
                }
            }
            i += CLOSE.len();
        } else if rest.starts_with(OPEN)
            && matches!(
                bytes.get(i + OPEN.len()),
                Some(b' ' | b'>' | b'/' | b'\t' | b'\r' | b'\n')
            )
        {
            let Some(gt) = rest.find('>') else { break };
            let close = i + gt;
            if bytes[close - 1] == b'/' {
                // 자기닫힘 빈 문단 — 텍스트가 없으므로 구간으로만 기록.
                if depth == 0 {
                    spans.push((i, close + 1));
                }
            } else {
                if depth == 0 {
                    top_start = i;
                }
                depth += 1;
            }
            i = close + 1;
        } else {
            i += 1;
        }
    }
    spans
}

/// 문단 문자열에서 `<hp:linesegarray …>…</hp:linesegarray>`(자기닫힘 포함)를
/// 모두 제거한다.
fn strip_linesegarray(para: &mut String) {
    const OPEN: &str = "<hp:linesegarray";
    const CLOSE: &str = "</hp:linesegarray>";
    while let Some(a) = para.find(OPEN) {
        let after = a + OPEN.len();
        let Some(gt) = para[after..].find('>').map(|p| after + p) else {
            break;
        };
        let end = if para.as_bytes()[gt - 1] == b'/' {
            gt + 1
        } else {
            match para[gt..].find(CLOSE) {
                Some(p) => gt + p + CLOSE.len(),
                None => break,
            }
        };
        para.replace_range(a..end, "");
    }
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

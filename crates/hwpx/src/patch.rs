//! 충실도 보존 자리표시자 치환 (패키지 외과 수술).
//!
//! IR을 거치는 [`write`](crate::write)는 미리보기 썸네일(`Preview/PrvImage.png`)·
//! `hp:switch` 2016 호환 블록·미모델 엔트리(settings/DocOptions/scripts)를 잃는다.
//! 이 모듈은 본문 `Contents/section*.xml`의 `{{name}}` 텍스트만 외과적으로 치환하고
//! 나머지 엔트리는 ZIP raw 복사로 **바이트 보존**한다(mimetype STORED·순서 포함).
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
/// 복사한다. 반환: 이름 → 치환 횟수(요청한 모든 이름 포함, 미발견은 0).
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

    for i in 0..archive.len() {
        let name = archive.by_index_raw(i)?.name().to_string();
        let is_section = name.starts_with("Contents/section") && name.ends_with(".xml");

        if is_section {
            let mut xml = String::new();
            archive.by_index(i)?.read_to_string(&mut xml)?;
            for (k, v) in values {
                let needle = format!("{{{{{k}}}}}"); // {{k}}
                let n = xml.matches(needle.as_str()).count();
                if n > 0 {
                    xml = xml.replace(needle.as_str(), &xml_escape(v));
                    if let Some(c) = counts.get_mut(k) {
                        *c += n;
                    }
                }
            }
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file(&name, opts)?;
            zip.write_all(xml.as_bytes())?;
        } else {
            // 미리보기·compat·BinData·mimetype(STORED) 등 전부 바이트 보존.
            let raw = archive.by_index_raw(i)?;
            zip.raw_copy_file(raw)?;
        }
    }
    zip.finish()?;
    Ok(counts)
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

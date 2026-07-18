//! 충실도 보존 텍스트 치환 (패키지 외과 수술).
//!
//! IR을 거치는 [`write`](crate::write)는 미리보기 썸네일(`Preview/PrvImage.png`)·
//! `hp:switch` 2016 호환 블록·미모델 엔트리(settings/DocOptions/scripts)를 잃는다.
//! 이 모듈은 본문 `Contents/section*.xml`(과 미리보기 텍스트)의 텍스트만 외과적으로
//! 치환하고 나머지 엔트리는 ZIP raw 복사로 **바이트 보존**한다(mimetype STORED·순서 포함).
//!
//! 한계: 대상 문자열이 `<hp:t>` 런/문단 경계를 가로지류면(예: `<hp:t>{{기</hp:t>
//! <hp:t>관명}}</hp:t>`) 문자열 치환이 매칭하지 못한다. XML 이벤트 단위 재구성이
//! 필요한 편집은 IR 경로(`hwp edit`)를 쓴다.

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
    let mut counts: BTreeMap<String, usize> = values.keys().map(|k| (k.clone(), 0)).collect();
    process_package(input, output, |name, data| {
        if !is_section_entry(name) {
            return Ok(None);
        }
        let mut xml = String::from_utf8(data).map_err(invalid_data)?;
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
        Ok(Some(xml.into_bytes()))
    })?;
    Ok(counts)
}

/// `Contents/section*.xml`에서 (from→to) 쌍을 **순서대로** 치환하고, 그 외 엔트리는
/// 원본 그대로 복사한다. `Preview/PrvText.txt`는 평문(UTF-8, 아니면 UTF-16LE)으로 치환한다.
///
/// 치환은 순차 적용이다 — 앞 쌍의 치환 결과가 뒤 쌍의 from에 매칭될 수 있으므로
/// 호출자는 **긴 이름을 먼저** 넣어야 한다(예: "제주한라대학교" → "제주한라대").
/// 반환: 변환 대상 엔트리(section*.xml, PrvText.txt) → 치환 건수(0 포함).
pub fn replace_texts(
    input: &Path,
    output: &Path,
    replacements: &[(String, String)],
) -> Result<BTreeMap<String, usize>> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    process_package(input, output, |name, data| {
        if is_section_entry(name) {
            let xml = String::from_utf8(data).map_err(invalid_data)?;
            let (xml, n) = replace_seq(xml, replacements, true);
            counts.insert(name.to_string(), n);
            return Ok(Some(xml.into_bytes()));
        }
        if name == "Preview/PrvText.txt" {
            let (data, n) = replace_in_prvtext(data, replacements);
            counts.insert(name.to_string(), n);
            return Ok(Some(data));
        }
        Ok(None)
    })?;
    Ok(counts)
}

/// UTF-8 디코드 실패를 io 오류로 변환.
fn invalid_data(e: std::string::FromUtf8Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e)
}

/// 변환 대상 엔트리 판별 (본문 섹션 XML).
fn is_section_entry(name: &str) -> bool {
    name.starts_with("Contents/section") && name.ends_with(".xml")
}

/// 패키지 순회 공통 뼈대: transform이 `Some(바이트)`를 돌려주면 그 내용으로 쓰고
/// (Deflated), `None`이면 원본 엔트리를 raw 복사한다(바이트 보존).
/// 제자리(input==output) 변환은 금지 — File::create(O_TRUNC)가 입력을 먼저 비운다.
fn process_package(
    input: &Path,
    output: &Path,
    mut transform: impl FnMut(&str, Vec<u8>) -> Result<Option<Vec<u8>>>,
) -> Result<()> {
    // 제자리 치환 방지: canonicalize로 ./·심링크·상대경로까지 비교(출력이
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

    for i in 0..archive.len() {
        let name = archive.by_index_raw(i)?.name().to_string();
        let mut data = Vec::new();
        archive.by_index(i)?.read_to_end(&mut data)?;
        if let Some(new_data) = transform(&name, data)? {
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file(&name, opts)?;
            zip.write_all(&new_data)?;
        } else {
            // 미리보기·compat·BinData·mimetype(STORED) 등 전부 바이트 보존.
            let raw = archive.by_index_raw(i)?;
            zip.raw_copy_file(raw)?;
        }
    }
    zip.finish()?;
    Ok(())
}

/// (from→to) 쌍을 순서대로 치환한다. `escape`면 XML 텍스트 규칙으로 이스케이프 후 치환.
/// 반환: (결과 문자열, 총 치환 건수).
fn replace_seq(
    mut text: String,
    replacements: &[(String, String)],
    escape: bool,
) -> (String, usize) {
    let mut total = 0;
    for (from, to) in replacements {
        if from.is_empty() {
            continue;
        }
        let (needle, value) = if escape {
            (xml_escape(from), xml_escape(to))
        } else {
            (from.clone(), to.clone())
        };
        let n = text.matches(needle.as_str()).count();
        if n > 0 {
            text = text.replace(needle.as_str(), &value);
            total += n;
        }
    }
    (text, total)
}

/// PrvText.txt 평문 치환. UTF-8이 우선이고, 실패 시 UTF-16LE로 디코드해 치환 후
/// 같은 인코딩으로 되돌린다. 치환 건수 0이어도 Some을 돌려준다(엔트리 존재 보고용).
fn replace_in_prvtext(data: Vec<u8>, replacements: &[(String, String)]) -> (Vec<u8>, usize) {
    if let Ok(text) = String::from_utf8(data.clone()) {
        let (text, n) = replace_seq(text, replacements, false);
        return (text.into_bytes(), n);
    }
    // UTF-16LE 폴백: 손실 허용 디코드(치환 대상 이름은 정상 문자 영역에 있음).
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let text = String::from_utf16_lossy(&units);
    let (text, n) = replace_seq(text, replacements, false);
    let encoded: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    (encoded, n)
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

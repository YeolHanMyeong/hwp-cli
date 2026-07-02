//! 충실도 보존 자리표시자 치환 (패키지 외과 수술).
//!
//! IR을 거치는 [`write`](crate::write)는 미리보기 썸네일(`Preview/PrvImage.png`)·
//! `hp:switch` 2016 호환 블록·미모델 엔트리(settings/DocOptions/scripts)를 잃는다.
//! 이 모듈은 본문 `Contents/section*.xml`의 `{{name}}` 텍스트만 외과적으로 치환하고
//! 나머지 엔트리는 ZIP raw 복사로 **바이트 보존**한다(mimetype STORED·순서 포함).
//!
//! 치환에 수반되는 정합성 처리:
//! - 치환은 문단 단위로 수행하며, 치환된 문단의 `<hp:linesegarray>`(줄 배치
//!   캐시)를 제거한다. 텍스트 길이가 바뀌었는데 줄 배치가 남아 있으면 macOS
//!   한글에서 글자가 겹쳐 보이고 "변조" 보안 경고의 원인이 된다. 치환되지
//!   않은 문단은 바이트 그대로 두고, 치환이 전혀 없는 섹션은 raw 복사한다.
//! - `Preview/PrvText.txt`(UTF-8/UTF-16LE 자동 감지)와 `Contents/content.hpf`에
//!   남은 자리표시자도 함께 치환한다 (탐색기·한글 미리보기에 옛 텍스트 잔류 방지).
//! - 다시 쓴 엔트리의 zip 중앙 디렉터리 메타데이터(생성 시스템·수정 시각·
//!   external attr)를 원본 값으로 되돌린다. hwp2hwpx 등 Java 변환본은
//!   FAT origin(생성 시스템 0)·external attr 0인데, 재작성 시 기본값이 섞이면
//!   한글이 "손상/변조" 경고를 띄우는 사례가 있다.
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
/// 미리보기·content.hpf 치환은 횟수에 포함하지 않는다.
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
        } else if name == "Preview/PrvText.txt" {
            let mut raw = Vec::new();
            archive.by_index(i)?.read_to_end(&mut raw)?;
            patch_preview_text(&raw, values)
        } else if name == "Contents/content.hpf" {
            let mut raw = Vec::new();
            archive.by_index(i)?.read_to_end(&mut raw)?;
            std::str::from_utf8(&raw)
                .ok()
                .and_then(|xml| replace_placeholders(xml, values, true))
                .map(String::into_bytes)
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

    // 다시 쓴 엔트리의 zip 메타데이터를 원본과 동일하게 복원 (best-effort:
    // ZIP64 등 파싱 불가 구조면 그대로 둔다 — 치환 결과 자체는 유효).
    sync_entry_metadata(input, output)?;
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

/// 미리보기 텍스트의 자리표시자를 치환한다. 원본 인코딩(UTF-8 또는
/// UTF-16LE, BOM 유무 포함)을 그대로 유지한다. 변경 없으면 `None`.
fn patch_preview_text(raw: &[u8], values: &BTreeMap<String, String>) -> Option<Vec<u8>> {
    const BOM: [u8; 2] = [0xFF, 0xFE];
    // hwp2hwpx 등 Java 변환본은 UTF-16LE(BOM), 한글 직접 저장본은 UTF-8이
    // 일반적이다. BOM 또는 선두 NUL 바이트(ASCII의 상위 바이트)로 판별.
    let utf16 = raw.starts_with(&BOM) || raw.iter().take(64).any(|&b| b == 0);
    if utf16 {
        let has_bom = raw.starts_with(&BOM);
        let body = if has_bom { &raw[2..] } else { raw };
        let units: Vec<u16> = body
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let text = String::from_utf16_lossy(&units);
        let replaced = replace_placeholders(&text, values, false)?;
        let mut out = Vec::with_capacity(raw.len());
        if has_bom {
            out.extend_from_slice(&BOM);
        }
        for u in replaced.encode_utf16() {
            out.extend_from_slice(&u.to_le_bytes());
        }
        Some(out)
    } else {
        let text = std::str::from_utf8(raw).ok()?;
        replace_placeholders(text, values, false).map(String::into_bytes)
    }
}

/// `{{name}}` → 값 치환. `escape`면 XML 이스케이프 적용. 변경 없으면 `None`.
fn replace_placeholders(
    text: &str,
    values: &BTreeMap<String, String>,
    escape: bool,
) -> Option<String> {
    let mut out: Option<String> = None;
    for (k, v) in values {
        let needle = format!("{{{{{k}}}}}");
        let target = out.as_deref().unwrap_or(text);
        if target.contains(needle.as_str()) {
            let value = if escape { xml_escape(v) } else { v.clone() };
            out = Some(target.replace(needle.as_str(), &value));
        }
    }
    out
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

// ---------------------------------------------------------------------------
// zip 엔트리 메타데이터 복원
// ---------------------------------------------------------------------------

/// zip 중앙 디렉터리 엔트리의 메타데이터 (진단·검증용).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZipEntryMeta {
    /// version made by — 상위 바이트가 생성 시스템(0=FAT/DOS, 3=Unix).
    pub version_made_by: u16,
    /// MS-DOS 수정 시각.
    pub mod_time: u16,
    /// MS-DOS 수정 날짜.
    pub mod_date: u16,
    /// external file attributes — Unix는 상위 16비트에 모드, FAT origin은 0.
    pub external_attr: u32,
}

/// zip 파일의 엔트리 이름 → 중앙 디렉터리 메타데이터. ZIP64 등 파싱 불가
/// 구조면 빈 맵을 반환한다.
pub fn zip_entry_metadata(path: &Path) -> Result<BTreeMap<String, ZipEntryMeta>> {
    let data = std::fs::read(path)?;
    let entries = parse_central_directory(&data).unwrap_or_default();
    Ok(entries.into_iter().map(|e| (e.name, e.meta)).collect())
}

struct CdEntry {
    name: String,
    /// 중앙 디렉터리 레코드 시작 오프셋.
    cd_pos: usize,
    /// 대응 로컬 헤더 오프셋.
    local_offset: u32,
    meta: ZipEntryMeta,
}

/// 출력 zip의 각 엔트리에 대해, 같은 이름이 입력에 있으면 생성 시스템
/// (version made by)·수정 시각·external attr를 입력 값으로 되돌린다.
/// 로컬 헤더의 수정 시각도 함께 맞춰 중앙 디렉터리와의 불일치를 막는다.
///
/// 근거: hwp2hwpx(Java) 변환본은 FAT origin(생성 시스템 0)·external attr 0으로
/// 기록되는데, 재작성 엔트리에 Unix 퍼미션/현재 시각이 섞이면 한글 문서 보안
/// 검사에서 "손상/변조" 경고가 뜨는 사례가 있다 (han-auto에서 실증).
fn sync_entry_metadata(input: &Path, output: &Path) -> Result<()> {
    let src = std::fs::read(input)?;
    let Some(src_entries) = parse_central_directory(&src) else {
        return Ok(());
    };
    let src_map: BTreeMap<&str, &CdEntry> =
        src_entries.iter().map(|e| (e.name.as_str(), e)).collect();

    let mut dst = std::fs::read(output)?;
    let Some(dst_entries) = parse_central_directory(&dst) else {
        return Ok(());
    };

    const LOCAL_SIG: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];
    let mut changed = false;
    for e in &dst_entries {
        let Some(s) = src_map.get(e.name.as_str()) else {
            continue;
        };
        if e.meta == s.meta {
            continue;
        }
        write_u16(&mut dst, e.cd_pos + 4, s.meta.version_made_by);
        write_u16(&mut dst, e.cd_pos + 12, s.meta.mod_time);
        write_u16(&mut dst, e.cd_pos + 14, s.meta.mod_date);
        write_u32(&mut dst, e.cd_pos + 38, s.meta.external_attr);
        let lo = e.local_offset as usize;
        if dst.len() >= lo + 30 && dst[lo..lo + 4] == LOCAL_SIG {
            write_u16(&mut dst, lo + 10, s.meta.mod_time);
            write_u16(&mut dst, lo + 12, s.meta.mod_date);
        }
        changed = true;
    }
    if changed {
        std::fs::write(output, &dst)?;
    }
    Ok(())
}

/// 중앙 디렉터리를 파싱한다. ZIP64이거나 구조가 어긋나면 `None`
/// (호출부는 메타데이터 복원을 건너뛴다).
fn parse_central_directory(data: &[u8]) -> Option<Vec<CdEntry>> {
    const EOCD_SIG: [u8; 4] = [0x50, 0x4B, 0x05, 0x06];
    const CD_SIG: [u8; 4] = [0x50, 0x4B, 0x01, 0x02];

    // EOCD는 파일 끝 22바이트 + 최대 65535바이트 주석 안에 있다. 뒤에서부터 탐색.
    let tail_start = data.len().saturating_sub(22 + 65_535);
    let last = data.len().checked_sub(22)?;
    let eocd = (tail_start..=last)
        .rev()
        .find(|&i| data[i..i + 4] == EOCD_SIG)?;

    let total = read_u16(data, eocd + 10)? as usize;
    let cd_offset = read_u32(data, eocd + 16)? as usize;
    if total == 0xFFFF || cd_offset == 0xFFFF_FFFF_usize {
        return None; // ZIP64 — hwpx에선 비현실적 크기, 복원 생략
    }

    let mut entries = Vec::with_capacity(total);
    let mut pos = cd_offset;
    for _ in 0..total {
        if data.get(pos..pos + 4)? != CD_SIG {
            return None;
        }
        let name_len = read_u16(data, pos + 28)? as usize;
        let extra_len = read_u16(data, pos + 30)? as usize;
        let comment_len = read_u16(data, pos + 32)? as usize;
        let name_bytes = data.get(pos + 46..pos + 46 + name_len)?;
        entries.push(CdEntry {
            name: String::from_utf8_lossy(name_bytes).into_owned(),
            cd_pos: pos,
            local_offset: read_u32(data, pos + 42)?,
            meta: ZipEntryMeta {
                version_made_by: read_u16(data, pos + 4)?,
                mod_time: read_u16(data, pos + 12)?,
                mod_date: read_u16(data, pos + 14)?,
                external_attr: read_u32(data, pos + 38)?,
            },
        });
        pos += 46 + name_len + extra_len + comment_len;
    }
    Some(entries)
}

fn read_u16(data: &[u8], pos: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(pos..pos + 2)?.try_into().ok()?))
}

fn read_u32(data: &[u8], pos: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(pos..pos + 4)?.try_into().ok()?))
}

fn write_u16(data: &mut [u8], pos: usize, v: u16) {
    data[pos..pos + 2].copy_from_slice(&v.to_le_bytes());
}

fn write_u32(data: &mut [u8], pos: usize, v: u32) {
    data[pos..pos + 4].copy_from_slice(&v.to_le_bytes());
}

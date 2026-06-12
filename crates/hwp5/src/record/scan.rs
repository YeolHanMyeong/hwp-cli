//! 레코드 스트림 스캐너.
//!
//! 압축 해제된 DocInfo/BodyText 스트림을 (tag, level, data) 평면 목록으로
//! 읽고 트리로 복원한다. 태그는 해석하지 않는다.

use crate::codec::ByteReader;
use crate::error::{Hwp5Error, Result};
use crate::record::header::RecordHeader;
use crate::record::tree::RecordNode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    /// 손상 발견 시 즉시 Err — writer 검증·왕복 테스트용.
    Strict,
    /// 가능한 만큼 읽고 경고 누적 — 야생 파일 진단용.
    Tolerant,
}

#[derive(Debug)]
pub struct ScanResult {
    pub roots: Vec<RecordNode>,
    pub warnings: Vec<String>,
    /// 스캔한 레코드 총 수.
    pub record_count: usize,
}

/// 압축 해제된 레코드 스트림을 스캔해 트리로 복원한다.
pub fn scan_stream(data: &[u8], mode: ScanMode) -> Result<ScanResult> {
    let mut r = ByteReader::new(data);
    let mut flat: Vec<(RecordHeader, Vec<u8>)> = Vec::new();
    let mut warnings = Vec::new();

    while !r.is_empty() {
        let at = r.pos();
        let header = match RecordHeader::decode(&mut r) {
            Ok(h) => h,
            Err(e) => match mode {
                ScanMode::Strict => return Err(e),
                ScanMode::Tolerant => {
                    warnings.push(format!("오프셋 {at}: 레코드 헤더가 잘림 — 스캔 중단"));
                    break;
                }
            },
        };
        let payload = match r.read_bytes(header.size as usize) {
            Ok(b) => b.to_vec(),
            Err(_) => match mode {
                ScanMode::Strict => {
                    return Err(Hwp5Error::MalformedRecord(format!(
                        "오프셋 {at}: tag 0x{:03X}의 페이로드 {}바이트 중 {}바이트만 남음",
                        header.tag,
                        header.size,
                        r.remaining(),
                    )));
                }
                ScanMode::Tolerant => {
                    warnings.push(format!(
                        "오프셋 {at}: tag 0x{:03X} 페이로드 잘림({}바이트 요구, {}바이트 잔여) — 잘린 채 보존",
                        header.tag,
                        header.size,
                        r.remaining(),
                    ));
                    r.take_rest().to_vec()
                }
            },
        };
        flat.push((header, payload));
    }

    let record_count = flat.len();
    let (roots, tree_warnings) = RecordNode::build_forest(flat);
    if mode == ScanMode::Strict && !tree_warnings.is_empty() {
        return Err(Hwp5Error::MalformedRecord(tree_warnings.join("; ")));
    }
    warnings.extend(tree_warnings);

    Ok(ScanResult {
        roots,
        warnings,
        record_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::ByteWriter;

    fn emit(records: &[(u16, u16, &[u8])]) -> Vec<u8> {
        let mut w = ByteWriter::new();
        for (tag, level, data) in records {
            RecordHeader {
                tag: *tag,
                level: *level,
                size: data.len() as u32,
            }
            .encode(&mut w);
            w.write_bytes(data);
        }
        w.into_bytes()
    }

    #[test]
    fn 정상_스트림_스캔() {
        let bytes = emit(&[(0x10, 0, b"abc"), (0x11, 1, b""), (0x12, 0, b"de")]);
        let res = scan_stream(&bytes, ScanMode::Strict).unwrap();
        assert_eq!(res.record_count, 3);
        assert_eq!(res.roots.len(), 2);
        assert!(res.warnings.is_empty());
    }

    #[test]
    fn 잘린_스트림은_strict에서_err_tolerant에서_경고() {
        let mut bytes = emit(&[(0x10, 0, b"abcdef")]);
        bytes.truncate(bytes.len() - 3); // 페이로드 절단
        assert!(scan_stream(&bytes, ScanMode::Strict).is_err());
        let res = scan_stream(&bytes, ScanMode::Tolerant).unwrap();
        assert_eq!(res.record_count, 1);
        assert_eq!(res.warnings.len(), 1);
    }

    #[test]
    fn 빈_스트림() {
        let res = scan_stream(&[], ScanMode::Strict).unwrap();
        assert_eq!(res.record_count, 0);
        assert!(res.roots.is_empty());
    }
}

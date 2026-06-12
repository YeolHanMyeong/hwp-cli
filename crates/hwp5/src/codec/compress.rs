//! HWP 스트림 압축/해제.
//!
//! FileHeader의 압축 플래그(bit 0)가 설정된 경우 DocInfo/BodyText 스트림은
//! **zlib 헤더 없는 raw deflate**로 압축되어 있다 (pyhwp의 wbits=-15와 동일).

use std::io::Read;

use flate2::Compression;
use flate2::read::{DeflateDecoder, DeflateEncoder};

use crate::error::{Hwp5Error, Result};

/// raw deflate 해제. `stream_name`은 오류 메시지용.
pub fn decompress(data: &[u8], stream_name: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    DeflateDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|source| Hwp5Error::Decompress {
            stream: stream_name.to_string(),
            source,
        })?;
    Ok(out)
}

/// raw deflate 압축 (writer 경로용).
pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    DeflateEncoder::new(data, Compression::default())
        .read_to_end(&mut out)
        .expect("메모리 버퍼 deflate 압축은 실패하지 않는다");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 압축_해제_왕복() {
        let original = "한글 문서 파일 형식 5.0".repeat(100);
        let packed = compress(original.as_bytes());
        assert!(packed.len() < original.len());
        let unpacked = decompress(&packed, "test").unwrap();
        assert_eq!(unpacked, original.as_bytes());
    }

    #[test]
    fn 잘못된_데이터는_err() {
        assert!(decompress(&[0xFF, 0xFF, 0xFF, 0xFF], "test").is_err());
    }
}

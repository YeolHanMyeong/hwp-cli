//! FileHeader 스트림 (256바이트 고정).
//!
//! 레이아웃 (한글문서파일형식 5.0 §4.1):
//! - 0..32   시그니처 `"HWP Document File"` + NUL 패딩
//! - 32..36  버전 DWORD (0xMMnnPPrr — 5.0.3.0 → 0x05000300)
//! - 36..40  속성 플래그 DWORD
//! - 40..44  라이선스(CCL/공공누리) 플래그 DWORD
//! - 44..48  EncryptVersion DWORD
//! - 48      공공누리 라이선스 지원 국가 BYTE
//! - 49..256 예약 (왕복 보존을 위해 그대로 유지)

use serde::Serialize;

use crate::codec::{ByteReader, ByteWriter};
use crate::error::{Hwp5Error, Result};

pub const FILE_HEADER_SIZE: usize = 256;
pub const SIGNATURE: &[u8; 17] = b"HWP Document File";

/// 파일 버전. 0xMMnnPPrr 인코딩의 각 바이트.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HwpVersion {
    pub major: u8,
    pub minor: u8,
    pub build: u8,
    pub revision: u8,
}

impl HwpVersion {
    pub fn from_u32(v: u32) -> Self {
        Self {
            major: (v >> 24) as u8,
            minor: (v >> 16) as u8,
            build: (v >> 8) as u8,
            revision: v as u8,
        }
    }

    pub fn to_u32(self) -> u32 {
        (u32::from(self.major) << 24)
            | (u32::from(self.minor) << 16)
            | (u32::from(self.build) << 8)
            | u32::from(self.revision)
    }
}

impl std::fmt::Display for HwpVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}.{}.{}.{}",
            self.major, self.minor, self.build, self.revision
        )
    }
}

/// 속성 플래그 비트 (36..40 DWORD).
mod attr {
    pub const COMPRESSED: u32 = 1 << 0;
    pub const ENCRYPTED: u32 = 1 << 1;
    pub const DISTRIBUTION: u32 = 1 << 2;
    pub const HAS_SCRIPT: u32 = 1 << 3;
    pub const DRM: u32 = 1 << 4;
    pub const HAS_XML_TEMPLATE: u32 = 1 << 5;
    pub const HAS_HISTORY: u32 = 1 << 6;
    pub const HAS_SIGNATURE: u32 = 1 << 7;
    pub const CERT_ENCRYPTED: u32 = 1 << 8;
    pub const SIGNATURE_SPARE: u32 = 1 << 9;
    pub const CERT_DRM: u32 = 1 << 10;
    pub const CCL: u32 = 1 << 11;
    pub const MOBILE_OPTIMIZED: u32 = 1 << 12;
    pub const PRIVACY_SECURITY: u32 = 1 << 13;
    pub const TRACK_CHANGES: u32 = 1 << 14;
    pub const KOGL: u32 = 1 << 15;
    pub const HAS_VIDEO_CONTROL: u32 = 1 << 16;
    pub const HAS_TOC_FIELD: u32 = 1 << 17;
}

#[derive(Debug, Clone)]
pub struct FileHeader {
    pub version: HwpVersion,
    pub attributes: u32,
    pub license: u32,
    pub encrypt_version: u32,
    pub kogl_country: u8,
    /// 49..256 예약 영역 — 왕복 보존용.
    pub reserved: [u8; FILE_HEADER_SIZE - 49],
}

impl FileHeader {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() != FILE_HEADER_SIZE {
            return Err(Hwp5Error::BadFileHeaderSize(data.len()));
        }
        let mut r = ByteReader::new(data);
        let sig = r.read_bytes(32)?;
        if &sig[..SIGNATURE.len()] != SIGNATURE {
            return Err(Hwp5Error::BadSignature);
        }
        let version = HwpVersion::from_u32(r.read_u32()?);
        let attributes = r.read_u32()?;
        let license = r.read_u32()?;
        let encrypt_version = r.read_u32()?;
        let kogl_country = r.read_u8()?;
        let mut reserved = [0u8; FILE_HEADER_SIZE - 49];
        reserved.copy_from_slice(r.take_rest());
        Ok(Self {
            version,
            attributes,
            license,
            encrypt_version,
            kogl_country,
            reserved,
        })
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut w = ByteWriter::new();
        let mut sig = [0u8; 32];
        sig[..SIGNATURE.len()].copy_from_slice(SIGNATURE);
        w.write_bytes(&sig);
        w.write_u32(self.version.to_u32());
        w.write_u32(self.attributes);
        w.write_u32(self.license);
        w.write_u32(self.encrypt_version);
        w.write_u8(self.kogl_country);
        w.write_bytes(&self.reserved);
        debug_assert_eq!(w.len(), FILE_HEADER_SIZE);
        w.into_bytes()
    }

    pub fn is_compressed(&self) -> bool {
        self.attributes & attr::COMPRESSED != 0
    }

    pub fn is_encrypted(&self) -> bool {
        self.attributes & attr::ENCRYPTED != 0
    }

    pub fn is_distribution(&self) -> bool {
        self.attributes & attr::DISTRIBUTION != 0
    }

    /// 사람이 읽을 수 있는 속성 플래그 이름 목록 (`hwp info`용).
    pub fn attribute_names(&self) -> Vec<&'static str> {
        const TABLE: &[(u32, &str)] = &[
            (attr::COMPRESSED, "압축"),
            (attr::ENCRYPTED, "암호화"),
            (attr::DISTRIBUTION, "배포용 문서"),
            (attr::HAS_SCRIPT, "스크립트 저장"),
            (attr::DRM, "DRM 보안"),
            (attr::HAS_XML_TEMPLATE, "XMLTemplate 스토리지"),
            (attr::HAS_HISTORY, "문서 이력 관리"),
            (attr::HAS_SIGNATURE, "전자 서명 정보"),
            (attr::CERT_ENCRYPTED, "공인 인증서 암호화"),
            (attr::SIGNATURE_SPARE, "전자 서명 예비 저장"),
            (attr::CERT_DRM, "공인 인증서 DRM 보안"),
            (attr::CCL, "CCL 문서"),
            (attr::MOBILE_OPTIMIZED, "모바일 최적화"),
            (attr::PRIVACY_SECURITY, "개인 정보 보안 문서"),
            (attr::TRACK_CHANGES, "변경 추적 문서"),
            (attr::KOGL, "공공누리(KOGL) 저작권 문서"),
            (attr::HAS_VIDEO_CONTROL, "비디오 컨트롤 포함"),
            (attr::HAS_TOC_FIELD, "차례 필드 컨트롤 포함"),
        ];
        TABLE
            .iter()
            .filter(|(bit, _)| self.attributes & bit != 0)
            .map(|(_, name)| *name)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn 표본_헤더() -> Vec<u8> {
        let mut data = vec![0u8; FILE_HEADER_SIZE];
        data[..SIGNATURE.len()].copy_from_slice(SIGNATURE);
        data[32..36].copy_from_slice(&0x05000300u32.to_le_bytes()); // 5.0.3.0
        data[36..40].copy_from_slice(&0b0000_0001u32.to_le_bytes()); // 압축
        data
    }

    #[test]
    fn 파싱과_직렬화_왕복() {
        let data = 표본_헤더();
        let h = FileHeader::parse(&data).unwrap();
        assert_eq!(h.version.to_string(), "5.0.3.0");
        assert!(h.is_compressed());
        assert!(!h.is_distribution());
        assert_eq!(h.serialize(), data);
    }

    #[test]
    fn 시그니처_불일치는_err() {
        let mut data = 표본_헤더();
        data[0] = b'X';
        assert!(matches!(
            FileHeader::parse(&data),
            Err(Hwp5Error::BadSignature)
        ));
    }
}

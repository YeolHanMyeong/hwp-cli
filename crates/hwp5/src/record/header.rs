//! 4바이트 레코드 헤더 코덱.
//!
//! ```text
//! u32 LE = tag(10비트) | level(10비트) << 10 | size(12비트) << 20
//! ```
//!
//! size 필드가 0xFFF이면 후속 u32가 실제 크기다. 따라서 0xFFF 이상의
//! 크기는 인라인으로 표현할 수 없고 반드시 확장형으로 기록한다.

use crate::codec::{ByteReader, ByteWriter};
use crate::error::Result;

/// 확장 크기 표식: size 비트필드의 최댓값.
pub const SIZE_EXTENDED: u32 = 0xFFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordHeader {
    pub tag: u16,
    pub level: u16,
    pub size: u32,
}

impl RecordHeader {
    pub fn decode(r: &mut ByteReader<'_>) -> Result<Self> {
        let v = r.read_u32()?;
        let tag = (v & 0x3FF) as u16;
        let level = ((v >> 10) & 0x3FF) as u16;
        let size_field = (v >> 20) & 0xFFF;
        let size = if size_field == SIZE_EXTENDED {
            r.read_u32()?
        } else {
            size_field
        };
        Ok(Self { tag, level, size })
    }

    pub fn encode(&self, w: &mut ByteWriter) {
        debug_assert!(self.tag <= 0x3FF, "tag는 10비트");
        debug_assert!(self.level <= 0x3FF, "level은 10비트");
        let size_field = if self.size >= SIZE_EXTENDED {
            SIZE_EXTENDED
        } else {
            self.size
        };
        let v = u32::from(self.tag) | (u32::from(self.level) << 10) | (size_field << 20);
        w.write_u32(v);
        if size_field == SIZE_EXTENDED {
            w.write_u32(self.size);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn roundtrip(h: RecordHeader) -> RecordHeader {
        let mut w = ByteWriter::new();
        h.encode(&mut w);
        let bytes = w.into_bytes();
        let mut r = ByteReader::new(&bytes);
        let out = RecordHeader::decode(&mut r).unwrap();
        assert!(r.is_empty(), "디코드가 모든 바이트를 소비해야 한다");
        out
    }

    #[test]
    fn 인라인_크기() {
        let h = RecordHeader {
            tag: 0x42,
            level: 1,
            size: 100,
        };
        assert_eq!(roundtrip(h), h);
    }

    #[test]
    fn 확장_크기_경계() {
        // 0xFFF 자체도 인라인 표현 불가 → 확장형
        for size in [0xFFE, 0xFFF, 0x1000, u32::MAX] {
            let h = RecordHeader {
                tag: 0x10,
                level: 0,
                size,
            };
            assert_eq!(roundtrip(h), h);
        }
        // 0xFFE는 인라인(4바이트), 0xFFF부터 확장(8바이트)
        let mut w = ByteWriter::new();
        RecordHeader {
            tag: 0,
            level: 0,
            size: 0xFFE,
        }
        .encode(&mut w);
        assert_eq!(w.len(), 4);
        let mut w = ByteWriter::new();
        RecordHeader {
            tag: 0,
            level: 0,
            size: 0xFFF,
        }
        .encode(&mut w);
        assert_eq!(w.len(), 8);
    }

    proptest! {
        #[test]
        fn 헤더_왕복(tag in 0u16..=0x3FF, level in 0u16..=0x3FF, size in 0u32..=u32::MAX) {
            let h = RecordHeader { tag, level, size };
            prop_assert_eq!(roundtrip(h), h);
        }
    }
}

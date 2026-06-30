//! `\x05HwpSummaryInformation` 파서 (MS-OLEPS 속성 집합).
//!
//! `write.rs::hwp_summary_information`이 쓰는 구조의 역연산. 제목/주제/지은이/키워드
//! (PIDSI 0x02/0x03/0x04/0x05, VT_LPWSTR)만 추출한다. 최선 노력(best-effort):
//! 어떤 단계에서든 형식이 어긋나면 그때까지 읽은 값으로 [`Metadata`]를 돌려준다
//! (손상 파일도 `info`가 진단을 계속할 수 있도록).

use hwp_model::Metadata;

const VT_LPWSTR: u32 = 31;

const PID_TITLE: u32 = 0x02;
const PID_SUBJECT: u32 = 0x03;
const PID_AUTHOR: u32 = 0x04;
const PID_KEYWORDS: u32 = 0x05;

fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    b.get(off..off + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// VT_LPWSTR 값을 읽는다. `off`는 값의 시작(타입 코드 위치).
fn read_lpwstr(b: &[u8], off: usize) -> Option<String> {
    if u32_at(b, off)? != VT_LPWSTR {
        return None;
    }
    let count = u32_at(b, off + 4)? as usize; // 코드 유닛 수(널 종단자 포함)
    let chars_start = off + 8;
    let mut units = Vec::with_capacity(count.saturating_sub(1));
    for i in 0..count {
        let u = u16_at(b, chars_start + i * 2)?;
        if u == 0 {
            break; // 널 종단자
        }
        units.push(u);
    }
    let s = String::from_utf16_lossy(&units);
    if s.is_empty() { None } else { Some(s) }
}

/// 요약 정보 스트림 바이트에서 메타데이터를 파싱한다(최선 노력).
pub fn parse_summary(data: &[u8]) -> Metadata {
    let mut meta = Metadata::default();
    // 헤더: byteorder(2) format(2) osver(4) clsid(16) sectioncount(4) = 28,
    // 이어서 첫 섹션의 FMTID(16) + 섹션 오프셋(4).
    let Some(0xFFFE) = u16_at(data, 0) else {
        return meta;
    };
    let Some(section_count) = u32_at(data, 24) else {
        return meta;
    };
    if section_count == 0 {
        return meta;
    }
    // 첫 섹션 오프셋(FMTID 16바이트 건너뜀, 위치 28).
    let Some(sec_off) = u32_at(data, 28 + 16).map(|v| v as usize) else {
        return meta;
    };
    // 섹션: section_size(4) prop_count(4) 이어서 [pid(4) offset(4)] 표.
    let Some(prop_count) = u32_at(data, sec_off + 4).map(|v| v as usize) else {
        return meta;
    };
    let table = sec_off + 8;
    for i in 0..prop_count {
        let entry = table + i * 8;
        let (Some(pid), Some(val_off)) = (u32_at(data, entry), u32_at(data, entry + 4)) else {
            break;
        };
        // 오프셋은 섹션 시작 기준.
        let abs = sec_off + val_off as usize;
        match pid {
            PID_TITLE => meta.title = read_lpwstr(data, abs),
            PID_SUBJECT => meta.subject = read_lpwstr(data, abs),
            PID_AUTHOR => meta.author = read_lpwstr(data, abs),
            PID_KEYWORDS => meta.keywords = read_lpwstr(data, abs),
            _ => {}
        }
    }
    meta
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write;

    #[test]
    fn round_trips_metadata() {
        let meta = Metadata {
            title: Some("제목 테스트".into()),
            author: Some("홍길동".into()),
            subject: Some("Subject A".into()),
            keywords: Some("ai, hwp".into()),
        };
        let bytes = write::hwp_summary_information(&meta);
        let parsed = parse_summary(&bytes);
        assert_eq!(parsed, meta);
    }

    #[test]
    fn empty_metadata_parses_to_default() {
        let bytes = write::hwp_summary_information(&Metadata::default());
        assert_eq!(parse_summary(&bytes), Metadata::default());
    }

    #[test]
    fn garbage_is_tolerated() {
        assert_eq!(parse_summary(&[0u8; 4]), Metadata::default());
        assert_eq!(parse_summary(&[]), Metadata::default());
    }
}

//! `\x05HwpSummaryInformation` 파서 (MS-OLEPS 속성 집합).
//!
//! `write.rs::hwp_summary_information`이 쓰는 구조의 역연산. 제목/주제/지은이/키워드
//! (PIDSI 0x02/0x03/0x04/0x05, VT_LPWSTR)에 더해 설명(0x06)·마지막 저장자(0x08,
//! VT_LPWSTR)와 작성/수정 일시(0x0C/0x0D, VT_FILETIME raw u64)를 추출한다.
//! 최선 노력(best-effort): 어떤 단계에서든 형식이 어긋나면 그때까지 읽은 값으로
//! [`Metadata`]를 돌려준다 (손상 파일도 `info`가 진단을 계속할 수 있도록).

use hwp_model::Metadata;

const VT_LPWSTR: u32 = 31;
const VT_FILETIME: u32 = 64;

const PID_TITLE: u32 = 0x02;
const PID_SUBJECT: u32 = 0x03;
const PID_AUTHOR: u32 = 0x04;
const PID_KEYWORDS: u32 = 0x05;
const PID_COMMENTS: u32 = 0x06; // 설명
const PID_LASTAUTHOR: u32 = 0x08; // 마지막 저장자
const PID_CREATE_DTM: u32 = 0x0C; // 작성 일시
const PID_LASTSAVE_DTM: u32 = 0x0D; // 수정(마지막 저장) 일시
// PID 0x14 = 한국어 KST 날짜 문자열(VT_LPWSTR). 정품 표본 전수 관찰상 이 값은
// 작성일시(0x0C)의 KST 표현이라 **파생 가능**하므로 IR에 별도 필드를 두지 않는다.
// 읽기는 무시하고(create_time만 보존) 쓰기 때 write.rs가 재생성한다 — 왕복 보존은
// create_time 라운드트립으로 확보된다(과제2 요구 3의 '읽기 무시·쓰기 재생성' 선택).

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
    // 손상/악의 count(최대 ~8.5GB 예약) 방어: 남은 바이트 기준 상한으로 capacity 클램프.
    let max_units = b.len().saturating_sub(chars_start) / 2;
    let mut units = Vec::with_capacity(count.saturating_sub(1).min(max_units));
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

/// VT_FILETIME 값을 raw u64로 읽는다. `off`는 값의 시작(타입 코드 위치).
/// FILETIME은 dwLowDateTime(4) + dwHighDateTime(4) 순서(리틀엔디언). 0(미설정)은 None.
fn read_filetime(b: &[u8], off: usize) -> Option<u64> {
    if u32_at(b, off)? != VT_FILETIME {
        return None;
    }
    let lo = u32_at(b, off + 4)? as u64;
    let hi = u32_at(b, off + 8)? as u64;
    let ft = (hi << 32) | lo;
    if ft == 0 { None } else { Some(ft) }
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
            PID_COMMENTS => meta.description = read_lpwstr(data, abs),
            PID_LASTAUTHOR => meta.last_saved_by = read_lpwstr(data, abs),
            PID_CREATE_DTM => meta.create_time = read_filetime(data, abs),
            PID_LASTSAVE_DTM => meta.modify_time = read_filetime(data, abs),
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
            ..Default::default()
        };
        let bytes = write::hwp_summary_information(&meta);
        let parsed = parse_summary(&bytes);
        assert_eq!(parsed, meta);
    }

    /// 확장 필드(설명·마지막 저장자·작성/수정 일시 raw u64) 왕복 보존.
    #[test]
    fn 요약정보_확장필드_왕복() {
        let meta = Metadata {
            title: Some("보고서".into()),
            author: Some("작성자".into()),
            subject: Some("주제".into()),
            keywords: Some("키워드".into()),
            description: Some("문서 설명입니다".into()),
            last_saved_by: Some("최종 저장자".into()),
            // FILETIME raw u64 (예: 2025-09-17 04:32:50Z 부근 실측값 근사).
            create_time: Some(133_713_837_705_000_000),
            modify_time: Some(133_713_839_930_000_000),
        };
        let bytes = write::hwp_summary_information(&meta);
        let parsed = parse_summary(&bytes);
        assert_eq!(parsed, meta);
    }

    /// PID 0x14(날짜 문자열)이 create_time의 KST 표현으로 방출되는지(정품 정합).
    /// 정품 표본: create=2004-04-19T06:24:11Z → "2004년 4월 19일 월요일 오후 3:24:11".
    #[test]
    fn 요약정보_pid14_날짜문자열_작성일시_kst() {
        // 2004-04-19 06:24:11 UTC = FILETIME 127268294518110000(초 정밀 근사).
        let ft = hwp_model::iso8601_utc_to_filetime("2004-04-19T06:24:11Z").unwrap();
        let meta = Metadata {
            create_time: Some(ft),
            // 수정일시가 달라도 0x14는 작성일시에서 나와야 한다.
            modify_time: Some(hwp_model::iso8601_utc_to_filetime("2025-07-17T07:13:43Z").unwrap()),
            ..Default::default()
        };
        let bytes = write::hwp_summary_information(&meta);
        // 스트림 안에 KST 날짜 문자열(UTF-16LE)이 포함돼야 한다.
        let needle: Vec<u8> = "2004년 4월 19일 월요일 오후 3:24:11"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert!(
            bytes.windows(needle.len()).any(|w| w == needle.as_slice()),
            "PID 0x14에 작성일시 KST 문자열이 없다"
        );
        // 파서는 0x14를 무시하므로 create_time/modify_time만 왕복 보존된다.
        let parsed = parse_summary(&bytes);
        assert_eq!(parsed.create_time, meta.create_time);
        assert_eq!(parsed.modify_time, meta.modify_time);
    }

    /// create_time이 없으면 0x14는 modify_time으로 대체된다.
    #[test]
    fn 요약정보_pid14_작성일시없으면_수정일시() {
        let ft = hwp_model::iso8601_utc_to_filetime("2025-09-17T04:32:50Z").unwrap();
        let meta = Metadata {
            modify_time: Some(ft),
            ..Default::default()
        };
        let bytes = write::hwp_summary_information(&meta);
        let needle: Vec<u8> = "2025년 9월 17일 수요일 오후 1:32:50"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert!(
            bytes.windows(needle.len()).any(|w| w == needle.as_slice()),
            "PID 0x14 대체(수정일시 KST) 문자열이 없다"
        );
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

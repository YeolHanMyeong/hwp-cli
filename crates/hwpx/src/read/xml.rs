//! quick-xml 보조 유틸.

use quick_xml::events::BytesStart;

/// 로컬 이름(네임스페이스 접두사 제거) 기준 속성 조회.
pub fn attr(e: &BytesStart<'_>, name: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        let key = a.key.local_name();
        if key.as_ref() == name.as_bytes() {
            Some(String::from_utf8_lossy(&a.value).into_owned())
        } else {
            None
        }
    })
}

pub fn attr_u32(e: &BytesStart<'_>, name: &str) -> Option<u32> {
    attr(e, name)?.parse().ok()
}

pub fn attr_i32(e: &BytesStart<'_>, name: &str) -> Option<i32> {
    attr(e, name)?.parse().ok()
}

/// 오프셋 등 부호 있는 32비트 속성. hwpx는 음수를 unsigned 2의보수 십진수로
/// 저장(예: -77 = "4294967219")하므로 i64로 파싱 후 i32로 재해석한다.
pub fn attr_offset_i32(e: &BytesStart<'_>, name: &str) -> Option<i32> {
    attr(e, name)?.parse::<i64>().ok().map(|v| v as i32)
}

pub fn attr_u16(e: &BytesStart<'_>, name: &str) -> Option<u16> {
    attr(e, name)?.parse().ok()
}

/// "#RRGGBB" → COLORREF(0x00BBGGRR). "none"/파싱 실패는 0xFFFF_FFFF.
pub fn parse_color(s: &str) -> u32 {
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() == 6
        && let Ok(rgb) = u32::from_str_radix(hex, 16)
    {
        let r = (rgb >> 16) & 0xFF;
        let g = (rgb >> 8) & 0xFF;
        let b = rgb & 0xFF;
        return (b << 16) | (g << 8) | r;
    }
    0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 색_변환() {
        assert_eq!(parse_color("#FF0000"), 0x0000_00FF); // 빨강 → BGR
        assert_eq!(parse_color("#000000"), 0);
        assert_eq!(parse_color("none"), 0xFFFF_FFFF);
    }
}

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

/// 원문에서 `<{prefix}:{local}…>` 요소 전문을 문서 순서로 추출한다(raw 에코 보존용).
/// 자체닫힘(`<hh:tabPr …/>`)은 그 태그만, 컨테이너는同名 중첩을 깊이 카운트로 처리한다.
/// 파서가 의미 해석하지 않는 요소(secPr, tabPr 등)의 무손실 왕복에 쓴다.
pub fn echo_elements(src: &str, prefix: &str, local: &str) -> Vec<String> {
    let open = format!("<{prefix}:{local}");
    let close = format!("</{prefix}:{local}>");
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(i) = src[pos..].find(&open) {
        let start = pos + i;
        // 요소명 경계: 바로 뒤가 공백/'>'/'/'가 아니면 긴 이름의 요소(예: tabProperties) —
        // 원하는 local(예: tabPr)이 아니므로 걸어넘는다.
        let after = &src[start + open.len()..];
        if !after.starts_with([' ', '\t', '\r', '\n', '>', '/']) {
            pos = start + open.len();
            continue;
        }
        let Some(gt) = src[start..].find('>').map(|j| start + j) else {
            break; // 잘린 태그 — 비정상, 중단
        };
        // 자체닫힘이면 그 태그만 취한다.
        if src[..gt].ends_with('/') {
            out.push(src[start..=gt].to_string());
            pos = gt + 1;
            continue;
        }
        // 컨테이너:同名 중첩을 깊이 카운트로 닫는 태그를 찾는다.
        let mut depth = 1usize;
        let mut cur = gt + 1;
        let mut end = None;
        loop {
            let no = src[cur..].find(&open).map(|j| cur + j);
            let nc = src[cur..].find(&close).map(|j| cur + j);
            match (no, nc) {
                (Some(o), Some(c)) if o < c => {
                    if let Some(ogt) = src[o..].find('>').map(|j| o + j) {
                        if !src[..ogt].ends_with('/') {
                            depth += 1;
                        }
                        cur = ogt + 1;
                    } else {
                        break;
                    }
                }
                (_, Some(c)) => {
                    depth -= 1;
                    cur = c + close.len();
                    if depth == 0 {
                        end = Some(cur);
                        break;
                    }
                }
                _ => break,
            }
        }
        let Some(end) = end else { break };
        out.push(src[start..end].to_string());
        pos = end;
    }
    out
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

    #[test]
    fn 에코_자체닫힘_요소() {
        let src = r#"<hh:tabProperties itemCnt="2"><hh:tabPr id="0" a="0"/><hh:tabPr id="1" a="1"/></hh:tabProperties>"#;
        let raws = echo_elements(src, "hh", "tabPr");
        assert_eq!(raws.len(), 2, "자체닫힘 tabPr 2개: {raws:?}");
        assert_eq!(raws[0], r#"<hh:tabPr id="0" a="0"/>"#);
        assert_eq!(raws[1], r#"<hh:tabPr id="1" a="1"/>"#);
    }

    #[test]
    fn 에코_컨테이너_중첩과_경계() {
        //同名 중첩 + 긴 이름 요소(tabProperties)는 걸어넘어야 한다.
        let src = "<hp:secPr id=\"\"><hp:a><hp:secPr2/></hp:a><hp:secPr><hp:b/></hp:secPr></hp:secPr><hp:p/>";
        let raws = echo_elements(src, "hp", "secPr");
        assert_eq!(raws.len(), 1, "바깥 secPr 1개: {raws:?}");
        assert!(raws[0].ends_with("</hp:secPr>"), "컨테이너 전문: {}", raws[0]);
        assert!(raws[0].contains("<hp:secPr><hp:b/></hp:secPr>"), "同名 중첩 포함");
        // tabPr는 tabProperties를 걸어넘는다(요소명 경계).
        let src2 = r#"<hh:tabProperties itemCnt="1"><hh:tabPr id="0"/></hh:tabProperties>"#;
        let raws2 = echo_elements(src2, "hh", "tabPr");
        assert_eq!(raws2, vec![r#"<hh:tabPr id="0"/>"#.to_string()]);
    }
}

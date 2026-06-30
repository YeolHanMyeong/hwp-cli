//! 탭 스톱 — TAB_DEF raw 바이트를 렌더 시점에 파싱한다(shape_draw가 도형 raw를
//! 파싱하듯). v1은 왼쪽 탭 위치만 사용하고, 명시 스톱이 없으면 기본 간격으로 둔다.

use hwp_model::{Document, Paragraph};

/// 문단의 명시 탭 스톱 위치(pt, 오름차순). 정의가 없으면 빈 벡터(기본 간격 사용).
pub fn tab_stops(doc: &Document, para: &Paragraph) -> Vec<f32> {
    let pid = doc
        .header
        .para_shapes
        .get(para.para_shape.0 as usize)
        .map_or(0, |p| p.tab_def_id);
    match doc.header.tab_defs.get(pid as usize) {
        Some(entry) => parse_tab_stops(&entry.data),
        None => Vec::new(),
    }
}

/// TAB_DEF raw: `u32 attr, i32 count, count×(i32 pos HWPUNIT, u8 type, u8 fill, u16 resv)`.
/// 왼쪽 탭(type 무관, v1) 위치만 pt로 모아 오름차순 정렬한다.
fn parse_tab_stops(raw: &[u8]) -> Vec<f32> {
    if raw.len() < 8 {
        return Vec::new();
    }
    let count = i32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]).max(0) as usize;
    let mut out = Vec::with_capacity(count.min(64));
    for i in 0..count {
        let base = 8 + i * 8;
        if base + 4 > raw.len() {
            break;
        }
        let pos = i32::from_le_bytes([raw[base], raw[base + 1], raw[base + 2], raw[base + 3]]);
        if pos > 0 {
            out.push(pos as f32 / 100.0);
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// 현재 위치 rel(줄 시작 기준 pt)에서 다음 탭 위치. 명시 스톱 우선, 없으면 기본 간격.
pub fn next_tab(tabs: &[f32], rel: f32, default_interval: f32) -> f32 {
    if let Some(&t) = tabs.iter().find(|&&t| t > rel + 0.01) {
        t
    } else {
        (rel / default_interval).floor() * default_interval + default_interval
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// attr(4) + count=2 + (5000pt? no, HWPUNIT) 두 탭(100pt, 200pt).
    #[test]
    fn 탭_파싱() {
        let mut raw = Vec::new();
        raw.extend_from_slice(&0u32.to_le_bytes()); // attr
        raw.extend_from_slice(&2i32.to_le_bytes()); // count
        for (pos, ty) in [(10000i32, 0u8), (20000, 2)] {
            raw.extend_from_slice(&pos.to_le_bytes());
            raw.push(ty);
            raw.push(0); // fill
            raw.extend_from_slice(&0u16.to_le_bytes()); // reserved
        }
        let stops = parse_tab_stops(&raw);
        assert_eq!(stops, vec![100.0, 200.0]); // HWPUNIT/100 = pt
    }

    #[test]
    fn 빈_정의는_빈_스톱() {
        assert!(parse_tab_stops(&[]).is_empty());
        assert!(parse_tab_stops(&0u32.to_le_bytes()).is_empty()); // 8바이트 미만
    }

    #[test]
    fn next_tab_명시_우선_기본_폴백() {
        let tabs = [100.0, 200.0];
        assert_eq!(next_tab(&tabs, 50.0, 40.0), 100.0); // 다음 명시 스톱
        assert_eq!(next_tab(&tabs, 150.0, 40.0), 200.0);
        // 마지막 스톱 너머 → 기본 간격.
        assert_eq!(next_tab(&tabs, 250.0, 40.0), 280.0); // floor(250/40)*40+40
        // 명시 없음 → 전부 기본 간격.
        assert_eq!(next_tab(&[], 10.0, 40.0), 40.0);
    }
}

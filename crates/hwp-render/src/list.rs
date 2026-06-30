//! 목록(번호매기기/글머리표) 마커 — ParaShape 머리 종류/수준과
//! `numbering_levels`/`bullet_chars`로 문단 머리에 그릴 마커 문자열을 만든다.

use hwp_model::{Document, NumFmt, Paragraph};

/// 구역 단위 목록 카운터(수준 1~7 사용).
#[derive(Default)]
pub struct ListState {
    counters: [u32; 8],
}

impl ListState {
    /// 이 문단의 머리 마커 문자열(불릿 문자 또는 "1.", "1.1."). 목록이 아니면 None.
    pub fn marker(&mut self, doc: &Document, para: &Paragraph) -> Option<String> {
        let ps = doc.header.para_shapes.get(para.para_shape.0 as usize)?;
        let ty = ps.head_type();
        // 번호(2)·불릿(3)만 마커를 그린다. 개요(1)는 머리 번호 없이 구조 수준으로만
        // 쓰이는 경우가 많아(스타일 제목 등) 가짜 번호가 붙지 않도록 제외(v1).
        if ty != 2 && ty != 3 {
            return None;
        }
        let id = ps.numbering_id as usize;
        if ty == 3 {
            return Some(bullet_char(doc, id).to_string()); // 불릿
        }
        // 번호: 수준 카운터 증가 + 더 깊은 수준 리셋.
        let level = ps.head_level() as usize; // 1..=7
        self.counters[level] += 1;
        for c in &mut self.counters[level + 1..] {
            *c = 0;
        }
        let levels = numbering_levels(doc, id);
        let parts: Vec<String> = (1..=level)
            .map(|lv| {
                let fmt = levels
                    .and_then(|l| l.get(lv - 1))
                    .map_or(NumFmt::Digit, |nl| nl.fmt);
                let start = levels.and_then(|l| l.get(lv - 1)).map_or(1, |nl| nl.start);
                let n = self.counters[lv].max(1) + start.saturating_sub(1);
                format_number(n, fmt)
            })
            .collect();
        Some(format!("{}.", parts.join(".")))
    }
}

fn bullet_char(doc: &Document, id: usize) -> char {
    doc.header
        .bullet_chars
        .get(id)
        .or_else(|| {
            id.checked_sub(1)
                .and_then(|i| doc.header.bullet_chars.get(i))
        })
        .copied()
        .unwrap_or('•')
}

fn numbering_levels(doc: &Document, id: usize) -> Option<&[hwp_model::NumLevel]> {
    doc.header
        .numbering_levels
        .get(id)
        .or_else(|| {
            id.checked_sub(1)
                .and_then(|i| doc.header.numbering_levels.get(i))
        })
        .map(Vec::as_slice)
}

/// 번호 n(1부터)을 형식에 맞게 표기.
pub fn format_number(n: u32, fmt: NumFmt) -> String {
    match fmt {
        NumFmt::Digit => n.to_string(),
        NumFmt::HangulSyllable => cycle("가나다라마바사아자차카타파하", n),
        NumFmt::HangulJamo => cycle("ㄱㄴㄷㄹㅁㅂㅅㅇㅈㅊㅋㅌㅍㅎ", n),
        NumFmt::CircledDigit => {
            if (1..=20).contains(&n) {
                char::from_u32(0x245F + n).map_or_else(|| n.to_string(), |c| c.to_string())
            } else {
                n.to_string()
            }
        }
        NumFmt::LatinUpper => latin(n, b'A'),
        NumFmt::LatinLower => latin(n, b'a'),
        NumFmt::RomanUpper => roman(n).to_uppercase(),
        NumFmt::RomanLower => roman(n),
    }
}

/// 문자열에서 (n-1)%len 위치 글자(반복). 큰 n은 순환.
fn cycle(set: &str, n: u32) -> String {
    let chars: Vec<char> = set.chars().collect();
    let i = (n.max(1) - 1) as usize % chars.len();
    chars[i].to_string()
}

/// A, B, … Z, AA, AB … (1부터).
fn latin(n: u32, base: u8) -> String {
    let mut n = n.max(1);
    let mut out = String::new();
    while n > 0 {
        let rem = ((n - 1) % 26) as u8;
        out.insert(0, (base + rem) as char);
        n = (n - 1) / 26;
    }
    out
}

/// 로마 숫자(소문자). 1~3999, 범위 밖은 십진.
fn roman(n: u32) -> String {
    if !(1..=3999).contains(&n) {
        return n.to_string();
    }
    const VALS: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut n = n;
    let mut out = String::new();
    for (v, s) in VALS {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 마커_카운터() {
        use hwp_model::{NumLevel, ParaShape, ParaShapeId, Paragraph};
        let mut doc = Document::default();
        let mk = |ty: u32, lv: u32| ParaShape {
            attr1: (ty << 23) | (lv << 25),
            numbering_id: 0,
            ..ParaShape::default()
        };
        doc.header.para_shapes = vec![mk(2, 1), mk(2, 2), mk(3, 1)];
        doc.header.numbering_levels = vec![vec![NumLevel::default(); 7]];
        doc.header.bullet_chars = vec!['•'];
        let mut st = ListState::default();
        let p = |id| Paragraph {
            para_shape: ParaShapeId(id),
            ..Paragraph::default()
        };
        assert_eq!(st.marker(&doc, &p(0)).as_deref(), Some("1."));
        assert_eq!(st.marker(&doc, &p(0)).as_deref(), Some("2."));
        assert_eq!(st.marker(&doc, &p(1)).as_deref(), Some("2.1.")); // 수준2
        assert_eq!(st.marker(&doc, &p(2)).as_deref(), Some("•")); // 불릿
        // 비목록 문단은 None.
        doc.header.para_shapes.push(mk(0, 0));
        assert_eq!(st.marker(&doc, &p(3)), None);
    }

    #[test]
    fn 번호_형식() {
        assert_eq!(format_number(3, NumFmt::Digit), "3");
        assert_eq!(format_number(1, NumFmt::HangulSyllable), "가");
        assert_eq!(format_number(3, NumFmt::HangulSyllable), "다");
        assert_eq!(format_number(1, NumFmt::CircledDigit), "①");
        assert_eq!(format_number(1, NumFmt::LatinUpper), "A");
        assert_eq!(format_number(27, NumFmt::LatinUpper), "AA");
        assert_eq!(format_number(4, NumFmt::RomanUpper), "IV");
        assert_eq!(format_number(9, NumFmt::RomanLower), "ix");
    }
}

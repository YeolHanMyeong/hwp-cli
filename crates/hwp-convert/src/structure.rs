//! 구조 편집 — 문단 삽입/삭제.
//!
//! 새 문단/셀은 `set_cell`과 동일한 최소 IR(문단끝 0x0d는 writer가 idempotent하게
//! 보장, line_segs 비움)로 만들고, 앵커/템플릿의 글자·문단 모양을 상속한다.
//! 구조 편집본은 **합성 경로**(convert/new와 동일, 한글 수용 검증됨)로 써야 삽입
//! 문단/행에 모든 불변식(0x0d·마지막문단 비트·카운트)이 적용된다.
//!
//! 표 행/열 연산은 `crate::edit`(재귀 표 로케이터 계열 — set-cell과 인덱스 일치)으로
//! 단일화됐다. 여기서는 문단 수준 편집만 둔다.

use hwp_model::{
    CharShapeId, Control, Document, HwpChar, ParaShapeId, Paragraph, StyleId, ctrl_char,
};

use crate::edit::find_match;

/// 텍스트로 최소 문단을 만든다(글자/문단 모양 상속). 빈 텍스트면 빈 문단.
fn make_paragraph(
    text: &str,
    para_shape: ParaShapeId,
    style: StyleId,
    char_shape: CharShapeId,
) -> Paragraph {
    let mut chars: Vec<HwpChar> = text
        .chars()
        .map(|c| {
            if c == '\n' {
                HwpChar::CharCtrl(ctrl_char::LINE_BREAK)
            } else {
                HwpChar::Text(c)
            }
        })
        .collect();
    if !chars.is_empty() {
        chars.push(HwpChar::CharCtrl(ctrl_char::PARA_BREAK));
    }
    Paragraph {
        para_shape,
        style,
        chars,
        char_shape_runs: vec![(0, char_shape)],
        line_segs: Vec::new(),
        ..Paragraph::default()
    }
}

/// 문단의 (para_shape, style, 첫 char_shape) 템플릿.
fn para_template(p: &Paragraph) -> (ParaShapeId, StyleId, CharShapeId) {
    (
        p.para_shape,
        p.style,
        p.char_shape_runs.first().map_or(CharShapeId(0), |r| r.1),
    )
}

/// `anchor`를 가진 첫 본문 문단 뒤(또는 앞)에 `text` 문단을 삽입한다. 반환=삽입 여부.
/// 새 문단은 앵커 문단의 글자/문단 모양을 상속한다.
pub fn insert_paragraph(doc: &mut Document, anchor: &str, text: &str, before: bool) -> bool {
    for section in &mut doc.sections {
        if let Some(i) = section
            .paragraphs
            .iter()
            .position(|p| find_match(&p.chars, anchor, 0).is_some())
        {
            let (ps, sty, cs) = para_template(&section.paragraphs[i]);
            let new = make_paragraph(text, ps, sty, cs);
            let at = if before { i } else { i + 1 };
            section.paragraphs.insert(at, new);
            return true;
        }
    }
    false
}

/// `matching`을 가진 본문 문단을 삭제한다(섹션에 최소 1문단·구역정의 문단은 보존).
/// 반환=삭제 개수.
pub fn delete_paragraph(doc: &mut Document, matching: &str) -> usize {
    let mut count = 0;
    for section in &mut doc.sections {
        let mut i = 0;
        while i < section.paragraphs.len() {
            let p = &section.paragraphs[i];
            let is_secd = p
                .controls
                .iter()
                .any(|c| matches!(c, Control::SectionDef(_)));
            if !is_secd
                && section.paragraphs.len() > 1
                && find_match(&p.chars, matching, 0).is_some()
            {
                section.paragraphs.remove(i);
                count += 1;
            } else {
                i += 1;
            }
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::from_markdown;

    #[test]
    fn 문단_삽입_삭제() {
        let mut doc = from_markdown("첫째 문단\n\n둘째 문단\n\n셋째 문단");
        let n0: usize = doc.sections.iter().map(|s| s.paragraphs.len()).sum();
        // 둘째 뒤에 삽입.
        assert!(insert_paragraph(&mut doc, "둘째", "삽입된 문단", false));
        let n1: usize = doc.sections.iter().map(|s| s.paragraphs.len()).sum();
        assert_eq!(n1, n0 + 1);
        let txt = doc.plain_text();
        assert!(txt.contains("삽입된 문단"));
        // 삽입 위치: "둘째"와 "셋째" 사이.
        let i2 = txt.find("둘째").unwrap();
        let ii = txt.find("삽입된").unwrap();
        let i3 = txt.find("셋째").unwrap();
        assert!(i2 < ii && ii < i3, "둘째 뒤·셋째 앞: {txt:?}");
        // 삭제.
        let d = delete_paragraph(&mut doc, "삽입된 문단");
        assert_eq!(d, 1);
        assert!(!doc.plain_text().contains("삽입된 문단"));
    }

    #[test]
    fn 마지막_문단은_안지움() {
        let mut doc = from_markdown("유일 문단");
        // 본문 문단이 secd 1개뿐이면 보존(섹션 빔 방지).
        let before: usize = doc.sections.iter().map(|s| s.paragraphs.len()).sum();
        delete_paragraph(&mut doc, "유일");
        let after: usize = doc.sections.iter().map(|s| s.paragraphs.len()).sum();
        assert_eq!(before, after, "최소 1문단 유지");
    }
}

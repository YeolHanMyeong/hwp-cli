//! 구조 편집 — 문단 삽입/삭제, 표 행 추가/삭제.
//!
//! 새 문단/셀은 `set_cell`과 동일한 최소 IR(문단끝 0x0d는 writer가 idempotent하게
//! 보장, line_segs 비움)로 만들고, 앵커/템플릿의 글자·문단 모양을 상속한다.
//! 구조 편집본은 **합성 경로**(convert/new와 동일, 한글 수용 검증됨)로 써야 삽입
//! 문단/행에 모든 불변식(0x0d·마지막문단 비트·카운트)이 적용된다.

use hwp_model::{
    Cell, CharShapeId, Control, Document, HwpChar, ParaShapeId, Paragraph, StyleId, Table,
    ctrl_char,
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

/// 문서 등장 순서 N번째 최상위 표(셀 안 중첩표 제외, v1).
fn nth_table(doc: &mut Document, index: usize) -> Result<&mut Table, String> {
    let mut seen = 0;
    for section in &mut doc.sections {
        for para in &mut section.paragraphs {
            for ctrl in &mut para.controls {
                if let Control::Table(t) = ctrl {
                    if seen == index {
                        return Ok(t);
                    }
                    seen += 1;
                }
            }
        }
    }
    Err(format!("표 #{index}를 찾을 수 없습니다 (표 {seen}개)"))
}

/// N번째 표 끝에 행을 추가한다(마지막 행 셀 구조를 복제, 내용은 비움).
pub fn add_table_row(doc: &mut Document, table_index: usize) -> Result<(), String> {
    let table = nth_table(doc, table_index)?;
    if table.rows == 0 {
        return Err("빈 표에는 행을 추가할 수 없습니다".to_string());
    }
    let last = table.rows - 1;
    let row_cells: Vec<Cell> = table
        .cells
        .iter()
        .filter(|c| c.row == last)
        .cloned()
        .collect();
    if row_cells.is_empty() {
        return Err("마지막 행에 셀이 없습니다".to_string());
    }
    let cnt = row_cells.len() as u16;
    for mut c in row_cells {
        c.row = table.rows;
        let (ps, sty, cs) = c
            .paragraphs
            .first()
            .map_or((ParaShapeId(0), StyleId(0), CharShapeId(0)), para_template);
        c.paragraphs = vec![make_paragraph("", ps, sty, cs)];
        table.cells.push(c);
    }
    table.rows += 1;
    table.row_cell_counts.push(cnt);
    Ok(())
}

/// N번째 표의 R행을 삭제한다(이후 행 재번호, row_cell_counts 갱신).
pub fn delete_table_row(doc: &mut Document, table_index: usize, row: u16) -> Result<(), String> {
    let table = nth_table(doc, table_index)?;
    if row >= table.rows {
        return Err(format!("행 {row}이 없습니다 (행 {}개)", table.rows));
    }
    if table.rows <= 1 {
        return Err("마지막 행은 삭제할 수 없습니다".to_string());
    }
    table.cells.retain(|c| c.row != row);
    for c in &mut table.cells {
        if c.row > row {
            c.row -= 1;
        }
    }
    table.rows -= 1;
    if (row as usize) < table.row_cell_counts.len() {
        table.row_cell_counts.remove(row as usize);
    }
    Ok(())
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

    #[test]
    fn 표_행_추가_삭제() {
        let mut doc = from_markdown("| 가 | 나 |\n|----|----|\n| 1 | 2 |\n");
        let rows0 = nth_table(&mut doc, 0).unwrap().rows;
        let cells0 = nth_table(&mut doc, 0).unwrap().cells.len();
        add_table_row(&mut doc, 0).unwrap();
        {
            let t = nth_table(&mut doc, 0).unwrap();
            assert_eq!(t.rows, rows0 + 1);
            assert_eq!(t.cells.len(), cells0 + t.cols as usize);
            assert_eq!(t.row_cell_counts.len(), t.rows as usize);
            // 새 행 셀은 row==rows0.
            assert!(t.cells.iter().any(|c| c.row == rows0));
        }
        // 삭제(추가한 마지막 행).
        delete_table_row(&mut doc, 0, rows0).unwrap();
        let t = nth_table(&mut doc, 0).unwrap();
        assert_eq!(t.rows, rows0);
        assert!(t.cells.iter().all(|c| c.row < rows0));
        assert_eq!(t.row_cell_counts.len(), t.rows as usize);
    }
}

//! 인메모리 IR 편집 프리미티브.
//!
//! 원본 문서를 읽어 메모리에서 텍스트/표 셀을 바꾼 뒤 다시 쓴다 — 이미지·opaque
//! 레코드 등 모든 비편집 데이터가 그대로 보존된다(JSON 파일 왕복과 달리 무손실).
//!
//! 편집된 문단은 줄 배치(PARA_LINE_SEG)·nchars·문단끝 0x0d 캐시가 낡으므로,
//! 쓸 때 반드시 writer의 합성 경로(hwp5: `WriteOptions.edited=true`)를 거쳐야
//! 한글이 수용한다. 이 모듈은 IR만 바꾸고, 불변식 재수립은 writer가 담당한다.

use hwp_model::{CharShapeId, Control, Document, HwpChar, Paragraph};

/// 문서 전체에서 `from`을 `to`로 치환한다(본문·표 셀·글상자 문단 재귀).
/// `all`이 거짓이면 첫 1건만 바꾼다. 반환값은 치환 횟수.
///
/// 한 문단의 연속된 일반 문자(Text) 안에서만 매칭한다 — 컨트롤 문자(표 앵커·
/// 문단끝 등)가 끼면 그 경계에서 매칭이 끊긴다(서식·구조 보존).
pub fn replace_text(doc: &mut Document, from: &str, to: &str, all: bool) -> usize {
    if from.is_empty() {
        return 0;
    }
    let mut budget = if all { usize::MAX } else { 1 };
    let mut count = 0;
    for section in &mut doc.sections {
        for para in &mut section.paragraphs {
            count += replace_in_para(para, from, to, &mut budget);
            if budget == 0 {
                return count;
            }
        }
    }
    count
}

fn replace_in_para(para: &mut Paragraph, from: &str, to: &str, budget: &mut usize) -> usize {
    let mut n = replace_in_chars(para, from, to, budget);
    for ctrl in &mut para.controls {
        if *budget == 0 {
            break;
        }
        match ctrl {
            Control::Table(t) => {
                for cell in &mut t.cells {
                    for p in &mut cell.paragraphs {
                        if *budget == 0 {
                            break;
                        }
                        n += replace_in_para(p, from, to, budget);
                    }
                }
            }
            Control::Generic(g) => {
                for list in &mut g.paragraph_lists {
                    for p in &mut list.paragraphs {
                        if *budget == 0 {
                            break;
                        }
                        n += replace_in_para(p, from, to, budget);
                    }
                }
            }
            _ => {}
        }
    }
    n
}

/// 한 문단의 `chars` 안에서 치환을 반복한다(budget 한도). char_shape_run 위치를
/// 보정한다. 줄 배치는 비워 두고(낡음) writer가 재합성하게 한다.
fn replace_in_chars(para: &mut Paragraph, from: &str, to: &str, budget: &mut usize) -> usize {
    let from_w = utf16_len(from);
    let from_chars = from.chars().count();
    let to_chars = to.chars().count();
    let mut count = 0;
    // 삽입한 `to` 다음부터 이어서 탐색한다 — `to`가 `from`을 포함하면(예:
    // "한라대학교"→"제주한라대학교") 처음부터 재탐색 시 삽입한 텍스트 안에서
    // 다시 매칭돼 무한 루프에 빠진다.
    let mut start = 0usize;
    while *budget > 0 {
        let Some((char_idx, wpos)) = find_match(&para.chars, from, start) else {
            break;
        };
        let to_hwp: Vec<HwpChar> = to
            .chars()
            .map(|c| {
                if c == '\n' {
                    HwpChar::CharCtrl(hwp_model::ctrl_char::LINE_BREAK)
                } else {
                    HwpChar::Text(c)
                }
            })
            .collect();
        let to_w = utf16_len(to);
        para.chars.splice(char_idx..char_idx + from_chars, to_hwp);
        adjust_runs(&mut para.char_shape_runs, wpos, from_w, to_w);
        para.line_segs.clear();
        count += 1;
        *budget -= 1;
        start = char_idx + to_chars;
    }
    count
}

/// 연속된 Text 문자열에서 `start_idx` 이후 `from`의 첫 위치를 찾는다.
/// 반환: (chars 벡터 내 시작 인덱스, 문단 내 WCHAR 오프셋).
fn find_match(chars: &[HwpChar], from: &str, start_idx: usize) -> Option<(usize, u32)> {
    let mut wpos: u32 = chars[..start_idx.min(chars.len())]
        .iter()
        .map(HwpChar::wchar_width)
        .sum();
    let mut i = start_idx;
    while i < chars.len() {
        if matches!(chars[i], HwpChar::Text(_)) {
            let seg_start = i;
            let seg_wstart = wpos;
            let mut seg = String::new();
            let mut j = i;
            while let Some(HwpChar::Text(c)) = chars.get(j) {
                seg.push(*c);
                j += 1;
            }
            if let Some(byte_off) = seg.find(from) {
                let prefix = &seg[..byte_off];
                let char_off = prefix.chars().count();
                let wchar_off = utf16_len(prefix);
                return Some((seg_start + char_off, seg_wstart + wchar_off));
            }
            wpos += utf16_len(&seg);
            i = j;
        } else {
            wpos += chars[i].wchar_width();
            i += 1;
        }
    }
    None
}

/// 치환 위치 `p`(WCHAR), 옛 길이 `lo`, 새 길이 `ln`에 맞춰 char_shape_run 경계를
/// 옮긴다. 치환 구간 내부 경계는 제거하고(치환 텍스트는 p에서 활성인 모양을 상속),
/// 이후 경계는 길이 변화만큼 평행 이동한다.
fn adjust_runs(runs: &mut Vec<(u32, CharShapeId)>, p: u32, lo: u32, ln: u32) {
    let delta = i64::from(ln) - i64::from(lo);
    let mut out: Vec<(u32, CharShapeId)> = Vec::with_capacity(runs.len());
    for &(pos, id) in runs.iter() {
        let np = if pos <= p {
            pos
        } else if pos >= p + lo {
            (i64::from(pos) + delta).max(0) as u32
        } else {
            continue; // 치환 구간 내부 경계 제거
        };
        match out.last() {
            Some(&(lp, _)) if lp == np => {}   // 같은 위치 중복 — 첫 것 유지
            Some(&(_, lid)) if lid == id => {} // 같은 모양 연속 — 잉여 경계 제거
            _ => out.push((np, id)),
        }
    }
    if out.is_empty() {
        out.push((0, CharShapeId::default()));
    }
    *runs = out;
}

/// `table_index`번째 표(문서 등장 순서, 0-기반)의 (row, col) 셀 텍스트를 바꾼다.
/// 셀의 첫 문단 서식을 템플릿으로 보존하고 내용만 교체한다.
pub fn set_cell(
    doc: &mut Document,
    table_index: usize,
    row: u16,
    col: u16,
    text: &str,
) -> Result<(), String> {
    let mut st = CellSet {
        index: table_index,
        seen: 0,
        row,
        col,
        text,
        result: None,
    };
    for section in &mut doc.sections {
        for para in &mut section.paragraphs {
            walk_set_cell(para, &mut st);
            if let Some(result) = st.result.take() {
                return result;
            }
        }
    }
    Err(format!(
        "표 #{table_index}를 찾을 수 없습니다 (문서의 표 개수: {})",
        st.seen
    ))
}

struct CellSet<'a> {
    index: usize,
    seen: usize,
    row: u16,
    col: u16,
    text: &'a str,
    result: Option<Result<(), String>>,
}

fn walk_set_cell(para: &mut Paragraph, st: &mut CellSet) {
    for ctrl in &mut para.controls {
        if st.result.is_some() {
            return;
        }
        match ctrl {
            Control::Table(t) => {
                if st.seen == st.index {
                    st.result = Some(set_cell_in_table(t, st.row, st.col, st.text));
                    st.seen += 1;
                    return;
                }
                st.seen += 1;
                for cell in &mut t.cells {
                    for p in &mut cell.paragraphs {
                        walk_set_cell(p, st);
                        if st.result.is_some() {
                            return;
                        }
                    }
                }
            }
            Control::Generic(g) => {
                for list in &mut g.paragraph_lists {
                    for p in &mut list.paragraphs {
                        walk_set_cell(p, st);
                        if st.result.is_some() {
                            return;
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn set_cell_in_table(
    table: &mut hwp_model::Table,
    row: u16,
    col: u16,
    text: &str,
) -> Result<(), String> {
    let cell = table
        .cells
        .iter_mut()
        .find(|c| c.row == row && c.col == col)
        .ok_or_else(|| format!("표에 셀 ({row}, {col})이 없습니다"))?;

    // 첫 문단을 서식 템플릿으로 — 문단/스타일/문자 모양/헤더 보존, 내용만 교체.
    let template = cell.paragraphs.first();
    let para_shape = template.map(|p| p.para_shape).unwrap_or_default();
    let style = template.map(|p| p.style).unwrap_or_default();
    let shape_id = template
        .and_then(|p| p.char_shape_runs.first().map(|r| r.1))
        .unwrap_or_default();
    let header = template.map(|p| p.header.clone()).unwrap_or_default();

    let mut chars: Vec<HwpChar> = text
        .chars()
        .map(|c| {
            if c == '\n' {
                HwpChar::CharCtrl(hwp_model::ctrl_char::LINE_BREAK)
            } else {
                HwpChar::Text(c)
            }
        })
        .collect();
    if !chars.is_empty() {
        chars.push(HwpChar::CharCtrl(hwp_model::ctrl_char::PARA_BREAK));
    }

    cell.paragraphs = vec![Paragraph {
        para_shape,
        style,
        chars,
        char_shape_runs: vec![(0, shape_id)],
        line_segs: Vec::new(),
        controls: Vec::new(),
        header,
        extras: Vec::new(),
    }];
    Ok(())
}

fn utf16_len(s: &str) -> u32 {
    s.encode_utf16().count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::from_markdown;
    use hwp_model::LineSeg;

    fn dummy_lineseg() -> LineSeg {
        LineSeg {
            text_start: 0,
            v_pos: 0,
            line_height: 1000,
            text_height: 1000,
            baseline_gap: 850,
            line_spacing: 600,
            col_start: 0,
            seg_width: 40000,
            flags: 0,
        }
    }

    #[test]
    fn 편집된_문단만_줄배치_무효화() {
        // 외과적 편집: 편집한 문단의 줄 배치만 비우고, 미편집 문단은 보존해야
        // (한글이 표 행 높이 등을 그대로 유지하도록).
        let mut doc = from_markdown("바꿀문단 있음\n\n그대로 둘 문단\n");
        for p in &mut doc.sections[0].paragraphs {
            p.line_segs.push(dummy_lineseg());
        }
        let n = replace_text(&mut doc, "바꿀문단", "변경됨", true);
        assert_eq!(n, 1);
        let paras = &doc.sections[0].paragraphs;
        let edited = paras
            .iter()
            .find(|p| p.plain_text().contains("변경됨"))
            .unwrap();
        let kept = paras
            .iter()
            .find(|p| p.plain_text().contains("그대로"))
            .unwrap();
        assert!(edited.line_segs.is_empty(), "편집 문단 줄 배치는 비워야 함");
        assert_eq!(kept.line_segs.len(), 1, "미편집 문단 줄 배치는 보존해야 함");
    }

    #[test]
    fn 본문_치환_길이변화_run보정() {
        let mut doc = from_markdown("부서명을 적으세요\n");
        let n = replace_text(&mut doc, "부서명", "기획팀입니다", true);
        assert_eq!(n, 1);
        let text = doc.plain_text();
        assert!(text.contains("기획팀입니다을 적으세요"), "got: {text:?}");
        // char_shape_run은 0에서 시작하고 단조 증가해야 한다.
        for section in &doc.sections {
            for p in &section.paragraphs {
                if let Some(first) = p.char_shape_runs.first() {
                    assert_eq!(first.0, 0, "첫 run은 0에서 시작");
                }
                let positions: Vec<u32> = p.char_shape_runs.iter().map(|r| r.0).collect();
                let mut sorted = positions.clone();
                sorted.sort_unstable();
                assert_eq!(positions, sorted, "run 위치 단조 증가");
            }
        }
    }

    #[test]
    fn 치환문이_찾기문_포함_무한루프_없음() {
        // "한라대학교" → "제주한라대학교": to가 from을 포함 → 재탐색 무한루프 방지.
        let mut doc = from_markdown("한라대학교 보고서\n");
        let n = replace_text(&mut doc, "한라대학교", "제주한라대학교", true);
        assert_eq!(n, 1);
        let text = doc.plain_text();
        assert!(text.contains("제주한라대학교 보고서"), "got: {text:?}");
        assert!(!text.contains("제주제주"), "중복 치환됨: {text:?}");
    }

    #[test]
    fn 치환_전체_vs_단일() {
        let mut doc = from_markdown("가 가 가\n");
        let single = replace_text(&mut doc.clone(), "가", "나", false);
        assert_eq!(single, 1);
        let all = replace_text(&mut doc, "가", "나", true);
        assert_eq!(all, 3);
        assert!(doc.plain_text().contains("나 나 나"));
    }

    #[test]
    fn 표_셀_설정() {
        let mut doc = from_markdown("| 가 | 나 |\n|----|----|\n| 1 | 2 |\n");
        set_cell(&mut doc, 0, 1, 0, "바뀐값").unwrap();
        let text = doc.plain_text();
        assert!(text.contains("바뀐값"), "got: {text:?}");
        // 셀이 1개 문단(내용+문단끝)만 갖는지.
        assert!(set_cell(&mut doc, 0, 99, 99, "x").is_err());
        assert!(set_cell(&mut doc, 5, 0, 0, "x").is_err());
    }
}

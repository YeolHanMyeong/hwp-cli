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
pub(crate) fn find_match(chars: &[HwpChar], from: &str, start_idx: usize) -> Option<(usize, u32)> {
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
pub(crate) fn adjust_runs(runs: &mut Vec<(u32, CharShapeId)>, p: u32, lo: u32, ln: u32) {
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
    with_nth_table(doc, table_index, |t| set_cell_in_table(t, row, col, text))
        .unwrap_or_else(|| Err(format!("표 #{table_index}를 찾을 수 없습니다")))
}

/// `table_index`번째 표(0-기반)에 빈 행을 `count`개 추가한다. `template_row`(0-기반,
/// 생략 시 마지막의 병합 없는 행)를 복제해 셀 서식(폭·여백·테두리·문자/문단 모양)을
/// 보존하고 내용은 비운다 — 추가된 행(인덱스 `기존행수`부터)은 이후 [`set_cell`]로
/// 채운다. hwp5 출력은 반드시 edited 합성 경로(`WriteOptions.edited=true`)를 거쳐야
/// 한글이 수용한다(줄 배치·문단끝·nchars 불변식 재합성).
pub fn add_rows(
    doc: &mut Document,
    table_index: usize,
    template_row: Option<u16>,
    count: usize,
) -> Result<(), String> {
    if count == 0 {
        return Ok(());
    }
    with_nth_table(doc, table_index, |t| {
        add_rows_in_table(t, template_row, count)
    })
    .unwrap_or_else(|| Err(format!("표 #{table_index}를 찾을 수 없습니다")))
}

/// `table_index`번째 표(0-기반)의 (행 수, 열 수)를 반환한다. 데이터 구동 채우기가
/// 추가할 행 수를 계산할 때 쓴다(현재 행 수 조회).
pub fn table_dims(doc: &mut Document, table_index: usize) -> Option<(u16, u16)> {
    with_nth_table(doc, table_index, |t| (t.rows, t.cols))
}

/// `table_index`번째 표(0-기반)의 `row`행을 삭제한다(이후 행 재번호, row_cell_counts
/// 갱신). 병합 셀이 있거나 세로 병합에 덮인 행은 그리드가 깨지므로 거부한다.
pub fn delete_table_row(doc: &mut Document, table_index: usize, row: u16) -> Result<(), String> {
    with_nth_table(doc, table_index, |t| delete_row_in_table(t, row))
        .unwrap_or_else(|| Err(format!("표 #{table_index}를 찾을 수 없습니다")))
}

fn delete_row_in_table(table: &mut hwp_model::Table, row: u16) -> Result<(), String> {
    if row >= table.rows {
        return Err(format!("행 {row}이 없습니다 (행 {}개)", table.rows));
    }
    if table.rows <= 1 {
        return Err("마지막 행은 삭제할 수 없습니다".to_string());
    }
    if !is_clean_row(table, row) {
        return Err(format!(
            "행 {row}에 병합 셀이 있거나 세로 병합에 덮여 있어 삭제를 지원하지 않습니다"
        ));
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

/// `table_index`번째 표(0-기반) 끝에 열을 하나 추가한다 — **전체 표 폭은 유지**한다.
/// 새 열은 균등 몫(`행총폭/(열수+1)`)을 갖고 기존 열은 비율로 축소된다(행별 정수 잔차는
/// 그 행 마지막 기존 셀에 가산해 행 총폭이 정확히 보존). 병합 셀이 있는 표는 거부한다.
pub fn add_col(doc: &mut Document, table_index: usize) -> Result<(), String> {
    with_nth_table(doc, table_index, add_col_in_table)
        .unwrap_or_else(|| Err(format!("표 #{table_index}를 찾을 수 없습니다")))
}

fn add_col_in_table(table: &mut hwp_model::Table) -> Result<(), String> {
    let cols = table.cols;
    if cols == 0 || table.rows == 0 {
        return Err("빈 표에는 열을 추가할 수 없습니다".to_string());
    }
    if cols == u16::MAX {
        return Err("열 수가 u16 범위를 넘습니다".to_string());
    }
    // 가드: 완전 단순 그리드(전 셀 1×1 + 모든 행이 전 열을 채움)만 허용한다.
    let simple = table
        .cells
        .iter()
        .all(|c| c.col_span == 1 && c.row_span == 1)
        && (0..table.rows).all(|r| is_clean_row(table, r));
    if !simple {
        return Err("병합 셀이 있는 표에는 열 추가를 지원하지 않습니다".to_string());
    }
    // 복제 문단 instance_id 충돌 방지(add_rows와 같은 규칙 — 표 내 최댓값 위로).
    let mut next_inst = table
        .cells
        .iter()
        .flat_map(|c| &c.paragraphs)
        .map(|p| p.header.instance_id)
        .max()
        .unwrap_or(0);
    // 행별로 폭 재분배 + 새 셀 추가. 행 우선 순서 유지를 위해 cells를 재구성한다.
    let mut new_cells = Vec::with_capacity(table.cells.len() + table.rows as usize);
    for r in 0..table.rows {
        let mut row_cells: Vec<hwp_model::Cell> =
            table.cells.iter().filter(|c| c.row == r).cloned().collect();
        row_cells.sort_by_key(|c| c.col);
        let row_total: i64 = row_cells.iter().map(|c| i64::from(c.width.0)).sum();
        if row_total <= 0 {
            return Err(format!(
                "행 {r}의 총폭이 0이라 열 폭을 재분배할 수 없습니다"
            ));
        }
        let new_w = (row_total / (i64::from(cols) + 1)).max(1);
        let scaled_target = row_total - new_w;
        // 기존 셀은 비율 축소, 정수 잔차는 마지막 기존 셀에 가산(행 총폭 정확 보존).
        let last_idx = row_cells.len() - 1;
        let mut acc: i64 = 0;
        for (i, c) in row_cells.iter_mut().enumerate() {
            let w = i64::from(c.width.0);
            let nw = if i == last_idx {
                scaled_target - acc
            } else {
                w * scaled_target / row_total
            };
            c.width = hwp_model::HwpUnit(nw as i32);
            acc += nw;
        }
        // 새 셀: 행 마지막 셀을 복제(높이·여백·테두리 상속), 내용은 빈 문단.
        let mut nc = row_cells[last_idx].clone();
        nc.col = cols;
        nc.width = hwp_model::HwpUnit(new_w as i32);
        let mut para = blank_para_like(
            table
                .cells
                .iter()
                .filter(|c| c.row == r)
                .max_by_key(|c| c.col)
                .and_then(|c| c.paragraphs.first()),
        );
        next_inst = next_inst.wrapping_add(1);
        para.header.instance_id = next_inst;
        nc.paragraphs = vec![para];
        new_cells.extend(row_cells);
        new_cells.push(nc);
    }
    table.cells = new_cells;
    table.cols += 1;
    for cnt in &mut table.row_cell_counts {
        *cnt += 1;
    }
    Ok(())
}

/// `"키=값"` 메타데이터 지정을 문서에 적용한다. 키: `title`|`author`|`subject`|`keywords`.
/// 값이 비면 해당 필드를 `None`으로 지운다. 알 수 없는 키/형식은 `Err`.
pub fn apply_meta(doc: &mut Document, spec: &str) -> Result<(), String> {
    let (key, value) = spec
        .split_once('=')
        .ok_or_else(|| format!("메타데이터 형식은 \"키=값\" 입니다: {spec:?}"))?;
    let val = (!value.is_empty()).then(|| value.to_string());
    match key.trim() {
        "title" => doc.metadata.title = val,
        "author" => doc.metadata.author = val,
        "subject" => doc.metadata.subject = val,
        "keywords" => doc.metadata.keywords = val,
        other => {
            return Err(format!(
                "메타데이터 키는 title|author|subject|keywords 입니다: {other:?}"
            ));
        }
    }
    Ok(())
}

/// 문서 등장 순서 `index`번째 표를 찾아 `f`를 적용한다(0-기반). 본문·표 셀·글상자
/// 문단을 재귀로 훑는다. 표를 찾으면 `Some(f의 결과)`, 못 찾으면 `None`.
fn with_nth_table<R, F: FnOnce(&mut hwp_model::Table) -> R>(
    doc: &mut Document,
    index: usize,
    f: F,
) -> Option<R> {
    let mut seen = 0;
    let mut f = Some(f);
    let mut out = None;
    for section in &mut doc.sections {
        for para in &mut section.paragraphs {
            walk_nth_table(para, index, &mut seen, &mut f, &mut out);
            if out.is_some() {
                return out;
            }
        }
    }
    out
}

fn walk_nth_table<R, F: FnOnce(&mut hwp_model::Table) -> R>(
    para: &mut Paragraph,
    index: usize,
    seen: &mut usize,
    f: &mut Option<F>,
    out: &mut Option<R>,
) {
    for ctrl in &mut para.controls {
        if out.is_some() {
            return;
        }
        match ctrl {
            Control::Table(t) => {
                if *seen == index {
                    if let Some(func) = f.take() {
                        *out = Some(func(t));
                    }
                    *seen += 1;
                    return;
                }
                *seen += 1;
                for cell in &mut t.cells {
                    for p in &mut cell.paragraphs {
                        walk_nth_table(p, index, seen, f, out);
                        if out.is_some() {
                            return;
                        }
                    }
                }
            }
            Control::Generic(g) => {
                for list in &mut g.paragraph_lists {
                    for p in &mut list.paragraphs {
                        walk_nth_table(p, index, seen, f, out);
                        if out.is_some() {
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
    let mut para = blank_para_like(cell.paragraphs.first());
    para.chars = text
        .chars()
        .map(|c| {
            if c == '\n' {
                HwpChar::CharCtrl(hwp_model::ctrl_char::LINE_BREAK)
            } else {
                HwpChar::Text(c)
            }
        })
        .collect();
    if !para.chars.is_empty() {
        para.chars
            .push(HwpChar::CharCtrl(hwp_model::ctrl_char::PARA_BREAK));
    }
    cell.paragraphs = vec![para];
    Ok(())
}

/// 표 행 추가/셀 설정용 빈 문단 — 템플릿 문단의 문단/스타일/첫 글자모양/헤더를
/// 보존하고 내용은 비운다(줄 배치도 비워 writer가 재합성). 한글 합성 게이트는
/// 셀당 문단 ≥1·문자모양 run ≥1만 요구하므로 빈 chars로 충분하다(writer가
/// nchars=1·PARA_TEXT 생략을 처리).
///
/// 이 문단은 항상 셀의 **유일·마지막** 문단이 되므로(set_cell·add_rows 모두
/// `cell.paragraphs = vec![이 문단]`), nchars bit31(리스트 마지막 문단 표식)을
/// 강제한다. hwp5 출신 편집 경로는 writer가 set_last_para_flag를 돌리지 않으므로
/// (synthesize=false) 여기서 세우지 않으면 다중 문단 셀을 복제할 때 비트가 빠진다.
fn blank_para_like(template: Option<&Paragraph>) -> Paragraph {
    let mut header = template.map(|p| p.header.clone()).unwrap_or_default();
    header.chars_flags |= 0x80;
    Paragraph {
        para_shape: template.map(|p| p.para_shape).unwrap_or_default(),
        style: template.map(|p| p.style).unwrap_or_default(),
        chars: Vec::new(),
        char_shape_runs: vec![(
            0,
            template
                .and_then(|p| p.char_shape_runs.first().map(|r| r.1))
                .unwrap_or_default(),
        )],
        line_segs: Vec::new(),
        controls: Vec::new(),
        header,
        extras: Vec::new(),
    }
}

fn add_rows_in_table(
    table: &mut hwp_model::Table,
    template_row: Option<u16>,
    count: usize,
) -> Result<(), String> {
    if table.rows == 0 {
        return Err("빈 표에는 행을 추가할 수 없습니다".to_string());
    }
    // 행 수는 u16 범위 — 남은 용량을 넘으면 거부(넘으면 count as u16 절단으로 cells/
    // row_cell_counts가 어긋나 표 레코드가 깨진다).
    let remaining = usize::from(u16::MAX) - usize::from(table.rows);
    if count > remaining {
        return Err(format!(
            "추가 행 수가 너무 많습니다: {count} (최대 {remaining}행 — 표 행 수는 u16 범위)"
        ));
    }
    // 템플릿 행 해소: 지정값(범위 검사) 또는 마지막의 '깨끗한'(병합 없는) 행.
    let tpl = match template_row {
        Some(r) if r < table.rows => r,
        Some(r) => {
            return Err(format!(
                "템플릿 행 {r}이 표 범위를 벗어남 (행 수: {})",
                table.rows
            ));
        }
        None => clean_template_row(table)
            .ok_or("복제할 병합 없는 행이 없습니다 — 템플릿 행을 지정하세요")?,
    };
    // 템플릿 행의 셀(열 순서) 수집. 병합 셀이 있거나 전 열을 채우지 않으면(세로 병합에
    // 덮인 부분 행) 거부 — 복제 시 그리드가 타일링되지 않아 누락 열이 생긴다.
    let tpl_cells: Vec<hwp_model::Cell> = table
        .cells
        .iter()
        .filter(|c| c.row == tpl)
        .cloned()
        .collect();
    if tpl_cells.is_empty() {
        return Err(format!("템플릿 행 {tpl}에 셀이 없습니다"));
    }
    // 템플릿 행은 전 열을 1×1로 채우는 깨끗한 행이어야 한다 — 병합 셀이 있거나
    // 세로 병합에 덮인 부분 행이면 복제 시 그리드가 타일링되지 않아 누락 열이 생긴다.
    if !is_clean_row(table, tpl) {
        return Err(format!(
            "템플릿 행 {tpl}에 병합 셀이 있거나 전체 열({})을 채우지 않아 복제 불가 — 병합 없는 행을 지정하세요",
            table.cols
        ));
    }
    // 복제 문단 instance_id 충돌 방지: hwp5 출신 편집 경로는 writer가 id를 재부여하지
    // 않으므로(synthesize=false), 표 내 최댓값 위로 고유 id를 부여한다(같은 템플릿
    // 문단을 N개 셀에 복제하면 비-0 id가 N+1개 중복돼 한글 개체 링크가 깨진다).
    let mut next_inst = table
        .cells
        .iter()
        .flat_map(|c| &c.paragraphs)
        .map(|p| p.header.instance_id)
        .max()
        .unwrap_or(0);
    // 새 행은 기존 최대 행 다음부터(행 우선 평탄 순서 유지 — append만, 중간 삽입 금지).
    let per_row = tpl_cells.len() as u16;
    let first_new = table.rows;
    for i in 0..count as u16 {
        for c in &tpl_cells {
            let mut nc = c.clone();
            nc.row = first_new + i;
            nc.col_span = 1;
            nc.row_span = 1;
            let mut para = blank_para_like(c.paragraphs.first());
            next_inst = next_inst.wrapping_add(1);
            para.header.instance_id = next_inst;
            nc.paragraphs = vec![para];
            table.cells.push(nc);
        }
    }
    table.rows += count as u16;
    for _ in 0..count {
        table.row_cell_counts.push(per_row);
    }
    Ok(())
}

/// 행 r이 전 열을 1×1 셀로 채우는 '깨끗한' 행인지 — 병합 셀이 없고(row/col_span==1)
/// 세로 병합에 덮이지도 않음(row_cell_counts==cols). 행 복제·삭제·열 추가 가드 공용.
fn is_clean_row(table: &hwp_model::Table, r: u16) -> bool {
    table.row_cell_counts.get(r as usize).copied() == Some(table.cols)
        && table
            .cells
            .iter()
            .filter(|c| c.row == r)
            .all(|c| c.col_span == 1 && c.row_span == 1)
}

/// 복제 기본 템플릿: 마지막의 '깨끗한' 행 — 전 열을 채우고(row_cell_counts==cols)
/// 병합 셀이 없는 행. 세로 병합에 덮인 행은 셀 수가 cols보다 적어 자동 제외된다.
fn clean_template_row(table: &hwp_model::Table) -> Option<u16> {
    (0..table.rows).rev().find(|&r| is_clean_row(table, r))
}

pub(crate) fn utf16_len(s: &str) -> u32 {
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

    fn first_table(doc: &Document) -> &hwp_model::Table {
        doc.sections[0]
            .paragraphs
            .iter()
            .flat_map(|p| &p.controls)
            .find_map(|c| match c {
                Control::Table(t) => Some(t),
                _ => None,
            })
            .expect("표 없음")
    }

    #[test]
    fn 행_추가_구조_불변식() {
        // 2행 2열 표 → 3행 추가 → rows=5, cells=10, row_cell_counts 길이=5·합=10.
        let mut doc = from_markdown("| 가 | 나 |\n|----|----|\n| 1 | 2 |\n");
        let before = first_table(&doc);
        let (r0, cells0, cols) = (before.rows, before.cells.len(), before.cols);
        add_rows(&mut doc, 0, None, 3).unwrap();
        let t = first_table(&doc);
        assert_eq!(t.rows, r0 + 3, "rows 증가");
        assert_eq!(t.cells.len(), cells0 + 3 * cols as usize, "셀 수 증가");
        assert_eq!(
            t.row_cell_counts.len(),
            t.rows as usize,
            "row_cell_counts 길이 == rows"
        );
        assert_eq!(
            t.row_cell_counts.iter().map(|c| *c as usize).sum::<usize>(),
            t.cells.len(),
            "row_cell_counts 합 == 셀 수 (hwp5 extract assert)"
        );
        // 새 행은 기존 최대 행 다음부터, 행 우선 평탄 순서 유지(append만).
        let rows_in_order: Vec<u16> = t.cells.iter().map(|c| c.row).collect();
        let mut sorted = rows_in_order.clone();
        sorted.sort_unstable();
        assert_eq!(rows_in_order, sorted, "cells 행 우선(단조 비감소) 순서");
        // 새 셀은 빈 문단 1개·문자모양 run 1개(한글 합성 게이트)·span 1.
        for c in t.cells.iter().filter(|c| c.row >= r0) {
            assert_eq!(c.paragraphs.len(), 1, "새 셀 문단 1개");
            assert!(c.paragraphs[0].chars.is_empty(), "새 셀 비어 있음");
            assert_eq!(c.paragraphs[0].char_shape_runs.len(), 1, "문자모양 run 1개");
            assert!(c.paragraphs[0].line_segs.is_empty(), "줄 배치 무효화");
            assert_eq!((c.col_span, c.row_span), (1, 1), "병합 없음");
        }
    }

    #[test]
    fn 행_추가_후_채우기() {
        let mut doc = from_markdown("| 가 | 나 |\n|----|----|\n| 1 | 2 |\n");
        let r0 = first_table(&doc).rows; // 2 (헤더+데이터)
        add_rows(&mut doc, 0, None, 1).unwrap();
        // 새 행 인덱스 = r0, 거기에 값 채움.
        set_cell(&mut doc, 0, r0, 0, "새값A").unwrap();
        set_cell(&mut doc, 0, r0, 1, "새값B").unwrap();
        let text = doc.plain_text();
        assert!(
            text.contains("새값A") && text.contains("새값B"),
            "got: {text:?}"
        );
    }

    #[test]
    fn 행_추가_서식_보존() {
        // 새 셀의 폭·여백·테두리·문단모양이 템플릿 행에서 복제되는지.
        let mut doc = from_markdown("| 가 | 나 |\n|----|----|\n| 1 | 2 |\n");
        let r0 = first_table(&doc).rows;
        let tpl: Vec<_> = {
            let t = first_table(&doc);
            t.cells
                .iter()
                .filter(|c| c.row == r0 - 1)
                .map(|c| (c.col, c.width, c.margins, c.border_fill))
                .collect()
        };
        add_rows(&mut doc, 0, None, 1).unwrap();
        let t = first_table(&doc);
        for (col, w, m, bf) in tpl {
            let nc = t
                .cells
                .iter()
                .find(|c| c.row == r0 && c.col == col)
                .expect("새 셀");
            assert_eq!(nc.width, w, "폭 보존");
            assert_eq!(nc.margins, m, "여백 보존");
            assert_eq!(nc.border_fill, bf, "테두리 보존");
        }
    }

    #[test]
    fn 행_추가_엣지케이스() {
        let mut doc = from_markdown("| 가 | 나 |\n|----|----|\n| 1 | 2 |\n");
        // count=0은 무변경.
        let before = first_table(&doc).rows;
        add_rows(&mut doc, 0, Some(0), 0).unwrap();
        assert_eq!(first_table(&doc).rows, before);
        // 없는 표.
        assert!(add_rows(&mut doc, 9, None, 1).is_err());
        // 범위 밖 템플릿 행.
        assert!(add_rows(&mut doc, 0, Some(99), 1).is_err());
    }

    #[test]
    fn 행_추가_u16_초과_거부() {
        // count가 남은 u16 용량을 넘으면 절단 손상 대신 깔끔히 거부(레코드 깨짐 방지).
        let mut doc = from_markdown("| 가 | 나 |\n|----|----|\n| 1 | 2 |\n");
        let err = add_rows(&mut doc, 0, None, 70_000).unwrap_err();
        assert!(err.contains("u16"), "u16 범위 안내: {err}");
        // 표는 변경되지 않아야(거부 전 무변경).
        assert_eq!(first_table(&doc).rows, 2);
    }

    #[test]
    fn 행_추가_새문단_고유_instance_id_와_마지막비트() {
        // 복제 문단은 (1) 서로 다른 비-0 instance_id, (2) nchars bit31(마지막 문단)을
        // 가져야 한다 — hwp5 출신 편집 경로는 writer가 재부여/세팅하지 않으므로.
        let mut doc = from_markdown("| 가 | 나 |\n|----|----|\n| 1 | 2 |\n");
        // 기존 셀 문단에 비-0 instance_id 부여(hwp5 출신 모사).
        for (i, c) in first_table_mut(&mut doc).cells.iter_mut().enumerate() {
            for p in &mut c.paragraphs {
                p.header.instance_id = (i as u32 + 1) * 100;
            }
        }
        add_rows(&mut doc, 0, None, 2).unwrap();
        let t = first_table(&doc);
        let new_paras: Vec<&Paragraph> = t
            .cells
            .iter()
            .filter(|c| c.row >= 2)
            .flat_map(|c| &c.paragraphs)
            .collect();
        assert_eq!(new_paras.len(), 4, "새 셀 4개(2행×2열)");
        let ids: Vec<u32> = new_paras.iter().map(|p| p.header.instance_id).collect();
        assert!(ids.iter().all(|&id| id != 0), "instance_id 비-0: {ids:?}");
        let mut uniq = ids.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), ids.len(), "instance_id 전부 고유: {ids:?}");
        for p in &new_paras {
            assert_ne!(p.header.chars_flags & 0x80, 0, "새 문단 nchars bit31");
        }
    }

    #[test]
    fn 행_추가_세로병합_덮인_부분행_거부() {
        // 세로 병합에 덮여 전 열을 채우지 않는 행(셀 수 < cols)을 템플릿으로 지정하면
        // 거부해야 한다(복제 시 누락 열이 생겨 그리드가 깨짐).
        let mut doc = from_markdown("| 가 | 나 |\n|----|----|\n| 1 | 2 |\n");
        {
            let t = first_table_mut(&mut doc);
            // (0,0)을 세로 2행 병합으로, (1,0) 셀 제거 → 행 1은 (1,1)만(셀 1개, cols=2).
            if let Some(c00) = t.cells.iter_mut().find(|c| c.row == 0 && c.col == 0) {
                c00.row_span = 2;
            }
            t.cells.retain(|c| !(c.row == 1 && c.col == 0));
            t.row_cell_counts = vec![2, 1];
        }
        // 행 1은 부분 행 → 거부.
        let err = add_rows(&mut doc, 0, Some(1), 1).unwrap_err();
        assert!(err.contains("열"), "전 열 미충족 안내: {err}");
    }

    /// 셀 폭을 원하는 대로 갖는 표를 만든다(행별 width 지정, 단순 그리드).
    fn width_table(widths: &[&[i32]]) -> Document {
        let mut doc = from_markdown("| 가 | 나 |\n|----|----|\n| 1 | 2 |\n");
        let t = first_table_mut(&mut doc);
        let base = t.cells[0].clone();
        t.rows = widths.len() as u16;
        t.cols = widths[0].len() as u16;
        t.cells.clear();
        t.row_cell_counts.clear();
        for (r, row) in widths.iter().enumerate() {
            t.row_cell_counts.push(row.len() as u16);
            for (c, w) in row.iter().enumerate() {
                let mut cell = base.clone();
                cell.row = r as u16;
                cell.col = c as u16;
                cell.width = hwp_model::HwpUnit(*w);
                t.cells.push(cell);
            }
        }
        doc
    }

    #[test]
    fn 열_추가_구조_불변식() {
        // 2x2 표 → 열 추가 → cols=3, 셀 6개, row_cell_counts [3,3], 행 우선 순서.
        let mut doc = from_markdown("| 가 | 나 |\n|----|----|\n| 1 | 2 |\n");
        let cells0 = first_table(&doc).cells.len();
        add_col(&mut doc, 0).unwrap();
        let t = first_table(&doc);
        assert_eq!(t.cols, 3);
        assert_eq!(t.cells.len(), cells0 + t.rows as usize);
        assert_eq!(t.row_cell_counts, vec![3, 3]);
        let rows_in_order: Vec<u16> = t.cells.iter().map(|c| c.row).collect();
        let mut sorted = rows_in_order.clone();
        sorted.sort_unstable();
        assert_eq!(rows_in_order, sorted, "행 우선 순서 유지");
        // 새 열(마지막 열) 셀은 빈 문단 1개.
        for c in t.cells.iter().filter(|c| c.col == 2) {
            assert_eq!(c.paragraphs.len(), 1);
            assert!(c.paragraphs[0].chars.is_empty());
        }
    }

    #[test]
    fn 열_추가_폭_합_정확보존() {
        // 행 총폭이 열 추가 전후로 정확히 일치해야 한다(균등 몫 + 잔차 마지막 셀).
        let mut doc = width_table(&[&[100, 50, 51], &[200, 200, 202]]);
        let before: Vec<i64> = (0..2)
            .map(|r| {
                first_table(&doc)
                    .cells
                    .iter()
                    .filter(|c| c.row == r)
                    .map(|c| i64::from(c.width.0))
                    .sum()
            })
            .collect();
        add_col(&mut doc, 0).unwrap();
        let t = first_table(&doc);
        for (r, expect) in before.iter().enumerate() {
            let sum: i64 = t
                .cells
                .iter()
                .filter(|c| c.row as usize == r)
                .map(|c| i64::from(c.width.0))
                .sum();
            assert_eq!(&sum, expect, "행 {r} 총폭 보존");
        }
        // 새 열 폭 = 행총폭/(기존열수+1).
        assert_eq!(
            t.cells
                .iter()
                .find(|c| c.row == 0 && c.col == 3)
                .unwrap()
                .width
                .0,
            201 / 4
        );
        // 모든 폭은 양수.
        assert!(t.cells.iter().all(|c| c.width.0 > 0));
    }

    #[test]
    fn 열_추가_병합표_거부() {
        let mut doc = from_markdown("| 가 | 나 |\n|----|----|\n| 1 | 2 |\n");
        first_table_mut(&mut doc).cells[0].col_span = 2;
        let err = add_col(&mut doc, 0).unwrap_err();
        assert!(err.contains("병합"), "병합 거부 안내: {err}");
        // 표는 무변경.
        assert_eq!(first_table(&doc).cols, 2);
    }

    #[test]
    fn 행_삭제_병합행_거부() {
        let mut doc = from_markdown("| 가 | 나 |\n|----|----|\n| 1 | 2 |\n");
        {
            let t = first_table_mut(&mut doc);
            t.rows = 3;
            t.row_cell_counts = vec![2, 2, 1];
            // (2,0)을 덮는 세로 병합: (1,0) rowspan=2, 행 2는 셀 1개(덮인 행).
            if let Some(c10) = t.cells.iter_mut().find(|c| c.row == 1 && c.col == 0) {
                c10.row_span = 2;
            }
            let mut c2 = t.cells[1].clone();
            c2.row = 2;
            c2.col = 1;
            t.cells.push(c2);
        }
        // 덮인 행(2) 삭제 거부.
        let err = delete_table_row(&mut doc, 0, 2).unwrap_err();
        assert!(err.contains("병합"), "병합 행 거부 안내: {err}");
        // 깨끗한 행(0)은 삭제 가능… 단 (1,0) rowspan이 행1에서 시작 → 행1도 거부.
        let err1 = delete_table_row(&mut doc, 0, 1).unwrap_err();
        assert!(err1.contains("병합"));
        delete_table_row(&mut doc, 0, 0).unwrap();
        assert_eq!(first_table(&doc).rows, 2);
    }

    #[test]
    fn 표_연산_재귀_인덱싱() {
        // 중첩 표가 있으면 set-cell과 같은 깊이 우선 인덱스로 행/열 연산이 걸린다.
        let mut doc = from_markdown("| 가 | 나 |\n|----|----|\n| 1 | 2 |\n");
        // 바깥 표의 (1,0) 셀 안에 1x1 중첩 표 삽입.
        let inner = {
            let t = first_table(&doc);
            let mut inner = t.clone();
            inner.rows = 1;
            inner.cols = 1;
            inner.cells.truncate(1);
            let mut c = inner.cells[0].clone();
            c.row = 0;
            c.col = 0;
            inner.cells = vec![c];
            inner.row_cell_counts = vec![1];
            inner
        };
        {
            let t = first_table_mut(&mut doc);
            let cell = t
                .cells
                .iter_mut()
                .find(|c| c.row == 1 && c.col == 0)
                .unwrap();
            cell.paragraphs[0].controls.push(Control::Table(inner));
        }
        // 인덱스 1 = 중첩 표(깊이 우선). set-cell과 같은 번호로 행 추가가 걸려야 한다.
        add_rows(&mut doc, 1, None, 1).unwrap();
        let outer = first_table(&doc);
        let inner_t = outer
            .cells
            .iter()
            .find(|c| c.row == 1 && c.col == 0)
            .and_then(|c| {
                c.paragraphs[0].controls.iter().find_map(|ct| match ct {
                    Control::Table(t) => Some(t),
                    _ => None,
                })
            })
            .expect("중첩 표");
        assert_eq!(inner_t.rows, 2, "중첩 표에 행 추가됨(재귀 인덱싱)");
    }

    fn first_table_mut(doc: &mut Document) -> &mut hwp_model::Table {
        doc.sections[0]
            .paragraphs
            .iter_mut()
            .flat_map(|p| &mut p.controls)
            .find_map(|c| match c {
                Control::Table(t) => Some(t),
                _ => None,
            })
            .expect("표 없음")
    }
}

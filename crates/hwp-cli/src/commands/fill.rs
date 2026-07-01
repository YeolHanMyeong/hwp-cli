//! `hwp fill` — 템플릿 채우기.
//!
//! 두 경로: (1) **자리표시자 치환**(기본) — `Contents/section*.xml`의 `{{name}}`만
//! 외과 치환하고 나머지 패키지 엔트리(미리보기·compat·BinData)를 바이트 보존(hwpx
//! 입력 전용). (2) **데이터 구동 표 채우기** — `--data`에 `tables` 지시가 있으면 IR로
//! 읽어 표 행을 데이터 수만큼 늘리고(add_rows) 셀을 채운 뒤 다시 쓴다(.hwp/.hwpx 모두).

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::Context;

use crate::commands::cat::load_document;

pub fn run(
    input: &Path,
    output: &Path,
    set: &[String],
    data: Option<&Path>,
    json: bool,
) -> anyhow::Result<()> {
    let data_value: Option<serde_json::Value> = match data {
        Some(d) => {
            let text = std::fs::read_to_string(d)?;
            Some(
                serde_json::from_str(&text)
                    .map_err(|e| anyhow::anyhow!("--data JSON 파싱 실패 ({}): {e}", d.display()))?,
            )
        }
        None => None,
    };

    // 데이터에 `tables`가 (객체 항목의) 비어있지 않은 배열이면 IR 기반 표 채우기로 분기.
    // 객체-배열만 인정해, "tables"라는 이름의 평범한 자리표시자(예: 문자열 배열 값)가
    // 표 채우기로 오인 라우팅돼 실패하지 않게 한다(평문 fill 경로로 떨어뜨림).
    let has_tables = data_value
        .as_ref()
        .and_then(|v| v.get("tables"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|arr| !arr.is_empty() && arr.iter().all(serde_json::Value::is_object));
    if has_tables {
        return fill_tables_ir(input, output, data_value.as_ref().unwrap(), set, json);
    }

    // 기본 경로: {{name}} 자리표시자 바이트 보존 치환(hwpx 전용).
    let mut values: BTreeMap<String, String> = BTreeMap::new();
    if let Some(serde_json::Value::Object(map)) = &data_value {
        for (k, v) in map {
            values.insert(k.clone(), value_to_string(v));
        }
    } else if data_value.is_some() {
        anyhow::bail!("--data 최상위는 객체({{...}})여야 합니다");
    }
    for pair in set {
        let (k, v) = pair
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--set 형식은 name=value 여야 합니다: {pair}"))?;
        values.insert(k.to_string(), v.to_string());
    }
    if values.is_empty() {
        anyhow::bail!(
            "치환 값이 없습니다 (--set name=value / --data values.json / --data tables 지시)"
        );
    }

    // 자리표시자 치환은 HWPX(ZIP) 패키지 외과 수술 전용 — .hwp는 모호한 ZIP 오류 대신 명확히 거절.
    // (.hwp 표 채우기는 위 --data tables 경로가 IR로 처리한다.)
    if crate::format::detect(input)? != crate::format::FileFormat::Hwpx {
        anyhow::bail!(
            "{}: 자리표시자 치환(기본 fill)은 HWPX 입력 전용입니다 (.hwp는 --data의 tables 표 채우기만 지원)",
            input.display()
        );
    }

    let counts = hwpx::patch::fill_placeholders(input, output, &values)
        .map_err(|e| anyhow::anyhow!("fill 실패: {e}"))?;
    let total: usize = counts.values().sum();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "output": output.display().to_string(),
                "replaced": total,
                "counts": counts,
            }))?
        );
    } else {
        let unfilled: Vec<&String> = counts
            .iter()
            .filter(|(_, n)| **n == 0)
            .map(|(k, _)| k)
            .collect();
        if !unfilled.is_empty() {
            eprintln!(
                "[hwp] ⚠️  미치환 자리표시자 {}개: {}",
                unfilled.len(),
                unfilled
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        eprintln!("[hwp] {total}건 치환 -> {}", output.display());
    }
    Ok(())
}

/// 데이터 구동 표 채우기. `data`는 다음 형태:
/// ```json
/// {
///   "fields": {"부서": "기획팀"},
///   "tables": [
///     {"table": 0, "start_row": 1, "template_row": 1,
///      "rows": [["노트북", "5"], ["모니터", "10"]]}
///   ]
/// }
/// ```
/// `fields`(선택)는 `{{키}}`를 본문 전역 치환한다. 각 표는 `start_row`(기본 1)부터
/// `rows` 길이만큼 행이 차도록 자동으로 늘린 뒤(add_rows) 셀을 채운다.
fn fill_tables_ir(
    input: &Path,
    output: &Path,
    data: &serde_json::Value,
    set: &[String],
    json: bool,
) -> anyhow::Result<()> {
    let mut doc = load_document(input)?;
    let mut filled = 0usize;
    let mut added = 0usize;

    // 1) fields: {{키}} → 값. 우선순위: 최상위 스칼라(flat 스키마 호환) < data.fields < --set.
    let mut fields: BTreeMap<String, String> = BTreeMap::new();
    if let serde_json::Value::Object(top) = data {
        for (k, v) in top {
            if k == "fields" || k == "tables" || v.is_object() || v.is_array() {
                continue; // 예약 키·복합값 제외 — 최상위 스칼라만 흡수.
            }
            fields.insert(k.clone(), value_to_string(v));
        }
    }
    if let Some(serde_json::Value::Object(f)) = data.get("fields") {
        for (k, v) in f {
            fields.insert(k.clone(), value_to_string(v));
        }
    }
    for pair in set {
        let (k, v) = pair
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("--set 형식은 name=value 여야 합니다: {pair}"))?;
        fields.insert(k.to_string(), v.to_string());
    }
    for (k, v) in &fields {
        filled += hwp_convert::replace_text(&mut doc, &format!("{{{{{k}}}}}"), v, true);
    }

    // 2) tables: 행 자동 증식 + 셀 채우기
    let tables = data
        .get("tables")
        .and_then(serde_json::Value::as_array)
        .expect("has_tables로 확인됨");
    for (ti, t) in tables.iter().enumerate() {
        let table_index = t
            .get("table")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize;
        let start_row = t
            .get("start_row")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(1) as u16;
        let template_row = t
            .get("template_row")
            .and_then(serde_json::Value::as_u64)
            .map(|r| r as u16);
        let rows = t
            .get("rows")
            .and_then(serde_json::Value::as_array)
            .with_context(|| format!("tables[{ti}].rows 배열이 필요합니다"))?;

        let (cur_rows, _cols) = hwp_convert::table_dims(&mut doc, table_index)
            .ok_or_else(|| anyhow::anyhow!("표 #{table_index}를 찾을 수 없습니다"))?;
        // start_row가 현재 행 수를 넘으면 그 사이가 빈 행으로 채워진다 — 보통 실수이므로 경고.
        if start_row as usize > cur_rows as usize {
            eprintln!(
                "[hwp] ⚠️  tables[{ti}] start_row={start_row} > 현재 행 수 {cur_rows} — 사이 {}행이 빈 행으로 추가됩니다",
                start_row as usize - cur_rows as usize
            );
        }
        let need = start_row as usize + rows.len();
        if need > cur_rows as usize {
            let n = need - cur_rows as usize;
            hwp_convert::add_rows(&mut doc, table_index, template_row, n)
                .map_err(|e| anyhow::anyhow!(e))?;
            added += n;
        }
        for (i, row) in rows.iter().enumerate() {
            let r = start_row + i as u16;
            let cells = row
                .as_array()
                .with_context(|| format!("tables[{ti}].rows[{i}]는 셀 값 배열이어야 합니다"))?;
            for (c, val) in cells.iter().enumerate() {
                hwp_convert::set_cell(&mut doc, table_index, r, c as u16, &value_to_string(val))
                    .map_err(|e| anyhow::anyhow!(e))?;
                filled += 1;
            }
        }
    }

    crate::commands::convert::write_by_ext(&doc, output, true, true)?;

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "output": output.display().to_string(),
                "filled": filled,
                "rows_added": added,
            }))?
        );
    } else {
        eprintln!(
            "[hwp] 표 채움: {filled}건 (+{added}행) -> {}",
            output.display()
        );
    }
    Ok(())
}

/// JSON 값을 셀/필드 문자열로 — 문자열은 그대로, null은 빈 칸, 수/불리언은 표기.
fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

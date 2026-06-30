//! `hwp mcp` — MCP(Model Context Protocol) stdio 서버.
//!
//! tokio/SDK 없이 serde_json만으로 동기 JSON-RPC 2.0(줄 단위 over stdio)을 구현한다.
//! 에이전트(Claude 등)가 도구 호출로 HWP를 **읽고·렌더해서 보고·편집·변환**하게 한다.
//! stdout은 프로토콜 전용(라이브러리 함수는 stdout 미오염, 로그는 stderr).
//!
//! 도구는 라이브러리 계층을 직접 감싼다(commands/*::run 아님 — 그건 stdout 출력).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::commands::cat::load_document;

const PROTOCOL_VERSION: &str = "2024-11-05";

/// 서버 컨텍스트 (렌더/diff 기본 폰트 디렉터리).
pub struct Ctx {
    pub font_dirs: Vec<PathBuf>,
}

/// stdio JSON-RPC 루프. EOF까지 한 줄씩 처리한다.
pub fn run(font_dirs: Vec<PathBuf>) -> anyhow::Result<()> {
    let ctx = Ctx { font_dirs };
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break; // EOF
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(resp) = handle_request(trimmed, &ctx) {
            out.write_all(resp.as_bytes())?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// 한 줄 JSON-RPC 요청 → 응답 JSON 문자열. 알림(id 없음)이면 None.
pub fn handle_request(line: &str, ctx: &Ctx) -> Option<String> {
    let req: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(error_response(
                Value::Null,
                -32700,
                &format!("parse error: {e}"),
            ));
        }
    };
    let id = req.get("id").cloned();
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let is_notification = id.is_none();

    match method {
        "initialize" => Some(result_response(
            id_or_null(id),
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "hwp-cli", "version": env!("CARGO_PKG_VERSION")},
            }),
        )),
        "notifications/initialized" | "notifications/cancelled" => None,
        "ping" => Some(result_response(id_or_null(id), json!({}))),
        "tools/list" => Some(result_response(
            id_or_null(id),
            json!({ "tools": tool_defs() }),
        )),
        "tools/call" => {
            if is_notification {
                return None;
            }
            let params = req.get("params").cloned().unwrap_or(Value::Null);
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            Some(result_response(id_or_null(id), call_tool(name, &args, ctx)))
        }
        _ => {
            if is_notification {
                return None;
            }
            Some(error_response(
                id_or_null(id),
                -32601,
                &format!("method not found: {method}"),
            ))
        }
    }
}

fn id_or_null(id: Option<Value>) -> Value {
    id.unwrap_or(Value::Null)
}

fn result_response(id: Value, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}).to_string()
}

/// 도구를 실행해 `tools/call` result를 만든다. 실행 오류는 isError=true content로.
fn call_tool(name: &str, args: &Value, ctx: &Ctx) -> Value {
    let result: Result<Vec<Value>, String> = match name {
        "hwp_info" => tool_info(args),
        "hwp_read" => tool_read(args),
        "hwp_list_fields" => tool_list_fields(args),
        "hwp_render" => tool_render(args, ctx),
        "hwp_edit" => tool_edit(args),
        "hwp_convert" => tool_convert(args),
        "hwp_new" => tool_new(args),
        "hwp_diff" => tool_diff(args, ctx),
        other => Err(format!("알 수 없는 도구: {other}")),
    };
    match result {
        Ok(content) => json!({"content": content, "isError": false}),
        Err(e) => json!({"content": [text_content(&format!("오류: {e}"))], "isError": true}),
    }
}

// ---- content/인자 헬퍼 ----

fn text_content(s: &str) -> Value {
    json!({"type": "text", "text": s})
}

fn image_content(png: &[u8]) -> Value {
    json!({"type": "image", "data": hwp_convert::base64::encode(png), "mimeType": "image/png"})
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("필수 인자 누락: {key}"))
}

fn arg_str_opt<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn arg_u64(args: &Value, key: &str, default: u64) -> u64 {
    args.get(key).and_then(Value::as_u64).unwrap_or(default)
}

fn arg_f64(args: &Value, key: &str, default: f64) -> f64 {
    args.get(key).and_then(Value::as_f64).unwrap_or(default)
}

fn font_dirs_for(args: &Value, ctx: &Ctx) -> Vec<PathBuf> {
    let mut dirs = ctx.font_dirs.clone();
    if let Some(d) = arg_str_opt(args, "font_dir") {
        dirs.push(PathBuf::from(d));
    }
    dirs
}

// ---- 도구 핸들러 ----

fn tool_info(args: &Value) -> Result<Vec<Value>, String> {
    let path = arg_str(args, "path")?;
    let v = crate::commands::info::info_json(Path::new(path)).map_err(|e| e.to_string())?;
    Ok(vec![text_content(
        &serde_json::to_string_pretty(&v).unwrap_or_default(),
    )])
}

fn tool_read(args: &Value) -> Result<Vec<Value>, String> {
    let path = arg_str(args, "path")?;
    let format = arg_str_opt(args, "format").unwrap_or("plain");
    let doc = load_document(Path::new(path)).map_err(|e| e.to_string())?;
    let text = match format {
        "plain" => doc.plain_text(),
        "markdown" | "md" => hwp_convert::to_markdown(&doc),
        "json" => hwp_convert::to_json(&doc, true, false).map_err(|e| e.to_string())?,
        other => return Err(format!("알 수 없는 format: {other} (plain|markdown|json)")),
    };
    Ok(vec![text_content(&text)])
}

fn tool_list_fields(args: &Value) -> Result<Vec<Value>, String> {
    let path = arg_str(args, "path")?;
    let doc = load_document(Path::new(path)).map_err(|e| e.to_string())?;
    let fields: Vec<Value> = hwp_convert::list_fields(&doc)
        .iter()
        .map(|f| {
            json!({
                "kind": f.kind, "ctrl_id": f.ctrl_id,
                "name": f.name, "command": f.command, "value": f.value,
            })
        })
        .collect();
    Ok(vec![text_content(
        &serde_json::to_string_pretty(&fields).unwrap_or_default(),
    )])
}

fn tool_render(args: &Value, ctx: &Ctx) -> Result<Vec<Value>, String> {
    let path = arg_str(args, "path")?;
    let page = arg_u64(args, "page", 1) as usize;
    let dpi = arg_f64(args, "dpi", 120.0) as f32;
    let doc = load_document(Path::new(path)).map_err(|e| e.to_string())?;
    let out = hwp_render::render_document(
        &doc,
        &hwp_render::RenderOptions {
            dpi,
            font_dirs: font_dirs_for(args, ctx),
        },
    )
    .map_err(|e| e.to_string())?;
    if page == 0 || page > out.pages.len() {
        return Err(format!(
            "페이지 범위 오류: 문서 {}쪽, 요청 {page}",
            out.pages.len()
        ));
    }
    let pixmap = &out.pages[page - 1];
    let png = pixmap
        .encode_png()
        .ok()
        .ok_or_else(|| "PNG 인코딩 실패".to_string())?;
    let summary = format!(
        "페이지 {page}/{} 렌더 ({}×{}px, {dpi}dpi). {}",
        out.pages.len(),
        pixmap.width(),
        pixmap.height(),
        out.report.join("; ")
    );
    Ok(vec![text_content(&summary), image_content(&png)])
}

fn tool_edit(args: &Value) -> Result<Vec<Value>, String> {
    let input = arg_str(args, "input")?;
    let output = arg_str(args, "output")?;
    let mut doc = load_document(Path::new(input)).map_err(|e| e.to_string())?;
    let mut summary = Vec::new();

    if let Some(arr) = args.get("replace").and_then(Value::as_array) {
        for r in arr {
            let from = r
                .get("from")
                .and_then(Value::as_str)
                .ok_or("replace 항목에 from 필요")?;
            let to = r
                .get("to")
                .and_then(Value::as_str)
                .ok_or("replace 항목에 to 필요")?;
            let n = hwp_convert::replace_text(&mut doc, from, to, true);
            summary.push(format!("치환 {from:?}→{to:?}: {n}건"));
        }
    }
    if let Some(arr) = args.get("set_cell").and_then(Value::as_array) {
        for c in arr {
            let table = c.get("table").and_then(Value::as_u64).unwrap_or(0) as usize;
            let row = c.get("row").and_then(Value::as_u64).unwrap_or(0) as u16;
            let col = c.get("col").and_then(Value::as_u64).unwrap_or(0) as u16;
            let text = c.get("text").and_then(Value::as_str).unwrap_or("");
            hwp_convert::set_cell(&mut doc, table, row, col, text)?;
            summary.push(format!("셀 표{table}({row},{col})={text:?}"));
        }
    }
    // 누름틀 생성은 set_field보다 먼저 — 같은 호출에서 생성→채우기 가능.
    if let Some(arr) = args.get("create_field").and_then(Value::as_array) {
        for f in arr {
            let anchor = f
                .get("anchor")
                .and_then(Value::as_str)
                .ok_or("create_field 항목에 anchor 필요")?;
            let name = f
                .get("name")
                .and_then(Value::as_str)
                .ok_or("create_field 항목에 name 필요")?;
            let value = f.get("value").and_then(Value::as_str).unwrap_or("");
            if hwp_convert::create_field(&mut doc, anchor, name, value) {
                summary.push(format!("누름틀 생성 {anchor:?}→{name:?}"));
            } else {
                summary.push(format!("경고: 앵커 {anchor:?} 못 찾음"));
            }
        }
    }
    if let Some(arr) = args.get("set_field").and_then(Value::as_array) {
        for f in arr {
            let name = f
                .get("name")
                .and_then(Value::as_str)
                .ok_or("set_field 항목에 name 필요")?;
            let value = f.get("value").and_then(Value::as_str).unwrap_or("");
            let n = hwp_convert::set_field(&mut doc, name, value);
            summary.push(format!("필드 {name:?}={value:?}: {n}건"));
        }
    }
    if let Some(arr) = args.get("set_format").and_then(Value::as_array) {
        for f in arr {
            let pattern = f
                .get("pattern")
                .and_then(Value::as_str)
                .ok_or("set_format 항목에 pattern 필요")?;
            let fmt = hwp_convert::CharFormat {
                bold: f.get("bold").and_then(Value::as_bool),
                italic: f.get("italic").and_then(Value::as_bool),
                underline: f.get("underline").and_then(Value::as_bool),
                strike: f.get("strike").and_then(Value::as_bool),
                size_pt: f.get("size").and_then(Value::as_f64).map(|v| v as f32),
                color: f
                    .get("color")
                    .and_then(Value::as_str)
                    .and_then(crate::commands::edit::parse_color),
            };
            let n = hwp_convert::set_char_format(&mut doc, pattern, &fmt);
            summary.push(format!("글자서식 {pattern:?}: {n}건"));
        }
    }
    if let Some(arr) = args.get("set_align").and_then(Value::as_array) {
        for a in arr {
            let pattern = a
                .get("pattern")
                .and_then(Value::as_str)
                .ok_or("set_align 항목에 pattern 필요")?;
            let align = match a.get("align").and_then(Value::as_str).unwrap_or("left") {
                "right" => 2,
                "center" => 3,
                "justify" | "both" => 0,
                "distribute" => 4,
                "divide" => 5,
                _ => 1, // left
            };
            let n = hwp_convert::set_para_align(&mut doc, pattern, align);
            summary.push(format!("문단정렬 {pattern:?}: {n}건"));
        }
    }
    let mut structural = false;
    if let Some(arr) = args.get("insert_para").and_then(Value::as_array) {
        for p in arr {
            let anchor = p
                .get("anchor")
                .and_then(Value::as_str)
                .ok_or("insert_para 항목에 anchor 필요")?;
            let text = p.get("text").and_then(Value::as_str).unwrap_or("");
            let before = p.get("before").and_then(Value::as_bool).unwrap_or(false);
            structural = true;
            if hwp_convert::insert_paragraph(&mut doc, anchor, text, before) {
                summary.push(format!("문단삽입 {anchor:?} {}", if before { "앞" } else { "뒤" }));
            } else {
                summary.push(format!("경고: 앵커 {anchor:?} 못 찾음"));
            }
        }
    }
    if let Some(arr) = args.get("delete_para").and_then(Value::as_array) {
        for p in arr {
            let matching = p
                .get("matching")
                .and_then(Value::as_str)
                .ok_or("delete_para 항목에 matching 필요")?;
            structural = true;
            let n = hwp_convert::delete_paragraph(&mut doc, matching);
            summary.push(format!("문단삭제 {matching:?}: {n}건"));
        }
    }
    if let Some(arr) = args.get("add_row").and_then(Value::as_array) {
        for r in arr {
            let table = r.get("table").and_then(Value::as_u64).unwrap_or(0) as usize;
            structural = true;
            hwp_convert::add_table_row(&mut doc, table)?;
            summary.push(format!("표{table} 행 추가"));
        }
    }
    if let Some(arr) = args.get("delete_row").and_then(Value::as_array) {
        for r in arr {
            let table = r.get("table").and_then(Value::as_u64).unwrap_or(0) as usize;
            let row = r.get("row").and_then(Value::as_u64).unwrap_or(0) as u16;
            structural = true;
            hwp_convert::delete_table_row(&mut doc, table, row)?;
            summary.push(format!("표{table} 행{row} 삭제"));
        }
    }
    if summary.is_empty() {
        return Err(
            "적용할 편집이 없습니다 (replace/set_cell/set_field/create_field/set_format/set_align/insert_para/delete_para/add_row/delete_row 확인)"
                .to_string(),
        );
    }

    let out_path = Path::new(output);
    let is_hwp = out_path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
        == Some("hwp");
    if structural && is_hwp {
        // 구조 편집 hwp는 삽입 불변식을 세우려 합성 경로를 강제한다.
        crate::commands::convert::write_hwp_structural(&doc, out_path).map_err(|e| e.to_string())?;
    } else {
        crate::commands::convert::write_by_ext(&doc, out_path, true, false)
            .map_err(|e| e.to_string())?;
    }
    Ok(vec![text_content(&format!(
        "편집 완료: {input} → {output}\n{}",
        summary.join("\n")
    ))])
}

fn tool_convert(args: &Value) -> Result<Vec<Value>, String> {
    let input = arg_str(args, "input")?;
    let output = arg_str(args, "output")?;
    let embed_bin = args
        .get("embed_bin")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let doc = load_document(Path::new(input)).map_err(|e| e.to_string())?;
    crate::commands::convert::write_by_ext(&doc, Path::new(output), false, embed_bin)
        .map_err(|e| e.to_string())?;
    Ok(vec![text_content(&format!(
        "변환 완료: {input} → {output}"
    ))])
}

fn tool_new(args: &Value) -> Result<Vec<Value>, String> {
    let output = arg_str(args, "output")?;
    let doc = if let Some(md) = arg_str_opt(args, "markdown") {
        hwp_convert::from_markdown(md)
    } else if let Some(j) = arg_str_opt(args, "json") {
        hwp_convert::from_json(j)?
    } else {
        hwp_convert::from_markdown("")
    };
    crate::commands::convert::write_by_ext(&doc, Path::new(output), false, false)
        .map_err(|e| e.to_string())?;
    Ok(vec![text_content(&format!("생성 완료: {output}"))])
}

fn tool_diff(args: &Value, ctx: &Ctx) -> Result<Vec<Value>, String> {
    let input = arg_str(args, "input")?;
    let reference = arg_str(args, "ref")?;
    let page = arg_u64(args, "page", 1) as usize;
    let dpi = arg_f64(args, "dpi", 120.0) as f32;
    let doc = load_document(Path::new(input)).map_err(|e| e.to_string())?;
    let out = hwp_render::render_document(
        &doc,
        &hwp_render::RenderOptions {
            dpi,
            font_dirs: font_dirs_for(args, ctx),
        },
    )
    .map_err(|e| e.to_string())?;
    if page == 0 || page > out.pages.len() {
        return Err(format!("페이지 범위 오류: 문서 {}쪽", out.pages.len()));
    }
    let refpx = hwp_render::load_png(Path::new(reference)).map_err(|e| e.to_string())?;
    let (rep, _) = hwp_render::compare(&out.pages[page - 1], &refpx, 16)?;
    let v = json!({
        "ink_ratio": rep.ink_ratio,
        "dx": rep.dx,
        "dy": rep.dy,
        "bad_pixel_pct": rep.bad_pixel_pct,
        "mae": rep.mae,
    });
    Ok(vec![text_content(
        &serde_json::to_string_pretty(&v).unwrap_or_default(),
    )])
}

// ---- 도구 정의 (tools/list) ----

fn tool_defs() -> Vec<Value> {
    vec![
        json!({
            "name": "hwp_info",
            "description": "HWP/HWPX 파일의 포맷·버전·속성·스트림 목록을 JSON으로 진단(본문 파싱 불필요).",
            "inputSchema": {"type": "object", "properties": {
                "path": {"type": "string", "description": "hwp/hwpx 파일 경로"}
            }, "required": ["path"]}
        }),
        json!({
            "name": "hwp_read",
            "description": "본문을 추출한다. format=json이면 전체 IR(구조)을, markdown/plain이면 텍스트를 반환.",
            "inputSchema": {"type": "object", "properties": {
                "path": {"type": "string"},
                "format": {"type": "string", "enum": ["plain", "markdown", "json"], "description": "기본 plain"}
            }, "required": ["path"]}
        }),
        json!({
            "name": "hwp_list_fields",
            "description": "필드/누름틀 목록(이름·종류·값·명령)을 JSON으로. 누름틀(%clk)은 name으로 채울 수 있다.",
            "inputSchema": {"type": "object", "properties": {
                "path": {"type": "string"}
            }, "required": ["path"]}
        }),
        json!({
            "name": "hwp_render",
            "description": "지정 페이지를 PNG 이미지로 렌더해 반환(에이전트가 문서를 직접 본다).",
            "inputSchema": {"type": "object", "properties": {
                "path": {"type": "string"},
                "page": {"type": "integer", "description": "1-기반, 기본 1"},
                "dpi": {"type": "number", "description": "기본 120"},
                "font_dir": {"type": "string", "description": "추가 폰트 디렉터리(선택)"}
            }, "required": ["path"]}
        }),
        json!({
            "name": "hwp_edit",
            "description": "기존 문서를 편집해 출력 경로에 쓴다(이미지·서식 보존). 출력 확장자(.hwp/.hwpx/.json/.md)로 포맷 결정.",
            "inputSchema": {"type": "object", "properties": {
                "input": {"type": "string"},
                "output": {"type": "string"},
                "replace": {"type": "array", "items": {"type": "object", "properties": {
                    "from": {"type": "string"}, "to": {"type": "string"}}, "required": ["from", "to"]},
                    "description": "텍스트 치환(모든 일치)"},
                "set_cell": {"type": "array", "items": {"type": "object", "properties": {
                    "table": {"type": "integer"}, "row": {"type": "integer"},
                    "col": {"type": "integer"}, "text": {"type": "string"}},
                    "required": ["table", "row", "col", "text"]}, "description": "표 셀 설정(0-기반)"},
                "set_field": {"type": "array", "items": {"type": "object", "properties": {
                    "name": {"type": "string"}, "value": {"type": "string"}},
                    "required": ["name", "value"]}, "description": "필드/누름틀 채우기(이름으로)"},
                "create_field": {"type": "array", "items": {"type": "object", "properties": {
                    "anchor": {"type": "string"}, "name": {"type": "string"}, "value": {"type": "string"}},
                    "required": ["anchor", "name"]}, "description": "앵커 텍스트 뒤에 %clk 누름틀 생성(이름·선택 표시값; set_field로 채움)"},
                "set_format": {"type": "array", "items": {"type": "object", "properties": {
                    "pattern": {"type": "string"}, "bold": {"type": "boolean"},
                    "italic": {"type": "boolean"}, "underline": {"type": "boolean"},
                    "strike": {"type": "boolean"}, "size": {"type": "number", "description": "pt"},
                    "color": {"type": "string", "description": "#RRGGBB 또는 색이름"}},
                    "required": ["pattern"]}, "description": "글자 서식(매칭 텍스트)"},
                "set_align": {"type": "array", "items": {"type": "object", "properties": {
                    "pattern": {"type": "string"},
                    "align": {"type": "string", "enum": ["left", "right", "center", "justify", "distribute", "divide"]}},
                    "required": ["pattern", "align"]}, "description": "문단 정렬(매칭 문단)"},
                "insert_para": {"type": "array", "items": {"type": "object", "properties": {
                    "anchor": {"type": "string"}, "text": {"type": "string"},
                    "before": {"type": "boolean", "description": "true면 앵커 문단 앞(기본 뒤)"}},
                    "required": ["anchor", "text"]}, "description": "문단 삽입(앵커 문단 앞/뒤, 모양 상속)"},
                "delete_para": {"type": "array", "items": {"type": "object", "properties": {
                    "matching": {"type": "string"}},
                    "required": ["matching"]}, "description": "매칭 텍스트가 든 문단 삭제(최소 1문단 유지)"},
                "add_row": {"type": "array", "items": {"type": "object", "properties": {
                    "table": {"type": "integer"}},
                    "required": ["table"]}, "description": "N번째 표 끝에 빈 행 추가(0-기반)"},
                "delete_row": {"type": "array", "items": {"type": "object", "properties": {
                    "table": {"type": "integer"}, "row": {"type": "integer"}},
                    "required": ["table", "row"]}, "description": "N번째 표의 R행 삭제(0-기반)"}
            }, "required": ["input", "output"]}
        }),
        json!({
            "name": "hwp_convert",
            "description": "포맷 변환. 출력 확장자(.hwp/.hwpx/.json/.md)로 결정. embed_bin이면 JSON에 이미지 base64 임베드.",
            "inputSchema": {"type": "object", "properties": {
                "input": {"type": "string"}, "output": {"type": "string"},
                "embed_bin": {"type": "boolean"}
            }, "required": ["input", "output"]}
        }),
        json!({
            "name": "hwp_new",
            "description": "새 문서 생성. markdown 또는 json(IR) 본문에서. 출력 확장자로 포맷 결정.",
            "inputSchema": {"type": "object", "properties": {
                "output": {"type": "string"},
                "markdown": {"type": "string", "description": "markdown 본문(선택)"},
                "json": {"type": "string", "description": "IR JSON 본문(선택)"}
            }, "required": ["output"]}
        }),
        json!({
            "name": "hwp_diff",
            "description": "렌더 결과를 기준 PNG와 비교해 오차(잉크 적용률·위치 오프셋·픽셀 차이율)를 측정.",
            "inputSchema": {"type": "object", "properties": {
                "input": {"type": "string"}, "ref": {"type": "string", "description": "기준 PNG 경로"},
                "page": {"type": "integer"}, "dpi": {"type": "number"},
                "font_dir": {"type": "string"}
            }, "required": ["input", "ref"]}
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        Ctx {
            font_dirs: vec![PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../fonts"
            ))],
        }
    }

    fn fixture(rel: &str) -> String {
        format!("{}/../../fixtures/{rel}", env!("CARGO_MANIFEST_DIR"))
    }

    fn call(line: &str) -> Value {
        let resp = handle_request(line, &ctx()).expect("응답 있어야 함");
        serde_json::from_str(&resp).unwrap()
    }

    #[test]
    fn initialize_응답() {
        let v = call(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#);
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert!(v["result"]["serverInfo"]["name"].is_string());
    }

    #[test]
    fn 알림은_응답없음() {
        assert!(
            handle_request(
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                &ctx()
            )
            .is_none()
        );
    }

    #[test]
    fn 미지원_메서드_에러() {
        let v = call(r#"{"jsonrpc":"2.0","id":2,"method":"no_such_method"}"#);
        assert_eq!(v["error"]["code"], -32601);
    }

    #[test]
    fn tools_list_도구_노출() {
        let v = call(r#"{"jsonrpc":"2.0","id":3,"method":"tools/list"}"#);
        let tools = v["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        for expected in [
            "hwp_info",
            "hwp_read",
            "hwp_render",
            "hwp_edit",
            "hwp_convert",
            "hwp_new",
            "hwp_diff",
        ] {
            assert!(names.contains(&expected), "{expected} 누락");
        }
    }

    #[test]
    fn call_hwp_read_json() {
        let line = format!(
            r#"{{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{{"name":"hwp_read","arguments":{{"path":"{}","format":"plain"}}}}}}"#,
            fixture("hwp5/hello_world.hwp")
        );
        let v = call(&line);
        assert_eq!(v["result"]["isError"], false);
        let text = v["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Hello"), "본문 추출: {text:?}");
    }

    #[test]
    fn call_hwp_render_이미지() {
        let line = format!(
            r#"{{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{{"name":"hwp_render","arguments":{{"path":"{}","page":1,"dpi":96}}}}}}"#,
            fixture("hwp5/hello_world.hwp")
        );
        let v = call(&line);
        assert_eq!(v["result"]["isError"], false);
        let content = v["result"]["content"].as_array().unwrap();
        let img = content
            .iter()
            .find(|c| c["type"] == "image")
            .expect("이미지 콘텐츠");
        assert_eq!(img["mimeType"], "image/png");
        assert!(
            img["data"].as_str().unwrap().len() > 100,
            "base64 PNG 비어있음"
        );
    }

    #[test]
    fn call_잘못된_인자_오류() {
        let v = call(
            r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"hwp_read","arguments":{}}}"#,
        );
        assert_eq!(v["result"]["isError"], true);
    }
}

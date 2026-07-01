//! 필드/누름틀 읽기·채우기 (한컴 GetFieldList/PutFieldText 동등).
//!
//! HWP 필드 = FIELD_START(문자 코드 3, 확장 컨트롤) ... 표시 텍스트 ... FIELD_END(코드 4).
//! - 종류: 컨트롤 ID (%clk=누름틀, %fmu=계산식, %hlk=하이퍼링크 등).
//! - 이름: CTRL_DATA(HWPTAG, Parameter Set)의 첫 BSTR — 누름틀에만 보통 존재.
//! - 명령: 필드 레코드의 command 문자열(계산식 식, 누름틀 지시문 등).
//! - 값: FIELD_START~FIELD_END 사이 텍스트.
//!
//! IR을 바꾸지 않고 온디맨드로 파싱한다(리더/writer/왕복 무영향). 채우기는 값 영역만
//! 교체하고 char_shape_run을 보정한다 — 쓰기는 편집 경로(write_hwp_edited)를 거친다.

use hwp_model::opaque::OpaqueRecord;
use hwp_model::{Control, Document, GenericControl, HwpChar, Paragraph};

use crate::edit::{adjust_runs, find_match, utf16_len};

const CTRL_DATA_TAG: u16 = 0x0010 + 71; // HWPTAG_CTRL_DATA
const FIELD_START: u16 = 3;
const FIELD_END: u16 = 4;

/// 필드 하나의 정보.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldInfo {
    /// 종류 표시명(누름틀/계산식/...).
    pub kind: String,
    /// 컨트롤 ID 원문(예: "%clk").
    pub ctrl_id: String,
    /// 필드 이름(누름틀; CTRL_DATA). 없으면 None.
    pub name: Option<String>,
    /// 명령/지시문(필드 레코드 command). 없으면 None.
    pub command: Option<String>,
    /// 화면 표시 값.
    pub value: String,
}

/// 필드 종류 컨트롤 ID인지(누름틀·계산식·하이퍼링크 등).
pub fn is_field_ctrl_id(ctrl_id: &[u8; 4]) -> bool {
    matches!(
        ctrl_id,
        b"%clk"
            | b"%fmu"
            | b"%hlk"
            | b"%mmg"
            | b"%dte"
            | b"%ddt"
            | b"%xrf"
            | b"%bmk"
            | b"%pat"
            | b"%smr"
            | b"%usr"
            | b"%unk"
    )
}

/// 컨트롤 ID → OWPML fieldBegin type 속성.
pub fn owpml_field_type(ctrl_id: &[u8; 4]) -> &'static str {
    match ctrl_id {
        b"%clk" => "CLICK_HERE",
        b"%fmu" => "FORMULA",
        b"%hlk" => "HYPERLINK",
        b"%mmg" => "MAIL_MERGE",
        b"%dte" => "DATE",
        b"%ddt" => "DOCUMENT_DATE",
        b"%xrf" => "CROSS_REF",
        b"%bmk" => "BOOKMARK",
        b"%pat" => "PATH",
        b"%smr" => "SUMMARY",
        b"%usr" => "USER_INFO",
        _ => "UNKNOWN",
    }
}

/// OWPML fieldBegin type → 컨트롤 ID(역매핑, 미지는 `%unk`).
pub fn field_ctrl_id_from_owpml(t: &str) -> [u8; 4] {
    match t {
        "CLICK_HERE" => *b"%clk",
        "FORMULA" => *b"%fmu",
        "HYPERLINK" => *b"%hlk",
        "MAIL_MERGE" => *b"%mmg",
        "DATE" => *b"%dte",
        "DOCUMENT_DATE" => *b"%ddt",
        "CROSS_REF" | "CROSS_REFERENCE" => *b"%xrf",
        "BOOKMARK" => *b"%bmk",
        "PATH" | "FILE_PATH" => *b"%pat",
        "SUMMARY" => *b"%smr",
        "USER_INFO" => *b"%usr",
        _ => *b"%unk",
    }
}

fn kind_of(ctrl_id: &[u8; 4]) -> &'static str {
    match ctrl_id {
        b"%clk" => "누름틀",
        b"%fmu" => "계산식",
        b"%hlk" => "하이퍼링크",
        b"%mmg" => "메일머지",
        b"%dte" => "날짜",
        b"%ddt" => "문서날짜",
        b"%xrf" => "상호참조",
        b"%bmk" => "책갈피",
        b"%pat" => "파일경로",
        b"%smr" => "문서요약",
        b"%usr" => "사용자정보",
        b"%unk" => "알수없음",
        _ => "필드",
    }
}

/// 문서의 모든 필드를 등장 순서로 나열한다(본문·표 셀·글상자 재귀).
pub fn list_fields(doc: &Document) -> Vec<FieldInfo> {
    let mut out = Vec::new();
    for section in &doc.sections {
        for para in &section.paragraphs {
            collect_fields(para, &mut out);
        }
    }
    out
}

fn collect_fields(para: &Paragraph, out: &mut Vec<FieldInfo>) {
    for (i, ch) in para.chars.iter().enumerate() {
        if let HwpChar::ExtCtrl {
            code,
            ctrl_id,
            ctrl_index,
            ..
        } = ch
            && *code == FIELD_START
        {
            let (name, command) = ctrl_index
                .and_then(|idx| para.controls.get(idx as usize))
                .map(field_meta)
                .unwrap_or((None, None));
            out.push(FieldInfo {
                kind: kind_of(ctrl_id).to_string(),
                ctrl_id: String::from_utf8_lossy(ctrl_id).into_owned(),
                name,
                command,
                value: field_value(&para.chars, i),
            });
        }
    }
    for ctrl in &para.controls {
        for_each_nested(ctrl, &mut |p| collect_fields(p, out));
    }
}

/// FIELD_START(start) 다음부터 FIELD_END 전까지의 표시 텍스트.
fn field_value(chars: &[HwpChar], start: usize) -> String {
    let mut s = String::new();
    for ch in &chars[start + 1..] {
        match ch {
            HwpChar::Text(c) => s.push(*c),
            HwpChar::CharCtrl(10) => s.push('\n'),
            HwpChar::InlineCtrl { code, .. } if *code == FIELD_END => break,
            _ => {}
        }
    }
    s
}

/// 필드 컨트롤에서 (이름, 명령)을 읽는다.
pub fn field_meta(ctrl: &Control) -> (Option<String>, Option<String>) {
    let Control::Generic(g) = ctrl else {
        return (None, None);
    };
    let name = g
        .raw_children
        .iter()
        .find(|r| r.tag == CTRL_DATA_TAG)
        .and_then(|r| first_bstr(&r.data));
    (name, parse_command(&g.data))
}

/// 필드 레코드(ctrl_id 제거됨) command 문자열: 속성(4) 기타(1) len(2) WCHAR[len] id(4).
fn parse_command(data: &[u8]) -> Option<String> {
    if data.len() < 7 {
        return None;
    }
    let len = u16::from_le_bytes([data[5], data[6]]) as usize;
    let end = 7usize.checked_add(len * 2)?;
    if end > data.len() || len == 0 {
        return None;
    }
    Some(decode_utf16le(&data[7..end]))
}

/// CTRL_DATA Parameter Set의 첫 BSTR(필드 이름). 알 수 없는 항목 타입을 만나면 중단.
fn first_bstr(data: &[u8]) -> Option<String> {
    if data.len() < 4 {
        return None;
    }
    let count = i16::from_le_bytes([data[2], data[3]]).max(0) as usize;
    let mut o = 4usize;
    for _ in 0..count {
        if o + 4 > data.len() {
            break;
        }
        let item_type = u16::from_le_bytes([data[o + 2], data[o + 3]]);
        o += 4;
        match item_type {
            0 => {} // PIT_NULL
            1 => {
                // PIT_BSTR: UINT32 len + WCHAR[len]
                if o + 4 > data.len() {
                    break;
                }
                let len =
                    u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]) as usize;
                o += 4;
                let end = o.checked_add(len * 2)?;
                if end > data.len() {
                    break;
                }
                return Some(decode_utf16le(&data[o..end]));
            }
            2 | 6 => o += 1,         // I1/UI1
            3 | 7 => o += 2,         // I2/UI2
            4 | 5 | 8 | 9 => o += 4, // I4/I/UI4/UI
            _ => break,              // SET/ARRAY/BINDATA/미지 — 안전 중단
        }
    }
    None
}

fn decode_utf16le(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

/// 이름이 일치하는 필드의 값을 `value`로 채운다. 반환=채운 개수.
pub fn set_field(doc: &mut Document, name: &str, value: &str) -> usize {
    let mut count = 0;
    for section in &mut doc.sections {
        for para in &mut section.paragraphs {
            count += set_field_para(para, name, value);
        }
    }
    count
}

fn set_field_para(para: &mut Paragraph, name: &str, value: &str) -> usize {
    // 불변 스캔: 이름 일치 필드의 (값 시작 char idx, 값 끝 char idx, 값 WCHAR 위치, 옛 값 WCHAR 길이).
    let mut regions: Vec<(usize, usize, u32, u32)> = Vec::new();
    let mut wpos = 0u32;
    for (idx, ch) in para.chars.iter().enumerate() {
        if let HwpChar::ExtCtrl {
            code, ctrl_index, ..
        } = ch
            && *code == FIELD_START
        {
            let nm = ctrl_index
                .and_then(|i| para.controls.get(i as usize))
                .and_then(|c| field_meta(c).0);
            if nm.as_deref() == Some(name) {
                let value_wpos = wpos + ch.wchar_width();
                let mut e = idx + 1;
                let mut vw = 0u32;
                while e < para.chars.len() {
                    match &para.chars[e] {
                        HwpChar::InlineCtrl { code, .. } if *code == FIELD_END => break,
                        other => vw += other.wchar_width(),
                    }
                    e += 1;
                }
                regions.push((idx + 1, e, value_wpos, vw));
            }
        }
        wpos += ch.wchar_width();
    }

    let count = regions.len();
    let new_wlen = utf16_len(value);
    // 인덱스 무효화 방지: 뒤에서 앞으로 적용.
    for (s, e, vp, old_wlen) in regions.into_iter().rev() {
        para.chars.splice(s..e, build_value_chars(value));
        adjust_runs(&mut para.char_shape_runs, vp, old_wlen, new_wlen);
    }
    if count > 0 {
        para.line_segs.clear();
    }

    let mut nested = 0;
    for ctrl in &mut para.controls {
        match ctrl {
            Control::Table(t) => {
                for cell in &mut t.cells {
                    for p in &mut cell.paragraphs {
                        nested += set_field_para(p, name, value);
                    }
                }
            }
            Control::Generic(g) => {
                for l in &mut g.paragraph_lists {
                    for p in &mut l.paragraphs {
                        nested += set_field_para(p, name, value);
                    }
                }
            }
            _ => {}
        }
    }
    count + nested
}

fn build_value_chars(value: &str) -> Vec<HwpChar> {
    value
        .chars()
        .map(|c| {
            if c == '\n' {
                HwpChar::CharCtrl(10)
            } else {
                HwpChar::Text(c)
            }
        })
        .collect()
}

/// `{{name}}` 텍스트 자리표시자 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceholderInfo {
    /// 자리표시자 이름(중괄호 안, 양끝 공백 제거).
    pub name: String,
    /// 문서 전체 등장 횟수.
    pub occurrences: usize,
}

/// 본문(표 셀·글상자 재귀)에서 `{{name}}` 텍스트 자리표시자를 등장 순서로 수집한다.
///
/// 누름틀(form field, [`list_fields`])과 별개 — 순수 텍스트 `{{...}}` 템플릿용.
/// `name`은 `[\w가-힣.-]`(영숫자·한글·`.`·`-`·`_`)만 허용하며, 한 문단 내 연속된
/// 텍스트만 이어 스캔하므로 제어문자/줄나눔을 가로지르는 패턴은 매칭하지 않는다.
/// 채우기는 `set_field`가 아니라 [`replace_text`](crate::replace_text)`("{{name}}", v)` 로 한다.
pub fn scan_placeholders(doc: &Document) -> Vec<PlaceholderInfo> {
    let mut out: Vec<PlaceholderInfo> = Vec::new();
    for section in &doc.sections {
        for para in &section.paragraphs {
            collect_placeholders(para, &mut out);
        }
    }
    out
}

fn collect_placeholders(para: &Paragraph, out: &mut Vec<PlaceholderInfo>) {
    let mut seg = String::new();
    for ch in &para.chars {
        match ch {
            HwpChar::Text(c) => seg.push(*c),
            _ => {
                scan_segment(&seg, out);
                seg.clear();
            }
        }
    }
    scan_segment(&seg, out);
    for ctrl in &para.controls {
        for_each_nested(ctrl, &mut |p| collect_placeholders(p, out));
    }
}

fn scan_segment(seg: &str, out: &mut Vec<PlaceholderInfo>) {
    let mut rest = seg;
    while let Some(open) = rest.find("{{") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else { break };
        let name = after[..close].trim();
        if !name.is_empty() && name.chars().all(is_name_char) {
            if let Some(p) = out.iter_mut().find(|p| p.name == name) {
                p.occurrences += 1;
            } else {
                out.push(PlaceholderInfo {
                    name: name.to_string(),
                    occurrences: 1,
                });
            }
        }
        rest = &after[close + 2..];
    }
}

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '-' | '_')
}

/// 컨트롤의 중첩 문단(표 셀·글상자 리스트)에 f를 적용한다(읽기 전용).
fn for_each_nested(ctrl: &Control, f: &mut impl FnMut(&Paragraph)) {
    match ctrl {
        Control::Table(t) => {
            for cell in &t.cells {
                for p in &cell.paragraphs {
                    f(p);
                }
            }
        }
        Control::Generic(g) => {
            for l in &g.paragraph_lists {
                for p in &l.paragraphs {
                    f(p);
                }
            }
        }
        _ => {}
    }
}

/// ExtCtrl payload(12B): 선두 4B = 역순 ctrl_id(리더가 역순으로 파싱), 나머지 0.
pub fn rev_payload(ctrl_id: &[u8; 4]) -> Vec<u8> {
    let mut p = vec![0u8; 12];
    p[0] = ctrl_id[3];
    p[1] = ctrl_id[2];
    p[2] = ctrl_id[1];
    p[3] = ctrl_id[0];
    p
}

/// CTRL_DATA Parameter Set: 필드 이름 BSTR 1개(setid0·count1·id1·BSTR).
pub fn make_field_ctrl_data(name: &str) -> Vec<u8> {
    let mut cd = vec![0u8, 0, 1, 0]; // setid=0, count=1
    cd.extend([1u8, 0, 1, 0]); // item id=1, type=BSTR(1)
    let units: Vec<u16> = name.encode_utf16().collect();
    cd.extend((units.len() as u32).to_le_bytes());
    for u in units {
        cd.extend(u.to_le_bytes());
    }
    cd
}

/// %clk 누름틀 컨트롤 레코드(이름 CTRL_DATA 포함).
fn make_field_control(name: &str) -> Control {
    Control::Generic(GenericControl {
        ctrl_id: *b"%clk",
        data: vec![0u8; 11], // 속성4 기타1 len2=0 id4
        paragraph_lists: Vec::new(),
        extras: Vec::new(),
        raw_children: vec![OpaqueRecord {
            tag: CTRL_DATA_TAG,
            data: make_field_ctrl_data(name),
            children: Vec::new(),
        }],
        gso_shapes: Vec::new(),
        equation: None,
    })
}

/// 필드 문자열: FIELD_START(ExtCtrl, 코드 3) + 표시값 + FIELD_END(InlineCtrl, 코드 4).
fn make_field_chars(value: &str) -> Vec<HwpChar> {
    let mut chars = vec![HwpChar::ExtCtrl {
        code: FIELD_START,
        ctrl_id: *b"%clk",
        payload: rev_payload(b"%clk"),
        ctrl_index: None,
    }];
    chars.extend(build_value_chars(value));
    chars.push(HwpChar::InlineCtrl {
        code: FIELD_END,
        payload: vec![0u8; 12],
    });
    chars
}

/// 문단의 ExtCtrl ↔ controls 등장순서 연결(ctrl_index)을 다시 매긴다.
pub(crate) fn relink_ctrl_index(para: &mut Paragraph) {
    let mut next = 0u32;
    for ch in &mut para.chars {
        if let HwpChar::ExtCtrl { ctrl_index, .. } = ch {
            *ctrl_index = Some(next);
            next += 1;
        }
    }
}

/// 한 문단에서 앵커 텍스트 뒤에 누름틀을 삽입한다. 반환=삽입 여부.
fn create_field_in_para(para: &mut Paragraph, anchor: &str, name: &str, value: &str) -> bool {
    let Some((cidx, wpos)) = find_match(&para.chars, anchor, 0) else {
        return false;
    };
    let ins = (cidx + anchor.chars().count()).min(para.chars.len());
    let iw = wpos + utf16_len(anchor);
    // control 삽입 위치 = ins 이전 ExtCtrl 개수(등장순서가 chars와 정합해야 함).
    let ci = para.chars[..ins]
        .iter()
        .filter(|c| matches!(c, HwpChar::ExtCtrl { .. }))
        .count()
        .min(para.controls.len());
    para.controls.insert(ci, make_field_control(name));
    let field_chars = make_field_chars(value);
    let inserted_w: u32 = field_chars.iter().map(HwpChar::wchar_width).sum();
    para.chars.splice(ins..ins, field_chars);
    adjust_runs(&mut para.char_shape_runs, iw, 0, inserted_w);
    relink_ctrl_index(para);
    para.header.ctrl_mask = 0; // writer가 chars에서 재계산(FIELD_START bit3 포함)
    para.line_segs.clear();
    true
}

/// 본문/표 셀/글상자 문단을 재귀로 훑어 첫 매칭에 누름틀을 삽입한다.
fn create_field_rec(para: &mut Paragraph, anchor: &str, name: &str, value: &str) -> bool {
    if create_field_in_para(para, anchor, name, value) {
        return true;
    }
    for ctrl in &mut para.controls {
        match ctrl {
            Control::Table(t) => {
                for cell in &mut t.cells {
                    for p in &mut cell.paragraphs {
                        if create_field_rec(p, anchor, name, value) {
                            return true;
                        }
                    }
                }
            }
            Control::Generic(g) => {
                for l in &mut g.paragraph_lists {
                    for p in &mut l.paragraphs {
                        if create_field_rec(p, anchor, name, value) {
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// `anchor` 텍스트를 가진 첫 문단의 그 뒤에 `name` 이름의 %clk 누름틀을 삽입한다
/// (표시값 `value`, 보통 빈 문자열). 반환=삽입 여부. 삽입 후 `set_field`로 채울 수 있다.
pub fn create_field(doc: &mut Document, anchor: &str, name: &str, value: &str) -> bool {
    for section in &mut doc.sections {
        for para in &mut section.paragraphs {
            if create_field_rec(para, anchor, name, value) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwp_model::opaque::OpaqueRecord;
    use hwp_model::{CharShapeId, GenericControl};

    fn wbytes(s: &str) -> Vec<u8> {
        s.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn parse_command_계산식() {
        // 속성(4) 기타(1) len(2) WCHAR[len] id(4)
        let cmd = "SUM(A1:A3)";
        let mut data = vec![0u8; 5];
        data.extend((cmd.encode_utf16().count() as u16).to_le_bytes());
        data.extend(wbytes(cmd));
        data.extend([0u8; 4]);
        assert_eq!(parse_command(&data).as_deref(), Some("SUM(A1:A3)"));
    }

    #[test]
    fn first_bstr_파라미터셋() {
        // SetID(2) count(2)=2; item1: id(2) type(2)=3(I2) + 2바이트; item2: id(2) type(2)=1(BSTR)
        let name = "수신";
        let mut data = vec![0, 0, 2, 0]; // setid=0, count=2
        data.extend([1, 0, 3, 0, 0xAA, 0xBB]); // item id=1 type=I2(3) value 2B
        data.extend([2, 0, 1, 0]); // item id=2 type=BSTR(1)
        data.extend((name.encode_utf16().count() as u32).to_le_bytes());
        data.extend(wbytes(name));
        assert_eq!(first_bstr(&data).as_deref(), Some("수신"));
    }

    /// 이름 있는 누름틀 필드 하나를 가진 합성 문단.
    fn field_para(name: &str, value: &str) -> Paragraph {
        // CTRL_DATA: Parameter Set(setid=0, count=1) + item(id=1, type=BSTR) + len + WCHAR[]
        let mut cd = vec![0, 0, 1, 0];
        cd.extend([1, 0, 1, 0]); // item id=1, type=BSTR(1)
        cd.extend((name.encode_utf16().count() as u32).to_le_bytes());
        cd.extend(wbytes(name));
        let ctrl = Control::Generic(GenericControl {
            ctrl_id: *b"%clk",
            data: vec![0u8; 11], // 속성4 기타1 len2=0 id4
            paragraph_lists: Vec::new(),
            extras: Vec::new(),
            raw_children: vec![OpaqueRecord {
                tag: CTRL_DATA_TAG,
                data: cd,
                children: Vec::new(),
            }],
            gso_shapes: Vec::new(),
            equation: None,
        });
        let mut chars = vec![HwpChar::ExtCtrl {
            code: FIELD_START,
            ctrl_id: *b"%clk",
            payload: vec![0u8; 12],
            ctrl_index: Some(0),
        }];
        chars.extend(value.chars().map(HwpChar::Text));
        chars.push(HwpChar::InlineCtrl {
            code: FIELD_END,
            payload: vec![0u8; 12],
        });
        chars.push(HwpChar::CharCtrl(13));
        Paragraph {
            chars,
            char_shape_runs: vec![(0, CharShapeId(0))],
            controls: vec![ctrl],
            ..Default::default()
        }
    }

    #[test]
    fn list_fields_이름_종류_값() {
        let mut doc = Document::default();
        doc.sections.push(hwp_model::Section {
            paragraphs: vec![field_para("수신", "내부결재")],
            extras: Vec::new(),
        });
        let fields = list_fields(&doc);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].kind, "누름틀");
        assert_eq!(fields[0].name.as_deref(), Some("수신"));
        assert_eq!(fields[0].value, "내부결재");
    }

    #[test]
    fn scan_placeholders_이름_횟수() {
        let doc = crate::from_markdown::from_markdown("{{기관명}} 안녕 {{제목}} 끝 {{기관명}}\n");
        let slots = scan_placeholders(&doc);
        let map: std::collections::HashMap<_, _> = slots
            .iter()
            .map(|p| (p.name.as_str(), p.occurrences))
            .collect();
        assert_eq!(map.get("기관명"), Some(&2));
        assert_eq!(map.get("제목"), Some(&1));
        assert_eq!(slots.len(), 2);
    }

    #[test]
    fn set_field_값_교체() {
        let mut doc = Document::default();
        doc.sections.push(hwp_model::Section {
            paragraphs: vec![field_para("수신", "")], // 빈 누름틀
            extras: Vec::new(),
        });
        let n = set_field(&mut doc, "수신", "기획팀");
        assert_eq!(n, 1);
        let fields = list_fields(&doc);
        assert_eq!(fields[0].value, "기획팀");
        // 다른 이름은 안 바뀜
        assert_eq!(set_field(&mut doc, "없는이름", "x"), 0);
    }

    #[test]
    fn create_field_삽입_후_읽기_채우기() {
        let mut doc = crate::from_markdown::from_markdown("수신: 부서");
        // 앵커 "수신:" 뒤에 누름틀 삽입.
        assert!(create_field(&mut doc, "수신:", "수신처", ""));
        let fields = list_fields(&doc);
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].kind, "누름틀");
        assert_eq!(fields[0].ctrl_id, "%clk");
        assert_eq!(fields[0].name.as_deref(), Some("수신처"));
        assert_eq!(fields[0].value, "");
        // 생성한 누름틀을 이름으로 채우기.
        assert_eq!(set_field(&mut doc, "수신처", "홍길동"), 1);
        assert_eq!(list_fields(&doc)[0].value, "홍길동");
        // 없는 앵커는 삽입 안 됨.
        assert!(!create_field(&mut doc, "없는앵커", "x", ""));
    }

    #[test]
    fn create_field_ctrl_index_정합() {
        // 삽입된 ExtCtrl이 controls[ctrl_index]와 등장순서로 맞물리는지.
        let mut doc = crate::from_markdown::from_markdown("가나다");
        assert!(create_field(&mut doc, "가", "필드", "값"));
        let para = &doc.sections[0].paragraphs[0];
        // ExtCtrl의 ctrl_index가 %clk 컨트롤을 가리킨다.
        let ext = para
            .chars
            .iter()
            .find_map(|c| match c {
                HwpChar::ExtCtrl {
                    code, ctrl_index, ..
                } if *code == FIELD_START => *ctrl_index,
                _ => None,
            })
            .expect("ExtCtrl 존재");
        match &para.controls[ext as usize] {
            Control::Generic(g) => assert_eq!(&g.ctrl_id, b"%clk"),
            _ => panic!("필드 컨트롤이 %clk Generic이어야"),
        }
        // 필드 ExtCtrl payload 선두 4B = 역순 ctrl_id("%clk"→"klc%").
        // (문단 선두 secd ExtCtrl과 구분하려 code로 필터.)
        let payload = para.chars.iter().find_map(|c| match c {
            HwpChar::ExtCtrl { code, payload, .. } if *code == FIELD_START => Some(payload.clone()),
            _ => None,
        });
        assert_eq!(&payload.unwrap()[..4], b"klc%");
    }
}

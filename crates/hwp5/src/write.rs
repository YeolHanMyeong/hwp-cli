//! IR → HWP 5.0 파일 쓰기.
//!
//! 모든 방출 함수는 doc_info/body_text 파서와 **거울 대칭**이다 —
//! "알려진 prefix + tail" 규칙 덕에 hwp5에서 읽은 문서는 바이트
//! 수준으로 복원된다(단순 컨트롤 한정; gso 등 평탄화된 구조는 의미
//! 수준). ID_MAPPINGS 카운트는 테이블 길이에서 유도한다(수동 동기화
//! 금지) — 원본에 버전별 추가 카운트가 있으면 꼬리만 보존.

use std::io::Write as _;
use std::path::Path;

use hwp_model::{
    BorderLine, Cell, CharShape, Control, Document, FaceName, HwpChar, LANG_COUNT, OpaqueRecord,
    ParaShape, Paragraph, Picture, Section, SectionDef, Style, Table,
};

use crate::codec::{ByteWriter, compress};
use crate::error::Result;
use crate::file_header::{FILE_HEADER_SIZE, FileHeader, HwpVersion};
use crate::record::{RecordNode, tag};

#[derive(Default)]
pub struct WriteOptions {
    /// PrvImage 스트림 내용 (PNG 권장 — 없으면 스트림 생략)
    pub prv_image: Option<Vec<u8>>,
    /// 줄 배치(PARA_LINE_SEG) 보존 여부.
    ///
    /// 한글은 줄 배치 캐시가 내용과 정합하지 않으면 "변조" 보안 경고를
    /// 띄운다 (한컴 공식: 내용 수정 시 제거 권장). 기본 false — 한글이
    /// 열 때 재계산한다. 무수정 바이트 왕복에만 true를 쓸 것.
    pub preserve_linesegs: bool,
}

/// 문서를 HWP 5.0 파일로 저장한다. 경고(평탄화/드롭) 목록을 반환한다.
pub fn write_document(doc: &Document, path: &Path, opts: &WriteOptions) -> Result<Vec<String>> {
    let mut warnings = Vec::new();

    // hwpx 출신 문서 정규화: hwp5 레코드(SHAPE_COMPONENT)가 없는 그림은
    // 쓸 수 없으므로 컨트롤과 확장 문자를 동기 제거한다
    let normalized;
    let doc = if needs_normalize(doc) {
        let mut d = doc.clone();
        for section in &mut d.sections {
            for para in &mut section.paragraphs {
                strip_unwritable_pictures(para, &mut warnings);
            }
        }
        normalized = d;
        &normalized
    } else {
        doc
    };

    // 레코드 스트림 구성
    let doc_info_nodes = emit_doc_info(doc, &mut warnings);
    let doc_info = RecordNode::serialize_forest(&doc_info_nodes);
    let sections: Vec<Vec<u8>> = doc
        .sections
        .iter()
        .map(|s| {
            RecordNode::serialize_forest(&emit_section(s, opts.preserve_linesegs, &mut warnings))
        })
        .collect();

    // FileHeader
    let header = FileHeader {
        version: parse_version(&doc.meta.source_version),
        attributes: 0x1, // 압축
        license: 0,
        // 비암호 문서라도 현대 한글(2010+)은 EncryptVersion=4(글 7.0+)를
        // 무조건 쓴다. fixtures/hwp5 표본 6개 전부 attr1 암호화 bit(=bit1)는
        // 0인데 encver=4. 0을 쓰면 한글이 '손상/변조'로 거부한다(실기 게이트).
        encrypt_version: 4,
        kogl_country: 0,
        reserved: [0u8; FILE_HEADER_SIZE - 49],
    };

    // 미리보기 텍스트 (UTF-16LE, 약 1000자)
    let mut preview = doc.plain_text();
    preview.truncate(
        preview
            .char_indices()
            .nth(1000)
            .map_or(preview.len(), |(i, _)| i),
    );
    let prv_text: Vec<u8> = preview.encode_utf16().flat_map(u16::to_le_bytes).collect();

    // CFB 조립 — 반드시 버전 3 (512B 섹터): 한글은 V4(4096B)를
    // "손상된 파일"로 판정한다 (실기 게이트 실측)
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;
    let mut cfb = cfb::CompoundFile::create_with_version(cfb::Version::V3, file)?;
    cfb.create_new_stream("/FileHeader")?
        .write_all(&header.serialize())?;
    cfb.create_new_stream("/DocInfo")?
        .write_all(&compress(&doc_info))?;
    cfb.create_storage("/BodyText")?;
    for (i, body) in sections.iter().enumerate() {
        cfb.create_new_stream(format!("/BodyText/Section{i}"))?
            .write_all(&compress(body))?;
    }
    // BIN_DATA 테이블이 참조하는 스트림만 동봉 (hwp5 명명 규칙)
    let referenced: Vec<String> = doc
        .header
        .bin_data
        .iter()
        .filter_map(|item| {
            let id = item.storage_id?;
            let ext = item.extension.as_deref().unwrap_or("");
            Some(format!("BIN{id:04X}.{ext}"))
        })
        .collect();
    let mut bin_written = 0usize;
    if !referenced.is_empty() && !doc.bin_streams.is_empty() {
        cfb.create_storage("/BinData")?;
        for bin in &doc.bin_streams {
            let base = bin.name.rsplit('/').next().unwrap_or(&bin.name);
            if referenced.iter().any(|r| r.eq_ignore_ascii_case(base)) {
                cfb.create_new_stream(format!("/BinData/{base}"))?
                    .write_all(&compress(&bin.data))?;
                bin_written += 1;
            }
        }
    }
    if bin_written < doc.bin_streams.len() {
        warnings.push(format!(
            "BinData {}개 중 {}개만 동봉 (hwp5 BIN_DATA 테이블이 참조하는 항목만)",
            doc.bin_streams.len(),
            bin_written
        ));
    }
    // 보조 스트림: 한글 저장 파일에 항상 존재 (부재 시 손상 판정 위험)
    cfb.create_storage("/DocOptions")?;
    cfb.create_new_stream("/DocOptions/_LinkDoc")?
        .write_all(&[0u8; 524])?;
    cfb.create_storage("/Scripts")?;
    // 표본(한글 빈 문서) 원시 바이트 그대로 — 해제 시 버전 마커/빈 스크립트
    cfb.create_new_stream("/Scripts/JScriptVersion")?
        .write_all(&[
            0x63, 0x64, 0x80, 0x00, 0x00, 0xF7, 0xDF, 0x88, 0xA9, 0x08, 0x00, 0x00, 0x00,
        ])?;
    cfb.create_new_stream("/Scripts/DefaultJScript")?
        .write_all(&[
            0x63, 0x60, 0x40, 0x05, 0xFF, 0x81, 0x00, 0x00, 0x6E, 0xBB, 0x6E, 0xD1, 0x14, 0x00,
            0x00, 0x00,
        ])?;
    // 요약 정보 (표본과 동일한 14개 속성 구조, 값은 비움)
    cfb.create_new_stream("/\u{5}HwpSummaryInformation")?
        .write_all(&hwp_summary_information())?;
    cfb.create_new_stream("/PrvText")?.write_all(&prv_text)?;
    if let Some(img) = &opts.prv_image {
        cfb.create_new_stream("/PrvImage")?.write_all(img)?;
    }
    cfb.flush()?;
    Ok(warnings)
}

/// hwp5로 쓸 수 없는 그림(SHAPE_COMPONENT 레코드 부재)이 있는지.
fn needs_normalize(doc: &Document) -> bool {
    fn para_has(para: &Paragraph) -> bool {
        para.controls.iter().any(|c| match c {
            Control::Picture(p) => p.extras.is_empty(),
            Control::Table(t) => t.cells.iter().flat_map(|c| &c.paragraphs).any(para_has),
            // hwp5 출신(raw_children 보존)은 원본 트리를 그대로 방출하므로
            // 정규화 불필요. raw_children가 없는데 data도 없는 컨트롤만
            // (hwpx/md 출신 합성 불가) 드롭 대상.
            Control::Generic(g) => {
                (g.data.is_empty() && g.ctrl_id != *b"cold" && g.raw_children.is_empty())
                    || g.paragraph_lists
                        .iter()
                        .flat_map(|l| &l.paragraphs)
                        .any(para_has)
            }
            _ => false,
        })
    }
    doc.sections
        .iter()
        .flat_map(|s| &s.paragraphs)
        .any(para_has)
}

/// hwp5 레코드가 없는 그림 컨트롤을 확장 문자와 동기 제거하고
/// 남은 ExtCtrl의 ctrl_index를 재조정한다 (중첩 구조 재귀).
fn strip_unwritable_pictures(para: &mut Paragraph, warnings: &mut Vec<String>) {
    let mut removed: Vec<u32> = Vec::new();
    let mut kept = Vec::with_capacity(para.controls.len());
    for (i, mut control) in std::mem::take(&mut para.controls).into_iter().enumerate() {
        match &mut control {
            Control::Picture(p) if p.extras.is_empty() => {
                warnings.push("hwp5 그림 레코드가 없는 이미지를 생략 (hwpx 출신)".to_string());
                removed.push(i as u32);
                continue;
            }
            // hwp5 페이로드를 합성할 수 없는 컨트롤(hwpx/md 출신 머리말/자동번호
            // 등)만 생략. raw_children가 있으면 hwp5 원본이므로 보존.
            Control::Generic(g)
                if g.data.is_empty() && g.ctrl_id != *b"cold" && g.raw_children.is_empty() =>
            {
                warnings.push(format!(
                    "hwp5 페이로드가 없는 {:?} 컨트롤을 생략 (hwpx 출신)",
                    String::from_utf8_lossy(&g.ctrl_id)
                ));
                removed.push(i as u32);
                continue;
            }
            Control::Table(t) => {
                for cell in &mut t.cells {
                    for cp in &mut cell.paragraphs {
                        strip_unwritable_pictures(cp, warnings);
                    }
                }
            }
            Control::Generic(g) => {
                for list in &mut g.paragraph_lists {
                    for lp in &mut list.paragraphs {
                        strip_unwritable_pictures(lp, warnings);
                    }
                }
            }
            _ => {}
        }
        kept.push(control);
    }
    para.controls = kept;
    if removed.is_empty() {
        return;
    }
    para.chars.retain(|ch| match ch {
        HwpChar::ExtCtrl {
            ctrl_index: Some(i),
            ..
        } => !removed.contains(i),
        _ => true,
    });
    for ch in &mut para.chars {
        if let HwpChar::ExtCtrl {
            ctrl_index: Some(i),
            ..
        } = ch
        {
            let shift = removed.iter().filter(|r| **r < *i).count() as u32;
            *i -= shift;
        }
    }
}

/// 한글 빈 문서 표본의 구역 정의 페이로드 (43B — 값 재현, 임베드 아님).
const DEFAULT_SECD_DATA: [u8; 43] = [
    0x00, 0x00, 0x00, 0x00, 0x6E, 0x04, 0x00, 0x00, 0x00, 0x00, 0x40, 0x1F, 0x00, 0x00, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// 한글 빈 문서 표본의 단 정의 페이로드 (12B).
const DEFAULT_COLD_DATA: [u8; 12] = [
    0x04, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

/// 최소 유효 `\x05HwpSummaryInformation` (OLE 속성 집합, 속성 0개).
/// 헤더 상수는 한글 저장 표본 실측값 — FMTID 9FA2B660-1061-11D4-B4C6-006097C09D8C.
fn hwp_summary_information() -> Vec<u8> {
    const HWP_FMTID: [u8; 16] = [
        0x60, 0xB6, 0xA2, 0x9F, 0x61, 0x10, 0xD4, 0x11, 0xB4, 0xC6, 0x00, 0x60, 0x97, 0xC0, 0x9D,
        0x8C,
    ];
    // 표본(한글 빈 문서)과 동일한 PID 순서/타입 — 값은 비움
    enum Val<'a> {
        Str(&'a str),
        FileTime,
        I4,
        Null,
    }
    let props: [(u32, Val); 14] = [
        (0x02, Val::Str("")),        // 제목
        (0x03, Val::Str("")),        // 주제
        (0x04, Val::Str("")),        // 지은이
        (0x14, Val::Str("")),        // 날짜 문자열
        (0x05, Val::Str("")),        // 키워드
        (0x06, Val::Str("")),        // 설명
        (0x08, Val::Str("")),        // 마지막 저장자
        (0x09, Val::Str("hwp-cli")), // 프로그램
        (0x0C, Val::FileTime),
        (0x0D, Val::FileTime),
        (0x0B, Val::FileTime),
        (0x0E, Val::I4),
        (0x15, Val::I4),
        (0x00, Val::Null),
    ];

    // 섹션 본문: ID/오프셋 표 + 값들
    let mut values = ByteWriter::new();
    let table_size = 8 + props.len() * 8;
    let mut offsets = Vec::with_capacity(props.len());
    for (_, val) in &props {
        offsets.push(table_size + values.len());
        match val {
            Val::Str(text) => {
                values.write_u32(31); // VT_LPWSTR
                let units: Vec<u16> = text.encode_utf16().chain([0]).collect();
                values.write_u32(units.len() as u32);
                for u in &units {
                    values.write_u16(*u);
                }
                while !values.len().is_multiple_of(4) {
                    values.write_u8(0);
                }
            }
            Val::FileTime => {
                values.write_u32(64); // VT_FILETIME
                values.write_u32(0);
                values.write_u32(0);
            }
            Val::I4 => {
                values.write_u32(3); // VT_I4
                values.write_u32(0);
            }
            Val::Null => values.write_u32(1), // VT_NULL
        }
    }
    let values = values.into_bytes();
    let section_size = table_size + values.len();

    let mut w = ByteWriter::new();
    w.write_u16(0xFFFE); // byte order
    w.write_u16(0); // format
    w.write_u32(0x0002_0400); // OS version (표본값)
    w.write_bytes(&HWP_FMTID); // CLSID
    w.write_u32(1); // 섹션 수
    w.write_bytes(&HWP_FMTID); // FMTID
    w.write_u32(48); // 섹션 오프셋
    w.write_u32(section_size as u32);
    w.write_u32(props.len() as u32);
    for ((pid, _), off) in props.iter().zip(&offsets) {
        w.write_u32(*pid);
        w.write_u32(*off as u32);
    }
    w.write_bytes(&values);
    w.into_bytes()
}

fn parse_version(s: &str) -> HwpVersion {
    let mut parts = s.split('.').filter_map(|p| p.parse::<u8>().ok());
    let (a, b, c, d) = (parts.next(), parts.next(), parts.next(), parts.next());
    match (a, b, c, d) {
        (Some(major), Some(minor), Some(build), Some(revision)) => HwpVersion {
            major,
            minor,
            build,
            revision,
        },
        _ => HwpVersion {
            major: 5,
            minor: 1,
            build: 0,
            revision: 1,
        },
    }
}

fn opaque_to_node(o: &OpaqueRecord) -> RecordNode {
    RecordNode {
        tag: o.tag,
        data: o.data.clone(),
        children: o.children.iter().map(opaque_to_node).collect(),
    }
}

fn hwp_string(w: &mut ByteWriter, s: &str) {
    let units: Vec<u16> = s.encode_utf16().collect();
    w.write_u16(units.len() as u16);
    for u in units {
        w.write_u16(u);
    }
}

// ─────────────────────────── DocInfo ───────────────────────────

fn emit_doc_info(doc: &Document, _warnings: &mut Vec<String>) -> Vec<RecordNode> {
    let h = &doc.header;
    let mut roots = Vec::new();

    // DOCUMENT_PROPERTIES — 구역 수는 실제 섹션 수에서 유도
    let mut w = ByteWriter::new();
    w.write_u16(doc.sections.len().max(1) as u16);
    for n in h.properties.start_numbers {
        w.write_u16(n);
    }
    w.write_u32(h.properties.caret.0);
    w.write_u32(h.properties.caret.1);
    w.write_u32(h.properties.caret.2);
    roots.push(RecordNode {
        tag: tag::DOCUMENT_PROPERTIES,
        data: w.into_bytes(),
        children: Vec::new(),
    });

    // ID_MAPPINGS — 카운트는 테이블 길이에서 유도, 원본의 추가 꼬리는 보존
    let mut counts: Vec<u32> = Vec::with_capacity(18);
    counts.push(h.bin_data.len() as u32);
    for slot in 0..LANG_COUNT {
        counts.push(h.fonts[slot].len() as u32);
    }
    counts.push(h.border_fills.len() as u32);
    counts.push(h.char_shapes.len() as u32);
    counts.push(h.tab_defs.len() as u32);
    counts.push(h.numberings.len() as u32);
    counts.push(h.bullets.len() as u32);
    counts.push(h.para_shapes.len() as u32);
    counts.push(h.styles.len() as u32);
    if h.id_mappings_counts.len() > counts.len() {
        counts.extend_from_slice(&h.id_mappings_counts[counts.len()..]);
    }
    // ID_MAPPINGS 카운트 개수는 선언 버전과 일치해야 한다(스펙 표 16, 표 15
    // "doc version 에 따라 가변적"). 인덱스 15=메모모양(5.0.2.1+),
    // 16·17=변경추적(5.0.3.2+). 파생 기본은 15개(인덱스 0~14, 스타일까지).
    //   - 원본을 왕복할 때는 h.id_mappings_counts 길이를 그대로 보존한다
    //     (실제 한컴 저장본은 이미 버전에 맞는 길이라서).
    //   - 합성 문서(원본 카운트 없음)는 선언 버전으로 목표 길이를 정한다.
    // 무조건 18 패딩을 하면 5.0.2.x 문서(16개)를 18개로 부풀려 버전-레이아웃이
    // 어긋나고 한글이 '손상'으로 거부한다(work_report 5.0.2.4 실증).
    let ver = parse_version(&doc.meta.source_version);
    let version_target = if ver.to_u32() >= 0x05_00_03_02 {
        18 // 5.0.3.2 이상: 메모 + 변경추적 ×2
    } else if ver.to_u32() >= 0x05_00_02_01 {
        16 // 5.0.2.1 이상: 메모 모양
    } else {
        15 // 그 이전: 스타일까지
    };
    let target = h
        .id_mappings_counts
        .len()
        .max(version_target)
        .max(counts.len());
    while counts.len() < target {
        counts.push(0);
    }
    let mut w = ByteWriter::new();
    for c in &counts {
        w.write_u32(*c);
    }

    let mut children = Vec::new();
    for item in &h.bin_data {
        children.push(emit_bin_data(item));
    }
    for slot in 0..LANG_COUNT {
        for f in &h.fonts[slot] {
            children.push(emit_face_name(f));
        }
    }
    for bf in &h.border_fills {
        children.push(emit_border_fill(bf));
    }
    for cs in &h.char_shapes {
        children.push(emit_char_shape(cs));
    }
    for t in &h.tab_defs {
        children.push(RecordNode {
            tag: tag::TAB_DEF,
            data: t.data.clone(),
            children: t.children.iter().map(opaque_to_node).collect(),
        });
    }
    for n in &h.numberings {
        children.push(RecordNode {
            tag: tag::NUMBERING,
            data: n.data.clone(),
            children: n.children.iter().map(opaque_to_node).collect(),
        });
    }
    for b in &h.bullets {
        children.push(RecordNode {
            tag: tag::BULLET,
            data: b.data.clone(),
            children: b.children.iter().map(opaque_to_node).collect(),
        });
    }
    for ps in &h.para_shapes {
        children.push(emit_para_shape(ps));
    }
    for st in &h.styles {
        children.push(emit_style(st));
    }
    children.extend(h.id_extras.iter().map(opaque_to_node));

    roots.push(RecordNode {
        tag: tag::ID_MAPPINGS,
        data: w.into_bytes(),
        children,
    });
    roots.extend(h.extras.iter().map(opaque_to_node));
    roots
}

fn emit_bin_data(item: &hwp_model::BinDataItem) -> RecordNode {
    let mut w = ByteWriter::new();
    w.write_u16(item.attr);
    if item.kind() == 0 {
        hwp_string(&mut w, item.link_abs.as_deref().unwrap_or(""));
        hwp_string(&mut w, item.link_rel.as_deref().unwrap_or(""));
    } else {
        w.write_u16(item.storage_id.unwrap_or(0));
        if item.kind() == 1 {
            hwp_string(&mut w, item.extension.as_deref().unwrap_or(""));
        }
    }
    w.write_bytes(&item.tail);
    RecordNode {
        tag: tag::BIN_DATA,
        data: w.into_bytes(),
        children: Vec::new(),
    }
}

fn emit_face_name(f: &FaceName) -> RecordNode {
    let mut attr = f.attr;
    if f.alt_name.is_some() {
        attr |= 0x80;
    }
    if f.panose.is_some() {
        attr |= 0x40;
    }
    if f.default_name.is_some() {
        attr |= 0x20;
    }
    let mut w = ByteWriter::new();
    w.write_u8(attr);
    hwp_string(&mut w, &f.name);
    if let Some(alt) = &f.alt_name {
        w.write_u8(f.alt_kind.unwrap_or(0));
        hwp_string(&mut w, alt);
    }
    if let Some(p) = &f.panose {
        w.write_bytes(p);
    }
    if let Some(d) = &f.default_name {
        hwp_string(&mut w, d);
    }
    w.write_bytes(&f.tail);
    RecordNode {
        tag: tag::FACE_NAME,
        data: w.into_bytes(),
        children: Vec::new(),
    }
}

fn write_border_line(w: &mut ByteWriter, line: &BorderLine) {
    w.write_u8(line.line_type);
    w.write_u8(line.width);
    w.write_u32(line.color);
}

fn emit_border_fill(bf: &hwp_model::BorderFill) -> RecordNode {
    let mut w = ByteWriter::new();
    w.write_u16(bf.attr);
    for side in &bf.sides {
        write_border_line(&mut w, side);
    }
    write_border_line(&mut w, &bf.diagonal);
    w.write_u32(bf.fill_type);
    if bf.fill_type & 0x1 != 0 {
        w.write_u32(bf.bg_color.unwrap_or(0xFFFF_FFFF));
    }
    if bf.tail.is_empty() {
        // hwpx/md 출신: hwp5 채우기 블록 완성 (표본 40B/53B 레이아웃 역산)
        if bf.fill_type & 0x1 != 0 {
            w.write_u32(0); // 무늬 색
            w.write_u32(0xFFFF_FFFF); // 무늬 종류 (-1 = 없음)
        }
        w.write_u32(0); // 추가 채우기 속성 크기
        if bf.fill_type & 0x1 != 0 {
            w.write_u8(0); // 투명도
        }
    } else {
        // hwp5 왕복: tail이 무늬색 이후 전부를 담고 있다
        w.write_bytes(&bf.tail);
    }
    RecordNode {
        tag: tag::BORDER_FILL,
        data: w.into_bytes(),
        children: Vec::new(),
    }
}

fn emit_char_shape(cs: &CharShape) -> RecordNode {
    let mut w = ByteWriter::new();
    for id in cs.face_ids {
        w.write_u16(id);
    }
    for v in cs.ratios {
        w.write_u8(v);
    }
    for v in cs.spacings {
        w.write_u8(v as u8);
    }
    for v in cs.rel_sizes {
        w.write_u8(v);
    }
    for v in cs.offsets {
        w.write_u8(v as u8);
    }
    w.write_i32(cs.base_size);
    w.write_u32(cs.attr);
    w.write_u8(cs.shadow_gap.0 as u8);
    w.write_u8(cs.shadow_gap.1 as u8);
    w.write_u32(cs.text_color);
    w.write_u32(cs.underline_color);
    w.write_u32(cs.shade_color);
    w.write_u32(cs.shadow_color);
    if cs.tail.is_empty() {
        // hwpx/md 출신: 5.1.x 규격 충전 (테두리채움 ID 5.0.2.1+, 취소선 색 5.0.3.0+)
        w.write_u16(cs.border_fill_id.max(2));
        w.write_u32(0); // 취소선 색
    } else {
        // hwp5 왕복: border_fill_id는 tail 선두에서 추출만 했으므로 그대로 담겨 있다
        w.write_bytes(&cs.tail);
    }
    RecordNode {
        tag: tag::CHAR_SHAPE,
        data: w.into_bytes(),
        children: Vec::new(),
    }
}

fn emit_para_shape(ps: &ParaShape) -> RecordNode {
    let mut w = ByteWriter::new();
    w.write_u32(ps.attr1);
    w.write_i32(ps.margin_left);
    w.write_i32(ps.margin_right);
    w.write_i32(ps.indent);
    w.write_i32(ps.spacing_top);
    w.write_i32(ps.spacing_bottom);
    w.write_i32(ps.line_spacing_old);
    w.write_u16(ps.tab_def_id);
    w.write_u16(ps.numbering_id);
    w.write_u16(ps.border_fill_id);
    for v in ps.border_offsets {
        w.write_u16(v as u16);
    }
    if ps.tail.is_empty() {
        // hwpx/md 출신: 5.0.2.5+ 필수 필드 충전 (속성2/속성3/줄간격)
        w.write_u32(0);
        w.write_u32(0);
        w.write_u32(if ps.line_spacing > 0 {
            ps.line_spacing as u32
        } else {
            160
        });
    } else {
        w.write_bytes(&ps.tail);
    }
    RecordNode {
        tag: tag::PARA_SHAPE,
        data: w.into_bytes(),
        children: Vec::new(),
    }
}

fn emit_style(st: &Style) -> RecordNode {
    let mut w = ByteWriter::new();
    hwp_string(&mut w, &st.name);
    hwp_string(&mut w, &st.english_name);
    w.write_u8(st.attr);
    w.write_u8(st.next_style);
    w.write_u16(st.lang_id as u16);
    w.write_u16(st.para_shape.0);
    w.write_u16(st.char_shape.0);
    if st.tail.is_empty() {
        w.write_u16(0); // 잠금 등 후행 2바이트 (표본 실측)
    } else {
        w.write_bytes(&st.tail);
    }
    RecordNode {
        tag: tag::STYLE,
        data: w.into_bytes(),
        children: Vec::new(),
    }
}

// ─────────────────────────── BodyText ───────────────────────────

fn emit_section(
    section: &Section,
    preserve_linesegs: bool,
    warnings: &mut Vec<String>,
) -> Vec<RecordNode> {
    let mut roots: Vec<RecordNode> = section
        .paragraphs
        .iter()
        .map(|p| emit_paragraph(p, preserve_linesegs, warnings))
        .collect();
    roots.extend(section.extras.iter().map(opaque_to_node));
    roots
}

fn emit_paragraph(
    para: &Paragraph,
    preserve_linesegs: bool,
    warnings: &mut Vec<String>,
) -> RecordNode {
    // PARA_HEADER
    let mut w = ByteWriter::new();
    // 빈 문단도 한글은 글자 수를 1로 기록한다(암묵적 문단끝, PARA_TEXT는
    // 생략). 표본 실측: 빈 셀 문단 60개 전부 nchars 하위=1. 0을 쓰면
    // 한글이 손상으로 판정.
    let char_count = if para.chars.is_empty() {
        1
    } else {
        para.wchar_len()
    };
    let nchars = char_count | (u32::from(para.header.chars_flags) << 24);
    w.write_u32(nchars);
    // ctrl_mask는 확장/인라인 컨트롤만 표시한다. 문자형 컨트롤(문단끝 13,
    // 줄나눔 10, 하이픈 등)은 포함하지 않는다 — 표본 실측: 텍스트만 있고
    // 문단끝(13)으로 닫히는 문단의 원본 ctrl_mask=0. CharCtrl까지 비트를
    // 켜면 한글이 'ctrl_mask에 있다는 컨트롤이 실제로 없다'고 손상 판정.
    let ctrl_mask = if para.header.ctrl_mask != 0 {
        para.header.ctrl_mask
    } else {
        para.chars
            .iter()
            .filter_map(|c| match c {
                HwpChar::InlineCtrl { code, .. } | HwpChar::ExtCtrl { code, .. } => {
                    Some(1u32 << code)
                }
                _ => None,
            })
            .fold(0, |a, b| a | b)
    };
    w.write_u32(ctrl_mask);
    w.write_u16(para.para_shape.0);
    w.write_u8(para.style.0 as u8);
    w.write_u8(para.header.break_type);
    w.write_u16(para.char_shape_runs.len() as u16);
    let range_tags = para
        .extras
        .iter()
        .filter(|e| e.tag == tag::PARA_RANGE_TAG)
        .count() as u16;
    w.write_u16(range_tags);
    let seg_count = if preserve_linesegs {
        para.line_segs.len()
    } else {
        0
    };
    w.write_u16(seg_count as u16);
    w.write_u32(para.header.instance_id);
    w.write_bytes(&para.header.tail);

    let mut children = Vec::new();
    if !para.chars.is_empty() {
        children.push(RecordNode {
            tag: tag::PARA_TEXT,
            data: emit_para_text(&para.chars),
            children: Vec::new(),
        });
    }
    if !para.char_shape_runs.is_empty() {
        let mut cw = ByteWriter::new();
        for (pos, id) in &para.char_shape_runs {
            cw.write_u32(*pos);
            cw.write_u32(u32::from(id.0));
        }
        children.push(RecordNode {
            tag: tag::PARA_CHAR_SHAPE,
            data: cw.into_bytes(),
            children: Vec::new(),
        });
    }
    if preserve_linesegs && !para.line_segs.is_empty() {
        let mut lw = ByteWriter::new();
        for seg in &para.line_segs {
            lw.write_u32(seg.text_start);
            lw.write_i32(seg.v_pos);
            lw.write_i32(seg.line_height);
            lw.write_i32(seg.text_height);
            lw.write_i32(seg.baseline_gap);
            lw.write_i32(seg.line_spacing);
            lw.write_i32(seg.col_start);
            lw.write_i32(seg.seg_width);
            lw.write_u32(seg.flags);
        }
        children.push(RecordNode {
            tag: tag::PARA_LINE_SEG,
            data: lw.into_bytes(),
            children: Vec::new(),
        });
    }
    children.extend(para.extras.iter().map(opaque_to_node));
    for control in &para.controls {
        children.push(emit_control(control, preserve_linesegs, warnings));
    }

    RecordNode {
        tag: tag::PARA_HEADER,
        data: w.into_bytes(),
        children,
    }
}

fn emit_para_text(chars: &[HwpChar]) -> Vec<u8> {
    let mut w = ByteWriter::new();
    for ch in chars {
        match ch {
            HwpChar::Text(c) => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    w.write_u16(*u);
                }
            }
            HwpChar::CharCtrl(code) => w.write_u16(*code),
            HwpChar::InlineCtrl { code, payload } | HwpChar::ExtCtrl { code, payload, .. } => {
                w.write_u16(*code);
                let mut p = payload.clone();
                p.resize(12, 0);
                w.write_bytes(&p);
                w.write_u16(*code);
            }
        }
    }
    w.into_bytes()
}

fn reversed(id: [u8; 4]) -> [u8; 4] {
    let mut r = id;
    r.reverse();
    r
}

fn emit_control(
    control: &Control,
    preserve_linesegs: bool,
    warnings: &mut Vec<String>,
) -> RecordNode {
    match control {
        Control::SectionDef(def) => emit_section_def(def),
        Control::Table(table) => emit_table(table, preserve_linesegs),
        Control::Picture(pic) => emit_picture(pic, warnings),
        Control::Generic(g) => {
            let mut w = ByteWriter::new();
            w.write_bytes(&reversed(g.ctrl_id));
            if g.data.is_empty() && g.ctrl_id == *b"cold" {
                w.write_bytes(&DEFAULT_COLD_DATA);
            } else {
                w.write_bytes(&g.data);
            }
            // 원본 hwp5 자식 서브트리가 있으면 중첩 그대로 방출(무손실).
            // paragraph_lists/extras는 텍스트 추출 전용이므로 평탄화하지 않는다.
            if !g.raw_children.is_empty() {
                return RecordNode {
                    tag: tag::CTRL_HEADER,
                    data: w.into_bytes(),
                    children: g.raw_children.iter().map(opaque_to_node).collect(),
                };
            }
            let mut children = Vec::new();
            for list in &g.paragraph_lists {
                children.push(RecordNode {
                    tag: tag::LIST_HEADER,
                    data: list.header_data.clone(),
                    children: Vec::new(),
                });
                for p in &list.paragraphs {
                    children.push(emit_paragraph(p, preserve_linesegs, warnings));
                }
            }
            if !g.extras.is_empty() && !g.paragraph_lists.is_empty() {
                warnings.push(format!(
                    "{:?} 컨트롤 내부 구조가 평탄화되어 저장됨 — 한글에서 확인 필요",
                    String::from_utf8_lossy(&g.ctrl_id)
                ));
            }
            children.extend(g.extras.iter().map(opaque_to_node));
            RecordNode {
                tag: tag::CTRL_HEADER,
                data: w.into_bytes(),
                children,
            }
        }
    }
}

fn emit_section_def(def: &SectionDef) -> RecordNode {
    let mut w = ByteWriter::new();
    w.write_bytes(b"dces");
    if def.data.is_empty() {
        w.write_bytes(&DEFAULT_SECD_DATA);
    } else {
        w.write_bytes(&def.data);
    }
    let mut children = Vec::new();
    if let Some(p) = &def.page {
        let mut pw = ByteWriter::new();
        pw.write_i32(p.width.0);
        pw.write_i32(p.height.0);
        pw.write_i32(p.margin_left.0);
        pw.write_i32(p.margin_right.0);
        pw.write_i32(p.margin_top.0);
        pw.write_i32(p.margin_bottom.0);
        pw.write_i32(p.margin_header.0);
        pw.write_i32(p.margin_footer.0);
        pw.write_i32(p.gutter.0);
        pw.write_u32(p.attr);
        children.push(RecordNode {
            tag: tag::PAGE_DEF,
            data: pw.into_bytes(),
            children: Vec::new(),
        });
    }
    children.extend(def.extras.iter().map(opaque_to_node));
    RecordNode {
        tag: tag::CTRL_HEADER,
        data: w.into_bytes(),
        children,
    }
}

fn emit_table(table: &Table, preserve_linesegs: bool) -> RecordNode {
    let mut w = ByteWriter::new();
    w.write_bytes(b" lbt");
    if table.common_data.is_empty() {
        // hwpx/md 출신: 개체 공통 속성 합성 (표본 속성값 + 계산된 크기)
        let mut col_w = vec![0i64; table.cols.max(1) as usize];
        let mut row_h = vec![0i64; table.rows.max(1) as usize];
        for cell in &table.cells {
            let (c, r) = (cell.col as usize, cell.row as usize);
            if cell.col_span == 1 && c < col_w.len() {
                col_w[c] = col_w[c].max(i64::from(cell.width.0));
            }
            if cell.row_span == 1 && r < row_h.len() {
                row_h[r] = row_h[r].max(i64::from(cell.height.0));
            }
        }
        w.write_u32(0x082A_2210); // 속성 (표본값)
        w.write_u32(0); // 세로 오프셋
        w.write_u32(0); // 가로 오프셋
        w.write_i32(col_w.iter().sum::<i64>() as i32);
        w.write_i32(row_h.iter().sum::<i64>() as i32);
        w.write_u32(0); // z-order
        for _ in 0..4 {
            w.write_u16(283); // 바깥 여백 (표본값)
        }
        w.write_u32(0); // instance id
        w.write_i32(0); // 쪽 나눔 방지
    } else {
        w.write_bytes(&table.common_data);
    }

    let mut tw = ByteWriter::new();
    tw.write_u32(table.attr);
    tw.write_u16(table.rows);
    tw.write_u16(table.cols);
    tw.write_u16(table.cell_spacing);
    for m in table.inner_margins {
        tw.write_u16(m);
    }
    for c in &table.row_cell_counts {
        tw.write_u16(*c);
    }
    tw.write_u16(table.border_fill.0);
    if table.table_tail.is_empty() {
        tw.write_u16(0); // 영역 속성 크기 (5.0.1.0+)
    } else {
        tw.write_bytes(&table.table_tail);
    }

    let mut children = vec![RecordNode {
        tag: tag::TABLE,
        data: tw.into_bytes(),
        children: Vec::new(),
    }];
    let mut cell_warnings = Vec::new();
    for cell in &table.cells {
        children.push(emit_cell_header(cell));
        for p in &cell.paragraphs {
            children.push(emit_paragraph(p, preserve_linesegs, &mut cell_warnings));
        }
    }
    children.extend(table.extras.iter().map(opaque_to_node));
    RecordNode {
        tag: tag::CTRL_HEADER,
        data: w.into_bytes(),
        children,
    }
}

fn emit_cell_header(cell: &Cell) -> RecordNode {
    let mut w = ByteWriter::new();
    w.write_i32(cell.paragraphs.len() as i32);
    w.write_u32(cell.list_attr);
    w.write_u16(cell.col);
    w.write_u16(cell.row);
    w.write_u16(cell.col_span);
    w.write_u16(cell.row_span);
    w.write_i32(cell.width.0);
    w.write_i32(cell.height.0);
    for m in cell.margins {
        w.write_u16(m);
    }
    w.write_u16(cell.border_fill.0);
    if cell.header_tail.is_empty() {
        // 표본 실측 46B 레이아웃 충전: 텍스트 폭(셀 폭 반복) + 예약 8B
        w.write_i32(cell.width.0);
        w.write_bytes(&[0u8; 8]);
    } else {
        w.write_bytes(&cell.header_tail);
    }
    RecordNode {
        tag: tag::LIST_HEADER,
        data: w.into_bytes(),
        children: Vec::new(),
    }
}

fn emit_picture(pic: &Picture, warnings: &mut Vec<String>) -> RecordNode {
    let mut w = ByteWriter::new();
    w.write_bytes(b" osg");
    if pic.common_data.is_empty() {
        // hwpx/md 출신: 개체 공통 속성 최소 구성 (글자처럼 취급)
        warnings.push("그림 개체 공통 속성을 기본값으로 생성 — 한글에서 확인 필요".to_string());
        w.write_u32(u32::from(pic.treat_as_char)); // 속성
        w.write_u32(0); // 세로 오프셋
        w.write_u32(0); // 가로 오프셋
        w.write_i32(pic.width.0);
        w.write_i32(pic.height.0);
        w.write_u32(0); // z-order
        for _ in 0..4 {
            w.write_u16(0); // 바깥 여백
        }
        w.write_u32(0); // instance id
        w.write_i32(0); // 쪽 나눔 방지
    } else {
        w.write_bytes(&pic.common_data);
    }
    let children = pic.extras.iter().map(opaque_to_node).collect();
    RecordNode {
        tag: tag::CTRL_HEADER,
        data: w.into_bytes(),
        children,
    }
}

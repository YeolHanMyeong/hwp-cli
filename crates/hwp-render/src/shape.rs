//! 텍스트 셰이핑.
//!
//! 파이프라인: 문자 모양 경계 → HWP 언어 분류 재분할 → 폰트 해석 →
//! rustybuzz 셰이핑 → 자간/장평 후처리.

use std::sync::Arc;

use hwp_model::{CharShape, Document, HwpChar, Paragraph, ctrl_char};
use rustybuzz::ttf_parser;

use crate::fonts::{FontStore, LoadedFont};

/// 셰이핑된 글리프 하나 (단위: pt).
#[derive(Debug, Clone, Copy)]
pub struct Glyph {
    pub id: u16,
    pub x_advance: f32,
    pub x_offset: f32,
    pub y_offset: f32,
}

/// 같은 (폰트, 크기, 스타일)로 셰이핑된 글리프 런.
pub struct ShapedRun {
    pub font: Arc<LoadedFont>,
    pub size_pt: f32,
    /// 장평 (1.0 = 100%)
    pub x_scale: f32,
    pub color: u32,
    pub bold: bool,
    pub italic: bool,
    /// 밑줄 (글자 아래)
    pub underline: bool,
    /// 취소선
    pub strike: bool,
    /// 밑줄 색 (COLORREF, 0xFFFFFFFF = 글자색 따름)
    pub underline_color: u32,
    pub glyphs: Vec<Glyph>,
    pub width_pt: f32,
    pub text: String,
    /// 이 런의 첫 글자 WCHAR 위치 (줄바꿈 시 글리프→WCHAR 매핑용 — lineseg 합성).
    pub start_wchar: u32,
}

impl ShapedRun {
    /// 글리프 [start, end) 구간으로 부분 런을 만든다 (줄바꿈용).
    /// CJK는 1글자=1글리프라 안전하다. 라틴 합자 분리는 허용 오차.
    pub fn slice(&self, start: usize, end: usize) -> ShapedRun {
        let glyphs: Vec<Glyph> = self.glyphs[start..end].to_vec();
        let width_pt = glyphs.iter().map(|g| g.x_advance).sum();
        ShapedRun {
            font: self.font.clone(),
            size_pt: self.size_pt,
            x_scale: self.x_scale,
            color: self.color,
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            strike: self.strike,
            underline_color: self.underline_color,
            glyphs,
            width_pt,
            text: String::new(), // 부분 런의 원문 추적은 PDF 백엔드(M7)에서
            start_wchar: self.start_wchar + start as u32, // CJK 1글자=1글리프 가정
        }
    }
}

/// 인라인 항목: 글리프 런 또는 고정 폭 진행(탭).
pub enum InlineItem {
    Run(ShapedRun),
    /// 다음 탭 위치까지 진행 (v1: 고정 간격)
    Tab,
}

/// 유니코드 → HWP 7언어 슬롯 분류 (U3 — 경계 문자는 실측 보정 예정).
fn lang_slot_of(c: char) -> usize {
    match c as u32 {
        // 한글 음절/자모/호환 자모
        0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F => 0,
        // CJK 한자
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF => 2,
        // 가나
        0x3040..=0x30FF | 0x31F0..=0x31FF => 3,
        // 라틴/숫자/기본 구두점 — 영문 슬롯
        0x0000..=0x024F => 1,
        // 그 외 기호
        _ => 5,
    }
}

/// 문단의 (wchar 위치 → 문자 모양 ID) 해석.
fn shape_id_at(para: &Paragraph, pos: u32) -> u16 {
    para.char_shape_runs
        .iter()
        .rev()
        .find(|(start, _)| *start <= pos)
        .map(|(_, id)| id.0)
        .unwrap_or(0)
}

/// 문단 전체(또는 wchar 구간)를 셰이핑한다.
/// `range`는 WCHAR 오프셋 [start, end) — lineseg 줄 단위 분할에 사용.
pub fn shape_range(
    store: &mut FontStore,
    doc: &Document,
    para: &Paragraph,
    range: (u32, u32),
    warnings: &mut Vec<String>,
) -> Vec<InlineItem> {
    // 1. (문자모양, 언어) 경계로 텍스트 조각 수집
    struct Piece {
        shape_id: u16,
        lang: usize,
        text: String,
        start: u32,
    }
    let mut pieces: Vec<Piece> = Vec::new();
    let mut items: Vec<(usize, InlineItem)> = Vec::new(); // (pieces 삽입 위치, 탭)
    let mut pos = 0u32;

    for ch in &para.chars {
        let w = ch.wchar_width();
        let in_range = pos >= range.0 && pos < range.1;
        if in_range {
            match ch {
                HwpChar::Text(c) => {
                    let shape_id = shape_id_at(para, pos);
                    let lang = lang_slot_of(*c);
                    match pieces.last_mut() {
                        Some(last) if last.shape_id == shape_id && last.lang == lang => {
                            last.text.push(*c);
                        }
                        _ => pieces.push(Piece {
                            shape_id,
                            lang,
                            text: c.to_string(),
                            start: pos,
                        }),
                    }
                }
                HwpChar::InlineCtrl { code, .. } if *code == ctrl_char::TAB => {
                    items.push((pieces.len(), InlineItem::Tab));
                    // 탭 뒤는 새 조각으로
                    pieces.push(Piece {
                        shape_id: shape_id_at(para, pos + 8),
                        lang: 0,
                        text: String::new(),
                        start: pos + 8,
                    });
                }
                _ => {} // 컨트롤은 v1 렌더 제외
            }
        }
        pos += w;
    }

    // 2. 조각별 셰이핑
    let mut out = Vec::new();
    let mut item_iter = items.into_iter().peekable();
    for (piece_idx, piece) in pieces.into_iter().enumerate() {
        while let Some((at, _)) = item_iter.peek() {
            if *at <= piece_idx {
                let (_, item) = item_iter.next().expect("peek 확인됨");
                out.push(item);
            } else {
                break;
            }
        }
        if piece.text.is_empty() {
            continue;
        }
        let shape = doc.header.char_shapes.get(piece.shape_id as usize);
        let runs = shape_piece(store, doc, shape, piece.lang, &piece.text, piece.start);
        if runs.is_empty() {
            warnings.push(format!("셰이핑 실패: {:?}", piece.text));
        }
        for run in runs {
            out.push(InlineItem::Run(run));
        }
    }
    for (_, item) in item_iter {
        out.push(item);
    }
    out
}

/// 한 조각을 셰이핑한다. 해석된 글꼴이 일부 글자를 갖지 않으면(.notdef) 그 글자만
/// 커버리지 폴백 글꼴로 바꿔, 글꼴 경계마다 별도 [`ShapedRun`]으로 나눈다
/// (예: macOS "휴먼명조"에 ❍(U+274D)가 없을 때 함초롬/Noto로 폴백 — 두부 글자 방지).
fn shape_piece(
    store: &mut FontStore,
    doc: &Document,
    shape: Option<&CharShape>,
    lang: usize,
    text: &str,
    start_wchar: u32,
) -> Vec<ShapedRun> {
    let default_shape = CharShape::default();
    let cs = shape.unwrap_or(&default_shape);
    let face_id = cs.face_ids.get(lang).copied().unwrap_or(0);
    let Some(primary) = store.resolve(doc, lang, face_id) else {
        return Vec::new();
    };

    // 요청 글꼴 이름이 굵은(heavy) 계열인데 우리 대체 글꼴엔 굵은 페이스가 없으면,
    // faux-bold로 굵기를 보강한다(예: HY견고딕/헤드라인M → 함초롬돋움 regular).
    let face_name = doc
        .header
        .fonts
        .get(lang)
        .and_then(|f| f.get(face_id as usize))
        .map(|f| f.name.as_str())
        .unwrap_or("");
    let bold = cs.is_bold() || is_heavy_name(face_name);

    // 글자별 글꼴 배정: primary가 글리프를 가지면 primary, 아니면 커버리지 폴백.
    let primary_face = ttf_parser::Face::parse(&primary.data, primary.index).ok();
    let primary_covers = |c: char| {
        // 공백·제어는 항상 primary 사용(폴백 분할 방지).
        c.is_whitespace()
            || primary_face
                .as_ref()
                .and_then(|f| f.glyph_index(c))
                .is_some_and(|g| g.0 != 0)
    };

    // (글꼴, 텍스트, 시작 wchar) 세그먼트로 분할 — Text 글자는 각 1 wchar.
    let mut segments: Vec<(Arc<LoadedFont>, String, u32)> = Vec::new();
    for (cur, c) in (start_wchar..).zip(text.chars()) {
        let font = if primary_covers(c) {
            primary.clone()
        } else {
            store.font_covering(c).unwrap_or_else(|| primary.clone())
        };
        match segments.last_mut() {
            Some((f, t, _)) if Arc::ptr_eq(f, &font) => t.push(c),
            _ => segments.push((font, c.to_string(), cur)),
        }
    }

    segments
        .into_iter()
        .filter_map(|(font, seg_text, seg_start)| {
            shape_with_font(&font, cs, lang, &seg_text, seg_start, bold)
        })
        .collect()
}

/// 글꼴 이름이 굵은(heavy/bold) 계열인지 — 대체 글꼴 faux-bold 판단용.
/// 숫자 굵기(예: 윤고딕 240/760)는 모호해 제외하고 명확한 키워드만 본다.
fn is_heavy_name(name: &str) -> bool {
    const HEAVY: &[&str] = &[
        "견고딕",
        "견명조",
        "헤드라인",
        "굵은",
        "Heavy",
        "Black",
        "ExtraBold",
        "Ultra",
        "Bold",
    ];
    HEAVY.iter().any(|k| name.contains(k))
}

/// 주어진 글꼴 하나로 텍스트를 셰이핑해 [`ShapedRun`]을 만든다.
fn shape_with_font(
    font: &Arc<LoadedFont>,
    cs: &CharShape,
    lang: usize,
    text: &str,
    start_wchar: u32,
    bold: bool,
) -> Option<ShapedRun> {
    let face = rustybuzz::Face::from_slice(&font.data, font.index)?;
    let upem = face.units_per_em() as f32;

    // 크기: 기준 크기 × 언어별 상대 크기%
    let base = if cs.base_size > 0 { cs.base_size } else { 1000 };
    let rel = cs.rel_sizes.get(lang).copied().unwrap_or(100).max(1);
    let size_pt = (base as f32 / 100.0) * (rel as f32 / 100.0);
    let scale = size_pt / upem;

    // 자간: 글자 크기 기준 % (U4 — 반올림 방식은 실측 보정 예정)
    let spacing_pt = size_pt * cs.spacings.get(lang).copied().unwrap_or(0) as f32 / 100.0;
    let x_scale = cs.ratios.get(lang).copied().unwrap_or(100).max(1) as f32 / 100.0;

    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    let output = rustybuzz::shape(&face, &[], buffer);

    let mut glyphs = Vec::with_capacity(output.len());
    let mut width = 0.0f32;
    for (info, gpos) in output.glyph_infos().iter().zip(output.glyph_positions()) {
        let advance = gpos.x_advance as f32 * scale * x_scale + spacing_pt;
        glyphs.push(Glyph {
            id: info.glyph_id as u16,
            x_advance: advance,
            x_offset: gpos.x_offset as f32 * scale * x_scale,
            y_offset: gpos.y_offset as f32 * scale,
        });
        width += advance;
    }

    Some(ShapedRun {
        font: font.clone(),
        size_pt,
        x_scale,
        color: cs.text_color,
        bold,
        italic: cs.is_italic(),
        underline: cs.has_underline(),
        strike: cs.has_strike(),
        underline_color: cs.underline_color,
        glyphs,
        width_pt: width,
        text: text.to_string(),
        start_wchar,
    })
}

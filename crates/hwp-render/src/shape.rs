//! 텍스트 셰이핑.
//!
//! 파이프라인: 문자 모양 경계 → HWP 언어 분류 재분할 → 폰트 해석 →
//! rustybuzz 셰이핑 → 자간/장평 후처리.

use std::collections::HashMap;
use std::sync::Arc;

use hwp_model::{CharShape, Document, HwpChar, LANG_COUNT, Paragraph, ctrl_char};

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
    /// 글자 음영(배경 하이라이트) 색 (COLORREF, 0xFFFFFFFF = 없음)
    pub shade_color: u32,
    /// 그림자 색 (Some이면 그림자 그림)
    pub shadow: Option<u32>,
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
            shade_color: self.shade_color,
            shadow: self.shadow,
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
    shape_range_notes(store, doc, para, range, &HashMap::new(), warnings)
}

/// `shape_range`에 각주/미주 마커(ctrl_index→번호)를 더한 버전. 본문 경로만 사용.
pub fn shape_range_notes(
    store: &mut FontStore,
    doc: &Document,
    para: &Paragraph,
    range: (u32, u32),
    marks: &HashMap<u32, u32>,
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
                // 각주/미주 앵커: 윗첨자 번호 마커를 본문 위치에 넣는다.
                HwpChar::ExtCtrl {
                    ctrl_index: Some(ci),
                    ..
                } if marks.contains_key(ci) => {
                    if let Some(run) = note_mark_run(store, doc, para, pos, marks[ci]) {
                        items.push((pieces.len(), InlineItem::Run(run)));
                    }
                }
                _ => {} // 그 외 컨트롤은 v1 렌더 제외
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
        match shape_piece(store, doc, shape, piece.lang, &piece.text, piece.start) {
            Some(run) => out.push(InlineItem::Run(run)),
            None => warnings.push(format!("셰이핑 실패: {:?}", piece.text)),
        }
    }
    for (_, item) in item_iter {
        out.push(item);
    }
    out
}

fn shape_piece(
    store: &mut FontStore,
    doc: &Document,
    shape: Option<&CharShape>,
    lang: usize,
    text: &str,
    start_wchar: u32,
) -> Option<ShapedRun> {
    let default_shape = CharShape::default();
    let cs = shape.unwrap_or(&default_shape);
    let face_id = cs.face_ids.get(lang).copied().unwrap_or(0);
    let font = store.resolve(doc, lang, face_id)?;

    let face = rustybuzz::Face::from_slice(&font.data, font.index)?;
    let upem = face.units_per_em() as f32;

    // 크기: 기준 크기 × 언어별 상대 크기%
    let base = if cs.base_size > 0 { cs.base_size } else { 1000 };
    let rel = cs.rel_sizes.get(lang).copied().unwrap_or(100).max(1);
    let full_size = (base as f32 / 100.0) * (rel as f32 / 100.0);
    // 위/아래 첨자: 크기 ~65% 축소 + 베이스라인 이동(원 크기 기준). 수동 글자위치(offsets%) 가산.
    let (sup, sub) = (cs.is_superscript(), cs.is_subscript());
    let size_pt = if sup || sub {
        full_size * 0.65
    } else {
        full_size
    };
    let scale = size_pt / upem;
    let y_raise = {
        let mut r = full_size * cs.char_offset(lang) as f32 / 100.0;
        if sup {
            r += full_size * 0.34;
        }
        if sub {
            r -= full_size * 0.16;
        }
        r
    };

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
            y_offset: gpos.y_offset as f32 * scale + y_raise,
        });
        width += advance;
    }

    Some(ShapedRun {
        font,
        size_pt,
        x_scale,
        color: cs.text_color,
        bold: cs.is_bold(),
        italic: cs.is_italic(),
        underline: cs.has_underline(),
        strike: cs.has_strike(),
        underline_color: cs.underline_color,
        shade_color: if cs.has_shade() {
            cs.shade_color
        } else {
            0xFFFF_FFFF
        },
        shadow: cs.has_shadow().then_some(cs.shadow_color),
        glyphs,
        width_pt: width,
        text: text.to_string(),
        start_wchar,
    })
}

/// 임의 문자열을 기본 글자모양으로 셰이핑한다(수식 근사 등 합성 텍스트용).
/// 한글이 섞이면 한글 슬롯, 아니면 라틴 슬롯 폰트를 쓴다.
pub fn shape_plain(
    store: &mut FontStore,
    doc: &Document,
    text: &str,
    size_pt: f32,
    color: u32,
) -> Option<ShapedRun> {
    let cs = CharShape {
        base_size: (size_pt * 100.0) as i32,
        ratios: [100; LANG_COUNT],
        rel_sizes: [100; LANG_COUNT],
        text_color: color,
        ..CharShape::default()
    };
    let lang = if text.chars().any(|c| ('가'..='힣').contains(&c)) {
        0
    } else {
        1
    };
    shape_piece(store, doc, Some(&cs), lang, text, 0)
}

/// 각주/미주 본문 마커(윗첨자 번호). 주변 글자모양을 따라 ~65% 크기로 줄이고
/// 베이스라인을 위로 올린다. 글리프↔WCHAR 매핑(start_wchar)은 앵커 위치로 둔다.
fn note_mark_run(
    store: &mut FontStore,
    doc: &Document,
    para: &Paragraph,
    pos: u32,
    number: u32,
) -> Option<ShapedRun> {
    let base_id = shape_id_at(para, pos);
    let base = doc.header.char_shapes.get(base_id as usize).cloned();
    let base_size = base
        .as_ref()
        .map(|c| if c.base_size > 0 { c.base_size } else { 1000 })
        .unwrap_or(1000);
    let mut cs = base.unwrap_or_else(|| CharShape {
        ratios: [100; LANG_COUNT],
        rel_sizes: [100; LANG_COUNT],
        ..CharShape::default()
    });
    cs.base_size = ((base_size as f32) * 0.65).max(500.0) as i32;
    cs.attr = 0; // 마커엔 굵게/기울임 등 합성 효과 불필요
    let text = number.to_string();
    let mut run = shape_piece(store, doc, Some(&cs), 1, &text, pos)?;
    // 윗첨자: 위로 올림(y-up 좌표, 백엔드가 baseline-y에서 y_offset만큼 올림).
    let raise = (base_size as f32 / 100.0) * 0.34;
    for g in &mut run.glyphs {
        g.y_offset += raise;
    }
    Some(run)
}

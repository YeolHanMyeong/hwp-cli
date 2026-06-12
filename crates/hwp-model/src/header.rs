//! 문서 헤더 — ID 참조 테이블 일체 (HWP DocInfo / HWPX header.xml에 대응).

use serde::{Deserialize, Serialize};

use crate::ids::{CharShapeId, ParaShapeId};
use crate::opaque::{OpaqueRecord, hex_bytes};

/// 언어 슬롯 수 (한글/영문/한자/일어/외국어/기호/사용자).
pub const LANG_COUNT: usize = 7;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DocHeader {
    pub properties: DocumentProperties,
    /// 언어 슬롯별 글꼴 목록
    pub fonts: [Vec<FaceName>; LANG_COUNT],
    pub bin_data: Vec<BinDataItem>,
    /// 테두리/배경. 참조는 1-기반(BorderFillId 1 = 첫 항목) 관례.
    pub border_fills: Vec<BorderFill>,
    pub char_shapes: Vec<CharShape>,
    pub tab_defs: Vec<RawEntry>,
    pub numberings: Vec<RawEntry>,
    pub bullets: Vec<RawEntry>,
    pub para_shapes: Vec<ParaShape>,
    pub styles: Vec<Style>,
    /// DocInfo 수준의 미해석 레코드 (DOC_DATA, 호환 설정 등)
    pub extras: Vec<OpaqueRecord>,
}

/// DOCUMENT_PROPERTIES (26바이트).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentProperties {
    pub section_count: u16,
    /// 시작 번호: 쪽/각주/미주/그림/표/수식
    pub start_numbers: [u16; 6],
    /// 캐럿 위치: 리스트 ID / 문단 ID / 문단 내 위치
    pub caret: (u32, u32, u32),
}

/// FACE_NAME — 글꼴 하나.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaceName {
    pub attr: u8,
    pub name: String,
    /// 대체 글꼴 (attr bit7)
    pub alt_kind: Option<u8>,
    pub alt_name: Option<String>,
    /// PANOSE 분류 10바이트 (attr bit6)
    pub panose: Option<[u8; 10]>,
    /// 기본 글꼴 이름 (attr bit5)
    pub default_name: Option<String>,
    #[serde(with = "hex_bytes")]
    pub tail: Vec<u8>,
}

/// CHAR_SHAPE — 문자 모양. 알려진 prefix + tail 보존.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharShape {
    /// 언어 슬롯별 글꼴 ID
    pub face_ids: [u16; LANG_COUNT],
    /// 장평 % (50~200)
    pub ratios: [u8; LANG_COUNT],
    /// 자간 % (-50~50)
    pub spacings: [i8; LANG_COUNT],
    /// 상대 크기 % (10~250)
    pub rel_sizes: [u8; LANG_COUNT],
    /// 글자 위치(첨자 오프셋) % (-100~100)
    pub offsets: [i8; LANG_COUNT],
    /// 기준 크기 (HWPUNIT — 10pt = 1000)
    pub base_size: i32,
    /// 속성 비트 (기울임/굵게/밑줄/취소선 등)
    pub attr: u32,
    pub shadow_gap: (i8, i8),
    /// COLORREF (0x00BBGGRR)
    pub text_color: u32,
    pub underline_color: u32,
    pub shade_color: u32,
    pub shadow_color: u32,
    /// 버전별 추가 필드 (테두리채움 ID 5.0.2.1+, 취소선 색 5.0.3.0+)
    #[serde(with = "hex_bytes")]
    pub tail: Vec<u8>,
}

impl CharShape {
    pub fn is_bold(&self) -> bool {
        self.attr & (1 << 1) != 0
    }

    pub fn is_italic(&self) -> bool {
        self.attr & 1 != 0
    }

    /// 밑줄 종류 (bits 2~3): 0 없음, 1 글자 아래, 3 글자 위.
    pub fn underline_kind(&self) -> u8 {
        ((self.attr >> 2) & 0x3) as u8
    }

    pub fn has_underline(&self) -> bool {
        self.underline_kind() == 1
    }

    /// 취소선 (bits 18~20).
    pub fn has_strike(&self) -> bool {
        (self.attr >> 18) & 0x7 != 0
    }
}

/// PARA_SHAPE — 문단 모양. 알려진 prefix + tail 보존.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParaShape {
    /// 속성1 (정렬: bit2~4, 줄간격 종류(구버전): bit0~1 등)
    pub attr1: u32,
    /// 들여쓰기/내어쓰기
    pub indent: i32,
    pub margin_left: i32,
    pub margin_right: i32,
    pub spacing_top: i32,
    pub spacing_bottom: i32,
    /// 줄간격 (5.0.2.5 미만에서 사용)
    pub line_spacing_old: i32,
    pub tab_def_id: u16,
    pub numbering_id: u16,
    pub border_fill_id: u16,
    /// 테두리 여백 (좌/우/위/아래)
    pub border_offsets: [i16; 4],
    /// 속성2/속성3/줄간격(5.0.2.5+) 등
    #[serde(with = "hex_bytes")]
    pub tail: Vec<u8>,
}

impl ParaShape {
    /// 정렬 (0:양쪽, 1:왼쪽, 2:오른쪽, 3:가운데, 4:배분, 5:나눔).
    pub fn alignment(&self) -> u8 {
        ((self.attr1 >> 2) & 0x7) as u8
    }
}

/// STYLE — 스타일 하나.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style {
    pub name: String,
    pub english_name: String,
    pub attr: u8,
    pub next_style: u8,
    pub lang_id: i16,
    pub para_shape: ParaShapeId,
    pub char_shape: CharShapeId,
    #[serde(with = "hex_bytes")]
    pub tail: Vec<u8>,
}

/// BIN_DATA (DocInfo) — 바이너리 데이터 참조.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinDataItem {
    pub attr: u16,
    /// 링크인 경우 절대/상대 경로
    pub link_abs: Option<String>,
    pub link_rel: Option<String>,
    /// 임베딩/스토리지인 경우 BinData 스토리지 내 ID
    pub storage_id: Option<u16>,
    /// 임베딩인 경우 확장자
    pub extension: Option<String>,
    #[serde(with = "hex_bytes")]
    pub tail: Vec<u8>,
}

impl BinDataItem {
    /// 타입 (0: 링크, 1: 임베딩, 2: 스토리지).
    pub fn kind(&self) -> u16 {
        self.attr & 0xF
    }
}

/// 의미 파싱 전의 ID 테이블 항목 — 원시 페이로드 보존.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawEntry {
    #[serde(with = "hex_bytes")]
    pub data: Vec<u8>,
    pub children: Vec<OpaqueRecord>,
}

/// 테두리선 하나. 굵기는 mm 테이블 인덱스 (0=0.1mm, 1=0.12mm, … 15=5.0mm).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BorderLine {
    /// 0 = 없음, 1 = 실선, 2+ = 점선/이중선 등
    pub line_type: u8,
    pub width: u8,
    /// COLORREF (0x00BBGGRR)
    pub color: u32,
}

impl BorderLine {
    /// 굵기 인덱스 → mm (한글문서파일형식 5.0 굵기 표).
    pub fn width_mm(&self) -> f32 {
        const TABLE: [f32; 16] = [
            0.1, 0.12, 0.15, 0.2, 0.25, 0.3, 0.4, 0.5, 0.6, 0.7, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0,
        ];
        TABLE.get(self.width as usize).copied().unwrap_or(0.12)
    }

    pub fn is_visible(&self) -> bool {
        self.line_type != 0
    }
}

/// BORDER_FILL — 테두리/배경 (실측으로 확정한 레이아웃).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BorderFill {
    pub attr: u16,
    /// 왼/오른/위/아래
    pub sides: [BorderLine; 4],
    pub diagonal: BorderLine,
    /// 채우기 종류 비트 (bit0 = 단색)
    pub fill_type: u32,
    /// 단색 배경 (COLORREF, 0xFFFFFFFF = 없음)
    pub bg_color: Option<u32>,
    #[serde(with = "hex_bytes")]
    pub tail: Vec<u8>,
}

impl BorderFill {
    /// 그릴 배경색 (없음 표식 제외).
    pub fn visible_bg(&self) -> Option<u32> {
        self.bg_color.filter(|&c| c != 0xFFFF_FFFF)
    }
}

//! 확장 컨트롤(CTRL_HEADER) 모델.
//!
//! M1에서는 표(`tbl `)와 구역 정의(`secd`)를 의미 파싱하고, 나머지는
//! [`GenericControl`]로 보존한다 — 문단 리스트는 텍스트 추출을 위해
//! 종류와 무관하게 재귀 수집한다(셀/글상자/머리말/각주 모두 같은 구조).

use serde::{Deserialize, Serialize};

use crate::ids::BorderFillId;
use crate::opaque::{OpaqueRecord, hex_bytes};
use crate::paragraph::Paragraph;
use crate::units::HwpUnit;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Control {
    /// `secd` — 구역 정의 (PAGE_DEF 등을 자식으로 가짐)
    SectionDef(SectionDef),
    /// `tbl ` — 표
    Table(Table),
    /// `gso `(그림) / `hp:pic` — 이미지 개체
    Picture(Picture),
    /// 그 외 — ctrl_id와 원본을 보존, 문단 리스트는 수집
    Generic(GenericControl),
}

impl Control {
    /// 정방향 ctrl_id (예: b"secd", b"tbl ").
    pub fn ctrl_id(&self) -> [u8; 4] {
        match self {
            Control::SectionDef(_) => *b"secd",
            Control::Table(_) => *b"tbl ",
            Control::Picture(_) => *b"gso ",
            Control::Generic(g) => g.ctrl_id,
        }
    }
}

/// 이미지 개체.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Picture {
    /// 개체 공통 속성 원본 (hwp5: CTRL_HEADER 페이로드, hwpx: 비어 있음)
    #[serde(with = "hex_bytes")]
    pub common_data: Vec<u8>,
    pub width: HwpUnit,
    pub height: HwpUnit,
    /// 글자처럼 취급 여부 (배치 힌트)
    pub treat_as_char: bool,
    /// 바이너리 데이터 참조
    pub bin_ref: BinRef,
    pub extras: Vec<OpaqueRecord>,
}

/// BinData 참조 방식.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinRef {
    /// hwp5: BIN_DATA 테이블 ID (1-기반)
    Id(crate::ids::BinDataId),
    /// hwpx: manifest 항목 ID (binaryItemIDRef)
    ItemRef(String),
}

/// 구역 정의 컨트롤.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SectionDef {
    /// CTRL_HEADER 페이로드 (ctrl_id 이후) — 미해석 보존
    #[serde(with = "hex_bytes")]
    pub data: Vec<u8>,
    /// PAGE_DEF — 용지 크기/여백
    pub page: Option<PageDef>,
    /// FOOTNOTE_SHAPE, PAGE_BORDER_FILL 등 미해석 자식
    pub extras: Vec<OpaqueRecord>,
}

/// PAGE_DEF (40바이트) — 용지 정의.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageDef {
    pub width: HwpUnit,
    pub height: HwpUnit,
    pub margin_left: HwpUnit,
    pub margin_right: HwpUnit,
    pub margin_top: HwpUnit,
    pub margin_bottom: HwpUnit,
    pub margin_header: HwpUnit,
    pub margin_footer: HwpUnit,
    pub gutter: HwpUnit,
    /// bit0: 용지 방향(가로), bit1~2: 제책 방법
    pub attr: u32,
}

/// 표 컨트롤.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    /// CTRL_HEADER 페이로드 (개체 공통 속성) — 미해석 보존
    #[serde(with = "hex_bytes")]
    pub common_data: Vec<u8>,
    pub attr: u32,
    pub rows: u16,
    pub cols: u16,
    pub cell_spacing: u16,
    /// 안쪽 여백 (왼/오른/위/아래)
    pub inner_margins: [u16; 4],
    /// 행별 셀 개수 (실측: 스펙의 "Row Size"는 행 높이가 아니라 셀 수)
    pub row_cell_counts: Vec<u16>,
    pub border_fill: BorderFillId,
    /// TABLE 레코드의 나머지 (영역 속성 등)
    #[serde(with = "hex_bytes")]
    pub table_tail: Vec<u8>,
    /// 셀 목록 (LIST_HEADER 등장 순서 — 행 우선)
    pub cells: Vec<Cell>,
    pub extras: Vec<OpaqueRecord>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cell {
    /// LIST_HEADER 속성 비트 (hwp5 왕복 보존)
    #[serde(default)]
    pub list_attr: u32,
    pub col: u16,
    pub row: u16,
    pub col_span: u16,
    pub row_span: u16,
    pub width: HwpUnit,
    pub height: HwpUnit,
    /// 셀 안쪽 여백 (왼/오른/위/아래, HWPUNIT)
    pub margins: [u16; 4],
    pub border_fill: BorderFillId,
    /// LIST_HEADER 페이로드 중 미해석 부분
    #[serde(with = "hex_bytes")]
    pub header_tail: Vec<u8>,
    pub paragraphs: Vec<Paragraph>,
}

/// 의미 파싱하지 않는 컨트롤 (머리말/꼬리말/각주/글상자/필드 등).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenericControl {
    /// 정방향 ctrl_id (예: b"gso ", b"head")
    pub ctrl_id: [u8; 4],
    /// CTRL_HEADER 페이로드 (ctrl_id 이후)
    #[serde(with = "hex_bytes")]
    pub data: Vec<u8>,
    /// LIST_HEADER 단위 문단 리스트 — 텍스트 추출용 재귀 수집
    pub paragraph_lists: Vec<ParagraphList>,
    pub extras: Vec<OpaqueRecord>,
    /// hwp5 원본 CTRL_HEADER 자식 서브트리(중첩 포함) — 무손실 재직렬화용.
    /// 존재하면 emit 시 이 트리를 그대로 방출하고 paragraph_lists/extras는
    /// 텍스트 추출 전용으로만 쓴다(gso 등 중첩 구조 평탄화 방지).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_children: Vec<OpaqueRecord>,
}

/// LIST_HEADER 하나가 여는 문단 리스트.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParagraphList {
    /// LIST_HEADER 페이로드 — 미해석 보존
    #[serde(with = "hex_bytes")]
    pub header_data: Vec<u8>,
    pub paragraphs: Vec<Paragraph>,
}

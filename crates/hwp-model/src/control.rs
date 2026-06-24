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
    /// 글자처럼 취급 여부 (배치 힌트). false면 떠 있는(floating) 개체.
    pub treat_as_char: bool,
    /// z-순서(겹침). hwpx `<hp:pic zOrder>`.
    #[serde(default)]
    pub z_order: u32,
    /// 떠 있는 개체의 세로/가로 오프셋(HWPUNIT). hwpx `<hp:pos vertOffset/horzOffset>`.
    #[serde(default)]
    pub vert_offset: i32,
    #[serde(default)]
    pub horz_offset: i32,
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

/// 개체 공통 속성(gso common)의 배치 정보 — hwpx `<hp:pos>`/`<hp:sz>`/`<hp:outMargin>`/
/// `zOrder`에서 읽어 hwp5 CTRL_HEADER 40바이트 공통 속성으로 합성한다. hwpx 출신 표가
/// 이 정보를 잃으면 writer가 떠 있는(floating) 상수로 덮어써 본문 흐름에서 빠진다.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GsoPlacement {
    /// 글자처럼 취급 (attr bit0). 정품 표는 대부분 true(인라인).
    pub treat_as_char: bool,
    /// 줄 간격에 영향 (attr bit2).
    pub affect_line_spacing: bool,
    /// 본문과 어울림(flowWithText) (attr bit13).
    pub flow_with_text: bool,
    /// 앵커/개체 고정(holdAnchorAndSO) — 공통 속성 @36.
    pub hold_anchor: bool,
    /// 세로 위치 기준: 0=PAPER, 1=PAGE, 2=PARA (attr bits3-4).
    pub vert_rel_to: u8,
    /// 가로 위치 기준: 0=PAPER, 1=PAGE, 2=COLUMN, 3=PARA (attr bits8-9).
    pub horz_rel_to: u8,
    /// 세로 정렬 (attr bits5-7).
    pub vert_align: u8,
    /// 가로 정렬 (attr bits10-12).
    pub horz_align: u8,
    pub vert_offset: i32,
    pub horz_offset: i32,
    pub z_order: i32,
    /// 개체 바깥 경계 너비/높이(HWPUNIT) — hwpx `<hp:sz>`. 병합 셀 합산보다 정확.
    pub width: i32,
    pub height: i32,
    /// 바깥 여백 (왼/오른/위/아래).
    pub out_margins: [u16; 4],
}

impl GsoPlacement {
    /// hwp5 개체 공통 속성 attr(u32)로 합성. 상위 16비트는 관측 상수(0x082a:
    /// widthRelTo/heightRelTo=ABSOLUTE, textWrap 등)로 둔다.
    pub fn synth_attr(&self) -> u32 {
        let mut low: u32 = 0;
        low |= u32::from(self.treat_as_char); // bit0
        low |= u32::from(self.affect_line_spacing) << 2; // bit2
        low |= (u32::from(self.vert_rel_to) & 0x3) << 3; // bits3-4
        low |= (u32::from(self.vert_align) & 0x7) << 5; // bits5-7
        low |= (u32::from(self.horz_rel_to) & 0x3) << 8; // bits8-9
        low |= (u32::from(self.horz_align) & 0x7) << 10; // bits10-12
        low |= u32::from(self.flow_with_text) << 13; // bit13
        0x082a_0000 | low
    }
}

/// 표 컨트롤.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Table {
    /// CTRL_HEADER 페이로드 (개체 공통 속성) — 미해석 보존
    #[serde(with = "hex_bytes")]
    pub common_data: Vec<u8>,
    /// hwpx 출신 표의 배치 정보 (hwp5 출신은 common_data가 채워져 None).
    #[serde(default)]
    pub placement: Option<GsoPlacement>,
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

#[cfg(test)]
mod tests {
    use super::GsoPlacement;

    /// 정품 한라대 .hwp 실측 표 공통 속성 attr를 비트 합성으로 재현하는지 확인한다.
    /// (글자처럼 취급 보존 — 인라인 표가 떠 있는 개체로 빠지던 버그의 회귀 방지.)
    #[test]
    fn 표_공통속성_attr_정품값_재현() {
        // 인라인 표(제목바·본문·꼬리말): treatAsChar=1, vertRelTo=PARA(2),
        // horzRelTo=PARA(3), flowWithText=1 → 0x082a2311
        let inline = GsoPlacement {
            treat_as_char: true,
            flow_with_text: true,
            vert_rel_to: 2,
            horz_rel_to: 3,
            ..Default::default()
        };
        assert_eq!(inline.synth_attr(), 0x082a_2311);

        // 표지/꼬리말 3x3: flowWithText=0 → 0x082a0311
        let cover = GsoPlacement {
            flow_with_text: false,
            ..inline.clone()
        };
        assert_eq!(cover.synth_attr(), 0x082a_0311);

        // 목차 박스: treatAsChar=0(떠 있음) → 0x082a2310
        let toc = GsoPlacement {
            treat_as_char: false,
            ..inline
        };
        assert_eq!(toc.synth_attr(), 0x082a_2310);
    }
}

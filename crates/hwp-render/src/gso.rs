//! 렌더 전용 gso(그리기 개체) 공통 속성 파서.
//!
//! `crates/hwp5/src/body_text.rs`의 `parse_picture_gso`와 **동일 바이트 레이아웃**
//! (단일 출처): 속성(4) 세로offset(4) 가로offset(4) 폭(4) 높이(4). 글상자·도형의
//! 위치/크기를 렌더 시 읽어 배치한다(IR을 바꾸지 않는 소비단 전용).

/// gso 개체 공통 속성에서 읽은 위치/크기 (HWPUNIT).
#[derive(Debug, Clone, Copy)]
pub struct GsoBox {
    pub attr: u32,
    pub vert_offset: i32,
    pub horz_offset: i32,
    pub width: i32,
    pub height: i32,
}

impl GsoBox {
    /// 글자처럼 취급(인라인) 여부. false면 떠 있는(floating) 개체.
    pub fn treat_as_char(&self) -> bool {
        self.attr & 1 != 0
    }
    /// 세로 위치 기준: 0=PAPER, 1=PAGE, 2=PARA (attr bits3-4).
    pub fn vert_rel_to(&self) -> u8 {
        ((self.attr >> 3) & 0x3) as u8
    }
    /// 가로 위치 기준: 0=PAPER, 1=PAGE, 2=COLUMN, 3=PARA (attr bits8-9).
    pub fn horz_rel_to(&self) -> u8 {
        ((self.attr >> 8) & 0x3) as u8
    }
}

/// gso CTRL_HEADER 페이로드(ctrl_id 이후)에서 공통 속성을 읽는다.
pub fn parse_gso_box(data: &[u8]) -> Option<GsoBox> {
    if data.len() < 20 {
        return None;
    }
    let rd = |o: usize| i32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
    Some(GsoBox {
        attr: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
        vert_offset: rd(4),
        horz_offset: rd(8),
        width: rd(12),
        height: rd(16),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annual_report_표본_디코드() {
        // 실측: 00406a04 d4120000 c1260000 b4b60000 9d1e0000
        let data = [
            0x00, 0x40, 0x6a, 0x04, 0xd4, 0x12, 0x00, 0x00, 0xc1, 0x26, 0x00, 0x00, 0xb4, 0xb6,
            0x00, 0x00, 0x9d, 0x1e, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00,
        ];
        let b = parse_gso_box(&data).unwrap();
        assert_eq!(b.attr, 0x046a_4000);
        assert_eq!(b.vert_offset, 0x12d4);
        assert_eq!(b.horz_offset, 0x26c1);
        assert_eq!(b.width, 0xb6b4);
        assert_eq!(b.height, 0x1e9d);
        assert!(!b.treat_as_char(), "floating");
        assert_eq!(b.vert_rel_to(), 0, "PAPER");
        assert_eq!(b.horz_rel_to(), 0, "PAPER");
    }

    #[test]
    fn 너무_짧으면_none() {
        assert!(parse_gso_box(&[0u8; 10]).is_none());
    }
}

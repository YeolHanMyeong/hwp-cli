//! HWP 길이 단위.
//!
//! HWP의 모든 길이는 HWPUNIT = 1/7200 인치다.
//! 1pt = 1/72 인치 = 정확히 100 HWPUNIT이므로 pt 변환은 손실이 없다.

use serde::{Deserialize, Serialize};

/// HWPUNIT (1/7200 인치). 레이아웃 계산은 이 정수 단위로 수행한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HwpUnit(pub i32);

impl HwpUnit {
    /// 1pt에 해당하는 HWPUNIT 수.
    pub const PER_PT: i32 = 100;
    /// 1인치에 해당하는 HWPUNIT 수.
    pub const PER_INCH: i32 = 7200;

    /// pt로 변환 (정확).
    pub fn to_pt(self) -> f64 {
        f64::from(self.0) / f64::from(Self::PER_PT)
    }

    /// mm로 변환.
    pub fn to_mm(self) -> f64 {
        f64::from(self.0) / f64::from(Self::PER_INCH) * 25.4
    }

    /// 주어진 DPI에서의 픽셀 값으로 변환.
    pub fn to_px(self, dpi: f64) -> f64 {
        f64::from(self.0) / f64::from(Self::PER_INCH) * dpi
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pt_변환은_정확하다() {
        assert_eq!(HwpUnit(1000).to_pt(), 10.0);
        assert_eq!(HwpUnit(59528).to_pt(), 595.28); // A4 폭 210mm
    }

    #[test]
    fn mm_변환() {
        // A4 폭 210mm = 59528.34... HWPUNIT — 한글은 59528을 사용
        assert!((HwpUnit(59528).to_mm() - 210.0).abs() < 0.01);
    }
}

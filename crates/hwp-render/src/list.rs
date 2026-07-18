//! 목록 마커 로직은 `hwp-model::list`로 이동했다(공용 SSOT — markdown 내보내기도
//! 같은 규칙을 쓴다). 여기서는 기존 호출부 호환을 위해 재수출만 한다.

pub use hwp_model::list::{ListState, format_number};

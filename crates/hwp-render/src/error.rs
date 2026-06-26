//! hwp-render 오류 타입.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RenderError {
    #[error("백엔드 오류: {0}")]
    Backend(String),

    #[error("PNG 인코딩 실패: {0}")]
    Encode(String),

    #[error("PDF 생성 실패: {0}")]
    Pdf(String),
}

//! HWP 5.0 바이너리 포맷 reader/writer.
//!
//! HWP 5.0 파일 = MS CFB(Compound File Binary) 컨테이너:
//! - `FileHeader`        — 256바이트 고정 헤더 (시그니처/버전/속성 플래그)
//! - `DocInfo`           — 문서 정보 레코드 스트림 (보통 raw deflate 압축)
//! - `BodyText/Section0..N` — 본문 레코드 스트림 (보통 raw deflate 압축)
//! - `BinData/*`         — 첨부 바이너리 (이미지 등)
//! - `PrvText`/`PrvImage` — 미리보기 (비압축)
//! - 기타: `\x05HwpSummaryInformation`, `Scripts/*`, `DocOptions/*` 등
//!
//! 계층 구조:
//! - [`container`] — CFB 래핑, 스트림 열거/읽기
//! - [`file_header`] — FileHeader 파싱/직렬화
//! - [`codec`] — 바이트 커서(reader/writer)와 raw deflate 압축
//! - [`record`] — 레코드 헤더 코덱, 평면 스트림 ↔ 트리 변환

pub mod codec;
pub mod container;
pub mod error;
pub mod file_header;
pub mod record;

pub use container::Hwp5Container;
pub use error::Hwp5Error;
pub use file_header::FileHeader;

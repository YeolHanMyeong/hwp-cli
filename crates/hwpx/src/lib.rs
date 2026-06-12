//! HWPX(OWPML, KS X 6101) 포맷 reader/writer.
//!
//! HWPX = ZIP 컨테이너 + XML:
//! - `mimetype`               — `application/hwp+zip` (첫 엔트리, 무압축)
//! - `version.xml`            — 포맷/작성 프로그램 버전
//! - `META-INF/container.xml`, `META-INF/manifest.xml`
//! - `Contents/header.xml`    — DocInfo에 해당 (ID 참조 테이블)
//! - `Contents/section0.xml…` — 본문
//! - `BinData/*`, `Preview/*`
//!
//! M0에서는 컨테이너 수준([`package`])만 구현한다. OWPML 파싱(M2)과
//! writer(M4)는 이후 마일스톤에서 추가한다.

pub mod error;
pub mod package;

pub use error::HwpxError;
pub use package::HwpxPackage;

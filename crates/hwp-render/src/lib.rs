//! IR → PNG/SVG/PDF 페이지 렌더러 (M3에서 구현).
//!
//! 파이프라인: IR → Layout(LineSegLayouter/FlowLayouter) → LayoutTree
//! → DisplayList → 백엔드(tiny-skia/SVG/krilla).
//! 모듈 경계: fonts/ shape/ layout/ display_list/ backend/ —
//! 컴파일 시간 문제 시 서브 크레이트로 분리 가능하게 유지한다.

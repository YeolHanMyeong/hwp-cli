//! DisplayList — 레이아웃과 백엔드 사이의 안정 계약.
//!
//! HWP 도메인 지식이 제거된 순수 그리기 명령. 좌표는 pt(f32),
//! 페이지 원점 좌상단, y축 아래 방향.

use std::sync::Arc;

use crate::shape::ShapedRun;

pub struct DisplayList {
    pub pages: Vec<PageList>,
}

pub struct PageList {
    pub width_pt: f32,
    pub height_pt: f32,
    pub items: Vec<Item>,
}

pub enum Item {
    /// 베이스라인 원점 (x, y)에 배치된 글리프 런
    Glyphs { x: f32, y: f32, run: ShapedRun },
    /// 채움 사각형 (셀 배경 등) — COLORREF
    Rect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        fill: u32,
    },
    /// 선분 (테두리 등) — COLORREF, 굵기 pt
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: u32,
        width: f32,
    },
    /// 이미지 — 인코딩된 원본 바이트 (PNG/JPEG/BMP/GIF)
    Image {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        data: Arc<Vec<u8>>,
    },
    /// 임의 경로 — 그리기 개체(선/사각형/타원/호/다각형). 좌표 pt, 페이지 공간.
    Path {
        commands: Vec<PathCmd>,
        /// 채움색 COLORREF. None=채움 없음.
        fill: Option<u32>,
        /// (선색 COLORREF, 굵기 pt). None=선 없음.
        stroke: Option<(u32, f32)>,
    },
}

/// 경로 명령 (좌표 pt, 페이지 공간).
#[derive(Debug, Clone, Copy)]
pub enum PathCmd {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    CubicTo(f32, f32, f32, f32, f32, f32),
    Close,
}

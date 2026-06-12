//! PNG 백엔드 — tiny-skia 래스터화.
//!
//! 글리프 윤곽선을 ttf-parser(rustybuzz 재수출)로 추출해 tiny-skia
//! Path로 채운다. 합성 굵게 = fill+stroke, 합성 기울임 = skew 변환.

use rustybuzz::ttf_parser;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::display::{DisplayList, Item, PageList};
use crate::error::RenderError;

/// 기울임 시뮬레이션 각도의 탄젠트 (≈12°).
const ITALIC_SKEW: f32 = 0.2126;

pub fn render_png(list: &DisplayList, dpi: f32) -> Result<Vec<Pixmap>, RenderError> {
    list.pages.iter().map(|p| render_page(p, dpi)).collect()
}

fn render_page(page: &PageList, dpi: f32) -> Result<Pixmap, RenderError> {
    let px_scale = dpi / 72.0;
    let w = (page.width_pt * px_scale).ceil().max(1.0) as u32;
    let h = (page.height_pt * px_scale).ceil().max(1.0) as u32;
    let mut pixmap =
        Pixmap::new(w, h).ok_or_else(|| RenderError::Backend("Pixmap 생성 실패".to_string()))?;
    pixmap.fill(Color::WHITE);

    for item in &page.items {
        match item {
            Item::Rect {
                x,
                y,
                w: rw,
                h: rh,
                fill,
            } => {
                if let Some(rect) = tiny_skia::Rect::from_xywh(
                    *x * px_scale,
                    *y * px_scale,
                    rw * px_scale,
                    rh * px_scale,
                ) {
                    let mut paint = Paint::default();
                    let (r, g, b) = colorref_rgb(*fill);
                    paint.set_color_rgba8(r, g, b, 255);
                    pixmap.fill_rect(rect, &paint, Transform::identity(), None);
                }
            }
            Item::Line {
                x1,
                y1,
                x2,
                y2,
                color,
                width,
            } => {
                let mut pb = PathBuilder::new();
                pb.move_to(*x1, *y1);
                pb.line_to(*x2, *y2);
                if let Some(path) = pb.finish() {
                    let mut paint = Paint::default();
                    let (r, g, b) = colorref_rgb(*color);
                    paint.set_color_rgba8(r, g, b, 255);
                    paint.anti_alias = true;
                    let stroke = Stroke {
                        width: width.max(0.2),
                        ..Stroke::default()
                    };
                    pixmap.stroke_path(
                        &path,
                        &paint,
                        &stroke,
                        Transform::from_scale(px_scale, px_scale),
                        None,
                    );
                }
            }
            Item::Glyphs { x, y, run } => {
                let face = match ttf_parser::Face::parse(&run.font.data, run.font.index) {
                    Ok(f) => f,
                    Err(_) => continue,
                };
                let upem = face.units_per_em() as f32;
                let glyph_scale = run.size_pt / upem;

                let mut paint = Paint::default();
                let (r, g, b) = colorref_rgb(run.color);
                paint.set_color_rgba8(r, g, b, 255);
                paint.anti_alias = true;

                let mut pen_x = *x;
                for glyph in &run.glyphs {
                    let mut builder = OutlinePath::default();
                    if face
                        .outline_glyph(ttf_parser::GlyphId(glyph.id), &mut builder)
                        .is_some()
                        && let Some(path) = builder.path.finish()
                    {
                        // 글리프 → 페이지 변환: 크기 스케일, y 뒤집기(폰트는 y-up),
                        // 장평 x스케일, 기울임 skew, 베이스라인 원점 이동, DPI 스케일
                        let mut t = Transform::from_scale(glyph_scale * run.x_scale, -glyph_scale);
                        if run.italic {
                            t = t.post_concat(Transform::from_skew(-ITALIC_SKEW, 0.0));
                        }
                        t = t.post_translate(pen_x + glyph.x_offset, *y - glyph.y_offset);
                        t = t.post_scale(px_scale, px_scale);

                        pixmap.fill_path(&path, &paint, FillRule::Winding, t, None);
                        if run.bold {
                            // 합성 굵게: 윤곽선 위 스트로크 (굵기 ≈ 크기의 3%)
                            let stroke = Stroke {
                                width: run.size_pt * 0.03 / glyph_scale,
                                ..Stroke::default()
                            };
                            pixmap.stroke_path(&path, &paint, &stroke, t, None);
                        }
                    }
                    pen_x += glyph.x_advance;
                }
            }
        }
    }
    Ok(pixmap)
}

/// COLORREF(0x00BBGGRR) → (r, g, b). 0xFFFFFFFF(없음)는 검정 취급.
fn colorref_rgb(c: u32) -> (u8, u8, u8) {
    if c == 0xFFFF_FFFF {
        return (0, 0, 0);
    }
    (
        (c & 0xFF) as u8,
        ((c >> 8) & 0xFF) as u8,
        ((c >> 16) & 0xFF) as u8,
    )
}

#[derive(Default)]
struct OutlinePath {
    path: PathBuilder,
}

impl ttf_parser::OutlineBuilder for OutlinePath {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.path.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.path.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.path.close();
    }
}

//! PNG 백엔드 — tiny-skia 래스터화.
//!
//! 글리프 윤곽선을 ttf-parser(rustybuzz 재수출)로 추출해 tiny-skia
//! Path로 채운다. 합성 굵게 = fill+stroke, 합성 기울임 = skew 변환.

use rustybuzz::ttf_parser;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point,
    RadialGradient, Shader, SpreadMode, Stroke, Transform,
};

use crate::display::{DisplayList, Fill, Gradient, Item, PageList, PathCmd, path_bbox};
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
            Item::Image {
                x,
                y,
                w: iw,
                h: ih,
                data,
            } => {
                match decode_image(data) {
                    Some(src) => {
                        let sx = (iw * px_scale) / src.width() as f32;
                        let sy = (ih * px_scale) / src.height() as f32;
                        let t = Transform::from_scale(sx, sy)
                            .post_translate(*x * px_scale, *y * px_scale);
                        pixmap.draw_pixmap(
                            0,
                            0,
                            src.as_ref(),
                            &tiny_skia::PixmapPaint::default(),
                            t,
                            None,
                        );
                    }
                    None => {
                        // 디코드 실패: 자홍색 placeholder (조용한 누락 금지)
                        if let Some(rect) = tiny_skia::Rect::from_xywh(
                            *x * px_scale,
                            *y * px_scale,
                            iw * px_scale,
                            ih * px_scale,
                        ) {
                            let mut paint = Paint::default();
                            paint.set_color_rgba8(255, 0, 255, 120);
                            pixmap.fill_rect(rect, &paint, Transform::identity(), None);
                        }
                    }
                }
            }
            Item::Glyphs { x, y, run } => {
                let face = match ttf_parser::Face::parse(&run.font.data, run.font.index) {
                    Ok(f) => f,
                    Err(_) => continue,
                };
                // 글자 음영(배경 하이라이트) — 글리프 뒤에 사각형.
                if run.shade_color != 0xFFFF_FFFF
                    && let Some(rect) = tiny_skia::Rect::from_xywh(
                        *x * px_scale,
                        (*y - run.size_pt * 0.8) * px_scale,
                        run.width_pt * px_scale,
                        run.size_pt * px_scale,
                    )
                {
                    let (sr, sg, sb) = colorref_rgb(run.shade_color);
                    let mut sp = Paint::default();
                    sp.set_color_rgba8(sr, sg, sb, 255);
                    pixmap.fill_rect(rect, &sp, Transform::identity(), None);
                }
                // 그림자 — 본문 전에 오프셋 복사.
                if let Some(sc) = run.shadow {
                    let d = run.size_pt * 0.06;
                    draw_glyph_run(&mut pixmap, &face, run, *x, *y, px_scale, sc, d, d);
                }
                draw_glyph_run(&mut pixmap, &face, run, *x, *y, px_scale, run.color, 0.0, 0.0);
            }
            Item::Path {
                commands,
                fill,
                stroke,
            } => {
                let mut pb = PathBuilder::new();
                for cmd in commands {
                    match *cmd {
                        PathCmd::MoveTo(x, y) => pb.move_to(x, y),
                        PathCmd::LineTo(x, y) => pb.line_to(x, y),
                        PathCmd::CubicTo(a, b, c, d, e, f) => pb.cubic_to(a, b, c, d, e, f),
                        PathCmd::Close => pb.close(),
                    }
                }
                if let Some(path) = pb.finish() {
                    let t = Transform::from_scale(px_scale, px_scale);
                    if let Some(f) = fill {
                        let mut paint = Paint {
                            anti_alias: true,
                            ..Default::default()
                        };
                        match f {
                            Fill::Solid(c) => {
                                let (r, g, b) = colorref_rgb(*c);
                                paint.set_color_rgba8(r, g, b, 255);
                            }
                            Fill::Gradient(grad) => match gradient_shader(grad, commands, px_scale) {
                                Some(sh) => paint.shader = sh,
                                None => {
                                    let (r, g, b) = grad
                                        .stops
                                        .first()
                                        .map_or((0, 0, 0), |&(_, c)| colorref_rgb(c));
                                    paint.set_color_rgba8(r, g, b, 255);
                                }
                            },
                        }
                        pixmap.fill_path(&path, &paint, FillRule::Winding, t, None);
                    }
                    if let Some(s) = stroke {
                        let (r, g, b) = colorref_rgb(s.color);
                        let mut paint = Paint::default();
                        paint.set_color_rgba8(r, g, b, 255);
                        paint.anti_alias = true;
                        let dash = (s.dash.len() >= 2)
                            .then(|| tiny_skia::StrokeDash::new(s.dash.clone(), 0.0))
                            .flatten();
                        let stroke = Stroke {
                            width: s.width.max(0.05),
                            dash,
                            ..Stroke::default()
                        };
                        pixmap.stroke_path(&path, &paint, &stroke, t, None);
                    }
                }
            }
        }
    }
    Ok(pixmap)
}

/// 그러데이션 → tiny-skia 셰이더. 경로 bbox(pt) 기준, transform=px_scale로 device 정합.
fn gradient_shader(g: &Gradient, cmds: &[PathCmd], px_scale: f32) -> Option<Shader<'static>> {
    let (x0, y0, x1, y1) = path_bbox(cmds);
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let stops: Vec<GradientStop> = g
        .stops
        .iter()
        .map(|&(p, c)| {
            let (r, gg, b) = colorref_rgb(c);
            GradientStop::new(p, Color::from_rgba8(r, gg, b, 255))
        })
        .collect();
    if stops.len() < 2 {
        return None;
    }
    let xf = Transform::from_scale(px_scale, px_scale);
    if g.radial {
        let radius = ((x1 - x0).max(y1 - y0) / 2.0).max(0.1);
        RadialGradient::new(
            Point::from_xy(cx, cy),
            0.0,
            Point::from_xy(cx, cy),
            radius,
            stops,
            SpreadMode::Pad,
            xf,
        )
    } else {
        let a = g.angle_deg.to_radians();
        let (dx, dy) = (a.cos(), a.sin());
        let proj = |x: f32, y: f32| (x - cx) * dx + (y - cy) * dy;
        let ps = [proj(x0, y0), proj(x1, y0), proj(x1, y1), proj(x0, y1)];
        let tmin = ps.iter().cloned().fold(f32::MAX, f32::min);
        let tmax = ps.iter().cloned().fold(f32::MIN, f32::max);
        if (tmax - tmin).abs() < 0.01 {
            return None;
        }
        LinearGradient::new(
            Point::from_xy(cx + dx * tmin, cy + dy * tmin),
            Point::from_xy(cx + dx * tmax, cy + dy * tmax),
            stops,
            SpreadMode::Pad,
            xf,
        )
    }
}

/// 인코딩된 이미지를 tiny-skia Pixmap으로 디코드한다 (premultiplied RGBA).
fn decode_image(data: &[u8]) -> Option<Pixmap> {
    let img = image::load_from_memory(data).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let mut pixmap = Pixmap::new(w, h)?;
    for (dst, src) in pixmap.pixels_mut().iter_mut().zip(img.pixels()) {
        let [r, g, b, a] = src.0;
        *dst = tiny_skia::PremultipliedColorU8::from_rgba(
            (u16::from(r) * u16::from(a) / 255) as u8,
            (u16::from(g) * u16::from(a) / 255) as u8,
            (u16::from(b) * u16::from(a) / 255) as u8,
            a,
        )?;
    }
    Some(pixmap)
}

/// 글리프 런 하나를 (x, y) 베이스라인에 그린다. (dx, dy) 평행이동(그림자용),
/// color로 채운다. bold/italic/장평/글리프 y_offset(첨자) 반영.
#[allow(clippy::too_many_arguments)]
fn draw_glyph_run(
    pixmap: &mut Pixmap,
    face: &ttf_parser::Face<'_>,
    run: &crate::shape::ShapedRun,
    x: f32,
    y: f32,
    px_scale: f32,
    color: u32,
    dx: f32,
    dy: f32,
) {
    let upem = face.units_per_em() as f32;
    let glyph_scale = run.size_pt / upem;
    let (r, g, b) = colorref_rgb(color);
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, 255);
    paint.anti_alias = true;
    let mut pen_x = x;
    for glyph in &run.glyphs {
        let mut builder = OutlinePath::default();
        if face
            .outline_glyph(ttf_parser::GlyphId(glyph.id), &mut builder)
            .is_some()
            && let Some(path) = builder.path.finish()
        {
            // 크기 스케일·y뒤집기(폰트 y-up)·장평·기울임·베이스라인 이동·DPI 스케일
            let mut t = Transform::from_scale(glyph_scale * run.x_scale, -glyph_scale);
            if run.italic {
                t = t.post_concat(Transform::from_skew(-ITALIC_SKEW, 0.0));
            }
            t = t.post_translate(pen_x + glyph.x_offset + dx, y - glyph.y_offset + dy);
            t = t.post_scale(px_scale, px_scale);
            pixmap.fill_path(&path, &paint, FillRule::Winding, t, None);
            if run.bold {
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

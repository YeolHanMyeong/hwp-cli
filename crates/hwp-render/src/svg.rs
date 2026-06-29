//! SVG 백엔드 — DisplayList를 페이지별 SVG 문서로 직렬화.
//!
//! 글리프는 윤곽선 `<path>`로 그린다(뷰어 폰트 의존 제거 — 픽셀 정확도
//! 우선). 이미지는 base64 data URI로 임베드한다.

use std::collections::HashMap;
use std::fmt::Write as _;

use rustybuzz::ttf_parser;

use crate::display::{DisplayList, Item, PageList, PathCmd};

pub fn render_svg(list: &DisplayList) -> Vec<String> {
    list.pages.iter().map(render_page).collect()
}

fn render_page(page: &PageList) -> String {
    let (w, h) = (page.width_pt, page.height_pt);
    let mut out = String::with_capacity(64 * 1024);
    let _ = write!(
        out,
        r##"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w:.2}pt" height="{h:.2}pt" viewBox="0 0 {w:.2} {h:.2}">
<rect width="{w:.2}" height="{h:.2}" fill="#ffffff"/>
"##
    );

    // (폰트 데이터 주소, 글리프 ID) → path d 캐시
    let mut outline_cache: HashMap<(usize, u16), Option<String>> = HashMap::new();

    for item in &page.items {
        match item {
            Item::Rect {
                x,
                y,
                w: rw,
                h: rh,
                fill,
            } => {
                let _ = writeln!(
                    out,
                    r#"<rect x="{x:.2}" y="{y:.2}" width="{rw:.2}" height="{rh:.2}" fill="{}"/>"#,
                    hex_color(*fill)
                );
            }
            Item::Line {
                x1,
                y1,
                x2,
                y2,
                color,
                width,
            } => {
                let _ = writeln!(
                    out,
                    r#"<line x1="{x1:.2}" y1="{y1:.2}" x2="{x2:.2}" y2="{y2:.2}" stroke="{}" stroke-width="{width:.2}"/>"#,
                    hex_color(*color)
                );
            }
            Item::Image {
                x,
                y,
                w: iw,
                h: ih,
                data,
            } => {
                let mime = sniff_mime(data);
                let _ = writeln!(
                    out,
                    r#"<image x="{x:.2}" y="{y:.2}" width="{iw:.2}" height="{ih:.2}" preserveAspectRatio="none" href="data:{mime};base64,{}"/>"#,
                    base64(data)
                );
            }
            Item::Glyphs { x, y, run } => {
                let Ok(face) = ttf_parser::Face::parse(&run.font.data, run.font.index) else {
                    continue;
                };
                let upem = face.units_per_em() as f32;
                let s = run.size_pt / upem;
                let font_key = run.font.data.as_ptr() as usize;
                let color = hex_color(run.color);
                let skew_c = if run.italic { 0.2126 * s } else { 0.0 };
                let stroke = if run.bold {
                    format!(r#" stroke="{color}" stroke-width="{:.1}""#, 0.03 * upem)
                } else {
                    String::new()
                };

                let mut pen_x = *x;
                for glyph in &run.glyphs {
                    let d = outline_cache
                        .entry((font_key, glyph.id))
                        .or_insert_with(|| glyph_path(&face, glyph.id))
                        .clone();
                    if let Some(d) = d {
                        let (a, dd) = (s * run.x_scale, -s);
                        let (e, f) = (pen_x + glyph.x_offset, y - glyph.y_offset);
                        let _ = writeln!(
                            out,
                            r#"<path transform="matrix({a:.4} 0 {skew_c:.4} {dd:.4} {e:.2} {f:.2})" d="{d}" fill="{color}"{stroke}/>"#
                        );
                    }
                    pen_x += glyph.x_advance;
                }
            }
            Item::Path {
                commands,
                fill,
                stroke,
            } => {
                let mut d = String::new();
                for cmd in commands {
                    match *cmd {
                        PathCmd::MoveTo(x, y) => {
                            let _ = write!(d, "M{x:.2} {y:.2}");
                        }
                        PathCmd::LineTo(x, y) => {
                            let _ = write!(d, "L{x:.2} {y:.2}");
                        }
                        PathCmd::CubicTo(a, b, c, e, f, g) => {
                            let _ = write!(d, "C{a:.2} {b:.2} {c:.2} {e:.2} {f:.2} {g:.2}");
                        }
                        PathCmd::Close => d.push('Z'),
                    }
                }
                let fill_attr = fill.map_or_else(|| "none".to_string(), hex_color);
                let stroke_attr = match stroke {
                    Some((c, w)) => {
                        format!(r#" stroke="{}" stroke-width="{:.2}""#, hex_color(*c), w)
                    }
                    None => String::new(),
                };
                let _ = writeln!(out, r#"<path d="{d}" fill="{fill_attr}"{stroke_attr}/>"#);
            }
        }
    }
    out.push_str("</svg>\n");
    out
}

fn glyph_path(face: &ttf_parser::Face<'_>, glyph_id: u16) -> Option<String> {
    let mut builder = SvgPath(String::new());
    face.outline_glyph(ttf_parser::GlyphId(glyph_id), &mut builder)?;
    Some(builder.0)
}

struct SvgPath(String);

impl ttf_parser::OutlineBuilder for SvgPath {
    fn move_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.0, "M{x:.1} {y:.1}");
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let _ = write!(self.0, "L{x:.1} {y:.1}");
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let _ = write!(self.0, "Q{x1:.1} {y1:.1} {x:.1} {y:.1}");
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let _ = write!(self.0, "C{x1:.1} {y1:.1} {x2:.1} {y2:.1} {x:.1} {y:.1}");
    }
    fn close(&mut self) {
        self.0.push('Z');
    }
}

/// COLORREF(0x00BBGGRR) → "#rrggbb". 없음(0xFFFFFFFF)은 검정.
fn hex_color(c: u32) -> String {
    if c == 0xFFFF_FFFF {
        return "#000000".to_string();
    }
    format!(
        "#{:02x}{:02x}{:02x}",
        c & 0xFF,
        (c >> 8) & 0xFF,
        (c >> 16) & 0xFF
    )
}

fn sniff_mime(data: &[u8]) -> &'static str {
    match data {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [0xFF, 0xD8, ..] => "image/jpeg",
        [b'G', b'I', b'F', b'8', ..] => "image/gif",
        [b'B', b'M', ..] => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// 표준 base64 인코딩 (의존성 없이).
fn base64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_인코딩() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn 색_변환() {
        assert_eq!(hex_color(0x00FF0000), "#0000ff"); // BGR → 파랑
        assert_eq!(hex_color(0), "#000000");
    }
}

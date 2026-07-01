//! 이미지 삽입 — 앵커 텍스트 뒤에 새 그림(Picture)을 꽂는다.
//!
//! writer가 빈-extras Picture에 hwp5 도형 레코드(SHAPE_COMPONENT + 그림)를 합성하므로
//! (hwpx→hwp와 동일한 검증된 경로), 여기서는 최소 Picture + BinStream + 앵커 ExtCtrl만
//! 만든다. bin_ref=ItemRef(name)로 hwpx 출력·hwp5 합성·렌더가 모두 바이트를 해석한다.

use std::path::Path;

use hwp_model::{BinRef, BinStream, Control, Document, HwpChar, HwpUnit, Paragraph, Picture};

use crate::edit::{adjust_runs, find_match, utf16_len};
use crate::field::{relink_ctrl_index, rev_payload};

/// gso 개체(그림/표 등) 확장 컨트롤 문자 코드.
const GSO_CODE: u16 = 11;
/// mm → HWPUNIT(1/7200 inch = 1/100 pt): 7200/25.4.
const MM_TO_HWPUNIT: f32 = 283.464_57;
/// PageDef 없을 때 본문 폭 기본값(HWPUNIT, ≈400pt).
const DEFAULT_CONTENT_WIDTH: i32 = 40_000;

/// 삽입 이미지 표시 크기.
pub enum ImageSize {
    /// 원본 픽셀 크기(96 DPI 기준), 본문 폭 초과 시 비례 축소.
    Natural,
    /// 밀리미터 지정(너비, 높이).
    Mm(f32, f32),
}

/// PNG/GIF/BMP/JPEG 헤더에서 픽셀 (너비, 높이)를 읽는다(무의존 헤더 파싱).
pub fn image_pixel_size(data: &[u8]) -> Option<(u32, u32)> {
    // PNG: IHDR 폭/높이 at 16..24 (big-endian)
    if data.len() >= 24 && data.starts_with(b"\x89PNG\r\n\x1a\n") {
        let w = u32::from_be_bytes(data[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(data[20..24].try_into().ok()?);
        return Some((w, h));
    }
    // GIF: Logical Screen Descriptor at 6..10 (little-endian)
    if data.len() >= 10 && data.starts_with(b"GIF") {
        let w = u16::from_le_bytes([data[6], data[7]]) as u32;
        let h = u16::from_le_bytes([data[8], data[9]]) as u32;
        return Some((w, h));
    }
    // BMP: BITMAPINFOHEADER 폭/높이 at 18..26 (little-endian)
    if data.len() >= 26 && data.starts_with(b"BM") {
        let w = i32::from_le_bytes(data[18..22].try_into().ok()?);
        let h = i32::from_le_bytes(data[22..26].try_into().ok()?);
        return Some((w.unsigned_abs(), h.unsigned_abs()));
    }
    // JPEG: SOF 마커(0xFFC0~0xFFCF, C4/C8/CC 제외)에서 높이·너비
    if data.len() >= 4 && data[0] == 0xFF && data[1] == 0xD8 {
        let mut i = 2;
        while i + 9 < data.len() {
            if data[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = data[i + 1];
            if (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC
            {
                let h = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
                let w = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
                return Some((w, h));
            }
            let seg = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
            if seg < 2 {
                break;
            }
            i += 2 + seg;
        }
    }
    None
}

/// 문서 첫 구역의 본문 폭(HWPUNIT). PageDef 없으면 A4 근사 기본값.
fn content_width(doc: &Document) -> i32 {
    doc.sections
        .first()
        .and_then(|s| s.section_def())
        .and_then(|sd| sd.page)
        .map(|p| (p.width.0 - p.margin_left.0 - p.margin_right.0).max(1))
        .unwrap_or(DEFAULT_CONTENT_WIDTH)
}

/// 확장자(소문자) 추출·검증. 지원: png/jpg/jpeg/bmp/gif.
fn ext_of(path: &Path) -> Result<String, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .ok_or_else(|| format!("이미지 확장자를 알 수 없습니다: {}", path.display()))?;
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "bmp" | "gif" => Ok(ext),
        other => Err(format!(
            "지원하지 않는 이미지 형식: {other:?} (png/jpg/jpeg/bmp/gif)"
        )),
    }
}

/// 표시 크기(HWPUNIT)를 계산한다. 자연 크기는 본문 폭(max_w) 초과 시 비례 축소.
fn display_size(data: &[u8], size: &ImageSize, max_w: i32) -> (i32, i32) {
    match size {
        ImageSize::Mm(w, h) => (
            (*w * MM_TO_HWPUNIT).round() as i32,
            (*h * MM_TO_HWPUNIT).round() as i32,
        ),
        ImageSize::Natural => {
            let (pw, ph) = image_pixel_size(data).unwrap_or((300, 200));
            let w = pw as i64 * 7200 / 96;
            let h = ph as i64 * 7200 / 96;
            if w > i64::from(max_w) && w > 0 {
                let scale = f64::from(max_w) / w as f64;
                (max_w, (h as f64 * scale).round() as i32)
            } else {
                (w as i32, h as i32)
            }
        }
    }
}

/// 한 문단에서 앵커 텍스트 뒤에 그림 앵커를 삽입한다. 반환=삽입 여부.
fn insert_image_in_para(para: &mut Paragraph, anchor: &str, pic: &Picture) -> bool {
    let Some((cidx, wpos)) = find_match(&para.chars, anchor, 0) else {
        return false;
    };
    let ins = (cidx + anchor.chars().count()).min(para.chars.len());
    let iw = wpos + utf16_len(anchor);
    // control 삽입 위치 = ins 이전 ExtCtrl 개수(등장순서가 chars와 정합해야 함).
    let ci = para.chars[..ins]
        .iter()
        .filter(|c| matches!(c, HwpChar::ExtCtrl { .. }))
        .count()
        .min(para.controls.len());
    para.controls.insert(ci, Control::Picture(pic.clone()));
    para.chars.insert(
        ins,
        HwpChar::ExtCtrl {
            code: GSO_CODE,
            ctrl_id: *b"gso ",
            payload: rev_payload(b"gso "),
            ctrl_index: None,
        },
    );
    adjust_runs(&mut para.char_shape_runs, iw, 0, 8); // ExtCtrl wchar_width=8
    relink_ctrl_index(para);
    para.header.ctrl_mask = 0; // writer가 chars에서 재계산(gso bit11 포함)
    para.line_segs.clear();
    true
}

/// 본문/표 셀/글상자 문단을 재귀로 훑어 첫 매칭에 그림을 삽입한다.
fn insert_image_rec(para: &mut Paragraph, anchor: &str, pic: &Picture) -> bool {
    if insert_image_in_para(para, anchor, pic) {
        return true;
    }
    for ctrl in &mut para.controls {
        match ctrl {
            Control::Table(t) => {
                for cell in &mut t.cells {
                    for p in &mut cell.paragraphs {
                        if insert_image_rec(p, anchor, pic) {
                            return true;
                        }
                    }
                }
            }
            Control::Generic(g) => {
                for l in &mut g.paragraph_lists {
                    for p in &mut l.paragraphs {
                        if insert_image_rec(p, anchor, pic) {
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// `anchor` 텍스트를 가진 첫 문단의 그 뒤에 `path` 이미지를 인라인(글자처럼)으로 삽입한다.
/// writer(hwp5)가 빈-extras Picture에 도형 레코드를 합성한다.
pub fn insert_image(
    doc: &mut Document,
    anchor: &str,
    path: &Path,
    size: ImageSize,
) -> Result<(), String> {
    let ext = ext_of(path)?;
    let data =
        std::fs::read(path).map_err(|e| format!("이미지 읽기 실패 {}: {e}", path.display()))?;
    if data.is_empty() {
        return Err(format!("빈 이미지 파일: {}", path.display()));
    }
    let (w, h) = display_size(&data, &size, content_width(doc));
    let name = format!("inserted{}.{ext}", doc.bin_streams.len() + 1);
    let pic = Picture {
        common_data: Vec::new(),
        width: HwpUnit(w.max(1)),
        height: HwpUnit(h.max(1)),
        treat_as_char: true,
        z_order: 0,
        vert_offset: 0,
        horz_offset: 0,
        bin_ref: BinRef::ItemRef(name.clone()),
        extras: Vec::new(),
    };
    let inserted = doc
        .sections
        .iter_mut()
        .flat_map(|s| &mut s.paragraphs)
        .any(|p| insert_image_rec(p, anchor, &pic));
    if !inserted {
        return Err(format!("앵커 {anchor:?}를 찾을 수 없습니다"));
    }
    doc.bin_streams.push(BinStream { name, data });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// image_pixel_size가 최소 PNG/BMP 헤더에서 치수를 읽는다.
    #[test]
    fn 픽셀_치수_헤더_파싱() {
        // PNG: 시그니처(8) + IHDR len(4) + "IHDR"(4) + w(4 BE) + h(4 BE)
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend([0, 0, 0, 13]);
        png.extend(b"IHDR");
        png.extend(200u32.to_be_bytes());
        png.extend(100u32.to_be_bytes());
        assert_eq!(image_pixel_size(&png), Some((200, 100)));

        // BMP: "BM" + 16바이트 채운 뒤 폭/높이 at 18/22 (LE)
        let mut bmp = b"BM".to_vec();
        bmp.extend([0u8; 16]);
        bmp.extend(50i32.to_le_bytes());
        bmp.extend(40i32.to_le_bytes());
        assert_eq!(image_pixel_size(&bmp), Some((50, 40)));
    }

    /// insert_image가 Picture+BinStream을 만들고 앵커 링크가 맞는다.
    #[test]
    fn 이미지_삽입_구조() {
        let mut doc = crate::from_markdown::from_markdown("사진: 여기");
        let dir = std::env::temp_dir().join("hwp-img-test");
        std::fs::create_dir_all(&dir).unwrap();
        let png_path = dir.join("t.png");
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend([0, 0, 0, 13]);
        png.extend(b"IHDR");
        png.extend(96u32.to_be_bytes());
        png.extend(96u32.to_be_bytes());
        png.extend([0u8; 8]); // 나머지(파싱 안 함)
        std::fs::write(&png_path, &png).unwrap();

        insert_image(&mut doc, "사진:", &png_path, ImageSize::Natural).unwrap();

        // BinStream 1개 + Picture 1개 + resolve_bin 성공.
        assert_eq!(doc.bin_streams.len(), 1);
        let para = &doc.sections[0].paragraphs[0];
        let pic = para.controls.iter().find_map(|c| match c {
            Control::Picture(p) => Some(p),
            _ => None,
        });
        let pic = pic.expect("Picture 존재");
        assert!(pic.extras.is_empty(), "writer가 합성하도록 빈 extras");
        assert!(doc.resolve_bin(&pic.bin_ref).is_some(), "bin_ref 해석");
        // 앵커 ExtCtrl가 Picture를 가리킨다.
        let ext = para.chars.iter().find_map(|c| match c {
            HwpChar::ExtCtrl {
                code, ctrl_index, ..
            } if *code == GSO_CODE => *ctrl_index,
            _ => None,
        });
        assert!(
            matches!(para.controls[ext.unwrap() as usize], Control::Picture(_)),
            "앵커 ExtCtrl가 Picture 컨트롤을 가리켜야 한다"
        );
        // 96px → 96*7200/96 = 7200 HWPUNIT.
        assert_eq!(pic.width.0, 7200);

        // 없는 앵커는 오류.
        assert!(insert_image(&mut doc, "없는앵커", &png_path, ImageSize::Natural).is_err());
    }
}

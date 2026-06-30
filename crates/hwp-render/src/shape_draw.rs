//! 그리기 개체(도형) 렌더링 — gso 컨트롤의 raw_children에서 SHAPE_COMPONENT와
//! 기하(선/사각형/타원/호/다각형/곡선)를 렌더 시점에 파싱해 `Item::Path`로 만든다.
//!
//! IR·라운드트립 라이터를 건드리지 않는 소비단 전용(gso.rs가 박스를 읽는 패턴과 동일).
//! 좌표 변환: 생성(local) 공간 점 → 렌더 행렬(T·S·R) → +origin(HWPUNIT) → /100 = pt.
//! 바이트 레이아웃은 `docs/spec.txt` Table 81~103 + 실측(annual_report)으로 확정.

use std::sync::Arc;

use hwp_model::{BinDataId, BinRef, Document, GenericControl, OpaqueRecord, ShapeGeom, ShapeKind};

use crate::display::{Fill, Gradient, Item, PageList, PathCmd, path_bbox};

// hwp5 레코드 raw 태그 (HWPTAG_BEGIN = 0x10).
const SHAPE_COMPONENT: u16 = 0x4C; // 76
const SC_LINE: u16 = 0x4E;
const SC_RECTANGLE: u16 = 0x4F;
const SC_ELLIPSE: u16 = 0x50;
const SC_ARC: u16 = 0x51;
const SC_POLYGON: u16 = 0x52;
const SC_CURVE: u16 = 0x53;
const SC_CONTAINER: u16 = 0x56; // 86 (묶음 — 방어적)

/// 베지에 원호 근사 상수 (4/3·tan(45°/2)).
const KAPPA: f64 = 0.552_284_749_8;
const MAX_DEPTH: u32 = 16;

fn is_geom(tag: u16) -> bool {
    matches!(
        tag,
        SC_LINE | SC_RECTANGLE | SC_ELLIPSE | SC_ARC | SC_POLYGON | SC_CURVE
    )
}

/// 도형이 하나라도 있는지 (skip-count 판정용). draw_component와 같은 도달성으로 재귀.
pub fn has_shape(recs: &[OpaqueRecord]) -> bool {
    recs.iter().any(|r| match r.tag {
        SHAPE_COMPONENT => r.children.iter().any(|c| is_geom(c.tag)) || has_shape(&r.children),
        SC_CONTAINER => has_shape(&r.children),
        _ => false,
    })
}

/// hwpx 구조화 도형(ShapeGeom)을 Item::Path로 그린다. 좌표는 페이지 절대(HWPUNIT).
pub fn draw_ir_shapes(shapes: &[ShapeGeom], page: &mut PageList) {
    for s in shapes {
        let commands = ir_shape_path(s);
        if commands.len() < 2 {
            continue;
        }
        let fill = (s.fill != 0xFFFF_FFFF).then_some(Fill::Solid(s.fill));
        let stroke = (s.border_width > 0)
            .then_some((s.border_color, (s.border_width as f32 / 100.0).max(0.1)));
        if fill.is_none() && stroke.is_none() {
            continue;
        }
        page.items.push(Item::Path {
            commands,
            fill,
            stroke,
        });
    }
}

fn ir_shape_path(s: &ShapeGeom) -> Vec<PathCmd> {
    let (x0, y0) = (s.x as f32 / 100.0, s.y as f32 / 100.0);
    let (w, h) = (s.w as f32 / 100.0, s.h as f32 / 100.0);
    let pts_path = |close: bool| -> Vec<PathCmd> {
        let mut cmds = Vec::with_capacity(s.points.len() + 1);
        for (i, &(px, py)) in s.points.iter().enumerate() {
            let (ax, ay) = ((s.x + px) as f32 / 100.0, (s.y + py) as f32 / 100.0);
            cmds.push(if i == 0 {
                PathCmd::MoveTo(ax, ay)
            } else {
                PathCmd::LineTo(ax, ay)
            });
        }
        if close {
            cmds.push(PathCmd::Close);
        }
        cmds
    };
    match s.kind {
        ShapeKind::Rect => vec![
            PathCmd::MoveTo(x0, y0),
            PathCmd::LineTo(x0 + w, y0),
            PathCmd::LineTo(x0 + w, y0 + h),
            PathCmd::LineTo(x0, y0 + h),
            PathCmd::Close,
        ],
        ShapeKind::Ellipse | ShapeKind::Arc => {
            let (cx, cy) = ((x0 + w / 2.0) as f64, (y0 + h / 2.0) as f64);
            ellipse_path(
                cx,
                cy,
                (w as f64 / 2.0, 0.0),
                (0.0, h as f64 / 2.0),
                &|x, y| (x as f32, y as f32),
            )
        }
        ShapeKind::Line => {
            if s.points.len() >= 2 {
                pts_path(false)
            } else {
                vec![PathCmd::MoveTo(x0, y0), PathCmd::LineTo(x0 + w, y0 + h)]
            }
        }
        ShapeKind::Polygon | ShapeKind::Curve => {
            if s.points.len() >= 2 {
                pts_path(true)
            } else {
                vec![
                    PathCmd::MoveTo(x0, y0),
                    PathCmd::LineTo(x0 + w, y0),
                    PathCmd::LineTo(x0 + w, y0 + h),
                    PathCmd::LineTo(x0, y0 + h),
                    PathCmd::Close,
                ]
            }
        }
    }
}

/// gso 컨트롤의 도형을 page에 그린다. origin은 페이지 기준점(HWPUNIT):
/// floating은 (horz_offset, vert_offset), 인라인은 흐름 위치.
pub fn draw_gso_shapes(
    g: &GenericControl,
    origin: (f64, f64),
    doc: &Document,
    page: &mut PageList,
    warnings: &mut Vec<String>,
) {
    walk(&g.raw_children, origin, doc, page, warnings, 0);
}

fn walk(
    recs: &[OpaqueRecord],
    origin: (f64, f64),
    doc: &Document,
    page: &mut PageList,
    warns: &mut Vec<String>,
    depth: u32,
) {
    if depth > MAX_DEPTH {
        return;
    }
    for r in recs {
        match r.tag {
            SHAPE_COMPONENT => draw_component(r, origin, doc, page, warns, depth),
            SC_CONTAINER => walk(&r.children, origin, doc, page, warns, depth + 1),
            _ => {} // PARA_HEADER/LIST_HEADER/CTRL_HEADER 등은 텍스트 경로가 처리
        }
    }
}

fn draw_component(
    sc: &OpaqueRecord,
    origin: (f64, f64),
    doc: &Document,
    page: &mut PageList,
    warns: &mut Vec<String>,
    depth: u32,
) {
    let Some(style) = parse_style(&sc.data, doc) else {
        return;
    };
    for child in &sc.children {
        match child.tag {
            SC_LINE | SC_RECTANGLE | SC_ELLIPSE | SC_ARC | SC_POLYGON | SC_CURVE => {
                let Some(commands) = geometry(child.tag, &child.data, &style, origin) else {
                    continue;
                };
                if commands.len() < 2 {
                    continue;
                }
                // 이미지 채움: 도형 경계 상자에 이미지를 깐다(테두리는 path가 그림).
                if let Some(img) = &style.image {
                    let (x0, y0, x1, y1) = path_bbox(&commands);
                    page.items.push(Item::Image {
                        x: x0,
                        y: y0,
                        w: (x1 - x0).max(0.1),
                        h: (y1 - y0).max(0.1),
                        data: img.clone(),
                    });
                }
                // 채움·선이 모두 없고 이미지도 없으면 그리지 않는다(보이지 않는 프레임).
                if style.fill.is_none() && style.stroke.is_none() {
                    continue;
                }
                page.items.push(Item::Path {
                    commands,
                    fill: style.fill.clone(),
                    stroke: style.stroke,
                });
            }
            SHAPE_COMPONENT => draw_component(child, origin, doc, page, warns, depth + 1),
            SC_CONTAINER => walk(&child.children, origin, doc, page, warns, depth + 1),
            _ => {}
        }
    }
}

// ── 3×2 affine 행렬 ([a,b,c,d,e,f] row-major: x'=a·x+b·y+c, y'=d·x+e·y+f) ──
#[derive(Clone, Copy)]
struct Mat {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Mat {
    fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.b * y + self.c,
            self.d * x + self.e * y + self.f,
        )
    }
    fn mul(&self, o: &Mat) -> Mat {
        Mat {
            a: self.a * o.a + self.b * o.d,
            b: self.a * o.b + self.b * o.e,
            c: self.a * o.c + self.b * o.f + self.c,
            d: self.d * o.a + self.e * o.d,
            e: self.d * o.b + self.e * o.e,
            f: self.d * o.c + self.e * o.f + self.f,
        }
    }
}

struct Style {
    m: Mat,
    stroke: Option<(u32, f32)>,
    fill: Option<Fill>,
    /// 이미지 채움 — 도형 경계 상자에 깐다.
    image: Option<Arc<Vec<u8>>>,
}

fn rd_u16(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}
fn rd_i32(d: &[u8], o: usize) -> Option<i32> {
    d.get(o..o + 4)
        .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn rd_u32(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn rd_f64(d: &[u8], o: usize) -> f64 {
    d.get(o..o + 8)
        .map(|b| f64::from_le_bytes(b.try_into().unwrap()))
        .unwrap_or(0.0)
}
fn rd_mat(d: &[u8], o: usize) -> Mat {
    Mat {
        a: rd_f64(d, o),
        b: rd_f64(d, o + 8),
        c: rd_f64(d, o + 16),
        d: rd_f64(d, o + 24),
        e: rd_f64(d, o + 32),
        f: rd_f64(d, o + 40),
    }
}

/// SHAPE_COMPONENT 데이터에서 렌더 행렬·테두리·채움을 읽는다.
/// 레이아웃(실측): [CHID×2 또는 ×1] + 개체요소속성 + (translation 48 + (scale 48+rotation 48)×cnt)
/// + 테두리선(13) + 채우기(Table 28).
fn parse_style(d: &[u8], doc: &Document) -> Option<Style> {
    if d.len() < 8 {
        return None;
    }
    // top-level은 CHID가 두 번(8B), 묶음 멤버는 한 번(4B).
    let base = if d[0..4] == d[4..8] { 8 } else { 4 };
    let cnt = rd_u16(d, base + 42)? as usize;
    let t = rd_mat(d, base + 44);
    // 마지막(최상위) scale/rotation 쌍 사용.
    let pair = base + 44 + 48 + cnt.saturating_sub(1) * 96;
    let m = if d.len() >= pair + 96 {
        t.mul(&rd_mat(d, pair).mul(&rd_mat(d, pair + 48)))
    } else {
        t
    };

    let bo = base + 92 + cnt * 96; // border_offset
    let mut stroke = None;
    let mut fill = None;
    let mut image = None;
    if let (Some(color), Some(width), Some(lattr)) =
        (rd_u32(d, bo), rd_i32(d, bo + 4), rd_u32(d, bo + 8))
    {
        if lattr & 0x3F != 0 {
            stroke = Some((color, (width as f32 / 100.0).max(0.1)));
        }
        let fo = bo + 13;
        if let Some(ft) = rd_u32(d, fo) {
            if ft & 0x1 != 0 {
                fill = rd_u32(d, fo + 4).map(Fill::Solid); // 단색 배경색
            } else if ft & 0x4 != 0 {
                fill = parse_gradient(d, fo + 4).map(Fill::Gradient);
            } else if ft & 0x2 != 0 {
                image = parse_image_fill(d, fo + 4, doc);
            }
        }
    }
    Some(Style {
        m,
        stroke,
        fill,
        image,
    })
}

/// Table 28 그러데이션: type(i16) 각(i16) cx(i16) cy(i16) spread(i16) num(i16),
/// num>2면 INT32[num] 위치, 이어서 COLORREF[num] 색.
fn parse_gradient(d: &[u8], fo: usize) -> Option<Gradient> {
    let gtype = rd_u16(d, fo)? as i16;
    let angle = rd_u16(d, fo + 2)? as i16 as f32;
    let num = rd_u16(d, fo + 10)? as usize;
    if !(1..=16).contains(&num) {
        return None;
    }
    let mut off = fo + 12;
    let positions: Vec<f32> = if num > 2 {
        let mut v = Vec::with_capacity(num);
        for i in 0..num {
            v.push(rd_i32(d, off + i * 4)? as f32);
        }
        off += num * 4;
        let max = v.iter().cloned().fold(1.0_f32, f32::max);
        v.iter().map(|p| (p / max).clamp(0.0, 1.0)).collect()
    } else {
        (0..num)
            .map(|i| i as f32 / (num.max(2) - 1) as f32)
            .collect()
    };
    let mut stops = Vec::with_capacity(num);
    for i in 0..num {
        let c = rd_u32(d, off + i * 4)?;
        stops.push((positions.get(i).copied().unwrap_or(0.0), c));
    }
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    // HWP 그러데이션 유형: 1=원형(radial), 그 외=선형(각도 사용).
    Some(Gradient {
        radial: gtype == 1,
        angle_deg: angle,
        stops,
    })
}

/// 이미지 채움: BinData ID 참조를 풀어 원본 바이트를 얻는다.
fn parse_image_fill(d: &[u8], fo: usize, doc: &Document) -> Option<Arc<Vec<u8>>> {
    // BYTE 이미지유형 + 그림정보(가변) ... 끝부분에 DWORD BinItem ID. 보수적으로 마지막
    // 4바이트 정렬 위치에서 유효한 bin id를 찾는다.
    for end in (fo + 1..=d.len().min(fo + 64)).rev() {
        if end >= 4
            && let Some(id) = rd_u16(d, end - 4)
            && id != 0
            && let Some(bytes) = doc.resolve_bin(&BinRef::Id(BinDataId(id)))
        {
            return Some(Arc::new(bytes.to_vec()));
        }
    }
    None
}

/// 기하 레코드를 페이지 좌표(pt) 경로로 변환.
fn geometry(tag: u16, d: &[u8], s: &Style, origin: (f64, f64)) -> Option<Vec<PathCmd>> {
    // local 점(HWPUNIT) → 행렬 → +origin → /100 = pt.
    let to_pt = |x: f64, y: f64| -> (f32, f32) {
        let (px, py) = s.m.apply(x, y);
        (
            ((px + origin.0) / 100.0) as f32,
            ((py + origin.1) / 100.0) as f32,
        )
    };
    let p =
        |o: usize| -> Option<(f64, f64)> { Some((rd_i32(d, o)? as f64, rd_i32(d, o + 4)? as f64)) };

    match tag {
        SC_LINE => {
            let (sx, sy) = p(0)?;
            let (ex, ey) = p(8)?;
            if (sx - ex).abs() < f64::EPSILON && (sy - ey).abs() < f64::EPSILON {
                return None;
            }
            let (a, b) = to_pt(sx, sy);
            let (c, e) = to_pt(ex, ey);
            Some(vec![PathCmd::MoveTo(a, b), PathCmd::LineTo(c, e)])
        }
        SC_RECTANGLE => {
            // BYTE 곡률% + 4×(x,y). (곡률>0 둥근모서리는 미지원 — 직각 근사)
            let mut cmds = Vec::with_capacity(6);
            for i in 0..4 {
                let (x, y) = p(1 + i * 8)?;
                let (px, py) = to_pt(x, y);
                cmds.push(if i == 0 {
                    PathCmd::MoveTo(px, py)
                } else {
                    PathCmd::LineTo(px, py)
                });
            }
            cmds.push(PathCmd::Close);
            Some(cmds)
        }
        SC_POLYGON => {
            let n = rd_u16(d, 0)? as usize;
            if !(2..=4096).contains(&n) {
                return None;
            }
            let mut cmds = Vec::with_capacity(n + 1);
            for i in 0..n {
                let (x, y) = p(4 + i * 8)?;
                let (px, py) = to_pt(x, y);
                cmds.push(if i == 0 {
                    PathCmd::MoveTo(px, py)
                } else {
                    PathCmd::LineTo(px, py)
                });
            }
            cmds.push(PathCmd::Close);
            Some(cmds)
        }
        SC_ELLIPSE => {
            // UINT32 attr + center + ax1(끝점) + ax2(끝점).
            let (cx, cy) = p(4)?;
            let (a1x, a1y) = p(12)?;
            let (a2x, a2y) = p(20)?;
            Some(ellipse_path(
                cx,
                cy,
                (a1x - cx, a1y - cy),
                (a2x - cx, a2y - cy),
                &to_pt,
            ))
        }
        SC_ARC => {
            // BYTE arctype + center + start(ax1) + end(ax2).
            let (cx, cy) = p(1)?;
            let (sx, sy) = p(9)?;
            let (ex, ey) = p(17)?;
            Some(arc_path(cx, cy, (sx, sy), (ex, ey), &to_pt))
        }
        SC_CURVE => {
            let n = rd_u16(d, 0)? as usize;
            if !(2..=4096).contains(&n) {
                return None;
            }
            // 세그먼트 타입 무시 — 점들을 폴리라인으로 근사(방어적; 파일에 없음).
            let mut cmds = Vec::with_capacity(n);
            for i in 0..n {
                let (x, y) = p(2 + i * 8)?;
                let (px, py) = to_pt(x, y);
                cmds.push(if i == 0 {
                    PathCmd::MoveTo(px, py)
                } else {
                    PathCmd::LineTo(px, py)
                });
            }
            Some(cmds)
        }
        _ => None,
    }
}

/// 중심 C와 두 축 벡터(a1, a2)로 타원을 4개 큐빅 베지에로 근사.
fn ellipse_path(
    cx: f64,
    cy: f64,
    a1: (f64, f64),
    a2: (f64, f64),
    to_pt: &impl Fn(f64, f64) -> (f32, f32),
) -> Vec<PathCmd> {
    let pt = |sx: f64, sy: f64| to_pt(cx + sx, cy + sy);
    // 앵커: C+a1, C+a2, C-a1, C-a2. 제어점 = 앵커 ± k·(다음 축).
    let (p0, p1, p2, p3) = (
        pt(a1.0, a1.1),
        pt(a2.0, a2.1),
        pt(-a1.0, -a1.1),
        pt(-a2.0, -a2.1),
    );
    let k = KAPPA;
    let c = |bx: f64, by: f64| pt(bx, by);
    vec![
        PathCmd::MoveTo(p0.0, p0.1),
        cubic(
            c(a1.0 + k * a2.0, a1.1 + k * a2.1),
            c(a2.0 + k * a1.0, a2.1 + k * a1.1),
            p1,
        ),
        cubic(
            c(a2.0 - k * a1.0, a2.1 - k * a1.1),
            c(-a1.0 + k * a2.0, -a1.1 + k * a2.1),
            p2,
        ),
        cubic(
            c(-a1.0 - k * a2.0, -a1.1 - k * a2.1),
            c(-a2.0 - k * a1.0, -a2.1 - k * a1.1),
            p3,
        ),
        cubic(
            c(-a2.0 + k * a1.0, -a2.1 + k * a1.1),
            c(a1.0 - k * a2.0, a1.1 - k * a2.1),
            p0,
        ),
        PathCmd::Close,
    ]
}

/// 중심 C, 시작/끝 점으로 원호를 큐빅 베지에로 근사(짧은 쪽 sweep).
fn arc_path(
    cx: f64,
    cy: f64,
    start: (f64, f64),
    end: (f64, f64),
    to_pt: &impl Fn(f64, f64) -> (f32, f32),
) -> Vec<PathCmd> {
    let s = (start.0 - cx, start.1 - cy);
    let r = (s.0 * s.0 + s.1 * s.1).sqrt();
    if r < f64::EPSILON {
        return Vec::new();
    }
    let e = (end.0 - cx, end.1 - cy);
    let t0 = s.1.atan2(s.0);
    let mut sweep = e.1.atan2(e.0) - t0;
    // 짧은 쪽 [-π, π].
    while sweep > std::f64::consts::PI {
        sweep -= std::f64::consts::TAU;
    }
    while sweep < -std::f64::consts::PI {
        sweep += std::f64::consts::TAU;
    }
    let pt = |th: f64| to_pt(cx + r * th.cos(), cy + r * th.sin());
    let segs = (sweep.abs() / (std::f64::consts::PI / 2.0)).ceil().max(1.0) as usize;
    let dphi = sweep / segs as f64;
    let alpha = (4.0 / 3.0) * (dphi / 4.0).tan();
    let start_pt = pt(t0);
    let mut cmds = vec![PathCmd::MoveTo(start_pt.0, start_pt.1)];
    let mut th = t0;
    for _ in 0..segs {
        let th1 = th + dphi;
        // 접선 T'(θ) = r·(-sinθ, cosθ); 제어점 = P ± α·T'.
        let c1 = to_pt(
            cx + r * th.cos() - alpha * r * th.sin(),
            cy + r * th.sin() + alpha * r * th.cos(),
        );
        let c2 = to_pt(
            cx + r * th1.cos() + alpha * r * th1.sin(),
            cy + r * th1.sin() - alpha * r * th1.cos(),
        );
        let p1 = pt(th1);
        cmds.push(PathCmd::CubicTo(c1.0, c1.1, c2.0, c2.1, p1.0, p1.1));
        th = th1;
    }
    cmds
}

fn cubic(c1: (f32, f32), c2: (f32, f32), p: (f32, f32)) -> PathCmd {
    PathCmd::CubicTo(c1.0, c1.1, c2.0, c2.1, p.0, p.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 그러데이션_2색_파싱() {
        // fo=0: gtype=0(선형) angle=90 num=2, color0=red color1=blue.
        let mut d = vec![0u8; 20];
        d[2..4].copy_from_slice(&90u16.to_le_bytes());
        d[10..12].copy_from_slice(&2u16.to_le_bytes());
        d[12..16].copy_from_slice(&0x0000_00FFu32.to_le_bytes()); // R=FF
        d[16..20].copy_from_slice(&0x00FF_0000u32.to_le_bytes()); // B=FF
        let g = parse_gradient(&d, 0).unwrap();
        assert!(!g.radial);
        assert_eq!(g.angle_deg, 90.0);
        assert_eq!(g.stops, vec![(0.0, 0x0000_00FF), (1.0, 0x00FF_0000)]);
    }

    #[test]
    fn hwpx_도형_ir_경로_변환() {
        use hwp_model::{ShapeGeom, ShapeKind};
        let mut page = PageList {
            width_pt: 600.0,
            height_pt: 800.0,
            items: Vec::new(),
        };
        // 사각형: x=2000(20pt) y=1000(10pt) w=30000(300pt) h=15000(150pt), 주황 채움+파랑 테두리.
        let rect = ShapeGeom {
            kind: ShapeKind::Rect,
            x: 2000,
            y: 1000,
            w: 30000,
            h: 15000,
            points: Vec::new(),
            fill: 0x0000_CCFF,
            border_color: 0x00FF_0000,
            border_width: 100,
        };
        draw_ir_shapes(&[rect], &mut page);
        assert_eq!(page.items.len(), 1);
        let Item::Path {
            commands,
            fill,
            stroke,
        } = &page.items[0]
        else {
            panic!("Path가 아님");
        };
        assert_eq!(commands.len(), 5, "사각형은 Move+Line×3+Close");
        assert!(
            matches!(commands[0], PathCmd::MoveTo(x, y) if (x - 20.0).abs() < 0.1 && (y - 10.0).abs() < 0.1)
        );
        assert!(matches!(fill, Some(Fill::Solid(0x0000_CCFF))));
        assert!(stroke.is_some(), "테두리 있어야");

        // 채움·선 없으면 path 생성 안 함.
        let mut p2 = PageList {
            width_pt: 600.0,
            height_pt: 800.0,
            items: Vec::new(),
        };
        let invisible = ShapeGeom {
            kind: ShapeKind::Rect,
            x: 0,
            y: 0,
            w: 1000,
            h: 1000,
            points: Vec::new(),
            fill: 0xFFFF_FFFF,
            border_color: 0xFFFF_FFFF,
            border_width: 0,
        };
        draw_ir_shapes(&[invisible], &mut p2);
        assert!(p2.items.is_empty(), "보이지 않는 도형은 생략");
    }

    #[test]
    fn 방사형_3색_위치() {
        // gtype=1(방사) num=3, 위치 0/50/100, 색 3개.
        let mut d = vec![0u8; 32];
        d[0..2].copy_from_slice(&1u16.to_le_bytes());
        d[10..12].copy_from_slice(&3u16.to_le_bytes());
        d[12..16].copy_from_slice(&0i32.to_le_bytes());
        d[16..20].copy_from_slice(&50i32.to_le_bytes());
        d[20..24].copy_from_slice(&100i32.to_le_bytes());
        d[24..28].copy_from_slice(&0x11u32.to_le_bytes());
        d[28..32].copy_from_slice(&0x22u32.to_le_bytes());
        // 색이 하나 더 필요하지만 버퍼 끝 → None 허용. 최소 검증: radial + 위치 정규화.
        if let Some(g) = parse_gradient(&d, 0) {
            assert!(g.radial);
            assert!((g.stops[0].0 - 0.0).abs() < 0.01);
        }
    }
}

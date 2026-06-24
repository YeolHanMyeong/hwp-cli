//! 렌더 결과를 한글 기준 이미지와 비교하는 오차 측정.
//!
//! 한글은 독자 엔진이라 픽셀 100% 일치는 불가능하므로, 오차를 두 축으로 분리해
//! 측정한다: **위치 오차**(열/행 잉크 프로파일의 상호상관 lag = `dx`/`dy` — 줄
//! 겹침·baseline 어긋남을 잡음)와 **모양/누락 오차**(`bad_pixel_pct`·`mae` — 폰트
//! 치환·미렌더 콘텐츠). 같은 페이지를 같은 DPI로 내보내면 치수가 같다고 가정한다.

use tiny_skia::{Pixmap, PremultipliedColorU8};

/// 비교 리포트.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiffReport {
    /// 채널 평균 절대 오차 (0~255).
    pub mae: f32,
    /// 채널 차이가 tolerance를 넘는 픽셀 비율 (0~1).
    pub bad_pixel_pct: f32,
    /// 우리 렌더를 기준에 맞추려면 가로로 옮길 픽셀 수(추정). +면 우리가 왼쪽에 있음.
    pub dx: i32,
    /// 세로 오프셋(추정). +면 우리가 위에 있음.
    pub dy: i32,
    /// 잉크 적용률 = 우리 잉크 픽셀 수 / 기준 잉크 픽셀 수 (완전성: 1.0이면 같은 양).
    /// 픽셀 위치가 아니라 내용이 얼마나 그려졌는지를 본다(누락/과잉 판단).
    pub ink_ratio: f32,
}

/// 두 픽스맵을 비교해 리포트와 차이 이미지를 만든다.
///
/// 차이 이미지: 회색=일치(양쪽 잉크), 빨강=우리만 잉크, 파랑=기준만 잉크, 흰=양쪽 여백.
/// 치수가 다르면 `Err`(같은 페이지·DPI면 동일해야 함).
pub fn compare(
    ours: &Pixmap,
    reference: &Pixmap,
    tolerance: u8,
) -> Result<(DiffReport, Pixmap), String> {
    let (ow, oh) = (ours.width(), ours.height());
    let (rw, rh) = (reference.width(), reference.height());
    // 반올림 차이(±몇 px)는 겹치는 좌상단 영역으로 비교하고, 큰 차이는 DPI/페이지 오류.
    if ow.abs_diff(rw) > 4 || oh.abs_diff(rh) > 4 {
        return Err(format!(
            "치수 불일치: 우리 {ow}x{oh} vs 기준 {rw}x{rh} (같은 페이지·DPI로 내보냈는지 확인)"
        ));
    }
    let (w, h) = (ow.min(rw), oh.min(rh));
    let op = ours.pixels();
    let rp = reference.pixels();

    let mut sum_abs: u64 = 0;
    let mut bad: u64 = 0;
    let mut ours_ink: u64 = 0;
    let mut ref_ink: u64 = 0;
    let mut diff = Pixmap::new(w, h).ok_or("차이 이미지 생성 실패")?;
    {
        let dp = diff.pixels_mut();
        for y in 0..h {
            for x in 0..w {
                let o = op[(y * ow + x) as usize];
                let r = rp[(y * rw + x) as usize];
                let da = u8diff(o.red(), r.red());
                let db = u8diff(o.green(), r.green());
                let dc = u8diff(o.blue(), r.blue());
                sum_abs += u64::from(da) + u64::from(db) + u64::from(dc);
                if da.max(db).max(dc) > tolerance {
                    bad += 1;
                }
                let (oi, ri) = (is_ink(o), is_ink(r));
                ours_ink += u64::from(oi);
                ref_ink += u64::from(ri);
                dp[(y * w + x) as usize] = match (oi, ri) {
                    (true, true) => rgba(170, 170, 170),   // 일치
                    (true, false) => rgba(220, 40, 40),    // 우리만
                    (false, true) => rgba(40, 80, 220),    // 기준만
                    (false, false) => rgba(255, 255, 255), // 여백
                };
            }
        }
    }

    let n = (w * h) as f32;
    let mae = sum_abs as f32 / (n * 3.0);
    let bad_pixel_pct = bad as f32 / n;
    let ink_ratio = if ref_ink > 0 {
        ours_ink as f32 / ref_ink as f32
    } else {
        1.0
    };

    let (col_o, row_o) = ink_profiles(ours);
    let (col_r, row_r) = ink_profiles(reference);
    let max_lag = 40;
    let dx = best_lag(&col_o, &col_r, max_lag);
    let dy = best_lag(&row_o, &row_r, max_lag);

    Ok((
        DiffReport {
            mae,
            bad_pixel_pct,
            dx,
            dy,
            ink_ratio,
        },
        diff,
    ))
}

/// 열별/행별 잉크(어두움) 합 프로파일. (col[x], row[y])
fn ink_profiles(p: &Pixmap) -> (Vec<f32>, Vec<f32>) {
    let (w, h) = (p.width() as usize, p.height() as usize);
    let px = p.pixels();
    let mut col = vec![0f32; w];
    let mut row = vec![0f32; h];
    for y in 0..h {
        for x in 0..w {
            let d = darkness(px[y * w + x]);
            col[x] += d;
            row[y] += d;
        }
    }
    (col, row)
}

/// `a`를 lag만큼 옮겨 `b`와 가장 잘 겹치는 lag(상호상관 최대)를 찾는다.
/// 반환 lag>0 = a가 b보다 왼쪽(오른쪽으로 lag만큼 옮기면 정렬).
fn best_lag(a: &[f32], b: &[f32], max_lag: i32) -> i32 {
    let n = a.len() as i32;
    let mut best_lag = 0i32;
    let mut best_score = f32::NEG_INFINITY;
    for lag in -max_lag..=max_lag {
        let mut dot = 0.0f32;
        let mut count = 0f32;
        for k in 0..n {
            let j = k + lag;
            if j >= 0 && j < b.len() as i32 {
                dot += a[k as usize] * b[j as usize];
                count += 1.0;
            }
        }
        // 겹침 구간 길이로 정규화(큰 lag의 표본 감소 편향 보정).
        let score = if count > 0.0 { dot / count } else { 0.0 };
        if score > best_score {
            best_score = score;
            best_lag = lag;
        }
    }
    best_lag
}

#[inline]
fn u8diff(a: u8, b: u8) -> u8 {
    a.abs_diff(b)
}

/// 어두움 0~1 (1=검정). 흰 배경 위 텍스트/도형 잉크를 잡는다.
#[inline]
fn darkness(p: PremultipliedColorU8) -> f32 {
    let luma =
        0.299 * f32::from(p.red()) + 0.587 * f32::from(p.green()) + 0.114 * f32::from(p.blue());
    1.0 - luma / 255.0
}

#[inline]
fn is_ink(p: PremultipliedColorU8) -> bool {
    darkness(p) > 0.25
}

#[inline]
fn rgba(r: u8, g: u8, b: u8) -> PremultipliedColorU8 {
    // 불투명이므로 premultiplied == 원색.
    PremultipliedColorU8::from_rgba(r, g, b, 255).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, r: u8, g: u8, b: u8) -> Pixmap {
        let mut p = Pixmap::new(w, h).unwrap();
        for px in p.pixels_mut() {
            *px = rgba(r, g, b);
        }
        p
    }

    /// 한 점만 어둡게 찍은 흰 배경.
    fn dot(w: u32, h: u32, x: u32, y: u32) -> Pixmap {
        let mut p = solid(w, h, 255, 255, 255);
        let i = (y * w + x) as usize;
        p.pixels_mut()[i] = rgba(0, 0, 0);
        p
    }

    #[test]
    fn 동일이미지_오차0() {
        let a = dot(50, 50, 25, 25);
        let b = dot(50, 50, 25, 25);
        let (rep, _) = compare(&a, &b, 10).unwrap();
        assert_eq!(rep.bad_pixel_pct, 0.0);
        assert_eq!((rep.dx, rep.dy), (0, 0));
        assert!(rep.mae < 0.01);
    }

    #[test]
    fn 가로_오프셋_검출() {
        // 우리는 x=20, 기준은 x=25 → 우리가 5px 왼쪽 → dx=+5.
        let ours = dot(60, 20, 20, 10);
        let reference = dot(60, 20, 25, 10);
        let (rep, _) = compare(&ours, &reference, 10).unwrap();
        assert_eq!(rep.dx, 5, "dx 추정");
    }

    #[test]
    fn 세로_오프셋_검출() {
        let ours = dot(20, 60, 10, 20);
        let reference = dot(20, 60, 10, 23);
        let (rep, _) = compare(&ours, &reference, 10).unwrap();
        assert_eq!(rep.dy, 3, "dy 추정");
    }

    #[test]
    fn 치수_불일치_에러() {
        // ±4px 반올림 차이는 허용, 큰 차이만 에러.
        let a = solid(10, 10, 255, 255, 255);
        assert!(compare(&a, &solid(20, 10, 255, 255, 255), 10).is_err());
        // 1px 차이는 겹치는 영역으로 비교(에러 아님).
        assert!(compare(&a, &solid(11, 10, 255, 255, 255), 10).is_ok());
    }
}

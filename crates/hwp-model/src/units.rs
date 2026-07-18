//! HWP 길이 단위.
//!
//! HWP의 모든 길이는 HWPUNIT = 1/7200 인치다.
//! 1pt = 1/72 인치 = 정확히 100 HWPUNIT이므로 pt 변환은 손실이 없다.

use serde::{Deserialize, Serialize};

/// HWPUNIT (1/7200 인치). 레이아웃 계산은 이 정수 단위로 수행한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HwpUnit(pub i32);

impl HwpUnit {
    /// 1pt에 해당하는 HWPUNIT 수.
    pub const PER_PT: i32 = 100;
    /// 1인치에 해당하는 HWPUNIT 수.
    pub const PER_INCH: i32 = 7200;

    /// pt로 변환 (정확).
    pub fn to_pt(self) -> f64 {
        f64::from(self.0) / f64::from(Self::PER_PT)
    }

    /// mm로 변환.
    pub fn to_mm(self) -> f64 {
        f64::from(self.0) / f64::from(Self::PER_INCH) * 25.4
    }

    /// 주어진 DPI에서의 픽셀 값으로 변환.
    pub fn to_px(self, dpi: f64) -> f64 {
        f64::from(self.0) / f64::from(Self::PER_INCH) * dpi
    }
}

// ── FILETIME(Windows) ↔ 문자열 변환 ────────────────────────────────────────
//
// FILETIME은 1601-01-01 00:00:00 UTC 기준 100ns 단위의 u64다(hwp5 요약정보
// VT_FILETIME, hwpx OPF CreatedDate/ModifiedDate의 원천). 외부 크레이트(chrono)
// 없이 Howard Hinnant의 civil_from_days/days_from_civil 계열 순수 정수 알고리즘으로
// 그레고리력 변환을 직접 구현한다.

/// FILETIME 1틱(100ns) 수. 1초 = 10,000,000틱.
const FT_PER_SEC: u64 = 10_000_000;
const SECS_PER_DAY: i64 = 86_400;

/// 그레고리력(y, m, d) → 1970-01-01 기준 일수(음수 가능). Howard Hinnant `days_from_civil`.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// 1970-01-01 기준 일수 → 그레고리력(y, m, d). Howard Hinnant `civil_from_days`.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 1970-01-01 기준 일수 → 요일(0=일요일 … 6=토요일). Howard Hinnant `weekday_from_days`.
fn weekday_from_days(z: i64) -> usize {
    (if z >= -4 {
        (z + 4) % 7
    } else {
        (z + 5) % 7 + 6
    }) as usize
}

/// FILETIME(1601 기준 100ns) → 1970-01-01 기준 (일수, 하루 내 초).
fn filetime_to_days_secs(ft: u64) -> (i64, i64) {
    // 1601-01-01 기준 총 초 → 1970-01-01 기준으로 이동.
    let total_secs_1601 = (ft / FT_PER_SEC) as i64;
    let days_1601_to_1970 = days_from_civil(1601, 1, 1); // 음수(-134774)
    let mut days = days_1601_to_1970 + total_secs_1601 / SECS_PER_DAY;
    let mut sod = total_secs_1601 % SECS_PER_DAY;
    if sod < 0 {
        sod += SECS_PER_DAY;
        days -= 1;
    }
    (days, sod)
}

/// FILETIME → ISO-8601 UTC 문자열 `"2026-07-15T09:00:00Z"`. 0이면 None(미설정).
///
/// 초 미만(100ns 하위)은 절사된다 — ISO 초 정밀도로 인한 손실이므로 왕복 시
/// 원래 u64가 `FT_PER_SEC`의 배수가 아니면 하위 틱이 소실된다.
pub fn filetime_to_iso8601_utc(ft: u64) -> Option<String> {
    if ft == 0 {
        return None;
    }
    let (days, sod) = filetime_to_days_secs(ft);
    let (y, m, d) = civil_from_days(days);
    let (h, min, s) = (sod / 3600, (sod % 3600) / 60, sod % 60);
    Some(format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}Z"))
}

/// FILETIME → 한국어 로캘 KST 문자열 `"2025년 9월 17일 수요일 오후 1:32:50"`.
///
/// UTC+9 고정 변환, 요일 한국어, 12시간제 오전/오후(시는 선행 0 없음, 분·초는 2자리).
/// 정품 표본 `date` 메타 형식 그대로다. 0이면 None.
pub fn filetime_to_korean_kst(ft: u64) -> Option<String> {
    if ft == 0 {
        return None;
    }
    // KST = UTC + 9시간. 초 단위로 더한 뒤 분해.
    let kst_ft = ft + 9 * 3600 * FT_PER_SEC;
    let (days, sod) = filetime_to_days_secs(kst_ft);
    let (y, m, d) = civil_from_days(days);
    let wd = ["일", "월", "화", "수", "목", "금", "토"][weekday_from_days(days)];
    let (h24, min, s) = (sod / 3600, (sod % 3600) / 60, sod % 60);
    let period = if h24 < 12 { "오전" } else { "오후" };
    let h12 = match h24 % 12 {
        0 => 12,
        h => h,
    };
    Some(format!(
        "{y}년 {m}월 {d}일 {wd}요일 {period} {h12}:{min:02}:{s:02}"
    ))
}

/// ISO-8601 UTC 문자열(`"2026-07-15T09:00:00Z"`) → FILETIME u64. 파싱 실패 시 None.
///
/// [`filetime_to_iso8601_utc`]의 역함수(초 정밀도). 결과 u64는 항상 `FT_PER_SEC`의
/// 배수이므로, 하위 100ns가 있던 원본과의 왕복은 초 이하가 절사된다.
pub fn iso8601_utc_to_filetime(s: &str) -> Option<u64> {
    // 형식: YYYY-MM-DDThh:mm:ss[Z]  (구분자는 위치로만 파싱)
    let b = s.trim();
    let digits = |part: &str| -> Option<i64> { part.parse::<i64>().ok() };
    let (date, time) = b.split_once('T').or_else(|| b.split_once(' '))?;
    let mut dp = date.split('-');
    let y = digits(dp.next()?)?;
    let m = digits(dp.next()?)?;
    let d = digits(dp.next()?)?;
    let time = time.trim_end_matches('Z');
    let mut tp = time.split(':');
    let h = digits(tp.next()?)?;
    let min = digits(tp.next()?)?;
    // 초에 소수부가 붙어도 정수부만 취한다.
    let sec_part = tp.next().unwrap_or("0");
    let s = digits(sec_part.split('.').next()?)?;
    if !(1..=9999).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let days = days_from_civil(y, m, d) - days_from_civil(1601, 1, 1);
    let total_secs = days * SECS_PER_DAY + h * 3600 + min * 60 + s;
    if total_secs < 0 {
        return None;
    }
    Some(total_secs as u64 * FT_PER_SEC)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pt_변환은_정확하다() {
        assert_eq!(HwpUnit(1000).to_pt(), 10.0);
        assert_eq!(HwpUnit(59528).to_pt(), 595.28); // A4 폭 210mm
    }

    #[test]
    fn mm_변환() {
        // A4 폭 210mm = 59528.34... HWPUNIT — 한글은 59528을 사용
        assert!((HwpUnit(59528).to_mm() - 210.0).abs() < 0.01);
    }

    /// 정품 표본과 정확히 일치: 2025-09-17T04:32:50Z → ISO + KST 문자열.
    #[test]
    fn filetime_정품표본_변환() {
        // 2025-09-17T04:32:50Z 를 FILETIME으로 만든다(초 정밀).
        let ft = iso8601_utc_to_filetime("2025-09-17T04:32:50Z").unwrap();
        assert_eq!(
            filetime_to_iso8601_utc(ft).as_deref(),
            Some("2025-09-17T04:32:50Z")
        );
        // KST = UTC+9 → 13:32:50, 2025-09-17은 수요일.
        assert_eq!(
            filetime_to_korean_kst(ft).as_deref(),
            Some("2025년 9월 17일 수요일 오후 1:32:50")
        );
    }

    /// 0(미설정)은 None.
    #[test]
    fn filetime_영은_none() {
        assert_eq!(filetime_to_iso8601_utc(0), None);
        assert_eq!(filetime_to_korean_kst(0), None);
    }

    /// 오전/자정/정오 12시간제 경계.
    #[test]
    fn kst_오전오후_경계() {
        // 00:00:00Z → KST 09:00:00 오전 9시
        let midnight = iso8601_utc_to_filetime("2026-01-01T00:00:00Z").unwrap();
        assert_eq!(
            filetime_to_korean_kst(midnight).as_deref(),
            Some("2026년 1월 1일 목요일 오전 9:00:00")
        );
        // 03:00:00Z → KST 12:00:00 오후 12시(정오)
        let noon = iso8601_utc_to_filetime("2026-01-01T03:00:00Z").unwrap();
        assert_eq!(
            filetime_to_korean_kst(noon).as_deref(),
            Some("2026년 1월 1일 목요일 오후 12:00:00")
        );
        // 15:00:00Z → KST 익일 00:00:00 오전 12시(자정)
        let mid = iso8601_utc_to_filetime("2026-01-01T15:00:00Z").unwrap();
        assert_eq!(
            filetime_to_korean_kst(mid).as_deref(),
            Some("2026년 1월 2일 금요일 오전 12:00:00")
        );
    }

    /// 윤년(2024-02-29)·연말 경계(2023-12-31 23:59:59) 왕복.
    #[test]
    fn filetime_윤년_연말_경계_왕복() {
        for iso in [
            "2024-02-29T12:00:00Z",
            "2023-12-31T23:59:59Z",
            "2000-02-29T00:00:00Z", // 400년 윤년
            "1900-03-01T00:00:00Z", // 1900은 평년(100년 규칙)
            "1601-01-01T00:00:01Z", // FILETIME 원점 직후(0은 미설정 취급이라 제외)
        ] {
            let ft = iso8601_utc_to_filetime(iso).unwrap();
            assert_eq!(filetime_to_iso8601_utc(ft).as_deref(), Some(iso), "{iso}");
        }
    }

    /// 초 이하(하위 100ns)는 ISO 초 절사로 소실됨을 명시.
    #[test]
    fn filetime_초이하_절사() {
        // 13371383770.5초 상당(0.5초 = 5,000,000틱 여분)
        let ft = 133_713_837_705_000_000u64;
        assert_ne!(ft % FT_PER_SEC, 0, "하위 틱 존재 전제");
        let iso = filetime_to_iso8601_utc(ft).unwrap();
        let back = iso8601_utc_to_filetime(&iso).unwrap();
        // 초 단위로 내림된 값과 일치.
        assert_eq!(back, (ft / FT_PER_SEC) * FT_PER_SEC);
    }
}

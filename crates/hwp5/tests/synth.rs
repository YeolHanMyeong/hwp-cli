//! 합성 문서(md/hwpx 출신, tail 없음)가 한글 5.1.0.1 규격을 따르는지 검증.
//!
//! 한글 실기 게이트에서 합성 문서만 "변조/보안경고"가 났던 5대 결함의
//! 회귀 방지: 버전-레이아웃 정합(PARA_SHAPE 58B/PARA_HEADER 24B),
//! TAB_DEF/NUMBERING 존재(dangling reference 방지), secd 필수 자식.

use std::path::PathBuf;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join("hwp5-synth-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// markdown→hwp 합성 문서가 한글 무결성 검사 통과 조건을 모두 만족해야 한다.
#[test]
fn 합성_문서_한글_규격_충족() {
    let doc = hwp_convert::from_markdown(
        "# 제목\n\n본문 문단입니다.\n\n| A | B |\n| - | - |\n| 1 | 2 |\n",
    );
    let out = tmp("synth.hwp");
    hwp5::write_document(&doc, &out, &hwp5::WriteOptions::default()).unwrap();

    let reread = hwp5::read_document(&out).unwrap();
    let d = reread.document;

    // 1. TAB_DEF/NUMBERING 비어 있지 않음 (PARA_SHAPE의 tab_def_id/numbering_id 참조처)
    assert!(!d.header.tab_defs.is_empty(), "TAB_DEF dangling reference");
    assert!(
        !d.header.numberings.is_empty(),
        "NUMBERING dangling reference"
    );

    // 1b. COMPATIBLE_DOCUMENT(0x1E) 존재 — 5.1.x 필수 (한글 정품 가나다·hello_world 보유)
    let mut c0 = hwp5::Hwp5Container::open(&out).unwrap();
    let di0 = c0.read_record_stream("/DocInfo").unwrap();
    let scan = hwp5::record::scan_stream(&di0, hwp5::record::ScanMode::Tolerant).unwrap();
    let compat = scan
        .roots
        .iter()
        .find(|r| r.tag == 0x1E)
        .expect("COMPATIBLE_DOCUMENT");
    let child_tags: Vec<u16> = compat.children.iter().map(|c| c.tag).collect();
    assert!(child_tags.contains(&0x1F), "LAYOUT_COMPATIBILITY 자식");
    assert!(child_tags.contains(&0x20), "TRACKCHANGE 자식");

    // 2. secd 필수 자식: 각주/미주 모양 + 쪽 테두리 3종
    let secd = d.sections[0].section_def().expect("구역 정의");
    let footnotes = secd.extras.iter().filter(|e| e.tag == 0x4A).count();
    let page_borders = secd.extras.iter().filter(|e| e.tag == 0x4B).count();
    assert_eq!(footnotes, 2, "secd 각주/미주 모양");
    assert_eq!(page_borders, 3, "secd 쪽 테두리 3종");
    assert!(secd.page.is_some(), "PAGE_DEF");

    // 3. EncryptVersion=4 (현대 한글 마커)
    let mut c = hwp5::Hwp5Container::open(&out).unwrap();
    assert!(c.file_header().is_compressed());

    // 4. 레코드 길이가 5.1.0.1 규격 (압축 해제 후 직접 측정)
    let di = c.read_record_stream("/DocInfo").unwrap();
    let bt = c.read_record_stream("/BodyText/Section0").unwrap();
    assert!(
        record_sizes(&di, 0x19).iter().all(|&s| s == 58),
        "PARA_SHAPE는 58B여야"
    );
    assert!(
        record_sizes(&di, 0x15).iter().all(|&s| s == 74),
        "CHAR_SHAPE는 74B여야"
    );
    assert!(
        record_sizes(&bt, 0x42).iter().all(|&s| s == 24),
        "PARA_HEADER는 24B여야"
    );
}

/// 합성 문단의 본문 구조가 정품 한글 문단(가나다.hwp 5.1.1.0)과 동형이어야 한다.
/// 정품 대조로 확정한 5대 본문 결함의 회귀 방지 — 이 결함들이 합쳐져
/// "보안 낮춤에도 손상" 경고를 냈다.
#[test]
fn 합성_문단_본문_구조_정품_동형() {
    let doc = hwp_convert::from_markdown("가나다\n");
    let out = tmp("synth_para.hwp");
    hwp5::write_document(&doc, &out, &hwp5::WriteOptions::default()).unwrap();
    let mut c = hwp5::Hwp5Container::open(&out).unwrap();
    let bt = c.read_record_stream("/BodyText/Section0").unwrap();

    // 1. PARA_TEXT는 문단끝 문자(0x0d=13)로 끝나야 한다 (정품 188문단 전수).
    let pt = first_record(&bt, 0x43).expect("PARA_TEXT");
    let last = u16::from_le_bytes([pt[pt.len() - 2], pt[pt.len() - 1]]);
    assert_eq!(last, 13, "PARA_TEXT는 문단끝 0x0d로 종료해야");

    // 2. PARA_HEADER nchars 최상위 비트(0x80000000) 세팅 (정품 단일 문단 전수).
    let ph = first_record(&bt, 0x42).expect("PARA_HEADER");
    let nchars = u32::from_le_bytes([ph[0], ph[1], ph[2], ph[3]]);
    assert_ne!(nchars & 0x8000_0000, 0, "nchars bit31");

    // 3. 구역 첫 문단 break_type=0x03 (offset 11) — 정품 동형.
    assert_eq!(ph[11], 0x03, "구역 첫 문단 break_type");

    // 4. PARA_CHAR_SHAPE run 수 = char_shape_cnt(offset 12, u16), 중복 병합으로 단일.
    let cs = first_record(&bt, 0x44).expect("PARA_CHAR_SHAPE");
    let cnt = u16::from_le_bytes([ph[12], ph[13]]);
    assert_eq!(cs.len() / 8, cnt as usize, "char_shape run 수=char_shape_cnt");
    assert_eq!(cnt, 1, "단일 문단은 단일 char_shape run (중복 없음)");

    // 5. PAGE_BORDER_FILL attribute 첫 u32 = 1 (hello_world 표본 잔재 garbage 아님).
    let pbf = first_record(&bt, 0x4B).expect("PAGE_BORDER_FILL");
    assert_eq!(
        u32::from_le_bytes([pbf[0], pbf[1], pbf[2], pbf[3]]),
        1,
        "PAGE_BORDER_FILL attribute"
    );
}

/// 스트림에서 특정 태그의 첫 레코드 페이로드.
fn first_record(data: &[u8], tag: u16) -> Option<Vec<u8>> {
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let h = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        let t = (h & 0x3FF) as u16;
        let mut sz = h >> 20;
        let mut hl = 4;
        if sz == 0xFFF {
            sz = u32::from_le_bytes([data[i + 4], data[i + 5], data[i + 6], data[i + 7]]);
            hl = 8;
        }
        if t == tag {
            return Some(data[i + hl..i + hl + sz as usize].to_vec());
        }
        i += hl + sz as usize;
    }
    None
}

/// 레코드 스트림에서 특정 태그 레코드들의 페이로드 크기 목록.
fn record_sizes(data: &[u8], tag: u16) -> Vec<u32> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let h = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        let t = (h & 0x3FF) as u16;
        let mut sz = h >> 20;
        let mut hl = 4;
        if sz == 0xFFF {
            sz = u32::from_le_bytes([data[i + 4], data[i + 5], data[i + 6], data[i + 7]]);
            hl = 8;
        }
        if t == tag {
            out.push(sz);
        }
        i += hl + sz as usize;
    }
    out
}

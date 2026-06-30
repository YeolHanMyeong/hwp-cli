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

    // 1a. char_shape 음영색(shade_color) != 0 — 0이면 한글이 글자 칸마다 불투명
    //     검정 음영을 그려 '검은 바'가 된다(14차 실기). 정품은 0xFFFFFFFF('없음').
    for cs in &d.header.char_shapes {
        assert_ne!(cs.shade_color, 0, "char_shape 음영색 0 = 검은 바");
    }

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
    assert_eq!(
        cs.len() / 8,
        cnt as usize,
        "char_shape run 수=char_shape_cnt"
    );
    assert_eq!(cnt, 1, "단일 문단은 단일 char_shape run (중복 없음)");

    // 5. PAGE_BORDER_FILL attribute 첫 u32 = 1 (hello_world 표본 잔재 garbage 아님).
    let pbf = first_record(&bt, 0x4B).expect("PAGE_BORDER_FILL");
    assert_eq!(
        u32::from_le_bytes([pbf[0], pbf[1], pbf[2], pbf[3]]),
        1,
        "PAGE_BORDER_FILL attribute"
    );
}

/// 빈 셀을 포함한 GFM 표 → 모든 표 셀 LIST_HEADER 의 nparas ≥ 1.
///
/// 셀에 PARA_HEADER 가 하나도 안 붙으면(nparas=0) 한글이 문서를 '손상'으로
/// 거부한다(M6-md생성.hwp 구 산출물의 실제 결함). from_markdown 은 셀 종료 시
/// flush_paragraph_inner(force=true) 와 누락 칸 vec![Paragraph::default()]
/// 충전으로 nparas≥1 을 보장한다. 짧은 행·빈 셀·헤더-only 표 모두 검증.
#[test]
fn 표_빈셀_포함_모든_셀_nparas_1이상() {
    // 빈 셀(`| |`)·짧은 행(2칸 < 3열 헤더)·헤더 only 행을 모두 포함.
    let doc =
        hwp_convert::from_markdown("|  |  |  |\n| --- | --- | --- |\n| a |  |  |\n| b | c |\n");
    let out = tmp("synth_empty_cell.hwp");
    hwp5::write_document(&doc, &out, &hwp5::WriteOptions::default()).unwrap();

    let mut c = hwp5::Hwp5Container::open(&out).unwrap();
    let bt = c.read_record_stream("/BodyText/Section0").unwrap();

    let list_headers = all_records(&bt, 0x48); // LIST_HEADER
    assert!(!list_headers.is_empty(), "표 셀 LIST_HEADER 가 있어야");
    for (i, lh) in list_headers.iter().enumerate() {
        let nparas = i32::from_le_bytes([lh[0], lh[1], lh[2], lh[3]]);
        assert!(
            nparas >= 1,
            "LIST_HEADER #{i}: nparas={nparas} — 빈 셀에도 문단 1개 필수(한글 손상 방지)"
        );
    }
}

/// 표 행 추가(add_rows)로 늘린 표가 한글 합성 규격을 만족해야 한다.
///
/// 양식 채우기에서 행/칸을 추가(`hwp edit --add-row`, `hwp fill --data tables`)한
/// 표는 새 셀이 빈 문단이라도 nparas≥1·문단끝·nchars bit31을 지켜야 하고,
/// row_cell_counts(행별 셀 수)가 행 수·셀 수와 정합해야 한다(hwp5 extract assert).
/// 어긋나면 한글이 '손상'으로 거부한다.
#[test]
fn 행_추가_표_합성_규격_충족() {
    let mut doc = hwp_convert::from_markdown("| 품목 | 수량 |\n| --- | --- |\n| | |\n");
    // 빈 행 3개 추가 후 일부 채움(양식 변형 시나리오).
    hwp_convert::add_rows(&mut doc, 0, None, 3).unwrap();
    hwp_convert::set_cell(&mut doc, 0, 1, 0, "노트북").unwrap();
    hwp_convert::set_cell(&mut doc, 0, 1, 1, "5").unwrap();
    // 행 4는 비워 둔 채(빈 셀 규격 검증).
    let out = tmp("synth_grown_table.hwp");
    hwp5::write_document(&doc, &out, &hwp5::WriteOptions::default()).unwrap();

    // 1) 재읽기 IR: 표가 5행으로 늘고 row_cell_counts 정합.
    let reread = hwp5::read_document(&out).unwrap().document;
    let table = reread.sections[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.controls)
        .find_map(|c| match c {
            hwp_model::Control::Table(t) => Some(t),
            _ => None,
        })
        .expect("표 재읽기");
    assert_eq!(table.rows, 5, "행 수 = 원래 2 + 추가 3");
    assert_eq!(
        table.row_cell_counts.len(),
        table.rows as usize,
        "row_cell_counts 길이 == 행 수"
    );
    assert_eq!(
        table
            .row_cell_counts
            .iter()
            .map(|c| *c as usize)
            .sum::<usize>(),
        table.cells.len(),
        "row_cell_counts 합 == 셀 수"
    );

    // 2) 모든 셀 LIST_HEADER nparas ≥ 1 (빈 새 셀 포함 — 한글 손상 방지).
    let mut c = hwp5::Hwp5Container::open(&out).unwrap();
    let bt = c.read_record_stream("/BodyText/Section0").unwrap();
    let list_headers = all_records(&bt, 0x48);
    assert_eq!(
        list_headers.len(),
        table.cells.len(),
        "셀 수 == LIST_HEADER 수"
    );
    for (i, lh) in list_headers.iter().enumerate() {
        let nparas = i32::from_le_bytes([lh[0], lh[1], lh[2], lh[3]]);
        assert!(
            nparas >= 1,
            "추가 셀 LIST_HEADER #{i}: nparas={nparas} (한글 손상)"
        );
    }

    // 3) 채운 셀은 본문에 반영(노트북/5), 빈 새 행은 빈 셀로 남음.
    let text = reread.plain_text();
    assert!(
        text.contains("노트북") && text.contains('5'),
        "채운 셀 반영: {text:?}"
    );
}

/// 빈 문단(빈 표 셀 포함)은 PARA_TEXT 레코드를 갖지 않아야 한다.
///
/// 정품(work_report·한라대 정품) 실측: 빈 문단은 nchars=1 + PARA_CHAR_SHAPE +
/// PARA_LINE_SEG 를 갖되 PARA_TEXT 레코드가 없다(문단끝은 암묵적). 합성 경로는
/// 모든 문단에 0x0d 를 붙이는데, 빈 문단을 PARA_TEXT=[0x0d] 로 방출하면 한글이
/// "파일이 손상되었습니다 + 본문 비어있음"으로 거부한다 — 빈 셀이 있는 표
/// (제목 박스·목차·구역 헤더) 전부 손상시킨 근본 원인. emit_paragraph 는
/// char_count>1 일 때만 PARA_TEXT 를 방출해야 한다. (pyhwp 는 빈 PARA_TEXT 를
/// 관대하게 통과시켜 23라운드 동안 미검출 — 정품 바이트 대조로만 잡힌 결함.)
#[test]
fn 빈_문단은_para_text_없음() {
    // 빈 셀(`| |`)을 다수 포함한 표 + 채워진 셀·본문 문단.
    let doc = hwp_convert::from_markdown(
        "본문 문단입니다.\n\n|  |  |  |\n| --- | --- | --- |\n| 채움 |  |  |\n|  |  |  |\n",
    );
    let out = tmp("synth_empty_para.hwp");
    hwp5::write_document(&doc, &out, &hwp5::WriteOptions::default()).unwrap();

    let mut c = hwp5::Hwp5Container::open(&out).unwrap();
    let bt = c.read_record_stream("/BodyText/Section0").unwrap();
    let recs = records_with_level(&bt);

    let mut empty_paras = 0;
    let mut filled_paras = 0;
    for (i, (tag, _lvl, p)) in recs.iter().enumerate() {
        if *tag != 0x42 {
            continue; // PARA_HEADER 만
        }
        let nchars = u32::from_le_bytes([p[0], p[1], p[2], p[3]]) & 0x7FFF_FFFF;
        // 이 문단의 자식(다음 PARA_HEADER 전까지)에 PARA_TEXT(0x43)가 있는가.
        let has_text = recs[i + 1..]
            .iter()
            .take_while(|(t, _, _)| *t != 0x42)
            .any(|(t, _, _)| *t == 0x43);
        if nchars == 1 {
            empty_paras += 1;
            assert!(
                !has_text,
                "빈 문단(nchars=1)은 PARA_TEXT 가 없어야 한다 — 0x0d 만 든 PARA_TEXT 는 한글 손상(빈 셀 표)"
            );
        } else {
            filled_paras += 1;
            assert!(has_text, "채워진 문단(nchars>1)은 PARA_TEXT 가 있어야");
        }
    }
    assert!(
        empty_paras > 0,
        "빈 셀 표는 빈 문단을 만들어야 한다(시험 전제)"
    );
    assert!(filled_paras > 0, "채워진 문단도 있어야 한다(시험 전제)");
}

/// 스트림의 모든 레코드를 (tag, level, payload) 로 펼친다.
fn records_with_level(data: &[u8]) -> Vec<(u16, u16, Vec<u8>)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let h = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        let t = (h & 0x3FF) as u16;
        let lvl = ((h >> 10) & 0x3FF) as u16;
        let mut sz = h >> 20;
        let mut hl = 4;
        if sz == 0xFFF {
            sz = u32::from_le_bytes([data[i + 4], data[i + 5], data[i + 6], data[i + 7]]);
            hl = 8;
        }
        out.push((t, lvl, data[i + hl..i + hl + sz as usize].to_vec()));
        i += hl + sz as usize;
    }
    out
}

/// 스트림에서 특정 태그 레코드들의 페이로드 목록.
fn all_records(data: &[u8], tag: u16) -> Vec<Vec<u8>> {
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
            out.push(data[i + hl..i + hl + sz as usize].to_vec());
        }
        i += hl + sz as usize;
    }
    out
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

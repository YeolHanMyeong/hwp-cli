//! 패키지 보조 파일 템플릿.
//!
//! 한글에서 저장한 기준 표본(minimal.hwpx)의 구조를 코드로 재현한다 —
//! 바이너리/원문 임베드가 아니라 값을 옮긴 것이므로 라이선스 문제가 없다.

pub const MIMETYPE: &str = "application/hwp+zip";

pub const VERSION_XML: &str = r##"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><hv:HCFVersion xmlns:hv="http://www.hancom.co.kr/hwpml/2011/version" tagetApplication="WORDPROCESSOR" major="5" minor="1" micro="1" buildNumber="0" os="1" xmlVersion="1.5" application="hwp-cli" appVersion="0.2.0"/>"##;

pub const CONTAINER_XML: &str = r##"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><ocf:container xmlns:ocf="urn:oasis:names:tc:opendocument:xmlns:container" xmlns:hpf="http://www.hancom.co.kr/schema/2011/hpf"><ocf:rootfiles><ocf:rootfile full-path="Contents/content.hpf" media-type="application/hwpml-package+xml"/><ocf:rootfile full-path="Preview/PrvText.txt" media-type="text/plain"/><ocf:rootfile full-path="META-INF/container.rdf" media-type="application/rdf+xml"/></ocf:rootfiles></ocf:container>"##;

pub const CONTAINER_RDF: &str = r##"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about=""><ns0:hasPart xmlns:ns0="http://www.hancom.co.kr/hwpml/2016/meta/pkg#" rdf:resource="Contents/header.xml"/></rdf:Description><rdf:Description rdf:about="Contents/header.xml"><rdf:type rdf:resource="http://www.hancom.co.kr/hwpml/2016/meta/pkg#HeaderFile"/></rdf:Description><rdf:Description rdf:about=""><ns0:hasPart xmlns:ns0="http://www.hancom.co.kr/hwpml/2016/meta/pkg#" rdf:resource="Contents/section0.xml"/></rdf:Description><rdf:Description rdf:about="Contents/section0.xml"><rdf:type rdf:resource="http://www.hancom.co.kr/hwpml/2016/meta/pkg#SectionFile"/></rdf:Description><rdf:Description rdf:about=""><rdf:type rdf:resource="http://www.hancom.co.kr/hwpml/2016/meta/pkg#Document"/></rdf:Description></rdf:RDF>"##;

pub const MANIFEST_XML: &str = r##"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><odf:manifest xmlns:odf="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>"##;

pub const SETTINGS_XML: &str = r##"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><ha:HWPApplicationSetting xmlns:ha="http://www.hancom.co.kr/hwpml/2011/app" xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0"><ha:CaretPosition listIDRef="0" paraIDRef="0" pos="0"/></ha:HWPApplicationSetting>"##;

/// OWPML 공통 네임스페이스 선언 (head/content.hpf용 전체 세트).
pub const FULL_XMLNS: &str = r##"xmlns:ha="http://www.hancom.co.kr/hwpml/2011/app" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph" xmlns:hp10="http://www.hancom.co.kr/hwpml/2016/paragraph" xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section" xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core" xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head" xmlns:hhs="http://www.hancom.co.kr/hwpml/2011/history" xmlns:hm="http://www.hancom.co.kr/hwpml/2011/master-page" xmlns:hpf="http://www.hancom.co.kr/schema/2011/hpf" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf/" xmlns:ooxmlchart="http://www.hancom.co.kr/hwpml/2016/ooxmlchart" xmlns:hwpunitchar="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar" xmlns:epub="http://www.idpf.org/2007/ops" xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0""##;

/// content.hpf — manifest 항목(섹션 수, 바이너리 항목) + 문서 메타데이터를 끼워 만든다.
pub fn content_hpf(
    section_count: usize,
    bin_items: &[(String, String, String)],
    meta: &hwp_model::Metadata,
) -> String {
    use std::fmt::Write as _;
    let mut manifest = String::new();
    let mut spine = String::new();
    let _ = write!(
        manifest,
        r##"<opf:item id="header" href="Contents/header.xml" media-type="application/xml"/>"##
    );
    let _ = write!(spine, r##"<opf:itemref idref="header" linear="yes"/>"##);
    for i in 0..section_count {
        let _ = write!(
            manifest,
            r##"<opf:item id="section{i}" href="Contents/section{i}.xml" media-type="application/xml"/>"##
        );
        let _ = write!(spine, r##"<opf:itemref idref="section{i}" linear="yes"/>"##);
    }
    let _ = write!(
        manifest,
        r##"<opf:item id="settings" href="settings.xml" media-type="application/xml"/>"##
    );
    for (id, href, mime) in bin_items {
        let _ = write!(
            manifest,
            r##"<opf:item id="{id}" href="{href}" media-type="{mime}" isEmbeded="1"/>"##
        );
    }
    let title_el = match meta.title.as_deref().filter(|t| !t.is_empty()) {
        Some(t) => format!("<opf:title>{}</opf:title>", esc(t)),
        None => "<opf:title/>".to_string(),
    };
    let creator_el = meta
        .author
        .as_deref()
        .filter(|a| !a.is_empty())
        .map(|a| format!("<dc:creator>{}</dc:creator>", esc(a)))
        .unwrap_or_default();
    let subject_el = meta
        .subject
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| format!("<dc:subject>{}</dc:subject>", esc(s)))
        .unwrap_or_default();
    let keywords_el = meta
        .keywords
        .as_deref()
        .filter(|k| !k.is_empty())
        .map(|k| format!("<opf:meta name=\"keywords\" content=\"{}\"/>", esc(k)))
        .unwrap_or_default();
    format!(
        r##"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><opf:package {FULL_XMLNS} version="" unique-identifier="" id=""><opf:metadata>{title_el}<opf:language>ko</opf:language>{creator_el}{subject_el}{keywords_el}<opf:meta name="creator" content="text">hwp-cli</opf:meta></opf:metadata><opf:manifest>{manifest}</opf:manifest><opf:spine>{spine}</opf:spine></opf:package>"##
    )
}

/// XML 텍스트/속성 이스케이프.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// COLORREF → "#rrggbb" (0xFFFFFFFF는 "none").
pub fn color_attr(c: u32) -> String {
    if c == 0xFFFF_FFFF {
        return "none".to_string();
    }
    format!(
        "#{:02X}{:02X}{:02X}",
        c & 0xFF,
        (c >> 8) & 0xFF,
        (c >> 16) & 0xFF
    )
}

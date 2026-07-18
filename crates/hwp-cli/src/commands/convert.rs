//! `hwp convert` — 포맷 변환.
//!
//! M2 범위: hwp/hwpx → markdown/JSON. hwpx 쓰기(M4)와 hwp 쓰기(M6)는
//! 이후 마일스톤.

use std::path::Path;

use crate::ConvertFormat;
use crate::commands::cat::load_document;

/// markdown 출력 전용 추가 옵션 (다른 포맷에서는 무시).
#[derive(Default)]
pub struct MdOpts<'a> {
    /// 이미지 추출 디렉터리 (기본: "<출력스템>.media").
    pub media_dir: Option<&'a Path>,
    /// 머리말/꼬리말 텍스트 포함.
    pub with_header_footer: bool,
    /// 숨은 설명 텍스트 포함.
    pub with_hidden: bool,
}

pub fn run(
    input: &Path,
    output: &Path,
    to: Option<ConvertFormat>,
    strict: bool,
    preserve_layout: bool,
    embed_bin: bool,
    md_opts: &MdOpts,
) -> anyhow::Result<()> {
    // PDF는 문서 포맷 변환이 아니라 렌더 출력 — render 경로에 위임한다
    // (사용자의 "변환" 프레이밍 대응: `hwp convert in.hwp -o out.pdf`).
    if to.is_none()
        && output
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
    {
        return crate::commands::render::run(
            input,
            output,
            "all",
            96.0,
            Some(crate::RenderFormat::Pdf),
            Vec::new(),
        );
    }

    let target = match to {
        Some(t) => t,
        None => infer_format(output)?,
    };

    match target {
        ConvertFormat::Md => {
            let doc = load_document(input)?;
            // 이미지가 있으면 media 디렉터리에 추출해 상대참조한다(이미지 없으면
            // 디렉터리를 만들지 않음 — to_markdown_with가 첫 이미지에서 지연 생성).
            // --media-dir 지정 시: 상대경로는 출력 파일 기준으로 해석하고, 링크는
            // 사용자가 준 경로 문자열을 그대로 쓴다(기본은 "<스템>.media" 관례).
            let (dir, prefix) = match md_opts.media_dir {
                Some(d) => {
                    let resolved = if d.is_absolute() {
                        d.to_path_buf()
                    } else {
                        output.parent().map_or_else(
                            || d.to_path_buf(),
                            |parent| {
                                if parent.as_os_str().is_empty() {
                                    d.to_path_buf()
                                } else {
                                    parent.join(d)
                                }
                            },
                        )
                    };
                    (resolved, Some(d.to_string_lossy().into_owned()))
                }
                None => (output.with_extension("media"), None),
            };
            let md = hwp_convert::to_markdown_with(
                &doc,
                &hwp_convert::MarkdownOptions {
                    media_dir: Some(&dir),
                    media_prefix: prefix.as_deref(),
                    text: hwp_model::TextOptions {
                        include_header_footer: md_opts.with_header_footer,
                        include_hidden: md_opts.with_hidden,
                    },
                },
            )?;
            std::fs::write(output, md)?;
        }
        ConvertFormat::Html => {
            let doc = load_document(input)?;
            std::fs::write(output, hwp_convert::to_html(&doc))?;
        }
        ConvertFormat::Odt => {
            let doc = load_document(input)?;
            std::fs::write(output, hwp_convert::to_odt(&doc)?)?;
        }
        ConvertFormat::Pdf => {
            let doc = load_document(input)?;
            let result = hwp_render::render_document_pdf(&doc, &pdf_render_opts(), None)?;
            print_warnings(&result.report);
            std::fs::write(output, &result.data)?;
        }
        ConvertFormat::Json => {
            let doc = load_document(input)?;
            std::fs::write(output, hwp_convert::to_json(&doc, true, embed_bin)?)?;
        }
        ConvertFormat::Hwpx => {
            let doc = load_document(input)?;
            let warnings = hwpx::write::write_document_with(
                &doc,
                output,
                &hwpx::write::HwpxWriteOptions {
                    preserve_linesegs: preserve_layout,
                },
            )?;
            print_warnings(&warnings);
            bail_on_strict(strict, &warnings)?;
        }
        ConvertFormat::Hwp => {
            let doc = load_document(input)?;
            let warnings = write_hwp(&doc, output, preserve_layout)?;
            print_warnings(&warnings);
            bail_on_strict(strict, &warnings)?;
        }
    }
    eprintln!("변환 완료: {} → {}", input.display(), output.display());
    Ok(())
}

/// 경고 목록을 stderr로 출력.
pub(crate) fn print_warnings(warnings: &[String]) {
    for w in warnings {
        eprintln!("경고: {w}");
    }
}

/// `--strict`이고 보존 불가(`DROP:`) 경고가 있으면 비정상 종료한다.
/// 구조 보존 대상(hwp/hwpx)에만 의미가 있다 (md/html/pdf는 본디 손실 변환).
fn bail_on_strict(strict: bool, warnings: &[String]) -> anyhow::Result<()> {
    if !strict {
        return Ok(());
    }
    let drops: Vec<&str> = warnings
        .iter()
        .filter(|w| w.starts_with("DROP: "))
        .map(|w| w.as_str())
        .collect();
    if drops.is_empty() {
        return Ok(());
    }
    anyhow::bail!(
        "--strict: 보존 불가 데이터 {}건 드롭\n{}",
        drops.len(),
        drops
            .iter()
            .map(|w| format!("  - {}", w.trim_start_matches("DROP: ")))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// 출력 확장자에 따라 문서를 쓴다 (edit/convert/new/MCP 공용).
/// `edited`면 hwp 쓰기에 외과적 편집 경로(`write_hwp_edited`)를 쓴다.
pub fn write_by_ext(
    doc: &hwp_model::Document,
    output: &Path,
    edited: bool,
    embed_bin: bool,
) -> anyhow::Result<()> {
    let warnings = match output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("hwp") => {
            if edited {
                write_hwp_edited(doc, output)?
            } else {
                write_hwp(doc, output, false)?
            }
        }
        Some("hwpx") => hwpx::write::write_document_with(
            doc,
            output,
            &hwpx::write::HwpxWriteOptions {
                preserve_linesegs: false,
            },
        )?,
        Some("json") => {
            std::fs::write(output, hwp_convert::to_json(doc, true, embed_bin)?)?;
            Vec::new()
        }
        Some("md") | Some("markdown") => {
            std::fs::write(output, hwp_convert::to_markdown(doc))?;
            Vec::new()
        }
        Some("html") | Some("htm") => {
            std::fs::write(output, hwp_convert::to_html(doc))?;
            Vec::new()
        }
        Some("odt") => {
            std::fs::write(output, hwp_convert::to_odt(doc)?)?;
            Vec::new()
        }
        Some("pdf") => {
            let result = hwp_render::render_document_pdf(doc, &pdf_render_opts(), None)?;
            std::fs::write(output, &result.data)?;
            result.report
        }
        other => anyhow::bail!("출력 포맷을 추론할 수 없습니다 (확장자: {other:?})"),
    };
    print_warnings(&warnings);
    Ok(())
}

/// `--font-dir`가 비었으면 `HWP_FONT_DIR`(없으면 `fonts/`)로 기본 폰트 디렉터리를 정한다.
/// render·diff·convert(PDF)가 번들 함초롬 글꼴을 명시 인자 없이도 로드하도록 —
/// 안 그러면 한글 기본 글꼴을 못 찾아 시스템 폰트로 대체돼 렌더 충실도가 크게 떨어진다.
pub fn resolve_font_dirs(given: Vec<std::path::PathBuf>) -> Vec<std::path::PathBuf> {
    if !given.is_empty() {
        return given;
    }
    vec![std::path::PathBuf::from(
        std::env::var("HWP_FONT_DIR").unwrap_or_else(|_| "fonts".into()),
    )]
}

/// PDF 렌더 옵션 — 폰트는 `HWP_FONT_DIR`(없으면 `fonts/`)에서 해석.
fn pdf_render_opts() -> hwp_render::RenderOptions {
    hwp_render::RenderOptions {
        dpi: 96.0,
        font_dirs: resolve_font_dirs(Vec::new()),
    }
}

fn infer_format(output: &Path) -> anyhow::Result<ConvertFormat> {
    match output
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("md") | Some("markdown") => Ok(ConvertFormat::Md),
        Some("html") | Some("htm") => Ok(ConvertFormat::Html),
        Some("odt") => Ok(ConvertFormat::Odt),
        Some("pdf") => Ok(ConvertFormat::Pdf),
        Some("json") => Ok(ConvertFormat::Json),
        Some("hwpx") => Ok(ConvertFormat::Hwpx),
        Some("hwp") => Ok(ConvertFormat::Hwp),
        other => {
            anyhow::bail!("출력 포맷을 추론할 수 없습니다 (확장자: {other:?}) — --to로 지정하세요")
        }
    }
}

/// hwp 바이너리 저장 (1쪽 렌더를 PrvImage로 동봉).
///
/// 합성 문서(md/hwpx 출신)는 줄 배치(PARA_LINE_SEG)가 없으면 5.1.x 한글이
/// 본문을 못 그린다(검은 바/빈 내용/손상). 폰트 셰이핑으로 정확한 줄바꿈을
/// 계산해 IR에 채운 뒤 쓴다 — 한글과 동일한 함초롬바탕 폰트가 필요하다
/// (HWP_FONT_DIR 환경변수 또는 프로젝트 `fonts/`).
pub fn write_hwp(
    doc: &hwp_model::Document,
    output: &std::path::Path,
    preserve_layout: bool,
) -> anyhow::Result<Vec<String>> {
    write_hwp_impl(doc, output, preserve_layout, false)
}

/// 편집된 문서를 hwp로 다시 쓴다.
///
/// hwp5 원본 편집은 **외과적**으로 처리한다: 편집된 문단만 줄 배치를 비워 한글이
/// 그 문단만 재계산하게 하고(편집 프리미티브가 이미 비움), 미편집 문단은 원본 줄
/// 배치를 보존한다. 줄 배치를 전부 비우면 한글이 모든 문단을 재계산하면서 표 셀의
/// 빈 문단까지 큰 글자 크기로 한 줄을 잡아 행 높이가 부풀어 빈 칸이 생긴다(실측).
/// 한글 자신이 편집 시 바뀐 문단만 다시 배치하는 것과 같은 동작.
///
/// hwpx/md 출신은 보존할 원본 줄 배치가 한글 hwp 레이아웃과 다르므로 합성 경로
/// (`edited`=true: 줄 배치 비우고 para_shape 복원 후 한글 재계산)를 쓴다.
pub fn write_hwp_edited(
    doc: &hwp_model::Document,
    output: &std::path::Path,
) -> anyhow::Result<Vec<String>> {
    if doc.meta.source_format == "hwp5" {
        // 원본 줄 배치 보존(preserve), 합성 정규화 없음 — 편집 문단만 count=0.
        write_hwp_impl(doc, output, true, false)
    } else {
        write_hwp_impl(doc, output, false, true)
    }
}

/// 구조 편집(문단/표 행 추가·삭제)본을 hwp로 쓴다.
///
/// 모든 출처에 합성 경로(edited=true)를 강제한다 — 삽입된 문단/행에 문단끝 0x0d·
/// 마지막문단 비트·PARA/셀 카운트 같은 불변식이 적용돼야 하기 때문(convert/new와
/// 동일한 한글 수용 검증 경로). hwp5 무수정용 surgical `write_hwp_edited`와 분리한다.
pub fn write_hwp_structural(
    doc: &hwp_model::Document,
    output: &std::path::Path,
) -> anyhow::Result<()> {
    let warnings = write_hwp_impl(doc, output, false, true)?;
    print_warnings(&warnings);
    Ok(())
}

fn write_hwp_impl(
    doc: &hwp_model::Document,
    output: &std::path::Path,
    preserve_layout: bool,
    edited: bool,
) -> anyhow::Result<Vec<String>> {
    let font_dir =
        std::path::PathBuf::from(std::env::var("HWP_FONT_DIR").unwrap_or_else(|_| "fonts".into()));
    let synthesize = doc.meta.source_format != "hwp5" || edited;
    let has_source_linesegs = doc
        .sections
        .iter()
        .flat_map(|s| &s.paragraphs)
        .any(|p| !p.line_segs.is_empty());

    let mut report: Vec<String> = Vec::new();
    let owned;
    let doc = if !synthesize || preserve_layout {
        // hwp5 무수정/preserve-layout: 원본 줄 배치 그대로.
        doc
    } else if has_source_linesegs {
        // hwpx 출신 또는 편집된 hwp5: 저장된 줄 배치는 (편집으로) 내용과 어긋나거나
        // 한글의 hwpx 내보내기 레이아웃이라 hwp 저장본과 다를 수 있다(예: 같은
        // 문서가 hwpx 6쪽, hwp 5쪽). 줄 배치를 제거하면 한글이 열 때 문단/글자
        // 모양 기준으로 재계산해 hwp 저장본과 같은 페이지로 흐른다(문단 모양을
        // 정품과 일치시킨 게 핵심). 편집본도 이 경로를 그대로 쓴다(편집으로 낡은
        // 줄 배치를 비우고 한글이 재계산 — convert hwpx→hwp와 동일한 검증된 동작).
        let mut d = doc.clone();
        clear_linesegs(&mut d);
        owned = d;
        &owned
    } else {
        // markdown 등 줄 배치 없는 출처: 폰트 셰이핑으로 합성.
        let mut d = doc.clone();
        let mut store = hwp_render::FontStore::new();
        store.load_dir(&font_dir);
        let mut warns = Vec::new();
        hwp_render::lineseg::synthesize_linesegs(&mut d, &mut store, &mut warns);
        report.append(&mut warns);
        owned = d;
        &owned
    };

    let prv_image = hwp_render::render_document(
        doc,
        &hwp_render::RenderOptions {
            dpi: 48.0,
            font_dirs: vec![font_dir],
        },
    )
    .ok()
    .and_then(|out| out.pages.first().and_then(|p| p.encode_png().ok()));

    let warnings = hwp5::write_document(
        doc,
        output,
        &hwp5::WriteOptions {
            prv_image,
            preserve_linesegs: preserve_layout,
            edited,
        },
    )?;
    report.extend(warnings);
    Ok(report)
}

/// 모든 문단(표 셀·머리말 등 중첩 포함)의 줄 배치를 제거한다 — 한글이 열 때
/// 문단/글자 모양 기준으로 재계산하도록(hwpx 내보내기 줄배치가 hwp와 다른 문제 회피).
fn clear_linesegs(doc: &mut hwp_model::Document) {
    fn clear_para(para: &mut hwp_model::Paragraph) {
        para.line_segs.clear();
        for control in &mut para.controls {
            match control {
                hwp_model::Control::Table(t) => {
                    for cell in &mut t.cells {
                        for p in &mut cell.paragraphs {
                            clear_para(p);
                        }
                    }
                }
                hwp_model::Control::Generic(g) => {
                    for list in &mut g.paragraph_lists {
                        for p in &mut list.paragraphs {
                            clear_para(p);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    for section in &mut doc.sections {
        for para in &mut section.paragraphs {
            clear_para(para);
        }
    }
}

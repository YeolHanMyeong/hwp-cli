//! hwp — HWP/HWPX 문서 처리 CLI.

mod commands;
mod format;

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "hwp", version, about = "HWP/HWPX 문서 처리 도구")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 파일 정보 표시: 포맷/버전/속성/스트림 목록
    Info {
        file: PathBuf,
        /// JSON으로 출력
        #[arg(long)]
        json: bool,
    },

    /// 텍스트 추출 (M1에서 구현)
    Cat {
        file: PathBuf,
        #[arg(long, value_enum, default_value = "plain")]
        format: TextFormat,
        /// 본문 파싱 없이 PrvText 미리보기만 출력
        #[arg(long)]
        preview: bool,
    },

    /// 포맷 변환 (M2부터 단계적 구현)
    Convert {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// 출력 포맷 (생략 시 확장자에서 추론)
        #[arg(long, value_enum)]
        to: Option<ConvertFormat>,
        /// 변환 중 보존 불가능한(opaque) 데이터 발견 시 실패 처리
        #[arg(long)]
        strict: bool,
        /// 줄 배치 캐시 보존 (무수정 왕복 전용 — 한글은 내용과 어긋난
        /// 줄 배치를 변조로 판정하므로 기본은 제거)
        #[arg(long)]
        preserve_layout: bool,
        /// JSON 출력 시 첨부 바이너리(이미지)를 base64로 임베드 (자급식 JSON)
        #[arg(long)]
        embed_bin: bool,
    },

    /// 페이지 렌더링 (M3에서 구현)
    Render {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// 페이지 범위: "1", "1-3", "all"
        #[arg(long, default_value = "all")]
        pages: String,
        #[arg(long, default_value_t = 96.0)]
        dpi: f64,
        /// 출력 포맷 (생략 시 확장자에서 추론)
        #[arg(long, value_enum)]
        format: Option<RenderFormat>,
        /// 추가 폰트 디렉터리 (반복 가능)
        #[arg(long)]
        font_dir: Vec<PathBuf>,
    },

    /// 새 문서 생성 (M4부터 구현)
    New {
        #[arg(short, long)]
        output: PathBuf,
        /// 입력 markdown/JSON 파일 (생략 시 빈 문서)
        #[arg(long)]
        from: Option<PathBuf>,
        /// 문서 스타일 프리셋 (markdown 입력에만 적용; JSON IR 입력은 무시)
        #[arg(long, value_enum, default_value = "plain")]
        preset: PresetArg,
        /// 문서 제목 (요약 정보 / OPF 메타데이터)
        #[arg(long)]
        title: Option<String>,
        /// 지은이
        #[arg(long)]
        author: Option<String>,
        /// 주제
        #[arg(long)]
        subject: Option<String>,
    },

    /// 렌더 결과를 한글 기준 PNG와 비교해 오차 측정 (위치 오프셋·픽셀 차이율)
    Diff {
        input: PathBuf,
        /// 한글에서 같은 페이지를 같은 DPI로 내보낸 기준 PNG
        #[arg(long)]
        r#ref: PathBuf,
        /// 비교할 페이지 (1-기반)
        #[arg(long, default_value_t = 1)]
        page: usize,
        #[arg(long, default_value_t = 96.0)]
        dpi: f64,
        /// 차이 이미지 출력 경로 (생략 시 <ref>.diff.png)
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// 추가 폰트 디렉터리 (반복 가능)
        #[arg(long)]
        font_dir: Vec<PathBuf>,
        /// 채널 차이 허용 오차 (이하면 동일 취급)
        #[arg(long, default_value_t = 16)]
        tolerance: u8,
    },

    /// 기존 문서 편집 (텍스트 치환·표 셀 설정) — 이미지·서식 보존
    Edit {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// 텍스트 치환 "찾기=>바꾸기" (반복 가능, 모든 일치 치환)
        #[arg(long)]
        replace: Vec<String>,
        /// 표 행 추가 "표:N" 또는 "표:템플릿행:N" (반복 가능, 0-기반). 마지막 행을
        /// 복제해 빈 행 N개 추가 — 새 행(인덱스 기존행수부터)은 --set-cell로 채운다.
        #[arg(long = "add-row")]
        add_row: Vec<String>,
        /// 표 셀 설정 "표:행:열=값" (반복 가능, 0-기반 인덱스)
        #[arg(long = "set-cell")]
        set_cell: Vec<String>,
        /// 필드/누름틀 채우기 "이름=값" (반복 가능 — hwp fields로 이름 확인)
        #[arg(long = "set-field")]
        set_field: Vec<String>,
        /// 메타데이터 설정 "키=값" (키: title|author|subject|keywords, 반복 가능)
        #[arg(long = "set-meta")]
        set_meta: Vec<String>,
        /// 메모(주석) 추가 "텍스트" (hwpx 출력 전용, 실험적, 반복 가능)
        #[arg(long = "add-memo")]
        add_memo: Vec<String>,
        /// 쓰기 후 재읽기로 검증
        #[arg(long)]
        verify: bool,
    },

    /// 필드/누름틀 목록 표시 (이름·종류·값)
    Fields {
        file: PathBuf,
        /// JSON으로 출력
        #[arg(long)]
        json: bool,
    },

    /// `{{name}}` 텍스트 자리표시자(템플릿 슬롯) 목록 표시
    Slots {
        file: PathBuf,
        /// JSON으로 출력
        #[arg(long)]
        json: bool,
    },

    /// 충실도 보존 템플릿 채우기 (hwpx의 `{{name}}` 치환, 패키지 보존)
    Fill {
        input: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
        /// 자리표시자 채우기 "이름=값" (반복 가능; `{{이름}}` 치환)
        #[arg(long)]
        set: Vec<String>,
        /// 이름→값 JSON 객체 파일 (일괄 채우기)
        #[arg(long)]
        data: Option<PathBuf>,
        /// 치환 요약을 JSON으로 출력 ({output, replaced, counts})
        #[arg(long)]
        json: bool,
    },

    /// 구조 검증 (mimetype/필수 엔트리/XML 파싱) — 유효하면 종료코드 0
    Validate {
        file: PathBuf,
        /// JSON으로 출력
        #[arg(long)]
        json: bool,
    },

    /// MCP(Model Context Protocol) stdio 서버 — AI 에이전트용 도구 인터페이스
    Mcp {
        /// 렌더/diff 도구의 기본 폰트 디렉터리 (반복 가능)
        #[arg(long)]
        font_dir: Vec<PathBuf>,
    },

    /// [개발자용] 레코드/패키지 구조 덤프
    Dump {
        file: PathBuf,
        /// 대상 스트림/엔트리 (예: "DocInfo", "BodyText/Section0", "Contents/header.xml")
        #[arg(long)]
        stream: Option<String>,
        /// 레코드 페이로드를 hex로 출력
        #[arg(long)]
        raw: bool,
        /// JSON으로 출력
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum TextFormat {
    Plain,
    Markdown,
    Json,
    Html,
}

#[derive(Clone, Copy, ValueEnum)]
enum ConvertFormat {
    Hwp,
    Hwpx,
    Md,
    Json,
    Html,
    Pdf,
    Odt,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum RenderFormat {
    Png,
    Svg,
    Pdf,
}

#[derive(Clone, Copy, ValueEnum)]
enum PresetArg {
    Plain,
    Gongmun,
    Bogoseo,
}

impl From<PresetArg> for hwp_convert::Preset {
    fn from(p: PresetArg) -> Self {
        match p {
            PresetArg::Plain => hwp_convert::Preset::Plain,
            PresetArg::Gongmun => hwp_convert::Preset::Gongmun,
            PresetArg::Bogoseo => hwp_convert::Preset::Bogoseo,
        }
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Info { file, json } => commands::info::run(&file, json),
        Cmd::Dump {
            file,
            stream,
            raw,
            json,
        } => commands::dump::run(&file, stream.as_deref(), raw, json),
        Cmd::Cat {
            file,
            preview: true,
            ..
        } => commands::cat::preview(&file),
        Cmd::Cat { file, format, .. } => commands::cat::run(&file, format),
        Cmd::Convert {
            input,
            output,
            to,
            strict,
            preserve_layout,
            embed_bin,
        } => commands::convert::run(&input, &output, to, strict, preserve_layout, embed_bin),
        Cmd::Render {
            input,
            output,
            pages,
            dpi,
            format,
            font_dir,
        } => commands::render::run(&input, &output, &pages, dpi, format, font_dir),
        Cmd::Diff {
            input,
            r#ref,
            page,
            dpi,
            out,
            font_dir,
            tolerance,
        } => commands::diff::run(
            &input,
            &r#ref,
            page,
            dpi,
            out.as_deref(),
            font_dir,
            tolerance,
        ),
        Cmd::Mcp { font_dir } => commands::mcp::run(font_dir),
        Cmd::New {
            output,
            from,
            preset,
            title,
            author,
            subject,
        } => commands::new::run(
            &output,
            from.as_deref(),
            preset.into(),
            title,
            author,
            subject,
        ),
        Cmd::Edit {
            input,
            output,
            replace,
            add_row,
            set_cell,
            set_field,
            set_meta,
            add_memo,
            verify,
        } => commands::edit::run(
            &input, &output, &replace, &add_row, &set_cell, &set_field, &set_meta, &add_memo,
            verify,
        ),
        Cmd::Fields { file, json } => commands::fields::run(&file, json),
        Cmd::Slots { file, json } => commands::slots::run(&file, json),
        Cmd::Fill {
            input,
            output,
            set,
            data,
            json,
        } => commands::fill::run(&input, &output, &set, data.as_deref(), json),
        Cmd::Validate { file, json } => commands::validate::run(&file, json),
    }
}

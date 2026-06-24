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
        /// 표 셀 설정 "표:행:열=값" (반복 가능, 0-기반 인덱스)
        #[arg(long = "set-cell")]
        set_cell: Vec<String>,
        /// 쓰기 후 재읽기로 검증
        #[arg(long)]
        verify: bool,
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
}

#[derive(Clone, Copy, ValueEnum)]
enum ConvertFormat {
    Hwp,
    Hwpx,
    Md,
    Json,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum RenderFormat {
    Png,
    Svg,
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
        Cmd::New { output, from } => commands::new::run(&output, from.as_deref()),
        Cmd::Edit {
            input,
            output,
            replace,
            set_cell,
            verify,
        } => commands::edit::run(&input, &output, &replace, &set_cell, verify),
    }
}

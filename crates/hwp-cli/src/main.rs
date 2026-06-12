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
        /// 입력 markdown/텍스트 파일 (생략 시 빈 문서)
        #[arg(long)]
        from: Option<PathBuf>,
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
        } => commands::convert::run(&input, &output, to, strict),
        Cmd::Render {
            input,
            output,
            pages,
            dpi,
            format,
            font_dir,
        } => commands::render::run(&input, &output, &pages, dpi, format, font_dir),
        Cmd::New { .. } => anyhow::bail!("`hwp new`는 아직 구현되지 않았습니다 (M4 예정)"),
    }
}

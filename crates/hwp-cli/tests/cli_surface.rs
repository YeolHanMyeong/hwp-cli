//! CLI 표면 커버리지 — CI에서 실행되는 서브커맨드 스모크 모음.
//!
//! 기존에는 `mcp`·`diff`·`render`·PDF 경로가 로컬 픽스처 게이트라 CI 커버리지가 0이었다.
//! 이 스위트는 **커밋된 픽스처**(fixtures/samples/report-tables.hwpx)에 하드 의존해
//! 스킵 없이 동작한다(신규 의존성 0 — hwp5·serde_json 기존 의존 재사용).
//!
//! 폰트가 필요한 경로(render/diff/pdf)는 `HWP_FONT_DIR=<repo>/fonts`를 명시해
//! CI(ubuntu noto-cjk·macOS)와 로컬에서 결정적으로 만든다.

use std::io::{BufRead, BufReader, Write as _};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn hwp() -> Command {
    Command::new(env!("CARGO_BIN_EXE_hwp"))
}

/// 폰트 번들이 필요한 hwp 호출 (렌더/디프/PDF 경로).
fn hwp_fonted() -> Command {
    let mut c = hwp();
    c.env("HWP_FONT_DIR", repo().join("fonts"));
    c
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn fixture() -> PathBuf {
    let p = repo().join("fixtures/samples/report-tables.hwpx");
    assert!(p.exists(), "커밋된 픽스처 없음: {}", p.display());
    p
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hwp-cli-surface-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

/// `--pages 1` 렌더는 정확히 out.png 1개, PNG 시그니처 + IHDR 794×1123(A4@96dpi —
/// 크기는 secPr 유래라 폰트 비의존이라 CI 폰트 환경에서도 동일).
#[test]
fn render_png_page1_smoke() {
    let out = tmp("page1.png");
    let r = hwp_fonted()
        .arg("render")
        .arg(fixture())
        .arg("-o")
        .arg(&out)
        .args(["--pages", "1"])
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "render: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    // 정확히 out.png 단일 파일(다중 페이지면 out-N.png로 갈리는지 확인).
    let siblings: Vec<_> = out
        .parent()
        .unwrap()
        .read_dir()
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("page1"))
        .collect();
    assert_eq!(siblings.len(), 1, "단일 페이지 출력 파일 1개");

    let data = std::fs::read(&out).unwrap();
    assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n", "PNG 시그니처");
    let w = u32::from_be_bytes(data[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(data[20..24].try_into().unwrap());
    assert_eq!((w, h), (794, 1123), "A4 @ 96dpi");
}

/// 자기 렌더를 기준 PNG로 되먹는 diff — 지표는 전부 완전 일치여야 한다
/// (diff는 불일치여도 exit 0이므로 **출력 지표 문자열이 검증 대상**).
#[test]
fn diff_self_consistency() {
    let png = tmp("self.png");
    assert!(
        hwp_fonted()
            .arg("render")
            .arg(fixture())
            .arg("-o")
            .arg(&png)
            .args(["--pages", "1"])
            .status()
            .unwrap()
            .success()
    );
    let r = hwp_fonted()
        .arg("diff")
        .arg(fixture())
        .arg("--ref")
        .arg(&png)
        .args(["--page", "1"])
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "diff: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    let stdout = String::from_utf8_lossy(&r.stdout);
    for needle in [
        "잉크 적용률(완전성): 100.0%",
        "dx=0px, dy=0px",
        "픽셀 차이율: 0.00%",
    ] {
        assert!(
            stdout.contains(needle),
            "자기 일치 지표: {needle}\n{stdout}"
        );
    }
}

/// PDF 두 경로(convert 위임·render 직접) 모두 구조적으로 유효한 PDF를 낸다.
/// 정확한 페이지 수는 폰트 리플로우로 달라질 수 있어 단언하지 않는다.
#[test]
fn pdf_smoke_convert_and_render_paths() {
    let check = |out: PathBuf, label: &str| {
        let data = std::fs::read(&out).unwrap();
        assert!(data.starts_with(b"%PDF-"), "{label}: %PDF- 헤더");
        assert!(
            data.windows(5).rev().take(2048).any(|w| w == b"%%EOF"),
            "{label}: %%EOF 트레일러"
        );
        let pages = data.windows(12).filter(|w| *w == b"/Type /Pages").count();
        assert_eq!(pages, 1, "{label}: /Type /Pages는 1개");
        let page = data.windows(11).filter(|w| *w == b"/Type /Page").count();
        assert!(page >= 2, "{label}: /Type /Page 마커(루트+페이지들) >= 2");
        assert!(data.len() > 10_000, "{label}: 내용 있는 크기 (>10KB)");
    };
    // convert 위임 경로 (hwp convert -o x.pdf → render 경로 위임).
    let c = tmp("conv.pdf");
    let r1 = hwp_fonted()
        .arg("convert")
        .arg(fixture())
        .arg("-o")
        .arg(&c)
        .output()
        .unwrap();
    assert!(
        r1.status.success(),
        "convert pdf: {}",
        String::from_utf8_lossy(&r1.stderr)
    );
    check(c, "convert");
    // render 직접 경로.
    let d = tmp("rend.pdf");
    let r2 = hwp_fonted()
        .arg("render")
        .arg(fixture())
        .arg("-o")
        .arg(&d)
        .output()
        .unwrap();
    assert!(
        r2.status.success(),
        "render pdf: {}",
        String::from_utf8_lossy(&r2.stderr)
    );
    check(d, "render");
}

/// MCP stdio 세션 — 라인 단위 JSON-RPC(실측: Content-Length 프레이밍 아님).
/// initialize → initialized → tools/list → tools/call(hwp_validate) 후 stdin EOF로 종료.
/// 수신은 스레드+채널 recv_timeout(60s), 종료는 try_wait 루프+kill(30s) — CI 행 방지.
#[test]
fn mcp_stdio_session() {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let mut child = hwp()
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("hwp mcp spawn");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    let (tx, rx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let recv = |what: &str| -> serde_json::Value {
        let line = rx
            .recv_timeout(Duration::from_secs(60))
            .unwrap_or_else(|_| panic!("MCP 응답 타임아웃: {what}"));
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("JSON 파싱: {line}"))
    };
    let mut send = |v: serde_json::Value| {
        stdin
            .write_all(serde_json::to_string(&v).unwrap().as_bytes())
            .unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    };

    send(
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2024-11-05","capabilities":{},
        "clientInfo":{"name":"cli_surface","version":"0"}}}),
    );
    let init = recv("initialize");
    assert_eq!(init["id"], 1);
    assert!(
        init["result"]["serverInfo"]["name"].is_string(),
        "serverInfo: {init}"
    );

    send(serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    send(serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}));
    let list = recv("tools/list");
    let mut names: Vec<String> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect();
    names.sort();
    let expect: Vec<String> = [
        "hwp_convert",
        "hwp_diff",
        "hwp_edit",
        "hwp_fill",
        "hwp_info",
        "hwp_list_bookmarks",
        "hwp_list_fields",
        "hwp_new",
        "hwp_read",
        "hwp_render",
        "hwp_slots",
        "hwp_validate",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(names, expect, "도구 12종");

    send(
        serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{
        "name":"hwp_validate","arguments":{"path": fixture().to_string_lossy()}}}),
    );
    let call = recv("tools/call");
    assert_eq!(call["id"], 3);
    let text = call["result"]["content"][0]["text"].as_str().unwrap();
    let v: serde_json::Value = serde_json::from_str(text).unwrap();
    assert_eq!(v["valid"], true, "hwp_validate 결과: {text}");

    // stdin EOF = 종료 신호. try_wait 루프(최대 30s) 후 kill.
    drop(stdin);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            assert!(status.success(), "MCP 종료 코드: {status}");
            break;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("MCP가 stdin EOF 후 30s 내 종료하지 않음");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// hwp5 합성 왕복 게이트: hwpx 픽스처 → a.hwp → `convert a.hwp -o b.hwp --preserve-layout`
/// 후 **스트림 단위 바이트 동일** 단언.
///
/// 주의: 이 테스트는 로컬 전용 정품 identity 게이트(crates/hwp5/tests/identity.rs,
/// 로컬 픽스처 필요)를 대체하지 않고 **보완**한다 — 커밋 픽스처로 CI에서 도는 합성 경로
/// 왕복이다. 전체 파일 비교는 불가: cfb 크레이트가 디렉터리 엔트리에 Timestamp::now()
/// (18바이트)를 찍어 파일 단위 해시는 매번 달라진다(실측). `--preserve-layout`은
/// 줄 배치 캐시 보존 전제 — 무수정 왕복 경로를 타게 하는 필수 플래그.
#[test]
fn hwp5_synthetic_identity_gate() {
    let a = tmp("a.hwp");
    let r1 = hwp()
        .arg("convert")
        .arg(fixture())
        .arg("-o")
        .arg(&a)
        .output()
        .unwrap();
    assert!(
        r1.status.success(),
        "hwpx→hwp: {}",
        String::from_utf8_lossy(&r1.stderr)
    );
    let b = tmp("b.hwp");
    let r2 = hwp()
        .arg("convert")
        .arg(&a)
        .arg("-o")
        .arg(&b)
        .arg("--preserve-layout")
        .output()
        .unwrap();
    assert!(
        r2.status.success(),
        "hwp→hwp(preserve-layout): {}",
        String::from_utf8_lossy(&r2.stderr)
    );

    let mut ca = hwp5::Hwp5Container::open(&a).unwrap();
    let mut cb = hwp5::Hwp5Container::open(&b).unwrap();
    let sa: Vec<String> = ca.list_streams().iter().map(|s| s.path.clone()).collect();
    let sb: Vec<String> = cb.list_streams().iter().map(|s| s.path.clone()).collect();
    assert_eq!(sa, sb, "스트림 목록 동일");
    for name in &sa {
        let ra = ca.read_stream_raw(name).unwrap();
        let rb = cb.read_stream_raw(name).unwrap();
        assert_eq!(ra, rb, "스트림 바이트 동일: {name}");
    }
}

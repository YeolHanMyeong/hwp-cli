#!/usr/bin/env bash
# 로컬 CI 미러 — .github/workflows/ci.yml의 3종 게이트와 동일한 커맨드를 그대로 실행.
# PR 전에 이 스크립트 한 방으로 로컬 검증을 끝낸다 (실패 시 그 지점에서 중단).
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
echo "== check: OK (fmt/clippy/test = CI 게이트) =="

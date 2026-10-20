#!/usr/bin/env bash
# 全量离线门（box 与 Mac 共用）。任何一步失败即非零退出。
# 用法：scripts/gates.sh            （在仓库根目录）
# 环境：Node ≥ .nvmrc；cargo 1.99（~/.cargo/bin）；CARGO_TARGET_DIR 可选。
set -euo pipefail
cd "$(dirname "$0")/.."
# 与 npm test 同源：Node 版本 / iCloud 路径守卫（docs/qa/vitest-root-cause.md）
node scripts/testEnvGuard.mjs
npx tsc --noEmit && echo TSC_OK
npx vitest run
npx vite build >/dev/null && echo BUILD_OK
cd src-tauri
cargo test --lib
cargo test --test headless_offline
cargo test --test repo_hygiene --test large_scale_matrix
cargo test --tests --no-run >/dev/null && echo TESTS_COMPILE_OK
# BLUEPRINT_v2 §5 修订：clippy 零告警门（-D warnings）
cargo clippy --all-targets -- -D warnings && echo CLIPPY_CLEAN

#!/usr/bin/env bash
# Mac 同步后验收脚本（P0 审计第 7 项）。在 Mac 仓库根目录运行：
#   cd /Users/hakayi/xiangmu/shiyi-music && scripts/mac-verify.sh
# 安全约束：本脚本只**读**真实账户 `global`（只取 SHA-256，不打印明文），绝不写/删 global 或 role:*；
# 实写测试只用隔离账户 test:shiyi:manual:<pid>，结束（含失败/中断）必删。
set -euo pipefail
cd "$(dirname "$0")/.."
SERVICE=shiyi-music
PATH="$HOME/.cargo/bin:$PATH"
pass() { printf '  ✅ %s\n' "$1"; }
fail() { printf '  ❌ %s\n' "$1"; exit 1; }
step() { printf '\n== %s ==\n' "$1"; }

step "0 环境"
cargo --version | grep -q "1.99" && pass "cargo 1.99" || fail "需要 ~/.cargo/bin/cargo 1.99（当前 $(cargo --version)）"
node -v
[[ "$PWD" != *"/Desktop/"* ]] && pass "仓库不在 iCloud Desktop 下（${PWD}）" || fail "仓库仍在 Desktop（C-01 根因）"

step "1 钥匙串验收（只读 global）"
kc_hash() { local pw; if pw=$(security find-generic-password -s "$SERVICE" -a global -w 2>/dev/null); then printf %s "$pw" | shasum -a 256 | cut -c1-16; fi; }
BEFORE=$(kc_hash)
[ -n "$BEFORE" ] && pass "global 存在（sha256 前缀 ${BEFORE}）" || echo "  ⚠️ global 不存在：请先在 App 内重录 Key（P0-0）"

step "2 macOS 专属钥匙串测试（隔离账户）"
OUT=$(cd src-tauri && cargo test --lib commands::keychain 2>&1) || { echo "$OUT"; fail "keychain 测试失败"; }
for t in classify_get_exit_44_means_absent classify_get_exit_other_codes_are_errors cli_round_trip cli_get_global_is_read_only; do
  echo "$OUT" | grep -q "keychain::tests::$t ... ok" && pass "$t" || { echo "$OUT"; fail "$t 未运行或失败"; }
done
AFTER=$(kc_hash)
[ "$BEFORE" = "$AFTER" ] && pass "测试前后 global 摘要一致" || fail "global 在测试期间被改动！"
LEFT=$(security dump-keychain 2>/dev/null | grep -c "test:shiyi" || true)
[ "$LEFT" = "0" ] && pass "无 test:shiyi 残留账户" || fail "残留 test:shiyi 账户 $LEFT 个"

step "3 security -i 特殊字符实写（隔离账户，读回比对）"
ACCT="test:shiyi:manual:$$"
cleanup() { security delete-generic-password -s "$SERVICE" -a "$ACCT" >/dev/null 2>&1 || true; }
trap cleanup EXIT
# 命令行格式与 keychain.rs::interactive_set_command 逐字一致
for K in 'sk-a#b' 'sk;rm' 'sk$HOME' 'sk&x|y' 'sk*?!' 'sk%~`' 'sk<>=@' 'sk,:/+' '#leading' 'trail;'; do
  printf 'add-generic-password -U -A -s %s -a %s -w %s\n' "$SERVICE" "$ACCT" "$K" | security -i >/dev/null 2>&1 || fail "security -i 写入失败：$K"
  GOT=$(security find-generic-password -s "$SERVICE" -a "$ACCT" -w 2>/dev/null || true)
  [ "$GOT" = "$K" ] && pass "读回一致：$K" || fail "读回不一致：写 [$K] 读 [$GOT] → 需在 interactive_set_command 加引号或扩大格式门"
done
cleanup

step "4 全量离线门（含 macOS cfg 代码的 clippy -D warnings）"
scripts/gates.sh >/tmp/shiyi-gates.log 2>&1 && pass "scripts/gates.sh" || { tail -40 /tmp/shiyi-gates.log; fail "gates 失败（日志 /tmp/shiyi-gates.log）"; }
for i in 1 2 3; do npm test >/dev/null 2>&1 && pass "npm test 第 $i 次" || fail "npm test 第 $i 次失败（C-01）"; done

step "5 GUI 手测（需要人工）"
cat <<'TXT'
  a) npm run tauri dev → 设置里确认 Key 显示正常；退出 App 再打开，Key 仍在（不被清空）。
  b) 设置 → 导出设置，保存到 ~/Desktop；历史 → 导出历史，保存到 ~/Desktop。
  c) 测试连接成功后，设置页出现"结构化输出：json_schema/json_object"提示（P1-2 探测）。
TXT
read -r -p "  完成 a-c 后回车继续…" _
ls -la ~/Desktop | grep -Ei "shiyi|suno|settings|history" && pass "Desktop 上可见导出文件" || fail "未找到导出文件（检查 fs scope / 对话框授权）"
AFTER2=$(kc_hash)
[ "$BEFORE" = "$AFTER2" ] || echo "  ⚠️ global 摘要变化：若 a) 中你重录了 Key 属正常，否则请排查"
printf '\n全部通过。\n'

# 发布与打包说明

本地**不保留**打包产物（`src-tauri/target/`、`dist/`、`.dmg`/`.app`/`.msi` 等）。正式打包在 Git 仓库经 GitHub Actions 完成；本机只做开发、门禁与推送。

## 1. 前置环境

| 项 | 要求 |
|---|---|
| Node.js | `.nvmrc`（当前 22.23.x）；`engines` 允许 `^22.22.2 \|\| ^24.15.0 \|\| >=26` |
| Rust | `stable`（与 CI `dtolnay/rust-toolchain@stable` 一致） |
| 包管理 | 仓库根目录执行 `npm ci`（勿改 lock 后用 `npm install` 提交意外漂移） |

```bash
node -v   # 应对齐 .nvmrc
rustc -V  # stable
npm ci
```

## 2. 本地门禁

推送前必须绿：

```bash
bash scripts/gates.sh
```

该脚本串行跑：TypeScript、ESLint、vitest、vite build、cargo lib / headless_offline / repo_hygiene / matrix、tests-compile、clippy `-D warnings`。任一步失败即退出。

Mac 专属验收（钥匙串只读、特殊字符实写等）见 `scripts/mac-verify.sh`；**永不**写入或删除钥匙串生产账户 `global`。

## 3. 版本号三处同步 + CHANGELOG

发版前把版本写到同一数字（当前为 `0.5.5`）：

1. `package.json` → `"version"`
2. `src-tauri/Cargo.toml` → `[package] version`
3. `src-tauri/tauri.conf.json` → 若有 `version` 字段一并改（无则跳过）

并在 `CHANGELOG.md`：

- 把 `## Unreleased` 下已定稿条目移入 `## X.Y.Z（日期）…`
- 保留空的 `## Unreleased` 段（`repo_hygiene` 会检查）

## 4. 推送到 GitHub（主分支）

```bash
git status
git push origin main
```

- **禁止** `--no-verify`（绕过 hooks / 门禁）。
- **禁止** 提交 API Key、`.env`、`.secrets/`、钥匙串导出。密钥只放本机钥匙串或仓库外的 `.secrets/`。

## 5. 打 tag → CI 打包 + Release

工作流：`.github/workflows/build.yml`

- `push` 到 `main` 或 `v*` tag、以及 `workflow_dispatch` 会跑测试矩阵（macOS + Windows）与构建。
- 推送版本 tag 触发产物与 GitHub Release：

```bash
git tag v0.5.6
git push origin v0.5.6
```

CI 会在门禁通过后产出 macOS / Windows 安装包，并挂到对应 Release。

## 6. 下载产物 · 签名与公证

1. 打开仓库 GitHub → Actions / Releases，下载对应平台工件。
2. **macOS**：分发前需 Apple Developer 签名与公证（Notarization）；个人自用可跳过，对外分发必须完成。
3. **Windows**：CI 产出 NSIS/ MSI 类安装包；正式分发建议 Authenticode 签名。
4. 本地 `npm run tauri build` 仅供自测，产物不要提交进 Git。

## 7. Key 安全

| 允许 | 禁止 |
|---|---|
| macOS Keychain 账户 `global` / `role:*` | 把真实 Key 写进源码、测试、文档、CI secret 日志 |
| 本机仓库外 `/path/.secrets/*.env`（已 gitignore） | 提交 `.env`、`.pem`、`.key`、钥匙串导出 |
| 测试哑值（如 `ak-plain-legacy`、`sk-test-…`） | 用生产 Key 跑会写入 Keychain `global` 的脚本 |

测试约定（RULE-KEYCHAIN-RO）：`cargo test` 只读真实 Key，写/删一律走 `test:shiyi:` 隔离账户。

## 附录：本地开发构建（可选）

```bash
npm run tauri dev      # 开发
npm run tauri build    # 本机打一次包自测；打完可删 src-tauri/target 与 dist
```

清理本机产物示例：

```bash
rm -rf src-tauri/target dist coverage
```

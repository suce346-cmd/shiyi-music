# C-01 vitest 不可复现：根因记录（P0-5，2026-10-20）

## 结论
**根因是环境，不是并发竞态。** 两个独立原因：

1. **工程位于 iCloud Drive 同步目录**（macOS“桌面与文稿文件夹”同步，`~/Desktop` 带
   `com.apple.file-provider-domain-id` 扩展属性，`FXICloudDriveDesktop=1`）。`node_modules`（≈29.5k 文件、297 MB）
   的每次读取都经 `fileproviderd`/`bird`；同步繁忙时（`brctl status`：CloudDocs `needs-sync / in-sync-up`）
   单文件读取可停顿到超时（实测 `cat node_modules/vitest/environments.d.ts` → `Operation timed out`；
   另有 48 个 dataless 占位文件）。vitest 并发冷启动 worker 读模块超时 →
   `Failed to start forks worker` / `Timeout waiting for worker to respond`。
2. **Node 版本低于依赖 engines**：vitest 5（`^22.12 || ^24 || >=26`）、jsdom 30（`^22.22.2 || ^24.15 || >=26`）、
   undici（`>=22.19`）。Node 20 下 `webidl.util.markAsUncloneable is not a function`。

## 证据（同一提交 187335a）

| 环境 | 命令 | 结果 |
|---|---|---|
| Mac mini，Node v26.8.1，10 核，`~/Desktop`（iCloud） | `vitest run`（默认并发）×2 | 两次均 exit 1：19/30 文件通过，11 errors（worker 启动超时），Duration 120 s（transform 63–70%），墙钟 434 s / 121 s |
| 同上 | `vitest run --maxWorkers=2` | **仍 exit 1**：28/30 文件、2 errors，Duration 67 s（environment 83%）。审计中“maxWorkers=2 全绿”是偶然 |
| box Linux，Node 22.23.3，8 核，本地盘 | `vitest run --pool=forks` / `--pool=threads` | 均 exit 0，30/30，≈2.7 s |
| box，Node 20 | `vitest run` | 失败：`markAsUncloneable` |

同一代码：本地盘 3 s 全绿，iCloud 目录 67–120 s 且随机红；降并发不消除失败——排除“并发竞态/配置”假说。
transform/environment 阶段（纯读文件）占比 63–83%，与 I/O 停顿一致。

## 处置（已入库）
- `vitest.config.ts`：显式 `environment: "node"`（DOM 测试逐文件 `// @vitest-environment jsdom`）、`pool: "forks"`；
  **不设 maxWorkers**（降并发只掩盖、且实测不能修复）。单测守护该约束。
- `package.json`：`engines.node` 与依赖一致；`.nvmrc`；`pretest` 运行 `scripts/testEnvGuard.mjs`：
  - Node 版本不满足 → 拒跑并提示 `nvm use`；
  - macOS 且工程处于 iCloud 管理目录 → 拒跑并给出处置；`SHIYI_ALLOW_ICLOUD_TESTS=1` 可显式放行（结果不作门禁依据）。
- `src/test/testEnvGuard.test.ts`：版本区间、engines/.nvmrc 同源、iCloud 识别、放行开关、无 maxWorkers。

## 需用户操作（仓库内无法替代）
把工程移出 iCloud 同步目录（例如 `~/dev/shiyi-music`，重新 `npm ci`），或关闭“桌面与文稿文件夹”iCloud 同步。
之后验收：`npm test` 连跑 3 次 exit 0。

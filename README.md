# shiyi音乐 — AI 音乐提示词生成器

面向 [Suno](https://suno.com) 的桌面端提示词工作台（Tauri 2 + React 19 + Rust）。输入歌词或灵感，经圆桌流水线产出可直接粘贴的 Style / Exclude / Settings / Lyrics 四栏提示词包。

**开源协议：[MIT](./LICENSE)** · **仓库：** [suce346-cmd/shiyi-music](https://github.com/suce346-cmd/shiyi-music)

---

## 下载安装包

正式安装包在 GitHub Releases（不是仓库文件列表里）：

**最新版 → [Releases · v6.0.0](https://github.com/suce346-cmd/shiyi-music/releases/tag/v6.0.0)**

| 平台 | 文件 |
|---|---|
| macOS Apple Silicon（M 系列） | [shiyi._6.0.0_aarch64.dmg](https://github.com/suce346-cmd/shiyi-music/releases/download/v6.0.0/shiyi._6.0.0_aarch64.dmg) |
| macOS Intel | [shiyi._6.0.0_x64.dmg](https://github.com/suce346-cmd/shiyi-music/releases/download/v6.0.0/shiyi._6.0.0_x64.dmg) |
| Windows x64 | [shiyi._6.0.0_x64-setup.exe](https://github.com/suce346-cmd/shiyi-music/releases/download/v6.0.0/shiyi._6.0.0_x64-setup.exe) |

历史版本见 [全部 Releases](https://github.com/suce346-cmd/shiyi-music/releases)。

> macOS 未做 Apple 公证：首次打开若提示「无法验证开发者」，在「系统设置 → 隐私与安全性」里允许打开即可（个人自用场景）。

---

## 使用步骤（装好之后）

1. **打开 App** → 进入「设置」。
2. **填自己的 API**：Base URL、模型名、API Key（**谁用谁填**，只保存在本机钥匙串 / 系统凭据，不进仓库）。
3. **选语言**：歌词输出中文或英文（Style 字段恒为英文）。
4. **选模式**，按界面提示输入，点生成：
   - **A 歌词模式**：已有歌词 → 完整制作方案  
   - **B 想法模式**：一句话灵感 → 整首歌  
   - **C 改写模式**：原词 + 新主题 → 保留字数硬校验、韵脚提示后重填  
   - **D 抖音模式**：灵感 → 60–90 秒短视频向片段（前奏短、副歌早进、可多场景）
5. **复制结果**：结果区分 Style / Exclude / Settings / Lyrics，可整包或分栏复制到 Suno。

可选：设置里打开「文本评估 Judge」（默认关），终稿后用同一模型打分，不影响出结果。

---

## 它怎么工作（简版）

圆桌流水线：主持人出初稿 → 角色审改 → 校验员把关 → 主持人汇总（最多约 3 轮）→ 校验员定稿为结构化 SongPlan（硬规则校验，最多打回 3 次）→ 渲染成 Suno 提示词包。

规则真源：[docs/BLUEPRINT_v2.md](./docs/BLUEPRINT_v2.md) · 最近全面复查：[docs/full-review-2026-10-20.md](./docs/full-review-2026-10-20.md)

---

## 从源码开发

```bash
# Node 22 / 24 / 26 均可（CI 用 .nvmrc 的 22）；Rust stable
npm ci
npm run tauri dev
```

推送前跑门禁：

```bash
bash scripts/gates.sh
```

> 钥匙串约定：`cargo test` **只读**你的真实 Key（`global` / `role:*`），永不写入或删除；需要写钥匙串的测试只用 `test:shiyi:` 隔离账户（RULE-KEYCHAIN-RO）。

---

## 发版与云端打包

本地不保留安装包。推 `vX.Y.Z` tag 后，GitHub Actions 构建三平台并挂到 Release。步骤见 **[docs/RELEASE.md](./docs/RELEASE.md)**。

---

## 依赖版本（摘要）

| 层 | 版本 |
|---|---|
| Node.js | 本机 22 / 24 / 26（`engines`）；CI / `.nvmrc` → 22.23.x |
| Rust | stable |
| Tauri | 2.x |
| React | 19.x |
| Vite | 7.x |

---

## License

MIT，见 [LICENSE](./LICENSE)。

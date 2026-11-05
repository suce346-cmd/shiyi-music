# shiyi音乐 — AI 音乐提示词生成器

面向 [Suno](https://suno.com) 的桌面端提示词工作台（Tauri 2 + React 19 + Rust）。输入歌词或灵感，经圆桌流水线产出可直接粘贴的 Style / Exclude / Settings / Lyrics 四栏提示词包。

**开源协议：[MIT](./LICENSE)** · **仓库：** [suce346-cmd/shiyi-music](https://github.com/suce346-cmd/shiyi-music)

---

## 下载安装包

正式安装包在 GitHub Releases（不是仓库文件列表里）：

**最新版 → [Releases · v6.2.0](https://github.com/suce346-cmd/shiyi-music/releases/tag/v6.2.0)**

| 平台 | 文件 |
|---|---|
| macOS Apple Silicon（M 系列） | [shiyi._6.2.0_aarch64.dmg](https://github.com/suce346-cmd/shiyi-music/releases/download/v6.2.0/shiyi._6.2.0_aarch64.dmg) |
| macOS Intel | [shiyi._6.2.0_x64.dmg](https://github.com/suce346-cmd/shiyi-music/releases/download/v6.2.0/shiyi._6.2.0_x64.dmg) |
| Windows x64 | [shiyi._6.2.0_x64-setup.exe](https://github.com/suce346-cmd/shiyi-music/releases/download/v6.2.0/shiyi._6.2.0_x64-setup.exe) |

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

v3 确定性词曲声学一体管线：
1. **阶段 0/1（立意五问 + 词曲声学一体创作）**：从五问锚点（`plot_type` / `one_thing` / `to_whom` / `at_what_moment` / `anchor_object`）、刻意不对称设计（`asymmetry`）与三维声场减法编曲出发，一次性构建完整结构化 `SongPlan` IR，并自动拆解各专业视角摘要（当开启「轮间确认门」、中途插话或点击「按反馈快速优化」时，自动路由对应领域专家按 JSON Pointer 靶向精修）。
2. **阶段 2（物理硬门 + 去机器味双层终审）**：执行 `plan::validate` 硬校验（Style ≤1000 字符纯英文、一行一气自然分句、Mode C 逐行等字对齐、Mode D 场景 BPM 与 15s 切片等）與 6 项反 AI 味检测（空洞形容词、主歌前四句悬空抽象词、能量单调无减法、末遍副歌零变化、单韵到底、高音闭口韵憋嗓），如有硬伤由校验员靶向修复（最多 3 次）。
3. **阶段 3（确定性编译）**：由 `render_suno` 按前置高权重顺序渲染出可直接粘贴的 Style / Exclude / Settings / Lyrics 四栏提示词包。

架构与规则契约：[docs/BLUEPRINT_v3_CONTRACT.md](./docs/BLUEPRINT_v3_CONTRACT.md) · 历史蓝图：[docs/BLUEPRINT_v2.md](./docs/BLUEPRINT_v2.md) · 全面复查：[docs/full-review-2026-10-20.md](./docs/full-review-2026-10-20.md)

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

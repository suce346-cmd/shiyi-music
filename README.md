# shiyi音乐 — AI 音乐提示词生成器

基于 Tauri 2 + React 19 + Rust 的桌面应用，为 Suno AI 生成结构化音乐提示词。

采用**圆桌讨论流水线架构（v3）**：主持人全局统领（各模式原版指令）产出初稿 → 动态角色专业审改（先分析再按需查 CSV，输出修订片段）→ 校验员讨论轮审查（质量审查门 + 观点返回主持人）→ 主持人汇总并分发下一轮任务（≤3 轮收敛）→ 校验员结构化定稿 SongPlan（plan::validate 硬校验 ≤3 次打回）→ render_suno 渲染最终提示词包。

## 开发

```bash
npm install
npm run tauri dev
```

> 测试与钥匙串：`cargo test` 只会**读取**你的真实 Key（`global` / `role:*`），永不写入、覆盖或删除；需要写钥匙串的测试只用 `test:shiyi:` 隔离账户并自动清理（RULE-KEYCHAIN-RO）。

## 构建

```bash
npm run tauri build
```

## 功能

- Mode A（歌词模式）：已有歌词 → 完整生产方案（情感 → 制作 → 主持 → 校验）
- Mode B（想法模式）：一个灵感 → 完整歌曲（情感 → 作词 → 制作 → 主持 → 校验）
- Mode C（改写模式）：原歌词 + 新主题 → 保留字数硬校验、韵脚提示（咨询式）重新填词（改词 → 制作 → 主持 → 校验）
- Mode D（抖音模式）：一个灵感 → 60–90 秒抖音爆款片段（情感 → 作词 → 流行风格 → 制作 → 主持 → 校验）

## 角色级 API

设置面板支持为每个角色单独配置 API（模型/密钥/地址），未配置的角色共用全局 API；留空的字段继承全局。歌词语言仅中文/英文两态（Style 恒英文，不再提供「混合」）。

## 依赖版本（摘要）

| 层 | 版本 |
|---|---|
| Node.js | 见 `.nvmrc`（22.23.x） |
| Rust | stable（开发机当前 1.99.x） |
| Tauri | 2.x（`@tauri-apps/api` / CLI / 插件） |
| React | 19.x |
| Vite | 7.x |

发版与 CI 打包步骤见 [docs/RELEASE.md](./docs/RELEASE.md)。规则真源见 [docs/BLUEPRINT_v2.md](./docs/BLUEPRINT_v2.md)。

## License

MIT，见 [LICENSE](./LICENSE) 文件。

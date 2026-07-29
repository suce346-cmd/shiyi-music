# shiyi音乐 — AI 音乐提示词生成器

基于 Tauri 2 + React 19 的桌面应用，为 Suno AI 生成结构化音乐提示词。

## 开发

```bash
npm install
npm run tauri dev
```

## 构建

```bash
npm run tauri build
```

## 功能

- Mode A：已有歌词 → 完整生产方案（Style Prompt + 格式化歌词 + 参数）
- Mode C：原歌词 + 新主题 → 保留字数韵脚重新填词
- Mode D：一个灵感 → 60 秒内抖音爆款片段全流程

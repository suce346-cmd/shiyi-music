# Suno Prompt 生成器 — 9 项审计修复方案

## 状态总览

| # | 类型 | 问题 | 文件 | 状态 |
|---|------|------|------|------|
| 1 | Bug 高 | `system_prompt` 冗余字段跨序列化 | `types/index.ts` `models/mod.rs` `useLLM.ts` | ✅ 已修复 |
| 2 | Bug 中 | 错误提示 `alert(e)` 显示 `[object Object]` | `InputPanel.tsx:57` | ✅ 已修复 |
| 3 | Bug 低 | Mode A 无歌词长度校验 | `InputPanel.tsx:34-37` | ❌ 待修复 |
| 4 | 冗余 中 | `ModeAInput`/`ModeDInput` 零引用 | `types/index.ts` | ✅ 已修复 |
| 5 | 冗余 低 | `sections` 字段始终空 HashMap | `types/index.ts` `models/mod.rs` `llm.rs` | ❌ 待修复 |
| 6 | 冗余 低 | `[DONE]` 发射前端不做处理 | `llm.rs:62-69` | ❌ 待修复 |
| 7 | 规范 低 | Cargo.toml 占位符 | `Cargo.toml:4-5` | ✅ 已修复 |
| 8 | 规范 低 | index.html 标题 | `index.html:7` | ✅ 已修复 |
| 9 | 规范 低 | `tokio features = ["full"]` | `Cargo.toml:26` | ❌ 待修复 |

## 剩余 4 项修复方案

### 修复 #3：Mode A 歌词长度校验

**根因:** `InputPanel.tsx` 中 Mode A 的 `lyrics` 无最小长度检查，用户可提交极短内容

**方案:** 在 `handleSubmit` 中为 Mode A 添加 `lyrics.trim().length >= 10` 校验

**涉及文件:** `src/components/InputPanel.tsx`

**验证:** 前端构建通过 (`npm run build`)

**边界:** 空歌词、1 字符、10 字符边界、含空格的歌词

### 修复 #5：删除 sections 字段

**根因:** `LLMResponse.sections` 在前端从未使用（`ResultPanel` 只读 `raw`），Rust 端始终 `HashMap::new()`

**方案:** 从 TS `LLMResponse`、Rust `LLMResponse` 中删除 `sections` 字段，同时删除 `use std::collections::HashMap` 导入（如无其他使用）

**涉及文件:** `src/types/index.ts` `src-tauri/src/models/mod.rs` `src-tauri/src/commands/llm.rs`

**验证:** 前端 + Rust 双构建通过

**边界:** 无功能影响，仅删死数据

### 修复 #6：删除 [DONE] 冗余发射

**根因:** Rust 端发射 `finished: true` 事件表示流结束，但前端 listener 收到后仅 `return` 不做处理，实际由 `unlisten()` 在 `finally` 中自动清理

**方案:** 删除 Rust `llm.rs` 中 `[DONE]` 检测分支（`:61-69`），依赖 `invoke` 返回后 listener 自动解绑

**涉及文件:** `src-tauri/src/commands/llm.rs`

**验证:** Rust 构建通过

**边界:** 前端 `listen` 在 `invoke` 返回后 `finally` 中 `unlisten()` — 流结束后 invoke 自然返回，listener 被清理，无需额外信号

### 修复 #9：精简 tokio features

**根因:** `tokio features = ["full"]` 拉入全部 tokio 功能（net、signal、io-util、process 等），reqwest 只需要基础 async runtime

**方案:** 改为 `features = ["rt", "macros"]` — 这是 reqwest 所需的最小集

**涉及文件:** `src-tauri/Cargo.toml`

**验证:** Rust 构建通过 (`cargo check`)

**风险评估:** 如果 reqwest 或 tauri 依赖 tokio 其他功能，编译会报错，逐步补齐即可。`["rt", "macros"]` 是安全起点。

## 执行顺序

```
#3（前端校验） → #5（跨前后端类型） → #6（纯 Rust） → #9（纯 Rust）
```

每改一个跑一次双构建验证。
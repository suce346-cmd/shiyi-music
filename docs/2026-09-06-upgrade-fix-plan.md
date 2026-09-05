# shiyi音乐 · 全量问题修复方案（v1）

> 日期：2026-09-06
> 范围：UI + 后端全量问题清单（34 项），每项按九要素展开：
> 问题原因 / 影响 / 修复方案 / 旧规则处理 / 新规则握手落地 / 修复后预期 / 验证方式 / 关联链路（+ 代码定位）
> 行号基于 2026-09-06 的 main 分支（HEAD = 2bd440b 之后的工作区状态）。

---

## 总索引

| 分级 | 编号 | 问题 | 归属 |
|------|------|------|------|
| Bug | B1 | 超时假中止（任务未真正停止） | 后端 |
| Bug | B2 | release `panic=abort` 使 spawn 隔离层失效 | 后端 |
| Bug | B3 | 生成无法取消 | 全链路 |
| Bug | B4 | 静默截断→假收敛链条 | 后端 |
| Bug | B5 | CI 发包未跑测试 | 工程 |
| Bug | B6 | BPM 提取取错数字 | 后端 |
| Bug | B7 | Style Prompt 过短校验被前缀放水 | 后端 |
| Bug | B8 | 抖音 Verse 行数只查最后一段 | 后端 |
| Bug | B9 | 能量解析三份拷贝 | 后端 |
| Bug | B10 | 审改降级静默（agree 默认 true / 异议丢弃） | 后端 |
| 功能 | F1 | "优化"= 全量重跑，无增量修订 | 全链路 |
| 功能 | F2 | 讨论过程黑盒（非流式 + 摘要截断 40 字） | 全链路 |
| 功能 | F3 | 历史记录存 localStorage，无导出 | UI |
| 功能 | F4 | 无 token 用量/费用统计 | 全链路 |
| 功能 | F5 | Windows 打包产不出安装包 | 工程 |
| 功能 | F6 | 无自动更新与代码签名 | 工程 |
| 功能 | F7 | 无深浅色主题切换 | UI |
| 功能 | F8 | 无 i18n | UI |
| 功能 | F9 | 单任务、无生成队列 | 全链路 |
| 功能 | F10 | 讨论中无人工干预点 | 全链路 |
| 功能 | F11 | test_api 用 max_tokens=1 误报 | 后端 |
| 功能 | F12 | Mode C 拼字符串再正则拆（脆弱协议） | 全链路 |
| 功能 | F13 | 后端对输入零校验 | 后端 |
| 功能 | F14 | 无快捷键 / 拖拽导入 / 设置导入导出 | UI |
| 架构 | A1 | 重试预算与全局超时无协调 | 后端 |
| 架构 | A2 | 角色串行执行（可并发 ×4） | 后端 |
| 架构 | A3 | plan/revisions_log 注入无预算 | 后端 |
| 架构 | A4 | 知识库每步重新解析（无缓存） | 后端 |
| 架构 | A5 | 错误处理全链 `Result<_, String>` | 后端 |
| 架构 | A6 | 无日志落盘 | 后端 |
| 架构 | A7 | CSV 一行错整表跳过 + 无 BOM 处理 | 后端 |
| 架构 | A8 | 角色阵容三处真源 + 前后端类型手抄 | 全链路 |
| 架构 | A9 | 事件通道无 run-id | 后端 |
| 架构 | A10 | prompt 编译进二进制，无远程配置 | 后端 |
| 架构 | A11 | temperature / max_tokens 硬编码 | 后端 |
| 架构 | A12 | API Key 明文存 localStorage | 全链路 |
| 架构 | A13 | 前端 0 测试 | 工程 |
| 架构 | A14 | 无 LICENSE | 工程 |

---

# 第一部分 · Bug 级（10 项）

## B1 超时假中止：任务没有被真正停掉

**定位**：`src-tauri/src/commands/orchestrator.rs:614-639`（`run_pipeline`）

**问题原因**：`tokio::time::timeout(PIPELINE_TIMEOUT, handle).await` 超时进入 `Err(_elapsed)` 分支（:629）后直接 `return Err(msg)`。此时 `JoinHandle` 已随 timeout future 被 drop——tokio 语义为 drop 即 detach，内层任务继续运行。

**影响**：超时后流水线继续发起 LLM 调用（继续花钱）、继续 emit 事件；前端同一 run 的 token 未变，会在"超时已中止"的失败态之后继续收到 step 事件，UI 诈尸。

**修复方案**：
```rust
Err(_elapsed) => {
    handle.abort();          // 新增：真正取消在途任务
    let _ = handle.await;    // 新增：回收任务句柄
    let msg = format!("流水线超时（{} 分钟）未完成，已中止", 15);
    ...
}
```
为可测试性，把超时时长抽为参数：新增 `run_pipeline_with_timeout(app, request, timeout)`，生产入口 `run_pipeline` 传 15 分钟常量，测试注入极小超时。abort 在下一个 await 点（全部 LLM 调用均为 await）取消 future，reqwest 流被 drop，TCP 连接关闭。

**旧规则处理**：`orchestrator.rs:612` 注释补充"超时 = abort 强制取消在途请求"；`PIPELINE_TIMEOUT` 语义从"放弃等待"升级为"强制中止"。

**新规则握手落地**：无前后端契约变化——`Failed` 事件与 `Err(String)` 返回路径不变，前端 `usePipeline` 的 `failed` 分支（usePipeline.ts:263-266）原样生效。

**修复后预期**：超时点后 1 秒内：在途 HTTP 连接关闭、无新调用、无新事件、UI 停在明确失败态。

**验证方式**：新增 `#[tokio::test]`：注入假 inner future（`sleep(1h)`）+ 50ms 超时，断言返回超时错误且任务被 abort；手动冒烟：dev 模式用挂起的 base_url 触发超时路径，观察终端无新 `[llm]` 日志。

**关联链路**：`run_pipeline_refine`（:736）走同一函数自动继承；与 B3 取消机制并存（用户取消=优雅返回，超时=强制 abort）。

---

## B2 release `panic=abort` 使 spawn 隔离层失效

**定位**：`src-tauri/Cargo.toml` `[profile.release]` 段 `panic = "abort"`（约 :37）vs `orchestrator.rs:612-613` 隔离层注释

**问题原因**：隔离层依赖 unwind——spawn 的任务 panic 被转为 `JoinError`（:622-628 的 `Ok(Err(join_err))` 分支正是为此而写）。`panic = "abort"` 使 release 下 panic 直接 abort 进程，该分支永远走不到。

**影响**：开发（unwind）与发布（abort）行为不一致：132 个测试全绿，但发布包内任何一处未来引入的 panic（unwrap/索引越界）= 应用闪退、无提示、丢全部状态。

**修复方案**：删除 `panic = "abort"` 一行，恢复默认 unwind。`strip`/`lto`/`codegen-units` 保留。

**旧规则处理**：`Cargo.toml` 处加注释"必须保持 unwind：orchestrator 的 spawn 隔离层依赖 JoinError 捕获 panic"；`orchestrator.rs:612` 注释反向补充"依赖 release 不设 panic=abort"。

**新规则握手落地**：无契约变化。

**修复后预期**：release 包中 panic → `JoinError` → `Failed` 事件 + 明确错误文本，不再闪退。

**验证方式**：`cargo test --release` 全量跑一遍确认行为一致；`cargo build --release` 产物冒烟启动；记录体积增量（预计 +1MB 级）到提交说明。

**关联链路**：spawn 捕获逻辑重新生效；与 B1 的 abort 机制互不干扰（abort 不依赖 panic 机制）。

---

## B3 生成无法取消（全链路新增取消通道）

**定位**：前端 `src/App.tsx:202-259`（无停止入口）、`InputPanel.tsx:86`（仅 disabled）；后端 `orchestrator.rs` 全文无检查点、`llm.rs:32-62`（重试循环无检查点）、`llm.rs:266`（流式循环无检查点）

**问题原因**：前后端都没有取消概念——invoke 发出后 promise 只能等到底。

**影响**：一次 Mode D 生成 8~16 次 LLM 调用、最长 15 分钟不可中断；填错输入只能烧完。

**修复方案**：
- 后端：新建 `commands/cancel.rs`，`static CANCELLED: AtomicBool`（现状单任务设计，单标志够用）；`llm.rs` 加两个检查点（`send_with_retry` 每次尝试前、`stream_response` 每个 chunk 循环）；`run_pipeline_inner` 在每个阶段入口检查（:647、:659 循环头、:669、:685、:700）；新增 `#[tauri::command] cancel_pipeline()`；取消路径 inner 返回哨兵错误，`run_pipeline` 识别后 emit 新事件 `PipelineEvent::Cancelled`。
- 前端：`models/mod.rs` PipelineEvent 加 `Cancelled` variant（serde 自动 `type="cancelled"`）；`src/types/index.ts:81-90` 联合类型同步加分支（**唯一契约变更点，两端同一提交**）；`usePipeline` 加 `cancel()` + `cancelled` 事件分支；App 在 loading/streaming 态的 StatusIndicator 区加"停止"按钮。

**旧规则处理**：`pipeline.reset()`（前端切模式，App.tsx:129-133）必须同时调 `cancel_pipeline()`——补上现有遗漏（现状切模式只作废事件，后台任务照跑）。旧 `Failed` 路径保留给真实错误。

**新规则握手落地**：新增 command + 新增事件 variant，均为附加式变更；旧前端遇未知事件走 default 分支不崩溃；前后端同 commit 发布。

**修复后预期**：点"停止"后 ≤1 个网络往返内：在途请求被 drop、UI 回 idle、费用停止；取消能穿透 429 退避等待。

**验证方式**：单测——置位标志后 `send_with_retry` 立即 Err；`stream_response` 用挂起流验证检查点；手动——生成中点停止，1 秒内 UI 复位、终端无新 `[llm]` 日志；切模式后确认后台无残留。

**关联链路**：`pipeline_refine` 共用检查点；H4 run-token 机制保留（取消后仍 token++ 双保险）；B1 的 abort 与取消互斥使用。

---

## B4 静默截断 → 假收敛链条

**定位**：`llm.rs:107-112`（`content_from_json` 丢弃 finish_reason）、`llm.rs:305-308` + `orchestrator.rs:439`（流式 finish_reason 返回了但只用 raw）、`orchestrator.rs:331` 等 4 处（max_tokens=3000）、`orchestrator.rs:100`（agree 缺省 true）、`orchestrator.rs:127-128`（解析失败默认 agree）、`orchestrator.rs:662`（无 changes 的异议被丢弃）

**问题原因**：三个环节叠加：① finish_reason 全程无人消费，截断不可见；② 审改 JSON 被 3000 max_tokens 截断 → 解析失败 → 默认 agree=true；③ `agree=false` 但未填 changes 的异议被静默丢弃，不进 revisions_log。

**影响**：长方案被 8192 截断后照常进入讨论轮；审改输出被截断的角色假同意；用户拿到未经审查的方案，全程零警告。

**修复方案**（四步）：
1. **打通 finish_reason**：`content_from_json` 同时提取 `choices[0].finish_reason`，`send_and_extract`/`call_llm_silent` 返回 `LLMResponse`（复用 models/mod.rs:25）；4 个调用点（:328/:403/:462/:520）适配。
2. **截断分级处置**：审改类截断 → max_tokens 提至 6000 重试一次，仍截断 → `ReviewResult` 加 `degraded: bool`，记入 revisions_log"⚠️ 输出被截断，本轮意见跳过"（不算 agree 也不算异议）；格式输出截断 → `collect_hard_issues` 附加 issue"输出被截断（finish_reason=length）"，走现有打回循环（:702-712 零改动）；主持人汇总截断 → 重试一次，仍失败沿用并 eprintln；阶段 0 流式截断 → 返回明确 `Err("方案初稿输出被截断，请简化输入后重试")`。
3. **解析失败不再假同意**：`parse_review` 返回 `Result`；失败重试一次，仍失败 → `degraded` 结果 + humanize 显示"⚠️ 输出无法解析，本轮意见未采纳"。
4. **异议不再丢失**：`round_changes` 类型改为 `Vec<(PipelineRole, Vec<ReviewChange>, String)>`（附角色总体意见）；`run_host_summarize`（:451-457）渲染"总体意见：{reason}"；`agree=false && changes.is_empty()` 进入该通道。

**旧规则处理**："垃圾=agree"旧行为废止；单测 `parse_review_garbage_is_agree`（:826-830）改写为新语义；"永远有产出"原则不变——降级方案仍返回，但降级事实从 stderr 升级为用户可见。

**新规则握手落地**：全部 Rust 内部类型变化；用户可见信息通过现有 `StepDone.summary` 与 `AuditResult.findings` 文本承载，前端零改动。

**修复后预期**：任何截断/解析失败都以"⚠️"文案出现在对话流；格式截断自动进打回；假同意通道关闭。

**验证方式**：单测——finish_reason 三分支提取；length 时审改重试与 degraded 断言；带 reason 空 changes 的渲染断言；`collect_hard_issues` 附加截断 issue 断言；手动——临时 max_tokens 降到 200 复现截断观察 ⚠️ 与打回，验证后恢复。

**关联链路**：打回循环末次重校验（:715-717）自动覆盖截断 issue；revisions_log 的 ⚠️ 条目进入后续轮注入（:311-317），文案短、预算可控；前端 step_done summary 渲染（usePipeline.ts:181）原样展示。

---

## B5 CI 发的包没跑过测试

**定位**：`.github/workflows/build.yml` 全文（:26-58，只有 build+upload）

**问题原因**：workflow 从创建起只有构建 job，无 test job。

**影响**：132 个 Rust 测试与 tsc 类型检查从未在 CI 执行，任何红灯代码都能直接产出 release 工件。

**修复方案**：
```yaml
jobs:
  test:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v5
      - uses: actions/setup-node@v5 (node 22, cache npm)
      - run: npm ci
      - run: npx tsc --noEmit
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test --manifest-path src-tauri/Cargo.toml
  build:
    needs: test      # 测试不过，构建不启动
    ...
```

**旧规则处理**：无（纯增量）。

**新规则握手落地**：`needs: test` 为 GitHub Actions 原生机制。

**修复后预期**：测试/类型检查失败 → 红灯、不产出任何工件。

**验证方式**：推含故意失败断言的分支确认红灯阻断，回滚确认绿灯；记录耗时增量（约 +3~5 分钟）。

**关联链路**：test job 复用已有 npm/rust 缓存 key；与 B2 的本地 `cargo test --release` 无冲突。

---

## B6 BPM 提取取错数字

**定位**：`orchestrator.rs:571-581`（`extract_bpm`，缺陷实现）+ `knowledge.rs:152-184`（`plan_bpm_value`，正确实现未被复用）；消费点 `orchestrator.rs:595-599`

**问题原因**：`extract_bpm` 取 Style Prompt 行第一个 40-220 的数字——"80年代复古Disco, 125BPM"取到 80。正确的"优先取 BPM 邻近数字"逻辑在 knowledge.rs 已存在却未复用。

**影响**：mode_d 硬校验误报"BPM 80 低于要求 90"，触发无意义打回；校验员若顺从误报改掉真实 BPM，产物被破坏。

**修复方案**：① 删除 `extract_bpm`，`collect_hard_issues` 改调 `knowledge::plan_bpm_value`（提为 `pub(crate)`）；② 收紧 `plan_bpm_value` fallback：删掉 `re_digits.first()`（:183）兜底，文本无 "BPM" 字样时返回 `None`（对齐 validator.rs:376-379 注释确立的哲学——代码只对明确标注的 BPM 报错，合理性归制作人审查）。

**旧规则处理**：`extract_bpm` 及调用点删除；knowledge 版单测 `plan_bpm_value_char_boundary_safe`（:989-997）3 个断言与新语义核对，天然兼容。

**新规则握手落地**：无前端契约变化；校验变化为"误杀消失"（放宽方向）。

**修复后预期**：含明确 BPM 的方案取值正确；无 BPM 字样的方案不被 BPM 检查触碰。

**验证方式**：新增 3 单测——"80年代复古Disco, 125BPM"→Some(125)；"80年代Disco"→None；"深夜民谣 68 BPM"→Some(68)。流派 BPM 加分路径（knowledge.rs:363-369）补回归测试。

**关联链路**：`render_filtered_any` 流派排序在无 BPM 时本就 `if let` 跳过，无回归风险；`check_bpm_range` 签名不变。

---

## B7 Style Prompt 过短校验被前缀放水

**定位**：`orchestrator.rs:560-568`（`extract_style_prompt_line` 返回整行）→ :592-593 调用 → `validator.rs:391-398`（对整行计数）

**问题原因**：返回值含 `Style Prompt**: ` 约 16 字符前缀，"≥30 字符"实际只要求内容 ≥14 字符。

**影响**：信息块缺失的极简 Style Prompt 畅通过检，兜底失效。

**修复方案**：validator 的 `extract_style_prompt`（:42-54，正确返回冒号后正文）提为 `pub(crate)`；`collect_hard_issues` 改用它取正文，`check_style_prompt_blocks` 与 BPM 检查（协同 B6）均消费正文。`extract_style_prompt_line` 及其 2 个单测（:946-954）删除。

**旧规则处理**：旧行为是放水，直接废止；阈值 30 本身不动。

**新规则握手落地**：无契约变化；校验收紧，历史方案不受影响（校验只对新生成执行）。

**修复后预期**：`Style Prompt**: 民谣` 报过短；正常 8 信息块方案无感。

**验证方式**：单测——正文 2 字报过短、正文 35 字通过、`**`/全角冒号变体正确剥离；确认 BPM 输入变正文后无行为差异（前缀无数字）。

**关联链路**：`extract_style_prompt` 已被 `validate_production`（:147-155）使用且正确——本次是复用统一，顺带消灭一处潜在第二实现。

---

## B8 抖音 Verse 行数只查最后一段

**定位**：`validator.rs:299-322`（`validate_douyin` 第 2 项）

**问题原因**：循环遇新 `[Verse]` 标签时 `verse_line_count = 0` 重置，循环结束只报一次——多段 Verse 只检最后一段。

**影响**：第一段 6 行、第二段 3 行的方案漏检超行。

**修复方案**：逐段结算——`in_verse` 状态下遇到下一个 `[标签]` 行时先 push `(verse_label, verse_line_count)` 再重置；文本结束再 push 一次；循环后逐段断言 `>4`，issue 带段名（"Verse 1 行数 6 超限（要求 ≤4）"）。

**旧规则处理**：存量测试核对——`douyin_valid_passes`（:479-483）Verse 1 行不报错，兼容；`douyin_many_overlong_condensed`（:703-711）单 Verse 不受影响。

**新规则握手落地**：无契约变化；`findings` 多出带段名条目，前端 `slice(0, 3)`（usePipeline.ts:242）自动兼容。

**修复后预期**：每个 Verse 独立受检，漏检归零、误报为零（说明行排除逻辑 :312-318 原样保留）。

**验证方式**：新单测——两段 Verse（6+2 行）必报且含 "Verse 1"；两段各 4 行通过；说明行不计行的既有规则回归。

**关联链路**：仅 `validate_douyin` 内部重构，`validate_for_mode` 分发与打回循环无感。

---

## B9 能量解析三份拷贝

**定位**：`validator.rs:83-122`（`extract_energy_values`）、`knowledge.rs:104-150`（`plan_energy_range_str` 内联同款）、前端 `ResultPanel.tsx:25`（ENERGY_RE，展示用途）

**问题原因**：后端两份逐字符扫描逻辑几乎逐行相同各自维护；knowledge.rs:103 注释声称"单一实现防漂移"，但 validator 那份就在旁边。

**影响**：未来调整能量格式只改一处 → 注入与校验判定漂移（乐器错区间/硬校验误判）。

**修复方案**：新建 `src-tauri/src/energy.rs`：`pub(crate) fn extract_energy_values(text) -> Vec<u32>` 唯一实现（以 knowledge 版为准——parse 失败跳过而非 unwrap_or(0)）；`plan_energy_range_str` 变为其 min/max 包装；validator 改调用；`lib.rs` 注册模块。前端 ENERGY_RE 为展示语义，保留不动，注释标注"与后端 energy.rs 格式族对齐，改动需双侧同步"。

**旧规则处理**：两处旧实现删除；knowledge.rs:103 与 validator.rs:6-8 注释统一指向新模块。

**新规则握手落地**：无契约变化——**等价重构**。

**修复后预期**：后端仅一份能量解析实现，漂移可能归零。

**验证方式**：等价性由存量测试证明——validator 4 个能量测试（:577-606）+ knowledge 3 个（:979-1010）一行不改全绿；另加跨场景测试（四种标注格式 × 段号跳过 × energy 词尾）锁死实现。

**关联链路**：instruments 能量注入（orchestrator.rs:243-251）、`validate_production` 能量差（validator.rs:164-176）、`plan_energy_range` 委托（orchestrator.rs:184-186）全部走新实现，行为逐位一致。

---

## B10 审改输出不可信时的静默降级

**定位**：`orchestrator.rs:100`（`as_bool().unwrap_or(true)`）、:127-128（解析失败分支）、:660-666（`!agree && changes.is_empty()` 直接跳过）

**问题原因**：三个"宽容默认值"同向——把不可信输出当"没有意见"。

**影响**：模型不守格式、字段缺失、故意简答，全部表现为"无异议 ✅"，讨论轮形同虚设且无任何可见痕迹。

**修复方案**（可独立于 B4 落地）：
1. `agree` 字段缺失视为解析失败（走 ⚠️ 降级记录路径）；
2. `agree=false && changes.is_empty()`：不丢弃——进入携带 reason 的 round_changes（B4 第 4 步的类型扩展），`all_agree=false`，主持人呈现"提出总体异议（无具体修订）：{reason}"；
3. `humanize_review`：agree=true 但 checked 为空 → 显示"无异议 ⚠️（未附核查清单）"——把"空手 agree"从隐形变可见（护栏测试 :267-284 只约束了 prompt 文本，代码层从未验证行为）。

**旧规则处理**：`parse_review_garbage_is_agree`（:826-830）、`parse_review_agree`（:810-814）改写为新语义。

**新规则握手落地**：`ReviewResult` 加 `degraded` 字段（内部结构），可见性走现有 summary 文本，无前后端契约影响。

**修复后预期**："谁没认真审、谁的输出坏了、谁提了笼统异议"全部在对话流可见；主持人收到全部异议。

**验证方式**：单测——缺 agree 字段→降级；agree=false 无 changes→进入 round_changes 且 host 渲染含 reason；agree=true 无 checked→humanize 含 ⚠️；手动——某角色配返回非 JSON 的 mock base_url，观察 ⚠️ 且流程不中断。

**关联链路**：`execute_audit_review` 复用同一 `parse_review` 自动继承；revisions_log 的 ⚠️ 条目短，无注入预算压力。

---

# 第二部分 · 功能缺失级（14 项）

## F1 "优化" = 全量重跑，无增量修订

**定位**：`orchestrator.rs:736-740`（`run_pipeline_refine` 把反馈拼进输入后走完整三阶段）

**问题原因**：refine 复用 `run_pipeline`，未设计局部重跑路径。

**影响**：改一句"副歌唢呐不够炸"，全部角色重新讨论一遍——5~10 分钟 + 16 次调用成本。

**修复方案**：新增增量模式：`PipelineRequest` 加 `refine_targets: Option<Vec<PipelineRole>>`；后端根据反馈涉及的 target（style_prompt→Producer/StyleAnalyst、lyrics→Lyricist/Reviser、params→Producer/Emotion）筛选讨论轮角色，未涉及角色注入"上一版已认可，无需重复审改"的免审提示；阶段 0 跳过（以上一版方案为 `current_plan` 起步）。UI 上让用户可选"快速优化（增量）/ 深度重做（全量）"。

**旧规则处理**：全量路径保留为默认选项；`pipeline_refine` 命令签名加可选参数（serde default 兼容旧前端）。

**新规则握手落地**：`PipelineRequest` 新增可选字段 + 前端 `PipelineRequest`（types/index.ts:93-104）同步 + ResultPanel 优化面板加模式开关（两端同 commit）。

**修复后预期**：单点反馈的优化从 16 次调用降到 4~6 次，耗时降 60% 以上。

**验证方式**：单测——target 筛选逻辑；手动——带"只改歌词"反馈观察仅 Lyricist/Reviser/Producer 出场。

**关联链路**：硬校验兜底（阶段 2）不变；H4 token 机制不变。

## F2 讨论过程黑盒（非流式 + 摘要截断 40 字）

**定位**：`orchestrator.rs:143-147`（humanize 截断 40 字）、:328-334 等（审改全走 `call_llm_silent`）

**问题原因**：只有阶段 0 用流式；审改结果只有截断摘要进前端，完整 JSON 修订内容丢弃。

**影响**：用户盯着"思考中…"几分钟无反馈，角色真实审改意见不可见，产品核心卖点（圆桌讨论）不可感知。

**修复方案**：分两步。① 审改完整意见可见：`StepDone.summary` 改为完整意见（humanize 取消 40 字截断，长修订完整输出），前端对话流气泡已有 `whitespace-pre-wrap` 天然支持长文本；② 审改流式：`call_llm_silent` 增加流式变体，审改过程中 emit 新事件 `PipelineEvent::StepStream { role, delta }`，前端 RoundtablePanel 头顶气泡实时滚动显示（该角色 working 时）。

**旧规则处理**：`StepDone` 契约保留（完整文本替换摘要）；`StepStream` 为新增事件，旧前端忽略不崩溃。

**新规则握手落地**：`StepStream` variant 两端同 commit；前端 usePipeline 增分支（working 角色气泡 append delta，step_done 时清空）。

**修复后预期**：每个角色思考过程实时可见，圆桌"讨论感"成立。

**验证方式**：单测——humanize 全文输出断言；手动——生成中观察气泡逐字出现。

**关联链路**：与 B4 的 finish_reason 打通协同（流式天然规避截断检测盲区）；事件频率控制（按 chunk emit，SSE 已是增量）。

## F3 历史记录存 localStorage，无导出

**定位**：`App.tsx:27-48`（HISTORY_KEY localStorage）、`ResultPanel.tsx:169-174`（唯一导出=复制按钮）

**问题原因**：用了 WebView 最弱的持久层；Tauri 有文件系统/SQLite 未用。

**影响**：约 5MB 上限、超限丢对话流（M9 降级）；换机/清缓存全丢；无法导出分享。

**修复方案**：引入 `tauri-plugin-store`（或 JSON 文件落 app data 目录）：新增 Rust 命令 `history_load/history_save`（读写 `{app_data}/history.json`）；前端 loadHistory/saveHistory 改走 invoke，localStorage 保留为**一次性迁移源**（首次启动读到非空 localStorage → 写入文件 → 清除 localStorage）。导出：新增 `history_export(id)` 命令，Tauri dialog 插件选路径，写 txt/md。

**旧规则处理**：localStorage 数据一次性迁移后清除；旧 HistoryEntry 结构（含 conversation 可选字段）原样序列化，无 schema 变更。

**新规则握手落地**：新增 3 个 command + capabilities/default.json 增 `store:default`/`dialog:default`/`fs:default` 权限；前端 hook 内部替换，App.tsx 调用点不变。

**修复后预期**：历史不再有 5MB 限制、卸载重装/换机可迁移文件；每条记录可导出。

**验证方式**：单测——Rust 端读写 roundtrip + 迁移逻辑；手动——写入历史→重启→在；导出文件内容与 UI 一致。

**关联链路**：`sanitizeStored`（useSettings.ts:20-44）迁移逻辑同套路复用到设置存储（联动 A12）。

## F4 无 token 用量/费用统计

**定位**：`llm.rs` 全文（OpenAI 响应 `usage` 字段被丢弃）、`models/mod.rs:28`（`finish_reason` 收集了无人消费）

**问题原因**：解析层只抽 content，usage/finish_reason 直接丢。

**影响**：一次生成 8~16 次调用的成本完全黑盒，用户无法决策是否开思考模式/减少轮次。

**修复方案**：`content_from_json` 同时提取 `usage.prompt_tokens/completion_tokens`（B4 已打通 finish_reason）；每次调用结束 emit `PipelineEvent::StepUsage { role, prompt_tokens, completion_tokens }`；前端累计展示在圆桌面板底部（"本次已用 X tokens"）+ 历史记录 entry 加 usage 字段。

**旧规则处理**：`LLMResponse` 扩展字段（Rust 内部）；旧事件消费者不受影响（新增 variant）。

**新规则握手落地**：`StepUsage` variant 两端同 commit；HistoryEntry 可选字段向后兼容旧记录（serde Option）。

**修复后预期**：每次生成实时显示 token 消耗；历史可对比不同模式成本。

**验证方式**：单测——usage 提取（含字段缺失容错）；手动——对比设置面板显示与 API 后台账单量级一致。

**关联链路**：与 B4 共用响应解析改造（一次改完）；与 F1 增量优化的成本对比展示联动。

## F5 Windows 打包产不出安装包

**定位**：`src-tauri/tauri.conf.json:28-30`（targets 只有 `"dmg"`）+ `.github/workflows/build.yml:19-21`（矩阵含 windows-latest）

**问题原因**：bundle targets 全局写死 dmg；Windows 无法产出 dmg，job 跑完 bundle 阶段无产物。

**影响**：Windows 用户从来没有过安装包；CI 白跑 ~20 分钟。

**修复方案**：tauri.conf.json `targets` 改 `["dmg", "nsis"]`（或 `all`）；Windows job 上传路径增加 nsis 产物目录（`bundle/nsis/`）；CI artifacts 命名不变。

**旧规则处理**：无；macOS 产物不受影响。

**新规则握手落地**：无前端契约；CI 产物新增 windows 条目。

**修复后预期**：push 后 artifacts 出现 shiyi-music-windows-x64（含 .exe 安装器）。

**验证方式**：推一次观察 CI 产物列表；本地如无 Windows 环境则完全依赖 CI 验证。

**关联链路**：NSIS 需要图标 ico（icons/ 已存在 :36）——无阻塞；后续 F6 签名时 Windows 侧用同样的 bundle 配置。

## F6 无自动更新与代码签名

**定位**：`tauri.conf.json`（无 updater 配置）、无签名配置

**问题原因**：未配置 tauri updater 插件与签名。

**影响**：发版后用户手动下载覆盖安装；macOS Gatekeeper 拦截未签名包（"无法打开，因为无法验证开发者"），实际分发体验差。

**修复方案**：分两阶段。阶段一（签名）：接入 Apple Developer ID 签名 + notarization（CI secrets: APPLE_CERTIFICATE 等，tauri 官方 action）；Windows 侧可选 EV 证书（成本高可后置）。阶段二（更新）：集成 `tauri-plugin-updater`，endpoint 指向 GitHub Releases latest.json，CI 加 `tauri-action` 生成更新清单。

**旧规则处理**：无签名状态下已分发过的包不受影响。

**新规则握手落地**：updater 需要公钥/私钥对（`tauri signer generate`），私钥入 CI secret；前端无需改动（插件内建 UI 或自定义检查按钮）。

**修复后预期**：macOS 包双击可开（不再右键绕 Gatekeeper）；应用内可检查并一键更新。

**验证方式**：签名验证 `codesign -vv` + `spctl -a`；更新流程用两个版本号实测。

**关联链路**：依赖 F5 的 bundle targets 修正；需要 Apple 开发者账号（$99/年）——**用户决策项**。

## F7 无深浅色主题切换

**定位**：`src/index.css:11-31`（@theme 写死暖色亮色系）

**问题原因**：色彩系统为单主题设计，无变量分组。

**影响**：暗光环境刺眼；无法个性化。

**修复方案**：`@theme` 拆两层——`:root`（亮色）与 `.dark`/`prefers-color-scheme`（暗色，surface 系转深、brand 微调）；Tauri 侧读系统主题（`window.theme`）+ 设置面板加三态开关（跟随系统/亮/暗），持久化进 settings（联动 F3 的新存储）。

**旧规则处理**：所有组件用语义 token（surface-*/text-*/brand-*），无硬编码色值——已核实组件层全部走 token，无需逐组件改。

**新规则握手落地**：settings 加 `theme?: "system"|"light"|"dark"`（serde default system，旧数据兼容）。

**修复后预期**：一键切换即时生效（html class 切换），跟随系统主题自动响应。

**验证方式**：手动三态切换全页面走查（重点 glass-panel、能量条、状态色）；单测——sanitizeStored 兼容旧 settings 无 theme 字段。

**关联链路**：settings 存储迁移（F3）；oklch 色值需在暗色下重新调 6 个 surface 层级。

## F8 无 i18n

**定位**：全部组件中文硬编码（如 App.tsx:349 "shiyi音乐"、567-569 流程文案）+ 后端 prompt 全中文（设计如此，不翻译）

**问题原因**：无文案层。

**影响**：无法触达非中文用户；对开源项目是明显的受众天花板。

**修复方案**：引入 `react-i18next`，提取 UI 文案到 `locales/zh.json` + `en.json`（约 60 条）；设置面板加语言选择；**后端 prompt 不翻译**（方法论为中文调优资产，翻译会破坏效果）——Style Prompt 输出本身中英混合，国际用户可接受。

**旧规则处理**：无；默认语言 zh。

**新规则握手落地**：settings 加 `language?: string`。

**修复后预期**：UI 一键切英文。

**验证方式**：双语言全页面走查无溢出（文案长度差异最大的按钮/标签重点看）。

**关联链路**：settings 存储（F3）；ModeSelector/InputPanel 等 9 个组件文案提取。

## F9 单任务、无生成队列

**定位**：`App.tsx` 全局单 status/conversation 状态；`usePipeline` 单 run token

**问题原因**：状态模型按"一次只有一个任务"设计。

**影响**：不能边生成 A 边构思 B；切模式会作废在途任务（App.tsx:493-499）。

**修复方案**：短期不做多并行 UI（复杂度高），先做**任务队列**：InputPanel 提交时若在途则入队提示；完成后台任务继续、通知用户。长期（大版本）：Tab 化会话，每 Tab 独立 conversation/status，后端 run-id 化（依赖 A9）。

**旧规则处理**：切模式作废在途任务的行为改为"提示确认"（防误触丢任务）。

**新规则握手落地**：短期零契约变更；长期依赖 A9 的事件 run-id。

**修复后预期**：短——误切模式不再静默丢任务；长——多歌并行。

**验证方式**：手动——在途时切模式出确认框；队列任务依次完成。

**关联链路**：B3 取消（队列取消单任务）；A9 run-id（长期前置）。

## F10 讨论中无人工干预点

**定位**：`orchestrator.rs:653-697`（讨论轮全自动，无用户输入口）

**问题原因**：流水线设计为全自动收敛。

**影响**：用户发现方向跑偏只能等跑完再"优化"，浪费整轮成本；产品"圆桌"隐喻下用户本人没有座位。

**修复方案**：阶段化引入。第一步（低风险）：讨论轮结束后、阶段 2 前，前端显示"方案已收敛，继续格式化 / 插入意见再讨论一轮"二选一（Rust 侧用 tauri channel 或轮询一个 `Arc<Mutex<Option<String>>>` 用户意见槽）；第二步（大版本）：每轮讨论后暂停等待用户确认（可配置自动/手动模式）。

**旧规则处理**：默认保持全自动（行为不变），干预为 opt-in。

**新规则握手落地**：新增 command `pipeline_inject_feedback` + 事件 `PipelineEvent::AwaitingUser`；前端 ResultPanel 增插入意见入口。

**修复后预期**：用户可以中途纠偏，避免整轮浪费。

**验证方式**：手动——插入意见后观察主持人下一轮汇总包含该意见；超时不插（30 秒）自动继续。

**关联链路**：与 F1 增量优化互补（中途插嘴 vs 事后优化）；与 B3 取消共享用户输入通道设计。

## F11 test_api 用 max_tokens=1 误报

**定位**：`llm.rs:486-518`（`test_api`，:497 max_tokens=1）

**问题原因**：max_tokens=1 时思考型模型（reasoning 吃预算）可能无正文或直接报错，HTTP 200 都拿不到。

**影响**：配置正确的思考型模型被误报"连接失败"，用户错误排查/换 API。

**修复方案**：max_tokens 提到 16 并解析响应成功（哪怕 content 为空也算连通——只验证 HTTP + 鉴权 + 模型存在）；401/404/429 分别映射为"密钥错误/模型不存在/限流"的明确文案，替代裸 HTTP 状态码。

**旧规则处理**：无（纯改进，命令签名不变）。

**新规则握手落地**：无契约变化；错误文案变化前端原样展示。

**修复后预期**："测试连接"对全部模型类型给出准确结论与可操作的错误提示。

**验证方式**：单测——错误映射函数；手动——错 key/错模型名/正常配置三分支各测一次。

**关联链路**：角色级测试（App.tsx:165-187）走同一命令自动受益。

## F12 Mode C 拼字符串再正则拆（脆弱协议）

**定位**：`InputPanel.tsx:25`（拼接 `原歌词：\n…\n\n新主题：\n…`）、`App.tsx:220, 278`（正则拆回）、`validator.rs:404-409`（extra 缺失时莫名报错）

**问题原因**：前端明明有两个独立 state（`originalLyrics`/`newTheme`，InputPanel.tsx:15-16），却拼成一个字符串再解析——P6 修复（贪婪匹配）就是在补这个自己造的洞。

**影响**：用户少打换行/冒号变体 → 拆不出 → 阶段 2 报"extra 参数缺失"，用户面对莫名错误。

**修复方案**：`PipelineRequest` 加 `original_lyrics: Option<String>` 独立字段（`extra` 保留一个版本做兼容后删除）；前端 mode_c 直接传两个字段，App.tsx 两处正则拆分（:217-225、:274-283）删除；后端 `run_host_initial`（:429-431）、`execute_review`（:308-310）、`execute_audit_review`（:367-371）、`run_audit_format`（:499-511）、`validate_for_mode` 的 extra 来源改读新字段。

**旧规则处理**：`extra` 字段标记 `#[serde(deprecated)]` 风格注释，保留一个版本的解析兼容（旧请求 extra 仍生效），下个版本删除；删除 App.tsx 中 P6 修复的注释块。

**新规则握手落地**：新字段两端同 commit；serde `#[serde(default)]` 保证旧请求不炸。

**修复后预期**：Mode C 输入不再可能被解析失败，"extra 参数缺失"错误从产品中消失。

**验证方式**：单测——serde 新旧字段兼容解析；手动——原歌词含"新主题："字样、无换行等边界输入全部正常。

**关联链路**：`handleGenerate`/`handleRefine` 的 extra 逻辑简化；`collect_hard_issues` 的 extra 传参（:703）改新字段。

## F13 后端对输入零校验

**定位**：`models/mod.rs:204-219`（PipelineRequest 无约束）、`orchestrator.rs:747-760`（command 入口无校验）

**问题原因**：信任前端校验，后端不做防御。

**影响**：50 万字粘贴直接进 prompt（token 爆炸+费用）；空串能穿透到 LLM；feedback 注入无长度限制。

**修复方案**：command 入口统一校验：`user_input` 非空且 ≤20000 字符、`extra/original_lyrics` ≤20000、`feedback` ≤2000、`base_url` 必须为合法 http(s) URL、`model`/`api_key` 非空；超限返回结构化错误文案（"输入过长（N 字符），请精简后重试"）。校验函数纯化放 models，单测覆盖。

**旧规则处理**：前端校验保留（快速反馈），后端为准入兜底——双层校验。

**新规则握手落地**：错误文案走现有 `Err(String)` 通道，前端 StatusIndicator 原样展示，无契约变化。

**修复后预期**：恶意/误操作输入被挡在 LLM 调用之前，费用可控。

**验证方式**：单测——边界值（空/1 字/20000/20001）；手动——粘贴超长文本看拦截文案。

**关联链路**：A5 错误分层后这些校验错误归 `Validation` 类；F12 的新字段一并纳入校验。

## F14 无快捷键 / 拖拽导入 / 设置导入导出

**定位**：`InputPanel.tsx`（textarea 无 drag-drop）、`App.tsx`（无键盘事件）

**问题原因**：交互细节未做。

**影响**：粘贴长歌词必须手动滚 textarea；无效率工具。

**修复方案**：① Cmd+Enter 提交生成、Cmd+K 聚焦输入、Cmd+, 打开设置（React onKeyDown + Tauri 全局注册二选一，前者够用）；② textarea 支持 drop .txt/.lrc 文件读入（Tauri drag-drop 事件 → fs 读取 → 填充对应 textarea，mode_a 填歌词、mode_c 填原歌词）；③ 设置面板加"导出/导入配置"（JSON 序列化 settings，**排除 api_key** 或可选含——安全决策：默认排除并提示）。

**旧规则处理**：无。

**新规则握手落地**：capabilities 需加 `drag-drop`/`fs` 相关权限。

**修复后预期**：常用操作全部有键盘路径；歌词文件拖入即填。

**验证方式**：手动走查三分支；边界——拖入非 txt 文件给提示。

**关联链路**：F3 设置存储（导入导出同 schema）；F8 i18n（新提示文案入 locale 文件）。

---

# 第三部分 · 架构债（14 项）

## A1 重试预算与全局超时无协调

**定位**：`llm.rs:22-29`（retry_plan）+ `llm.rs:65-71`（单请求 120/300s 超时）+ `orchestrator.rs:616`（全局 15 分钟）

**问题原因**：单调用最坏 3 次重试 × 120s + 90s 退避 ≈ 7.5 分钟；16 个调用点各自独立重试，总最坏远超 15 分钟——然后触发 B1 的 abort，前面费用全部浪费。

**修复方案**：引入**流水线级共享预算**：`Budget` 结构（`Arc<Mutex<Duration>>` 剩余时间），每次重试决策前查询剩余预算，不足则跳过重试直接失败；单调用超时也随剩余预算收紧（`min(120s, remaining)`）。全局超时从"事后兜底"变为"事前约束"。

**旧规则处理**：retry_plan 退避表不变，只加预算闸门；现有 retry_plan 测试补预算无限用例保持兼容。

**新规则握手落地**：纯后端内部，无契约变化。

**修复后预期**：全局耗时数学上不可能超 15 分钟；B1 的 abort 退化为极罕见的最后防线。

**验证方式**：单测——预算耗尽时 retry_plan 返回 None 的分支；模拟连续 429 耗尽预算后整体快速失败。

**关联链路**：B1（超时语义）、B3（取消优先级高于预算判断）。

## A2 角色串行执行（可并发 ×4）

**定位**：`orchestrator.rs:659-667`（for 循环逐个 await）

**问题原因**：审改角色之间无依赖（都审同一份 current_plan），顺序执行仅为实现简单。

**影响**：Mode D 一轮 = 4 个角色串行 ≈ 4× 单角色耗时；整条流水线时间 ×3~4。

**修复方案**：同轮角色 `futures::future::join_all` 并发；`revisions_log` 的可见性语义调整——并发后本轮角色互相看不到对方当轮修订（原本串行可见），改为统一在 auditor 审查时看到全部（execute_audit_review 本就收 round_changes，语义更公平）；events 乱序到达——前端 updateExpert 按 role 更新，天然无序安全（已核实 usePipeline 无顺序依赖）。

**旧规则处理**：串行时"后面角色能看到前面角色的当轮修订"这一隐性行为取消（本来就不是承诺的契约）。

**新规则握手落地**：无契约变化；事件并发 emit 需确认 Tauri emitter 线程安全（官方支持）。

**修复后预期**：一轮讨论耗时 = 最慢单角色，整体提速 3 倍左右。

**验证方式**：单测——4 个假异步任务并发完成时间 < 串行和；手动——观察 round 内角色几乎同时"working"。

**关联链路**：F2 审改流式（并发流式事件按 role 区分，前端已按 role 定位卡片）；A1 预算共享（并发下预算扣减需原子操作）。

## A3 plan/revisions_log 注入无预算

**定位**：`orchestrator.rs:306-323`（每角色注入完整 plan + 全量 revisions_log）、:652（revisions_log 只增不减）

**问题原因**：知识库注入有 INJECT_MAX_* 纪律，对话上下文没有对称的预算。

**影响**：成本放大 = (角色数+1) × 轮数 × plan 长度；3 轮 Mode D 最坏 16 倍方案体积的输入 token。

**修复方案**：① `revisions_log` 每角色每表只保留**最近 2 轮**完整条目 + 更早轮次的单行摘要（role+reason 前 50 字）；② plan 注入加字数检查：超 8000 字符时警告（与知识库 INJECT_MAX_TOTAL_CHARS 同款 eprintln 风格），提示未来做 plan 摘要压缩；③ 常量集中定义（对齐 orchestrator.rs:198-207 的既有风格）+ 测试锁。

**旧规则处理**："不要重复提同一问题"的去重语义由全量日志承担——改为摘要后保留 reason 关键词，语义基本等价。

**新规则握手落地**：纯后端，无契约变化。

**修复后预期**：3 轮场景输入 token 降 40% 以上，长方案不再滚雪球。

**验证方式**：单测——3 轮后 revisions_log 渲染长度上限断言；预算常量测试锁（仿 :1049-1108 的 budget 测试）。

**关联链路**：B4 的 ⚠️ 条目同样纳入 log 预算；F1 增量优化天然减少 log 体积。

## A4 知识库每步重新解析（无缓存）

**定位**：`orchestrator.rs:38-40`（`load_knowledge` 每次调用 `load_embedded`）+ 调用点 :295/:352/:486

**问题原因**：`load_embedded` 每次从头解析 6 张 CSV；一次生成被调用 8~16 次。

**影响**：重复解析开销（毫秒级×16，小但暴露无共享状态习惯）；改造成本低收益直接。

**修复方案**：`static KB: OnceLock<KnowledgeBase>` 惰性初始化；`load_knowledge()` 改为读缓存。注意 `KnowledgeBase` 含 HashMap 需要跨 await 共享——用 `Arc<KnowledgeBase>`，调用点签名适配（`render_*` 都是 `&self`，改传 `&Arc` 即可）。

**旧规则处理**：`load_embedded` 保留（缓存 miss 时调用）；目录加载版保留给测试（knowledge.rs:208）。

**新规则握手落地**：无契约变化。

**修复后预期**：CSV 解析从每步一次变为进程一次。

**验证方式**：现有 132 测试全绿（load_embedded 语义不变）；新增测试——两次 `load_knowledge()` 返回同源（Arc ptr_eq）。

**关联链路**：三个调用点签名适配；与 A2 并发安全（OnceLock 初始化线程安全）。

## A5 错误处理全链 `Result<_, String>`

**定位**：`orchestrator.rs`/`llm.rs` 全部函数签名；前端只能 `String(e)` 展示

**问题原因**：原型期图快，未建错误类型。

**影响**：网络错/凭据错/格式错/校验错不可区分；前端无法按类型给建议（比如 401 提示检查密钥 vs 超时提示重试）；B3 的取消哨兵值只能用魔法字符串。

**修复方案**：新建 `errors.rs`：`AppError { kind: ErrorKind, message: String }`，`ErrorKind = Network | Auth | RateLimit | Timeout | Cancelled | Parse | Validation | Internal`；全部 `Result<_, String>` 换 `Result<_, AppError>`（实现 `Serialize` 让前端拿到 `{kind, message}`）；Tauri command 返回结构化错误；前端 StatusIndicator 按 kind 给差异化文案与动作（Auth→"检查密钥"、RateLimit→"稍后自动重试中"）。

**旧规则处理**：`__cancelled__` 哨兵（B3 临时方案）被 `ErrorKind::Cancelled` 正式替代；全部 `format!("API returned {}: ...")` 处归类。

**新规则握手落地**：command 错误从 string 变 object——**前端 `catch(e)` 需适配**（`typeof e === "object" ? e.message : String(e)`），两端同 commit；过渡期后端可同时序列化 kind+message。

**修复后预期**：用户看到的每条错误都带类型化建议；代码层可按 kind 做策略（如 RateLimit 才值得重试）。

**验证方式**：单测——各 kind 构造与序列化；手动——断网/错 key/超长输入三分支看差异化文案。

**关联链路**：B3（Cancelled）、A1（RateLimit 策略）、B4（Parse/截断归类）、F13（Validation）。

## A6 无日志落盘

**定位**：全项目 `eprintln!`（llm.rs:42/:53/:268/:272、orchestrator.rs:271/:277、knowledge.rs:227/:237 等）

**问题原因**：开发期 stderr 直出，未接日志框架。

**影响**：发布版用户遇到问题，无任何回查手段；B4/A7 的降级告警用户看不到也查不到。

**修复方案**：引入 `tracing` + `tracing-subscriber`（rolling file appender 到 `{app_data}/logs/shiyi.log`，保留 3 天 × 5MB）；eprintln 全量替换为 `warn!/error!`；日志级别 info，设置面板加"打开日志目录"按钮（诊断用）。pipeline 关键节点（阶段开始/结束/打回/降级）打 info。

**旧规则处理**：eprintln 文本原样迁移到宏（信息不丢）；`[llm]`/`[pipeline]` 前缀转为结构化 span。

**新规则握手落地**：无契约变化；新增"打开日志目录"用 tauri opener 插件（capability 增权限）。

**修复后预期**：任何用户报障都有日志可查；B4/A7 的降级事件可追溯。

**验证方式**：单测——无；手动——触发一次打回后检查日志文件内容与滚动。

**关联链路**：B4/A7 的告警可见性兜底；A5 错误类型打结构化字段。

## A7 CSV 一行错整表跳过 + 无 BOM 处理

**定位**：`knowledge.rs:589-620`（`parse_csv` 任一行列数不符返回 Err）+ :231-239/:262-272（整表跳过）

**问题原因**：容错粒度设计在表级而非行级。

**影响**：91 行的 instruments 表第 80 行多一个逗号 → 整表消失 → 制作人知识注入空转，且只有 stderr（A6 之前用户不可见）。

**修复方案**：① 行级容错：坏行跳过 + 记录行号，表头有效即加载；坏行数 > 10% 时整个表拒绝（防半残表静默注入错误知识）；② BOM 处理：`content.strip_prefix('\u{feff}')`；③ 坏行明细进日志（依赖 A6）。

**旧规则处理**：`rejects_column_mismatch` 等单测（:674-677）语义从"整表拒绝"改为"坏行跳过"，测试改写；P3 降级测试（:811-835）同步调整。

**新规则握手落地**：无契约变化；`Table` 可携带 `skipped_rows: usize` 供日志。

**修复后预期**：单行笔误不再废掉整表；半残表不会静默注入。

**验证方式**：单测——1 坏行跳过+计数；>10% 坏行整表拒；BOM 前缀剥离；行号日志断言。

**关联链路**：`load_embedded` 与 `load`（目录版）同一 parse 路径自动一致；embedded_load_matches_directory 测试（:838-852）守护两路等价。

## A8 角色阵容三处真源 + 前后端类型手抄

**定位**：`orchestrator.rs:20-35`（steps_for_mode）、`usePipeline.ts:14-46`（MODE_EXPERTS）、`roles.rs:36-46`（role_for）；类型 `models/mod.rs` vs `types/index.ts` 手工同步

**问题原因**：前后端各自定义同一领域模型，靠人肉+注释对齐。

**影响**：加一个角色要改 6 处（2 枚举+2 阵容+2 emoji 映射），漏一处即静默漂移（如前端卡片缺失）。

**修复方案**：短期：后端新增 command `get_pipeline_meta()` 返回 `{modes: {mode: roles[]}, roles: {role: {name, emoji, knowledge[]}}}`，前端 usePipeline 初始化时 invoke 获取，删除本地 MODE_EXPERTS/ROLE_NAMES/ROLE_EMOJIS 三张表——**单一真源在后端**。长期：schemars 生成 TS 类型（`types/index.ts` 由 `cargo test` 产出，CI 校验 diff）。

**旧规则处理**：前端三张表删除；`MODE_EXPERTS` 的展示字段（color）留前端（纯 UI 属性不进后端）。

**新规则握手落地**：新 command + 前端启动时一次 invoke；类型生成走 schemars + ts-rs 择一。

**修复后预期**：加角色只改 Rust 一处；前后端阵容不可能漂移。

**验证方式**：单测——meta 输出与 steps_for_mode/roles 一致性断言；手动——后端改角色名，前端 UI 自动跟随。

**关联链路**：F9 多任务演进依赖此统一；B10 的 emoji/name 展示统一来源。

## A9 事件通道无 run-id

**定位**：`models/mod.rs:232-253`（PipelineEvent 无标识字段）；前端靠 runTokenRef 补（usePipeline.ts:136/:173）

**问题原因**：事件协议按单任务设计。

**影响**：多任务/多窗口/队列场景无法区分事件归属；前端 token 补丁是应用层 workaround，后端无法取消特定任务（B3 被迫用全局标志）。

**修复方案**：`PipelineEvent` 加 `run_id: u32` 字段（`#[serde(default)]` 兼容旧事件消费）；`pipeline_generate` 请求带 run_id 或由后端生成并在首事件回传；`cancel_pipeline(run_id)` 精确取消；CANCEL registry 改 HashMap<u32, CancellationToken>。

**旧规则处理**：前端 token 机制保留（双保险）；serde default 保证事件消费向后兼容。

**新规则握手落地**：事件加字段（附加式）+ command 加可选参数（serde default）——均向后兼容，两端同 commit。

**修复后预期**：取消精确到任务；F9 队列/多任务的地基就绪。

**验证方式**：单测——事件序列化含 run_id 且旧 JSON（无 run_id）可反序列化；手动——两个浏览器窗口（dev 模式多开）各自取消互不干扰。

**关联链路**：B3 取消升级；F9 多任务；F10 用户干预通道按 run_id 路由。

## A10 prompt 编译进二进制，无远程配置

**定位**：`prompts.rs` 全文（1076 行 prompt 字符串）+ `knowledge.rs:253-260`（CSV include_str!）

**问题原因**：静态嵌入方案简单可靠，但无热更新能力。

**影响**：改一个方法论措辞 = 重新编译发版（用户要重新下载安装）；prompt 迭代周期被发版绑架。

**修复方案**：分两步。① **用户级覆盖**：启动时检查 `{app_data}/prompts/` 目录存在同名 prompt 文件则优先加载（高级用户/测试可热调）；CSV 同理（knowledge.rs:205 的 `load(dir)` 早已支持目录加载，只差生产路径接入 + embedded 兜底）。② 远程下发（大版本）：设置"从 URL 同步知识库"或 GitHub Release 附件拉取（校验 hash），核心方法论不出现在明文 URL——注意本项目开源，prompt 本身无需保密，此项价值在**迭代速度**而非保密。

**旧规则处理**：`include_str!` 嵌入版保留为兜底真源（目录缺失/损坏时使用）；`embedded_load_matches_directory` 测试守护语义。

**新规则握手落地**：加载优先级：app_data 目录 > embedded；加载结果日志（A6）注明来源。

**修复后预期**：prompt/知识库改版不用发版（覆盖文件即可）；实验性 prompt 可灰度。

**验证方式**：单测——目录存在时优先加载、文件损坏回退 embedded；手动——放一个改过的 csv 重启生效。

**关联链路**：A7 行级容错（目录加载同路径）；A6 日志记录加载来源。

## A11 temperature / max_tokens 硬编码

**定位**：`llm.rs:160`（temperature 0.6）、:200（流式 0.7/8192）、`orchestrator.rs:331/:406/:464/:522`（max_tokens 3000）

**问题原因**：调优参数写死在代码。

**影响**：不同模型最优参数不同；用户无法权衡速度/质量/成本；B4 的截断重试需要动态调 max_tokens，硬编码使改造点分散。

**修复方案**：`AppSettings` 加可选 `generation: { temperature?: f32, max_tokens_review?: u32, max_tokens_stream?: u32 }`（serde default 取现值——0.6/3000/8192，行为不变）；设置面板"高级"折叠区暴露；后端 `call_llm_*` 从 request 读取。

**旧规则处理**：默认值=现行值，零行为变化；旧 localStorage 无该字段走 default。

**新规则握手落地**：请求字段可选（serde default）——旧前端不带字段时后端用默认，完全兼容。

**修复后预期**：参数可调；B4 的"截断提额重试"有了统一的配置来源。

**验证方式**：单测——default 解析等于现值；手动——temperature 调 0.2 观察输出收敛。

**关联链路**：B4（max_tokens 动态化）；F13 输入校验（数值范围校验 0-2）。

## A12 API Key 明文存 localStorage

**定位**：`useSettings.ts:16-17`（代码自留 NOTE）+ :47-54

**问题原因**：最简实现；WebView localStorage 无加密。

**影响**：本机其他进程/用户可读 WebView 存储目录拿到 key；备份/同步工具可能带出去。

**修复方案**：引入 `tauri-plugin-stronghold`（或 keyring crate 走系统钥匙串）：keychain 存 api_key，settings 文件只存非敏感字段；启动时从钥匙串读取；**迁移**：首次启动检测 localStorage 有 key → 写入钥匙串 → 从 settings/localStorage 删除明文。

**旧规则处理**：一次性迁移后明文清除；角色级 override 的 api_key（role_overrides）同样迁移。

**新规则握手落地**：settings 结构不变（apiKey 字段运行时从钥匙串合并）；新增 keychain 权限到 capabilities。

**修复后预期**：磁盘上无明文 key；代码里的 NOTE 兑现。

**验证方式**：手动——迁移后 `grep -r` settings 文件无 key 明文；重启后设置面板回显正常。

**关联链路**：F3（存储迁移同套路）；F14 设置导入导出（key 默认排除）。

## A13 前端 0 测试

**定位**：`src/` 无任何测试文件；`package.json` 无 test script

**问题原因**：未搭建前端测试设施。

**影响**：App.tsx 的 H4 token 竞态、P5/P6 修复等历史 bug 无回归防线；重构（F12/A8）无安全网。

**修复方案**：引入 Vitest + React Testing Library；优先覆盖**纯函数与 hooks**（性价比最高）：`parseEnergy`（ResultPanel.tsx:27-51，含区间/标签分支）、`buildRoleOverrides`（usePipeline.ts:103-109）、`sanitizeStored`（useSettings.ts:20-44）、usePipeline 事件分支（mock listen）；App.tsx 级集成测试放第二批。`package.json` 加 `"test": "vitest run"`，CI test job（B5）追加该步。

**旧规则处理**：无。

**新规则握手落地**：CI test job 追加 `npm test`。

**修复后预期**：核心解析/竞态逻辑有回归防线；重构有安全网。

**验证方式**：首批 ≥20 个用例；故意改坏 parseEnergy 看 CI 红灯。

**关联链路**：B5 CI；F12/F7 等重构的验收依赖。

## A14 无 LICENSE

**定位**：仓库根目录无 LICENSE 文件，README 无许可证声明

**问题原因**：开源时遗漏。

**影响**："Public ≠ 开源"：无许可证 = 默认版权保留，他人无法律许可使用/修改/分发；想合规引用的人只能干看。

**修复方案**：添加 MIT LICENSE（版权行：`Copyright (c) 2026 suce346-cmd`）；README 底部加 License 章节；package.json 加 `"license": "MIT"`；Cargo.toml 加 `license = "MIT"`。

**旧规则处理**：无（纯增量）。注意：若未来担心 prompt 方法论被整包搬运商用，可选 AGPL-3.0——MIT 与 AGPL 二选一，**用户决策项**，默认建议 MIT（个人项目传播优先）。

**新规则握手落地**：无代码契约；下次 push 附带。

**修复后预期**：GitHub 仓库页显示 license 标签，法律意义上成为开源项目。

**验证方式**：GitHub 页面侧栏出现 "MIT license"；`cargo` 与 `npm` 元数据一致。

**关联链路**：无。

---

# 实施批次与依赖图

```
第 1 批（止血，互不依赖，可并行）:
  B1 超时abort · B2 panic恢复unwind · B5 CI测试门禁
第 2 批（可信化）:
  B10 审改降级可见 → B4 截断分级处置（10 先行，4 复用其降级机制）
第 3 批（校验纠偏）:
  B9 能量统一 → B7 Style正文提取 → B6 BPM复用+收紧 → B8 Verse逐段
第 4 批（取消与错误地基）:
  A5 错误类型 → B3 取消通道（依赖 A5 的 Cancelled kind）→ A1 预算协调
第 5 批（体验跃升）:
  A2 角色并发 · F2 讨论流式 · F4 token统计 · F11 test_api · F12 ModeC直传 · F13 输入校验
第 6 批（数据与配置）:
  F3 历史持久化+导出 · A12 keychain · A11 参数可配 · F7 主题 · F14 快捷键/拖拽
第 7 批（工程化收尾）:
  F5 Windows包 · B2遗留验证 · A13 前端测试 · A6 日志 · A7 CSV容错 · A4 KB缓存 · A3 注入预算
第 8 批（大版本方向）:
  F1 增量优化 · F9 队列 · F10 人工干预 · A8 单一真源 · A9 run-id · A10 热配置 · F6 签名更新 · F8 i18n · A14 LICENSE（可随第1批顺手提交）
```

**每批验收标准（统一）**：`cargo test` 全绿 + `npx tsc --noEmit` 全绿 + 前端 `npm test`（A13 后）全绿 + `cargo test --release`（B2 后）+ 一次 `tauri dev` 手动冒烟 + 单独 commit + push（CI 门禁生效后红灯即停）。

**用户决策项（需拍板，不阻塞前 5 批）**：
1. A14：MIT 还是 AGPL-3.0？
2. F6：是否购买 Apple Developer 账号（$99/年）做签名？
3. F1/F10：增量优化与人工干预的交互形态（全自动优先还是干预优先）？

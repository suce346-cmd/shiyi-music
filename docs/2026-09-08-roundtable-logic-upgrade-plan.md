# 圆桌流水线逻辑升级项目书（v0.5.2）

> 文档定位：**升级规划项目书（非实现代码）**。吸收《多Agent协作对话系统-详细规划方案书-修订版》的机制设计，对 shiyi-music 圆桌流水线做逻辑升级。
> 基线 commit：`10f2b14`（v0.5.1 已发布）｜规划日期：2026-09-08｜前置输入：架构一致性深度审查报告（裁决 FAIL，2×P1 + 3×P2）与四模式 GUI 实测报告。
> **铁律：规则知识零丢失**——roles.rs 角色方法论、prompts.rs 模式指令、suno_rules.csv 知识库、rules.rs 数值全部保留；升级的是"谁执行、何时执行、如何衔接"的逻辑骨架，不是重写。

---

## 0. 一句话结论

圆桌的问题不是"规则太多"，而是**规则的用户承诺与代码执行各修各的**——本次升级把每条规则接到唯一执行者上（吸收任务书"每条规则唯一执行者"原则），把收敛-终稿断层改成受控转写（吸收"三重停止判据"的独立复核思想），把降级从静默变成诚实声明（吸收 §5.6.1）。约 600-800 行改动，零新依赖，全程 feature flag 可回退。

---

## 1. 现状诊断（为什么必须升级）

### 1.1 架构审查确认发现（置信度 ≥80）

| 级别 | 发现 | 证据 |
|------|------|------|
| P1 | **参数区间"提示词承诺、代码零执行"**：抖音 12-20/85-95 与 B 20-35/75-85 在 `validate_douyin`（validator.rs:333-394）与 `validate_production`（validator.rs:108-176）中均无检查；`rules.rs:59-67` 的 `DOUYIN_WEIRD_*`/`MODE_B_STYLE_*` 常量全库零生产消费者（死常量）；rules.rs:142 测试只锁 checklist 文本字样；"叙事型可回落例外"条款三个提示词都有（roles.rs:72/92/113/210）、代码无对应物 | grep 铁证：`grep -rn "DOUYIN_WEIRD" src-tauri/src` 仅命中 rules.rs 自身 |
| P1 | **收敛判据与硬校验作用于不同文本**：讨论轮"全员无异议"承诺的是中间方案（current_plan）；阶段 2 校验员重新生成终稿=重新创作而非无损转写。GUI 实测复现：Mode C 改词人 checked"字数对齐"✅、校验员无异议✅，代码随即打回"第 2 行字数不符：原 7 字 vs 新 8 字" | GUI 运行 s-70 事件流 [115]；终稿存档 /private/tmp/shiyi_acceptance/gui_mode_c.txt |
| P2 | **"单源清单"只注入讨论轮**：阶段 0 主持人初稿用 `prompt_for_mode`（orchestrator.rs:712-741）+primer，不拼 `rules::checklist`；同一规则现存 5 处表述（模式指令/角色 prompt/checklist/校验器/CSV），"350 字符"在 prompts.rs 出现 8 次、roles.rs 5 次 | orchestrator.rs:491/653 有 checklist，:712-741 无 |
| P2 | **orchestrator.rs 2319 行 9 种职责**（调度/事件/汇总/校验门/预算/检查点/取消/插话/usage） | 文件规模 + 方法清单 |
| P2 | **多角色同 target 修订无确定性冲突消解**：`build_summarize_user_prompt`（orchestrator.rs:807-833）把全部修订交 LLM 自由整合；D 实测第 1 轮作词/流行/校验三方同时改 lyrics | GUI 运行 s-61 事件流 |

### 1.2 与任务书的设计差距（用户"自相矛盾"感受的结构根源）

| 任务书原则 | 圆桌现状 | 差距定性 |
|-----------|---------|---------|
| §5.4 三重停止判据（硬上限+LLM 裁判+独立复核） | 有硬上限（3 轮+打回 2 次+预算熔断）✅、有 LLM 裁判（agree 结构化✅），独立复核只覆盖格式不覆盖内容承诺 | 缺一条腿 |
| 单活跃发言者（冲突物理消除） | 多角色并行修订同一 target，靠 LLM 事后整合 | 冲突被推迟而非消除 |
| §5.4.1 收敛判定不得依赖自由文本语义判断 | agree=true 已是结构化 JSON（比任务书还早）✅ | 已达标，保持 |
| §5.6.1 诚实降级 | 只有 AuditResult(pass=false) 显示，无 run 级 degraded 聚合 | 部分达标 |
| 每条规则唯一执行者 | 参数类规则有承诺无执行者（上表 P1） | 核心差距 |

### 1.3 保留资产（明确不动）

- 硬校验器哲学（validator.rs:3"不信任 LLM 自检，格式类规则由代码裁决"）——实测拦截率 100%
- 全部角色方法论、模式指令、知识库 CSV、checklist 单源清单机制
- 预算熔断/检查点续跑/run_id 事件过滤/取消/插话/usage 追踪
- 四模式座位表、逐行字数对齐、K-3 Audio Influence=0 硬门、"80 字符说明行"闭环样板（CSV→常量→提示词→代码四环节）

---

## 2. 升级方案总览（六个决策）

| # | 决策 | 选定方案 | 对应发现 | 吸收的任务书机制 |
|---|------|---------|---------|----------------|
| D1 | 收敛-终稿断层 | 转写契约 + 保真校验 + 定点重写 | P1-发现2 | §5.4 独立复核 / DecisionLevelGuard 双层防线 |
| D2 | 规则唯一执行者 | 参数硬门（消费死常量）+ RULE_REGISTRY 注册表守护 | P1-发现1 | 每条规则唯一执行者 |
| D3 | 阶段 0 纳入单源 | 拼 checklist + prompts.rs 数值 format! 化 | P2-发现3 | 单源原则 |
| D4 | 发言冲突消解 | 冲突预检标注 + 领地声明仲裁指令 | P2-发现5 | 单活跃发言者（消解版） |
| D5 | 诚实降级 | run 级 degraded 聚合 + 导出完整度声明 | 新增 | §5.6.1 |
| D6 | 握手与旧逻辑处置 | 数据只增不改、旧条款退役、feature flag | 全部 | §5.16 功能开关 |

明确**不做**（属于任务书新项目，不是本次）：完整调度状态机（全边转移表）、语义竞价、互评 rubric/匿名盲评、SQLite/SSE、工具调用、沉默顾问圈。本次是逻辑升级，不是架构重构。

---

## 3. 详细设计

### 3.1 D1：转写契约 + 保真校验（修收敛-终稿断层）

**问题本质**：阶段 2 校验员拿到收敛方案后"按标准格式输出最终提示词包"，模型实际重新创作——字数、行数、内容都可能变。收敛承诺（改词人"字数对齐"✅）不传递到终稿。

**转写契约**（改 `roles.rs:92` auditor 格式端口 system prompt）：
```
你拿到的是已收敛的最终方案。你的任务是【转写】不是【重写】：
1. 歌词部分逐行保留：每行字数、行数、断句空格、用语一律不变——
   原文某行 7 字，输出必须 7 字；原文 3 行，输出必须 3 行（尾部收尾除外）。
2. 你只允许做三类格式操作：补齐缺失的格式要素（说明行/参数行/结构标签）、
   修正说明行格式（配器表述/能量标注）、调整段落标签命名。
3. 若发现收敛方案本身违反硬规则（字数不符/缺骤停等），不要自行修改——
   原样转写并在输出第一行加标记 `TRANSCRIPTION_ISSUE: <问题描述>`。
```

**保真校验**（新函数 `check_transcription_fidelity(converged_plan, final_text, mode, original_lyrics)`，validator.rs）：
- 歌词正文逐行比对（去断句空格后）：字数不符/行数增减 → issue 并标注**精确行号与两侧原文**
- Mode C：以原歌词为基准逐行比对（已有 validate_lyric_fill 逻辑复用）
- 其他模式：比对结构标签集合与歌词行内容集合（顺序无关的 multiset diff）
- Style Prompt：仅校验存在性与长度（模型允许润色措辞，不允许丢块）
- 参数行：校验存在性与格式（数值合法区间由 D2 硬门管）

**定点重写**：打回时不再"重新格式化全文"——把违规 issue（含行号）+ 收敛方案原段落交给校验员，指令"只输出修正后的该段落"，代码侧按段落替换。retry 预算仍为 ≤2 次。

**TRANSCRIPTION_ISSUE 处理**：出现该标记 = 收敛方案本身有硬伤（讨论轮漏网），走既有打回通道回炉（重开一轮讨论，消耗轮次预算），不静默修复——保证"谁发现谁修"的责任链清晰。

### 3.2 D2：规则注册表 + 参数硬门（每条规则唯一执行者）

**参数硬门**（补进既有校验函数，消费现有死常量）：
- `validate_production`（mode_a/mode_b）：mode_b 增加 `MODE_B_WEIRD_MIN/MAX`、`MODE_B_STYLE_MIN/MAX` 区间检查；mode_a 保持弧线区间（ARC_PARAMS）——ARC_PARAMS 当前在 validator 中已被清单引用检查（K-1 落地时的 checklist 锁定测试），确认其执行点存在，若无则一并补
- `validate_douyin`（mode_d）：增加 `DOUYIN_WEIRD_MIN/MAX`、`DOUYIN_STYLE_MIN/MAX` 区间检查
- **叙事型例外代码化**：mode_d 越界时，若满足"结构含 ≥2 个非 Hook 叙事段（Verse/Pre-Chorus/Bridge）"**且**方案文本含结构归类说明（"叙事型"字样）→ 放行并记 info；否则打回。理由声明由讨论轮校验员把关（其 prompt 已有该职责），代码只做结构事实校验——诚实分层：LLM 判语义、代码判结构
- Audio Influence=0（K-3）既有硬门不动

**RULE_REGISTRY**（rules.rs 新增）：
```rust
pub struct RuleSpec {
    pub id: &'static str,          // 如 "mode_b_param_range"
    pub modes: &'static [&'static str],  // 适用模式
    pub constant: &'static str,    // 关联常量名（文档用途）
    pub prompt_sites: &'static [&'static str], // 提示词表述点标识
    pub executor: &'static str,    // 执行点（函数名）
}
pub const RULE_REGISTRY: &[RuleSpec] = &[ /* 每条参数/格式规则一行 */ ];
```

**两类守护测试**（防复发的关键）：
1. **死常量守护**：遍历 RULE_REGISTRY 中引用的每个常量名，grep 生成物断言其在 src 下有非 rules.rs 的引用（执行点或 format! 生成点）——死常量在 CI 即失败
2. **表述生成**：prompts.rs/roles.rs 中的数值段改为 `format!("...{}...", rules::X)` 后，源码中不再允许出现手写的关键数字字面量（`"12-20"`、`"20-35"`、`"75-85"`、`"85-95"`、`"≤80"` 等 grep 断言，白名单：注释与测试）

**例外条款退役**：roles.rs 四处手写的"叙事型可回落 A/B 弧线区间，须说明理由"改为一句"叙事型回落规则由代码执行（DOUYIN_WEIRD/STYLE 区间外需满足叙事型结构判定）"——删除与代码行为不一致的旧表述。

### 3.3 D3：阶段 0 纳入单源

- orchestrator.rs `run_host_initial`（:712-741）：`system.push_str(crate::rules::checklist(req.mode.to_str_name()))` —— 与审改员/校验员同口径（一行）
- prompts.rs 四个模式指令中的数值段（350 字符/12-20/85-95/配器 3-7 件/能量差≥3/说明行 80 等）改为 `format!` 运行时拼装引用 rules 常量——手写数字从源头消灭（守护测试见 3.2）
- 注意：`prompt_for_mode` 返回 `&'static str` 改为 `String`——调用方（:712）同步调整；知识库覆盖机制（`set_prompt_override_dir`）的 override 文件语义不变（仍整段替换）

### 3.4 D4：冲突预检标注

在 `build_summarize_user_prompt`（orchestrator.rs:807）构建修订清单时，代码检测**同 target 且内容不相容**的修订对：
- 相容判定（保守）：同 target 且修订文本互不包含 → 记为候选冲突；同 target 且一个包含另一个 → 视为细化，不算冲突
- 命中时在对应修订条目前注入：
  ```
  ⚠️ 冲突：{角色A} 与 {角色B} 同时修订了 {target}。
  请按领地声明裁决（R-2：金句文字形态归作词人/传播动态归风格分析师；
  R-3：人声终裁归制作人/参数弧线终裁归情感分析师），并在方案后注明取舍理由。
  ```
- 领地声明表（TERRITORY 表，rules.rs 新增常量）：`(target, 角色) → 终裁者` 的静态映射，与 roles.rs 的领地声明文字同源（用测试锁定二者一致）

### 3.5 D5：诚实降级聚合

- 降级源清单（吸收任务书四源，裁剪为圆桌适用三类）：
  - `call_degraded`：任一角色调用失败后走了降级路径（重试/降级重发/跳过）
  - `gate_degraded`：硬校验打回耗尽降级返回（AuditResult pass=false）
  - `budget_degraded`：预算打断/强制收敛
- 数据流：orchestrator 维护 run 级 `degradation_flags: Vec<&str>` → PipelineEvent 增加透传 → `HistoryEntry.degraded: Option<Vec<String>>`（serde default，旧记录兼容）
- 导出完整度声明（history.rs `render_export` 头部）：
  ```
  > 完整度声明：本次生成 {degraded 为空 ? "全环节正常" : "存在降级：{flags.join('；')}"}，
    硬校验 {pass ? "通过" : "未完全通过（详见对话记录）"}，请据此评估方案可信度。
  ```
- UI：历史详情页 degraded 标记徽章（前端只读消费）

### 3.6 D6：握手与旧逻辑处置清单

| 项 | 处置 | 握手方式 |
|---|---|---|
| history.json | 只增字段（degraded） | serde default，旧文件零迁移直接读 |
| 检查点格式 | **不动** | 新旧版本互相可读（升级中途回退也安全） |
| PipelineEvent | 只增枚举值 | 前端 match 加兜底分支（未知事件忽略） |
| localStorage 设置 | 不动 | 无新字段 |
| 知识库 CSV | 不动 | override 机制不变 |
| 提示词手写数值 | **退役**（format! 化） | 生成结果与原文逐字节一致（快照测试锁定） |
| 叙事型例外条款 | **改写**（引用代码行为） | 语义不变，表述对齐执行 |
| extra 旧协议兼容层 | **保留** | 老前端兼容契约仍有效 |
| 功能开关 | 新增 `features: {strict_param_gate, transcription_fidelity}`，默认 true | Settings 透传，出问题可不发版关闭（后端读默认值） |

---

## 4. 数据流（升级后全链路）

```
用户输入 → [阶段0 主持人：模式指令+checklist(新D3)+知识库] → 初稿
        → [讨论轮 ×N：角色审改(既有)+冲突预检标注(新D4)] → 修订片段
        → [主持人整合：领地仲裁指令(新D4)] → 新版方案 → 收敛判定(既有 agree)
        → [阶段2 校验员：转写契约(新D1)] → 终稿(或 TRANSCRIPTION_ISSUE 标记)
        → [保真校验(新D1) + 模式硬校验(含参数门(新D2))]
             ├─ 通过 → [degraded聚合(新D5)] → 历史落盘+完整度声明 → 交付
             └─ 打回 → [定点重写(新D1)：只重写违规段，≤2次] → 复验 → 同上
```

每节点失败行为：阶段 0 失败=整 run 报错（既有）；讨论轮角色失败=降级继续+flag（既有+D5）；收敛失败=轮次上限强制收敛（既有）；阶段 2 打回耗尽=降级返回+flag（既有+D5）。

---

## 5. 构建顺序（每个组件 = 一次 code-rules 五步流程）

```
Phase 1（两组件无依赖，可并行）：
  C1 规则注册表+参数硬门（rules.rs + validator.rs）
  C2 转写契约+保真校验+定点重写（roles.rs:92 + orchestrator.rs:1350-1414 + validator.rs）
Phase 2（依赖 Phase 1）：
  C3 阶段0 checklist + prompts.rs format!化（orchestrator.rs + prompts.rs；依赖 C1 的注册表/守护测试）
  C4 冲突预检标注（orchestrator.rs:807-833 + rules.rs TERRITORY 表；独立，可与 C3 并行）
Phase 3：
  C5 degraded 聚合+完整度声明（models/mod.rs + history.rs + 前端只读消费；依赖 C1/C2 定稿降级源）
Phase 4：
  全量验证 + tauri 版本 bump 0.5.2（Cargo.toml 单源）+ 发布（CI Version guard 自动核对 tag）
```

## 6. 验证方案（逐项）

### 6.1 单元测试（每组件）

| 用例 | 断言 |
|------|------|
| 参数门-B | Weirdness=50 → fail"区间"；20/75 边界值 → pass |
| 参数门-抖音 | Weirdness=15/Style=88 → pass；50/50 → 结构非叙事型 → fail；50/50+叙事型结构+归类说明 → pass |
| 死常量守护 | 每个 RULE_REGISTRY 常量在 src 有非测试消费者（grep 生成断言）——删掉执行点 CI 即红 |
| 手写数字归零 | grep 断言 prompts.rs/roles.rs 无白名单外数字字面量 |
| 保真校验 | 字数篡改（7→8）/加行/删行/纯格式化（应 pass）四用例；issue 含精确行号 |
| 定点重写 | 违规段替换后其余段落逐字节不变 |
| TRANSCRIPTION_ISSUE | 标记出现 → 走回炉通道不静默修复 |
| 冲突预检 | 0 对/1 对/多对/包含关系（不算冲突）四用例 |
| TERRITORY 一致性 | 表内容与 roles.rs 领地声明文字同步（双向断言） |
| degraded | 三降级源逐一触发；无降级 → 空 |
| 旧数据回归 | v0.5.1 真实 history.json/检查点样本加载无损 |

### 6.2 集成测试

- headless 四模式实网（既有 harness）：**新增断言——终稿通过全部硬校验（含新参数门）**；B/抖音用实网验证转写契约下打回率下降
- 保真校验对 v0.5.1 四份真实终稿回归：`/private/tmp/shiyi_acceptance/gui_mode_*.txt` 全部可判定且结果与已知一致

### 6.3 GUI 验收（安装版，对齐既有四模式实测规程）

1. 配置+连接成功+32000 越界归一（回归）
2. 四模式各一次生成：座位数/讨论轮/收敛事件与 v0.5.1 实测同构
3. **新增观察点**：⚠️ 出现时明细是否含行号；degraded 徽章出现时机；导出文本头部完整度声明
4. usage 落盘四模式与 UI 一致（回归）
5. Info.plist=0.5.2（Version guard 自动核对）

### 6.4 验收标准（对照任务书 §12 口径）

- [ ] 每条参数/格式规则在 RULE_REGISTRY 中恰有一个执行点（自动化测试证明）
- [ ] 收敛承诺传递到终稿：转写保真校验通过率（headless 四模式无 ⚠️ 降级，或降级均有明确行号级原因）
- [ ] 降级可辨识：人为构造校验打回耗尽 → 导出头部出现完整度声明
- [ ] 冲突可裁决：构造同 target 冲突修订 → 汇总输出含取舍理由
- [ ] 零数据迁移：v0.5.1 的 history/检查点/设置在 v0.5.2 下直接可用

## 7. 风险与对策

| 风险 | 对策 |
|------|------|
| 模型不遵守转写契约（仍重写） | 保真校验兜住；feature flag `transcription_fidelity` 可关回 v0.5.1 行为 |
| 叙事型结构判定误判（非叙事被放行/叙事被打回） | 阈值保守（≥2 非Hook段）；误判率实测超预期 → ADR-2 重审（改判定或删例外） |
| prompts.rs format! 化引入拼接 bug | 快照测试锁定生成文本与 v0.5.1 原文逐字节一致（数值除外——本来就是要消除漂移） |
| 定点重写段落定位错位 | 用结构标签切分（既有 extract_section_tags 复用），不依赖行号跨模型稳定 |
| 改动波及 5 文件（历史模式） | Phase 划分+每组件独立五步流程+全量回归；本期后置目标：C1 落地后同类修复只动 rules.rs+校验器 |

## 8. ADR 决策记录

```
ADR-1：收敛-终稿断层以转写契约+保真校验修复
日期：2026-09-08 | 状态：Proposed
上下文：GUI 实测证明格式化=重新创作，收敛承诺不传递（P1，置信度 95）
决策：转写契约 + 保真校验 + 定点重写 + TRANSCRIPTION_ISSUE 回炉通道
理由：不引入新 LLM 调用（成本零增），把"重新创作"约束为"受控转写"；
      任务书"独立复核"思想的圆桌化落地
后果：正=空转打回减少、"专家说OK系统说NO"观感消除；负=歌词 diff 逻辑需维护
假设：模型能遵守转写契约 | 重审触发：实测打回率 >30% 或模型更换

ADR-2：参数区间规则闭环=消费现有死常量+RULE_REGISTRY 守护
日期：2026-09-08 | 状态：Proposed
上下文：DOUYIN_*/MODE_B_* 常量零消费者，提示词四处承诺无执行者（P1，置信度 90）
决策：两校验函数消费常量 + RULE_REGISTRY + 死常量守护测试 + 手写数字归零测试
理由：常量已存在（rules.rs:59-67），最小增量；守护测试防复发
后果：正=规则永不裸奔；负=新增规则需注册（一步成本）
假设：叙事型例外可用"≥2 非Hook段+归类说明"结构判定近似
重审触发：例外误判率明显，或任务书新项目给出更好的判定

ADR-3：诚实降级聚合进历史与导出
日期：2026-09-08 | 状态：Proposed
上下文：任务书 §5.6.1——降级会话不应与健康会话同等可信
决策：run 级 degraded 标志 + 导出头部完整度声明 + 历史徽章
理由：降级事件已存在（Retry/AuditResult/预算打断），只缺聚合与展示
后果：正=用户可辨识降级产出；负=HistoryEntry 加字段（serde default 兼容，零迁移）
假设：无 | 重审触发：无
```

## 9. 交付物清单

1. 代码：rules.rs（注册表+TERRITORY）、validator.rs（参数门+保真校验）、orchestrator.rs（阶段0注入+定点重写+冲突预检+degraded聚合）、prompts.rs（format!化）、roles.rs（转写契约+例外条款改写）、models/history.rs（degraded 字段）、前端（徽章+事件兜底）
2. 测试：单元 ~15 个新用例 + 守护测试 2 类 + 集成断言升级
3. 文档：本文件状态更新（Proposed→Accepted）、CHANGELOG、v0.5.2 发布
4. 验证记录：headless 四模式日志 + GUI 四模式实测报告（对齐本文件 §6.3 规程）

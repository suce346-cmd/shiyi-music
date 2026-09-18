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
| — | ⚠️ 2026-09-18 更正注记（第八批）：本行诊断当时**只对了一半**。D4（§3.4）确已落地冲突预检（`detect_revision_conflicts`）并把裁决指引注入汇总 **user** prompt，但主持人 **system 人设**仍写"不自己改细节，只做整合"——**同一职责两个对立口径**，user 侧指引被 system 抵消，冲突实际仍无人裁决。且并发的物理前提未除：`execute_review` 走 `join_all`，同轮角色只拿得到**轮前**修订快照（同轮互盲），兄弟修订在产生时不可见。第八批两腿齐下：讨论轮改**阵容序串行**（`round_peers` 同轮可见性，物理消除互盲）+ 裁决口径**单源锚点锁**（人设与注入同读 `rules::territory_rules_text()`，`ADJUDICATION_KEYS` 双载体锁定）。 | 见 §3.4 更正注记 |

### 1.2 与任务书的设计差距（用户"自相矛盾"感受的结构根源）

| 任务书原则 | 圆桌现状 | 差距定性 |
|-----------|---------|---------|
| §5.4 三重停止判据（硬上限+LLM 裁判+独立复核） | 有硬上限（3 轮+打回 2 次+预算熔断）✅、有 LLM 裁判（agree 结构化✅），独立复核只覆盖格式不覆盖内容承诺 | 缺一条腿 |
| 单活跃发言者（冲突物理消除） | 多角色并行修订同一 target，靠 LLM 事后整合 | 冲突被推迟而非消除 |
| — | ⚠️ 2026-09-18 更正（第八批）：**已达成**。讨论轮由 `join_all` 并发改为**阵容序串行**（`roles` = `steps_for_mode` 权威单源），同一时刻至多一个角色审改；且第 k 个角色的 prompt 携带前 k-1 个角色的**本轮**修订（`round_peers` 同轮可见性），冲突在产生时即被看见、可明确反对。前端"多专家并发"叙事（横幅 + i18n + 3 项用例）随机制退役——`RoundtablePanel` 座位角标语义由"抢话"变为"轮到的当前发言人"。残留分歧仍由汇总阶段的冲突预检 + 领地裁决兜底（两条腿缺一不可）。 | |
| §5.4.1 收敛判定不得依赖自由文本语义判断 | agree=true 已是结构化 JSON（比任务书还早）✅ | 已达标，保持 |
| §5.6.1 诚实降级 | 只有 AuditResult(pass=false) 显示，无 run 级 degraded 聚合 | 部分达标 |
| 每条规则唯一执行者 | 参数类规则有承诺无执行者（上表 P1） | 核心差距 |

### 1.3 保留资产（明确不动）

- 硬校验器哲学（validator.rs:3"不信任 LLM 自检，格式类规则由代码裁决"）——实测拦截率 100%
- 全部角色方法论、模式指令、知识库 CSV、checklist 单源清单机制
- 预算熔断/检查点续跑/run_id 事件过滤/取消/插话/usage 追踪
- 四模式座位表、逐行字数对齐、K-3 Audio Influence=0 硬门、"80 字符说明行"闭环样板（CSV→常量→提示词→代码四环节）
  - ⚠️ 2026-09-18 更正：**80 上限已废止**（与"最强段≥5件且每件带行为动词"数学冲突，5/5 run 全被拦）。闭环样板保留，数字改为单源 `rules::DESC_LINE_MAX_CHARS = 200`，全模式统一。
  - 2026-09-18 闭环样板补强（第二批）：手写数字的**守护网**从手写枚举升级为「字面量网 + 常量派生『数值+单位』扫描网 + 阈值语境窗口门」（新增常量自动纳入）；未登记执行者的上游承诺同样视为降级源——`mode_d` 的配器/能量硬门文案已改写为设计方向（`validate_douyin` 不查这两项），并以 `mode_d_prompt_has_no_unenforced_instrument_or_energy_placeholders` 锁定。**规则闭环 = 数字单源 + 上游告知 + 下游执行者三者同域**。

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

**五类守护测试**（防复发的关键）：
1. **死常量守护**：遍历 RULE_REGISTRY 中引用的每个常量名，断言其在 src 下有非 rules.rs 的引用（消费点）——死常量在 CI 即失败。
   - ⚠️ 2026-09-18 第三批升级：消费点检查**不足以**证明执行者真的被执行——旧实现用 `src.contains(executor)` 文件全文文本匹配，函数"存在但无人调用"照样过关（实测 `douyin_bpm_min` 错挂到 `check_style_prompt_blocks`）。现改为**调用点存在性**校验：`has_call_site` 匹配 `executor(` 调用表达式，排除 `fn executor(` 定义与注释行；符号消费检查保留。自检用例 `has_call_site_distinguishes_definition_from_invocation`。
   - 残留边界：调用点校验能发现"无调用/只定义"，但**不能**发现"执行者调用点存在却不消费本规则常量"的语义错挂。同类错挂须靠"每条规则唯一执行者 + 模式域单源常量"人工评审兜底（如 `style_prompt_max/min` 的模式域由 `rules::STYLE_PROMPT_*_MODES` 单源锁定）。
2. **表述生成**：prompts.rs/roles.rs 中的数值段改为 `format!("...{}...", rules::X)` 后，源码中不再允许出现手写的关键数字字面量（`"12-20"`、`"20-35"`、`"75-85"`、`"85-95"`、`"≤80"` 等 grep 断言，白名单：注释与测试）
   - ⚠️ 2026-09-18 第八批升级（扫描面单源化）：旧网只扫 `prompts.rs`/`roles.rs` **两个源文件**，而同类上游告知载体 `CHECKLIST_A–D`/`PRIMER_AB/C/D` 就在 `rules.rs` 内，**漏在网外**——`CHECKLIST_D` 抖音区间、`PRIMER_D` 的 Hook/Verse/字数/BPM 四阈值、`PRIMER_AB` 的弱强差/配器差/全曲上限全是**无锁项**，改常量即静默漂移。
   - **修法**：四清单与三 primer **全文占位符化**（`${STYLE_PROMPT_MAX}`/`${MIN_ENERGY_GAP}`/`${HOOK_MIN}次`/`${DOUYIN_BPM_MIN}`/`${ARC_INLINE}` 等），`checklist()` 出口统一 `interpolate`（返回 `String`）；扫描面由 `ProseCarrier`/`PROSE_CARRIERS` **声明式**给出（**新增载体必须登记**，否则守护测试 `no_handwritten_rule_numbers_in_prose_carriers` 无从覆盖）；数值权威在 CSV、无常量可派的行（`PRIMER_AB` 的"≥5 件具体物"，权威在 `lyric_craft::LC-01`）走 `csv_locked` 豁免并**必须写明锁定测试名**——且豁免做**双向**校验：未登记字面量即红、登记了却无实际命中即红（防无主豁免随时间腐化）。
   - **分工**：`no_handwritten_rule_numbers_in_prompt_sources`（两个源文件）+ `no_handwritten_rule_numbers_in_prose_carriers`（`PROSE_CARRIERS`）**合起来才是完整守护网**；红灯先行自检 `prose_carrier_guard_catches_handwritten_restore`（把占位符还原为手写字面量必须被网住）。
3. **知识注入域守护**（2026-09-18 第四批新增）：注入层与 prompt 要求层必须同源，防"角色被要求核查内容不在上下文里的规则"（#16 悬空引用）与"打分恒 0 致排序退化为文件行序"（#15）复发。
   - **单源**：`rules::CRAFT_REFS_*`（角色 prompt 点名要求核查的 LC-/CC- 编号 = 注入必须强制投递的编号）；`orchestrator::INJECT_MAX_CRAFT_ROWS`（注入条数上限）。
   - **三向锁**：`roles::craft_refs_match_prompt_and_single_source`——① `Role.craft_refs` 集合 == prompt 文本中出现的 LC-/CC- 编号集合；② 绑定 `lyric_craft`/`compose_craft` 表 ⇔ 声明了非空引用契约（无表不得声明、有表必须声明）；③ 每个编号在对应 CSV 中真实存在，且 trigger 通过**唯一注入门** `knowledge::craft_inject_gate`（2026-09-18 第七批起：阶段域 + 条件域**双域**判定，与渲染层同一函数；角色**真实出场的全部模式**（名册取自 `steps_for_mode` 权威单源）都要能命中该编号）；④ `craft_refs.len() <= INJECT_MAX_CRAFT_ROWS`（否则必达行本身撑爆上限）。
   - **端到端锁**：`orchestrator::craft_referenced_ids_always_injected_for_every_role`——5 个 craft 角色在最坏输入（plan 为占位符、相关性全 0）下，每个点名编号的表格行仍必须出现在注入文本内；`knowledge::craft_required_rows_force_injected_beyond_cap`（上限小于必达数时必达行不被截断 + 头部计数）、`knowledge::craft_rows_ranked_by_plan_overlap_not_csv_order`（相关性可令尾部行上浮挤出首行，证明不再是文件行序）。
   - **残留边界**：相关性评分是"行语义列 ↔ 方案"的 CJK 二字组字面重合度启发式——对同义改写（方案说"押韵"、规则写"韵脚"）可能低分，但**被点名行不受影响**（必达优先），且条数上限之外的漏召回属可接受降级（有 `tracing::warn!` 与注入头计数可见）。
4. **注入门双域守护**（2026-09-18 第五批新增，**第七批升级为双域模型**）：trigger 是"/"分隔的多标签列，注入门必须按**标签**判定（子串匹配会漏标签、也会误命中），且**"声明适用范围"必须就是"实际注入范围"**——根治三族静默不可达："标签语义无单源、无守护"（#17/#18）、"条件修饰标签只登记不执行"（#29）、"阶段标签零消费者"（#23）。
   - **单源（双域）**：**阶段域**（析取适用）`rules::CRAFT_TRIGGER_ACTIVE`（审改·全程）、`CRAFT_TRIGGER_HOST_ONLY`（阶段0）、`CRAFT_TRIGGER_RESERVED`（扩展位，非注入）+ `CRAFT_RESERVED_ROWS`（预留位 id+理由登记）；**条件域**（**限定**，叠加在阶段域之上，行内条件析取）`rules::CRAFT_CONDITIONAL_TAGS`（抖音→mode_d / A→mode_a / B→mode_b / C→mode_c / 制作→producer）+ `CraftConditionalScope`；模式域取值单源 `ALL_MODES`。旧常量 `CRAFT_TRIGGER_CONDITIONAL` 第七批已删除（grep 零残留）。
   - **消费者登记**：`rules::CRAFT_STAGE_CONSUMERS`（`host_stage0` = 阶段0 / `reviewer` = 审改·全程）+ `stage_consumer(name)`——**登记即被运行期消费**：`orchestrator::host_craft_injection`（唯一调用点 `host_system_with`，阶段0 初稿与阶段1 汇总/回炉共享）与 `inject_knowledge` 都从登记取上下文，调用点不得自造标签集合（#23 的解药）。
   - **唯一门共用**：`knowledge::craft_inject_gate(trigger, &CraftInjectCtx { consumer, stage_tags, mode, role })` 同时被渲染层、守护测试、角色引用契约调用——**测试口径 == 运行口径**；渲染入口 `render_craft_table` **缺上下文直接 Err**（craft 表无法绕过门）；`trigger_tags_intersect` 降级为"阶段域子判定"，仅供预留位登记检查与守护测试。
   - **词表锁**：`knowledge::craft_trigger_vocabulary_has_consumers_and_real_scope`（第七批更名自 `craft_trigger_tags_are_registered_vocabulary`）——① CSV 标签 ⊆ 词表；② 每个阶段标签至少有一个消费者上下文、且消费者只能消费阶段域标签（防"词表可见、执行零消费者"）；③ 每个条件标签的判定域取值真实且该标签确被 CSV 使用（防"登记即死条目"）。
   - **精确匹配锁**：`knowledge::craft_trigger_gate_is_tag_exact_not_substring`（纯函数六态）；`knowledge::craft_full_process_rows_are_injectable`（"全程"行 CC-23/CC-28 必须出现在渲染文本；第五批红灯先行实测：把 `CRAFT_TRIGGER_ACTIVE` 收窄回 `["审改"]` 三项测试即红）。
   - **限定锁（红灯先行）**：`knowledge::craft_conditional_gate_is_restrictive_gate`——抖音行只进 mode_d、制作行只进 producer、A/B/C 行不进 mode_d、`role=None` 遇角色域条件 fail-closed；**第七批实测**：把门末行改 `true`（模拟条件域零门）→ 本测试 + 死行审计双红，复现 #29。
   - **死行审计**：`orchestrator::craft_rows_reachable_in_every_real_context`——每条非预留 CSV 行必须在至少一个**真实上下文**被渲染（4 模式主持人阶段0 × **真实装配入口** `host_initial_system`；4 模式 × `steps_for_mode` 角色），并同时校验条件域限定与阶段隔离；**第七批实测**：把 `host_craft_injection` 短路成空 → 本测试 + `host_stage0_injects_only_stage0_rows` 双红，复现 #23（该演练暴露了"只走渲染层会漏检装配未接线"的盲点，已改为走真实装配入口）。
   - **阶段0 锁**：`orchestrator::host_stage0_injects_only_stage0_rows`——四模式 × 两阶段（初稿/汇总）必须含 LC-01/LC-12/CC-24，且不得混入纯审改/条件域行。
   - **双载体同源锁**：`rules::primer_ab_stage0_threshold_matches_lyric_craft_row`——primer 与知识库是阶段0 的两个载体，primer 重申的数值必须等于 CSV（LC-01 物件清单下限），防"改 CSV 不联动 primer"。
   - **预留位锁 / 运行期通道**：`knowledge::craft_reserved_rows_match_registry`——CSV 中"扩展位"行集合必须 == `CRAFT_RESERVED_ROWS` 登记集合，且每条登记必须有理由；新增预留行未登记即红（把"预留"从隐式约定变成显式决策）。`knowledge::warn_reserved_rows_once`——命中"扩展位"即告警（每 (表,行集合) 每进程一次），覆盖用户**覆盖目录**新增预留行这一编译期测试覆盖不到的场景。
   - **扩展约定**：新增条件标签必须同时登记域取值（否则词表锁 ③ 即红）；新增阶段标签必须同时登记消费者（否则词表锁 ② 即红）——两者都是"登记即被校验"的闭环。
5. **知识可达性守护**（2026-09-18 第六批新增）：知识行的**归属与检索**必须有单源、有登记、有审计——根治"规则只绑给输出角色、审查角色 0 条"（#19）与"检索键被长度门静默丢弃致整行永久休眠"（#22）。
   - **规则职责单源**：`rules::SUNO_RULES_FOR_{EMOTION,LYRICIST,REVISER,PRODUCER,STYLE_ANALYST}` + `SUNO_RULE_OWNERS`（规则名→职责角色，40 条全覆盖；校验员保持全量不入 owner 表）。
   - **三向锁**：`roles::suno_rules_bindings_match_role_ownership`——① CSV 每条规则必须登记职责、登记不得悬空；② 角色常量集合 == 注册表中归属该角色的集合；③ 角色**实际绑定的行子集** == 对应常量（防止 roles.rs 引用漂移）；④ 端口干道断言（校验员全量、主持人零表）。
   - **检索契约单源**：`rules::KEYWORD_TABLES`（`KeywordTableReachability`：表名+检索键列+条数上限）+ `KEYWORD_MIN_CHARS` + `ENERGY_GATE_COLUMNS`；`orchestrator::inject_knowledge` 的 `_` 分支查单源路由（**新增关键词表只改单源**），`INJECT_MAX_KEYWORD_ROWS`/`INJECT_MAX_STYLE_GENRE_ROWS` 为单源别名。
   - **可达性审计**：`knowledge::reachability_violations()`——空/短于 `KEYWORD_MIN_CHARS` 的检索键行 + instruments 能量区间非数值 / min>max / 列缺失；`keyword_tables_have_no_structurally_unreachable_rows` 断言真实 CSV 审计为空；红灯先行 `reachability_audit_detects_single_char_key_and_bad_energy` 证明审计真能抓死行。
   - **运行期通道**：`knowledge::warn_reachability_violations()`（每表每进程一次）+ `orchestrator::warn_keyword_table_all_dormant_once`（整表零命中告警，含行数）——覆盖用户**覆盖目录**场景。
   - **预算重标定**：`orchestrator::role_injection_budget_under_limits` 为注入总量封顶的实测锁（第六批扩绑后制作人 4969 字 → `INJECT_MAX_TOTAL_CHARS` 5200→5400）；**任何扩大注入面的批次都必须重跑并复核该封顶与注释**。

**例外条款退役**：roles.rs 四处手写的"叙事型可回落 A/B 弧线区间，须说明理由"改为一句"叙事型回落规则由代码执行（DOUYIN_WEIRD/STYLE 区间外需满足叙事型结构判定）"——删除与代码行为不一致的旧表述。

### 3.3 D3：阶段 0 纳入单源

- orchestrator.rs `run_host_initial`（:712-741）：`system.push_str(crate::rules::checklist(req.mode.to_str_name()))` —— 与审改员/校验员同口径（一行）
- prompts.rs 四个模式指令中的数值段（350 字符/12-20/85-95/配器 3-7 件/能量差≥3/说明行 80 等）改为 `format!` 运行时拼装引用 rules 常量——手写数字从源头消灭（守护测试见 3.2）
  - ⚠️ 2026-09-18 更正：上句"说明行 80"已过时，现行单源 `rules::DESC_LINE_MAX_CHARS = 200`（经 `${DESC_LINE_MAX}` 占位符插值，四模式同门）；其余数值段结论不变。
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

> ⚠️ 2026-09-18 第八批更正注记（D4 的落地缺口与补法）
> **缺口**：D4 只做了"user 侧注入裁决指引"这一半——`territory_adjudication_text` 要求主持人"按领地声明裁决、注明取舍理由"，而主持人 **system 人设** 仍写"**只做整合，不自己改细节**"。同一职责两个对立口径，裁决指引被 system 抵消 → 冲突实际仍无人裁决；且无任何守护测试。
> **补法（两腿缺一不可）**：
> 1. **口径单源**：`rules::territory_rules_text()`（`TERRITORY_RULES` 的唯一 prose 形态）→ `${TERRITORY_RULES}` 占位符 → 主持人人设；`territory_adjudication_text` 同读这一份。人设 item 2 改写为"**不自己发明细节**（细节仍归专业角色）+ **冲突必须裁决**（按领地声明判定归属、采用终裁者写法、NOTES 注明取舍理由）"。
> 2. **双载体锚点锁**：`rules::ADJUDICATION_KEYS = ["领地声明","裁决","取舍理由"]`，测试 `adjudication_language_single_sourced_across_host_persona_and_injection` 断言**主持人人设（interpolate 后）与冲突注入**两个载体都必须命中三锚点，并反查旧口径"只做整合/不自己改细节"零残留、人设领地声明与 `territory_rules_text()` 逐字同源。
> 3. **前提修复**：并发的 `join_all` 使同轮修订互不可见——讨论轮改**阵容序串行** + `round_peers` 同轮可见块（`build_role_review_user_prompt` 纯函数，三段分工：当前方案 / 本轮同轮修订 / 往轮修订），冲突在**产生时**即可被看到与反对；`discussion_round_review_is_serial_with_same_round_visibility` 以源码形状锁"阵容序串行要素齐备、`join_all`/`review_futs` 零残留"。

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
| 讨论轮并发（`join_all` + `index*2s` 错峰） | **退役**（第八批，改阵容序串行） | 旧并发测试 `concurrent_reviews_preserve_order_and_fail_fast` 删除，替换为串行语义 + 同轮可见性守护；前端并发叙事（横幅 / `round.parallel.hint` / 3 项用例）同批收回 |
| 冲突"事后整合"口径 | **退役**（第八批，改"必须裁决 + 注明取舍理由"） | 主持人人设与 `territory_adjudication_text` 同读 `territory_rules_text()`，`ADJUDICATION_KEYS` 双载体锁定；旧口径"只做整合/不自己改细节"零残留断言 |
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
| 死常量守护 | 每个 RULE_REGISTRY 常量在 src 有非测试消费者；执行者须有**调用点**（`has_call_site`，排除定义/注释）——删掉执行点或不接线 CI 即红（2026-09-18 第三批由文本匹配升级为调用点校验） |
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
日期：2026-09-08 | 状态：Accepted（2026-09-08，C1-C5 已落地，提交 10913b6/d5202d2）
上下文：GUI 实测证明格式化=重新创作，收敛承诺不传递（P1，置信度 95）
决策：转写契约 + 保真校验 + 定点重写 + TRANSCRIPTION_ISSUE 回炉通道
理由：不引入新 LLM 调用（成本零增），把"重新创作"约束为"受控转写"；
      任务书"独立复核"思想的圆桌化落地
后果：正=空转打回减少、"专家说OK系统说NO"观感消除；负=歌词 diff 逻辑需维护
假设：模型能遵守转写契约 | 重审触发：实测打回率 >30% 或模型更换

ADR-2：参数区间规则闭环=消费现有死常量+RULE_REGISTRY 守护
日期：2026-09-08 | 状态：Accepted（2026-09-08，C1-C5 已落地，提交 10913b6/d5202d2）
上下文：DOUYIN_*/MODE_B_* 常量零消费者，提示词四处承诺无执行者（P1，置信度 90）
决策：两校验函数消费常量 + RULE_REGISTRY + 死常量守护测试 + 手写数字归零测试
理由：常量已存在（rules.rs:59-67），最小增量；守护测试防复发
后果：正=规则永不裸奔；负=新增规则需注册（一步成本）
假设：叙事型例外可用"≥2 非Hook段+归类说明"结构判定近似
重审触发：例外误判率明显，或任务书新项目给出更好的判定

ADR-3：诚实降级聚合进历史与导出
日期：2026-09-08 | 状态：Accepted（2026-09-08，C1-C5 已落地，提交 10913b6/d5202d2）
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

# shiyi-music 施工图 v2（2026-10-20，取代 BLUEPRINT.md）

> 基线：HEAD `766e747`。输入：REPORT.md（26 条工程问题）、scan.md（M-01..M-12）、2026-10-20 复审（R-01..R-12）、用户第三轮拍板。
> 本文件只是施工图；未改任何代码。所有 file:line 为 2026-10-20 只读核对值，施工前以 `grep -n` 复核。

## 0. 拍板与范围（第三轮，覆盖 v1 的 Q2/Q4/Q5/Q7）

| # | 决定 | 对 v1 的影响 |
|---|---|---|
| D1 | **自用**，不对外分发 | C-08/C-12/D-02 降为"最小修复+文档声明"，不做签名/公证/更新器；D-08 Release 移出 |
| D2 | 角色模型多为国产 LLM，**全部支持 JSON 输出** → 用原生结构化输出（`json_schema` 优先，`json_object` 次之，能力探测），**修复重试不作主路径** | 替换 v1 的"repair ≤2" |
| D3 | **只做 Suno**；删除 Mureka/海绵/Qwen/天音与跨平台转换 | M-06 关闭为"不做"；v1 B2 整批删除；`Platform` 枚举不建 |
| D4 | 旧格式**一刀切**，不留 `PROMPT_PACK_IR=0`，也删除现有 `PLAN_ENVELOPE_CONTRACT` 开关 | 退役清单见 §5 |
| D5 | 旧历史/检查点是测试垃圾，**不兼容，删除** | B-03/C-09/D-03 的"迁移"部分改为"清空+新 schema 版本" |
| D6 | 测试可**只读**真实钥匙串 Key；**永不写/覆盖/删除** `global` 与 `role:*` | A-01 规则定义见 §3.1 |
| D7 | 用户有 Suno 账号，**手动**执行探针（§7） | B0.5 改为 P2 用户探针 |
| D8 | 清理仓库无关垃圾（§8） | 新增 P1-J |
| D9 | D 模式按 2026 抖音调研**重设计**（§6） | 现有 DOUYIN_* 常量整体退役重建 |
| D10 | 8 份旧计划文档与移动端图标**直接删除**（不归档），删前零引用检查 | §8、P1-J |
| D11 | D 模式加**场景选择按钮**：系统推荐、用户选；未选用默认 `daily`（日常 Vlog，100–120 BPM） | §6.3 |
| D12 | **Suno Style 栏恒为英文**；仅当无对应英文术语时保留中文，**与 output_lang 无关** | §3.7；output_lang 改为只管歌词语言 |
| D13 | `output_lang` 收敛为 **zh/en 两态**（歌词语言）；退役 `mix`、`wants_style_en`、`wants_all_en` | §3.8 迁移、§5.1 #14 |
| D14 | Windows Store 图标 `Square*Logo.png`（10 个）与 `StoreLogo.png` 一并删除（零引用检查） | §8、P1-J |

非目标：音频生成、音频盲评、生成 API、多平台、Album Mode、圆桌架构重写。

## 1. 复审勘误（v1 → v2 已修正）

| ID | v1 错误 | v2 修正 |
|---|---|---|
| R-01 | B0 验收 `security add-generic-password ... -a global -w DUMMY` **会覆盖用户真实 Key**（BLUEPRINT.md:167、REPORT.md:464） | 验收只读：测试前后各取 `global` 的 SHA-256 指纹比对（§3.1 V-A01）；任何命令不得写 `global` |
| R-02 | 未处理"审计已删掉用户 Key" | P0-0：用户确认 Key 已重录（只读查存在性） |
| R-03 | B-01 根因漏项：`enforce_envelope` 降级时 `Ok(plan)` 丢弃了第二次重写（orchestrator.rs:~1159），且文案"保真校验降权"(:1157) 与实际"跳过"（validator.rs:584-586）矛盾 | 根因写全；新架构下该函数整体退役 |
| R-04 | "Variety Low / 控制双 take 方差" | 官方 v6 FAQ：Variety 会**改写 Style 提示词**，要保留标签设 **0**（https://help.suno.com/en/articles/13924481）。默认输出 `Variety: 0` |
| R-05 | Style 1,000 / Lyrics 5,000 / Exclude 1,000 被称"v6 字段" | Suno **从未官方公布**上限；标为 `measured`（HookGenius 2026-09-09 读计数器与 maxlength；APMonster 2026-09-17 读表单），P2 由用户实机复核 |
| R-06 | Mureka 数字/六段式无出处 | 平台已删除，不再涉及 |
| R-07 | "B4 14 条"(v1:378) vs 13 条 | 计数以 §9 映射表为准 |
| R-08 | R8 行被注释切断表格(v1:341-343) | §4 rubric 表连续 |
| R-09 | 路径 `src/commands/history.rs`(v1:203) | 应为 `src-tauri/src/commands/history.rs` |
| R-10 | judge 校准"20% 抽检" vs scan"150–250 条" | 自用场景：人工标 40 条校准、≥80% 一致才采信，报告标"小样本" |
| R-11 | C-01 只缓解未找根因 | P0-5 先做根因调查 |
| R-12 | 无旧规则退役线、无可合并标注、无风险栏 | 本文 §5、每步含"风险/可合并" |

## 2. 目标架构

```
输入(mode, user_input, original_lyrics, output_lang)
  → Host:   PlanIR      (结构化输出, schema=plan_ir.v1)
  → 角色审改/校验员讨论（读写 PlanIR 片段，JSON patch 风格 revisions）
  → Auditor: SongPlan   (结构化输出, schema=song_plan.v1)
  → validate(mode, SongPlan)   纯函数、单源规则
  → render_suno(SongPlan)       纯函数 → {style, lyrics, exclude, settings, title}
  → 前端：分字段复制（Style / Lyrics / Exclude / Settings）
```

### 2.1 SongPlan（参照 ElevenLabs composition plan，渲染到 Suno）

ElevenLabs Music 官方 schema：`positive_global_styles` / `negative_global_styles` / `sections[{section_name, positive_local_styles, negative_local_styles, duration_ms, lines}]`（https://elevenlabs.io/docs/api-reference/music/compose-detailed）。本项目沿用其"全局/段落 × 正向/负向"四象限，但**只渲染到 Suno**。

```rust
// src-tauri/src/plan/mod.rs（新，唯一定义点；serde + schemars 生成 JSON Schema）
pub struct SongPlan {
    pub schema_version: u32,               // = 1
    pub title: Option<String>,
    pub global: GlobalStyle,
    pub sections: Vec<Section>,
    pub settings: SunoSettings,
    pub mode_meta: ModeMeta,               // 各模式专属字段（§2.3）
    pub notes: Option<String>,             // 不渲染给 Suno
}
pub struct GlobalStyle {
    pub genre: Vec<String>,                // 六层：1 genre/era
    pub bpm: Option<u16>, pub key: Option<String>, pub meter: Option<String>,
    pub vocal: Vec<String>,                // 2 vocal（引用 vocal_mix.csv id）
    pub instruments: Vec<String>,          // 3 instrumentation & performance
    pub arc: Option<String>,               // 4 arrangement/energy
    pub mix: Vec<String>,                  // 5 production & mix（引用 vocal_mix.csv id）
    pub negative: Vec<String>,             // → Exclude
}
pub struct Section {
    pub kind: SectionKind,                 // Intro|Verse|PreChorus|Chorus|Hook|PostChorus|Bridge|Interlude|Drop|Breakdown|Outro|End
    pub label: String,                     // "Verse 1"
    pub positive_local: Vec<String>,       // → [Label | cue] 或 (cue)
    pub negative_local: Vec<String>,       // Suno 无段落级 Exclude：渲染为正向改写("no drums"→"drums drop out")；**不**并入全局 Exclude（避免段级否定误伤全曲）
    pub energy: Option<u8>,                // 1–10；A/B 必填
    pub instruments: Vec<String>,          // 段落在场乐器（B-02 / plan::validate 门只数 len()）
    pub minimal: bool,                     // 极简段声明：仅 Intro|Outro|Bridge|Interlude 且 energy≤2 可 true（b02 结论 2）
    pub target_seconds: Option<u16>,       // 仅 D 模式必填，用于时间预算校验
    pub lines: Vec<String>,
}
pub struct SunoSettings { pub weirdness: u8, pub style_influence: u8, pub variety: u8 /*默认0*/, pub vocal_gender: Option<VocalGender>, pub audio_influence: Option<u8> }
pub struct ModeMeta { pub lyrics_lang: LyricLang } // Zh|En；与 §3.8 IPC output_lang 同型（唯一定义点 plan::LyricLang）
```

### 2.2 结构化输出与能力探测（D2）

- 定义点：`src-tauri/src/commands/llm.rs` 新 `call_llm_structured(schema: &Schema, ...)`；能力表 `src-tauri/src/llm_caps.rs`（新）。
- 探测顺序：① 用户在角色设置里显式选择 `json_schema|json_object|none`；② 未选 → 设置页"测试连接"时发送最小 `response_format:{type:"json_schema",strict:true}` 请求，成功即记 `json_schema`，400 则试 `json_object`；结果按 `(base_url, model)` 缓存到 settings。
- `json_object` 档：schema 文本放入 system prompt + 本地 `serde_json` 反序列化 + `jsonschema` 校验。
- 失败语义：反序列化或 schema 校验失败 → `Failed{kind=Parse, detail=<JSON 路径+错误>}`，**不静默降级、不自由文本兜底**。允许 1 次"同请求重发"（网络/截断类，非修复提示）。
- 流式：结构化阶段关闭 token 级正文流，改发 `PhaseProgress` 事件（圆桌面板显示"主持人正在输出方案…"+ 解析后结构化预览）；讨论发言仍可流式（非 JSON 阶段）。
- 风险：国产网关 `json_schema` 实现不一致（字段忽略 `strict`）→ 本地二次 schema 校验必做。

### 2.3 各模式硬规则在 IR 中的落点

| 模式 | 规则（单源常量位置） | IR 字段 | 校验函数（新 `src-tauri/src/plan/validate.rs`） |
|---|---|---|---|
| 全模式 | Style 字符上限（`SUNO_STYLE_MAX`=1000, measured）、Lyrics 上限 5000、Exclude 上限 1000、标签数 8–15 | 渲染结果 | `validate_render_budget` |
| 全模式 | 至少 1 vocal + 1 mix 词表项 | `global.vocal/mix` | `validate_vocal_mix_coverage` |
| 全模式 | Exclude 不得与正向冲突；Style 不得含否定词 | `global.negative` vs 正向 | `validate_polarity` |
| 全模式 | Variety=0 默认；Weirdness/Style Influence 区间 | `settings` | `validate_settings(mode)`（区间沿用 `ARC_PARAMS`/`MODE_B_*`） |
| 全模式 | 歌词语言（zh/en，D13，迁移见 §3.8）；Style 恒英文（§3.7） | `mode_meta.lyrics_lang`（= `LyricLang`）+ IPC `output_lang`；Style 走 §3.7 | `validate_lang`（歌词语言）+ `validate_style_english`（§3.7，硬门在 validate，不在 schema description） |
| A | 能量差、配器差、段下限（极简 1 / 核心 2 / Chorus 类 3）、最强段≥4、极简段（`minimal`+kind 白名单+energy≤2） | `sections[].energy/instruments/minimal/kind` | `validate_arc_mode_a` / `section_floor`（取代 validator.rs 文本正则） |
| A | 保真：歌词 multiset 与用户输入一致 | `sections[].lines` vs `original_lyrics` | `validate_fidelity`（**始终执行**，不再因格式跳过） |
| B | 结构标签≥N、参数 B 区间 | `sections`, `settings` | `validate_mode_b` |
| C | 行数 + 逐行字数守恒（`validate_lyric_fill` 逻辑迁入）；韵脚咨询式 | `sections[].lines` vs 原词 | `validate_fill_mode_c` + `advise_rhyme`（拼音韵母，仅告警） |
| D | 见 §6 | `mode_meta.douyin` + `sections[].target_seconds` | `validate_douyin` |

## 3. 新规则定义（定义点 / 作用域 / 迁移 / 文档 / 守护）

### 3.1 RULE-KEYCHAIN-RO（A-01）
- 定义：`keychain.rs` 常量 `PROTECTED_ACCOUNT_EXACT = "global"` + `PROTECTED_ACCOUNT_PREFIX = "role:"`（判定 `is_protected_account`），测试隔离前缀 `TEST_ACCOUNT_PREFIX = "test:shiyi:"`；`cli_set/cli_delete` 在 `#[cfg(test)]` 下对受保护账户 `panic!`；测试读真实 Key 只走 `cli_get`。
- 测试账户：`test:shiyi:{pid}:{nanos}`，`Drop` guard 清理。
- 守护：`keychain_tests_never_write_protected`（扫描 `#[cfg(test)]` 源码文本中 `cli_set(`/`cli_delete(` 的实参不得是受保护字面量）+ 运行期 panic。
- **明文覆盖钥匙串（有意行为）**：用户在设置里改 Key 并成功 `keychain_set` 后，新值覆盖旧钥匙串条目；迁移路径仅当读回=明文才释放 hold（F1）。失败时明文留在 localStorage/hold，**不会**用哨兵覆盖唯一副本。
- 文档：README"开发"节一句话；CHANGELOG；本条。

### 3.2 RULE-STRUCTURED-ONLY（B-01）
- 定义：§2.2。作用域：Host/Auditor 两个产出口。守护：`grep` 测试断言 orchestrator 中不再出现 `envelope_`、`Degraded{flag:"envelope_fallback"}`。

### 3.3 RULE-SUNO-PROFILE
- 定义：`src-tauri/src/plan/suno_profile.rs`：上限、标签区间、Variety 默认、`fact: Measured{source,date}|UserProbed{date}`。前端经 `get_suno_meta` 读取，不在 TS 抄。
- 迁移：P2 探针结果回填 `UserProbed`。守护：`suno_profile_every_limit_has_fact`。

### 3.4 RULE-SCENARIO-VOCAB（C-02）
- 定义：`knowledge.rs` `enum Scenario`（登记全部取值）；CSV 加载期未知值 → 构建失败（测试）/ 启动日志 error。D 模式只吃 `douyin|all`。

### 3.5 RULE-HISTORY-V2（B-03/C-09/D-03）
- `history.json` 增 `schema_version: 2`；读到无版本或 v1 → 直接丢弃（D5）。写入串行队列 + 关闭前 flush；上限 200 条；检查点 TTL 7 天。

### 3.6 RULE-RUN-ID（C-05/C-06/D-06）
- 取消只认非空 run_id（空 id 返回错误）；`StreamChunk{run_id}`；run 结束 `remove`。
- 活动登记：`cancel::begin` 返回 RAII 守卫 `ActiveRun`（Drop = finish）；两个 run 入口（`run_pipeline`、`pipeline_resume`）只经 `cancel::with_active_run` 包裹，生产代码禁止裸调 begin/finish（守护：`run_entries_use_with_active_run`；future 被丢弃/超时 abort/panic unwind 均 finish：`guard_finishes_when_future_dropped`）。
- begin 之前的取消（前端已生成 run_id、后端未登记的竞态窗）记为 pending（TTL 30s = `PENDING_TTL`），begin 消费后立即处于取消态；TTL 内已结束 run 的迟到取消为 no-op。`request_cancel` 返回 `Cancelled|Pending|Noop`，`cancel_pipeline` 返回 `bool`（是否生效），前端 `cancelNotice` 显示"停止未产生效果"/失败原因（守护：`cancel_before_begin_is_consumed_by_begin`、`usePipelineGate.test.tsx` F3 组）。
- **F2 孤儿 future**：若调用方丢弃 `with_active_run` 返回的 future（或 `spawn_guarded` 超时 abort），`ActiveRun` Drop 仍 `finish`——活动表不泄漏；不保证业务 future 的取消点已被轮询到（协作式取消）。
- **F3 30s 边界**：pending/finished 条目超过 `PENDING_TTL` 即剪除；超时后同 id 的迟到"停止"视为 Noop（不再变 Pending），需用户再点一次或新 run。

### 3.7 RULE-STYLE-ENGLISH（D12）
- 定义点：`src-tauri/src/plan/style_lang.rs`（新）：`fn style_token_ok(token) -> Ok | AllowedCjk(term) | Violation`。
- 作用域：SongPlan 中所有渲染进 Suno Style/Exclude 的字段（`global.*`、`sections[].positive_local/negative_local`）；歌词 `lines` 不受约束；**不读 output_lang**。
- 回退词表：`src-tauri/knowledge/style_cjk_allow.csv`（新）`term_cn,reason,suggest_en,source,probe_id`：仅登记"无公认英文对应"的中文术语（初始候选：古风/国风细分如“戏腔”、民乐技法如“轮指”、地方曲艺如“评弹”“秦腔”等，逐条需 P2-S12 实测确认英文写法无效后才入表）。乐器类优先英文/音译（guzheng, erhu, pipa, dizi）不入表。
- 校验：`validate_style_english`（`plan/validate.rs`，**唯一硬门**）：Style/Exclude 中每个含 CJK 字符的 token 必须命中 allow 表，否则 Violation；命中 allow 表的中文 token 计入预算并在渲染时置于英文标签之后。
- 生成侧：**不**把 English-only 写进 JSON Schema 的 `description`/`format`——`plan/schema.rs::strictify` 会剥掉所有 schema 元键 `description`/`format`（只保留形状与取值域；且不得误删 `properties` 里名为 `description`/`format` 的字段定义）。字段语义由提示词 + 【SongPlan 字段契约】说明；allow 表的 `term_cn+suggest_en` 注入 system prompt。
- 迁移：见 §3.8（D13）。
- 守护：`style_english_rule_independent_of_output_lang`（zh/en 两种请求渲染同一 SongPlan，Style 字节一致）；`style_cjk_allow_rows_have_probe_id`；负路径：Style 含未登记中文即红。
- 文档：README"产出语言"节改为"歌词语言"；i18n 见 §3.8。

### 3.8 RULE-LYRIC-LANG（D13：output_lang 收敛 zh/en）
- 现状（只读核对）：Rust `PipelineRequest.output_lang: String`，文档注释三态（models/mod.rs:686）；`wants_all_en`(:700)、`wants_style_en`(:706-707，含 `"mix"`)、测试(:1022,1031,1041-1042)；`orchestrator.rs` 8 处引用（阶段指令拆分与"阶段2 mix 注入"，commit ae80448）；前端 `src/types/index.ts:31 outputLang?: "zh"|"mix"|"en"` 等 4 处、`usePipeline.ts` 1 处、`App.tsx` 8 处、`i18n.ts` 10 处（含 +mix 键，commit 636b2da）；集成测试 `headless_modes.rs`、`large_scale_matrix.rs` 各 1 处。施工前以 `grep -rnE '"mix"|outputLang|output_lang|wants_all_en|wants_style_en' src src-tauri/src src-tauri/tests` 复核。
- 新定义（**唯一定义点**）：`src-tauri/src/plan/mod.rs` `#[serde(rename_all="snake_case")] pub enum LyricLang { Zh, En }`（SongPlan `mode_meta.lyrics_lang` 已用；**禁止**再另立 `LyricsLang`）。IPC：`models` 对 `PipelineRequest.output_lang` **复用同一类型**（`pub use crate::plan::LyricLang`），必填、不再 serde 缺省；方法 `lyric_lang()`。前端 `type LyricLang = "zh"|"en"`，`AppSettings.outputLang: LyricLang`（必填）。字段名 `output_lang` 保留（IPC 稳定），仅值域收敛。
- 作用域：只影响歌词 `sections[].lines`、`notes` 与讨论层语言指令；Style/Exclude 恒英文（§3.7）。
- 迁移（按序，可合并为 1 个提交 `[可合并:§5.1#14]`）：
  1. Rust：引入 `LyricLang`；IPC 反序列化收到 `"mix"` → **报错**（D4 一刀切，不做静默映射）；`wants_all_en/wants_style_en` 调用点改为 `lyric_lang()==En` 或删除（Style 分支全部删，归 §3.7）。
  2. orchestrator：删除"阶段2 mix 注入"与 Style 语言分支；歌词语言指令按 LyricLang 二态注入；`zh` 空串零漂移不变。
  3. 前端设置：`useSettings.ts` 读取持久化设置时 `outputLang==="mix"` → 写回 `"zh"`（设置是用户偏好非测试垃圾，做一次性归一；`sanitizeStored` 单源处理）；下拉选项删 `mix`（App.tsx 设置面板）。
  4. i18n：删除 mix 相关键与 desc 三态文案，改两态；`i18nKeys.test.ts` 同步。
  5. IPC/类型：`types/index.ts` 值域与必填；`wireProtocol.test.ts` 锁 `output_lang ∈ {zh,en}`。
  6. 测试：删 `mix_wants_style_en_only`(models:1041) 等三态测试；新增 `output_lang_rejects_mix`、`output_lang_required`、`lyric_lang_does_not_affect_style`（与 §3.7 守护共用夹具）；`headless_modes.rs`/`large_scale_matrix.rs` 中 `make_request` 显式传 `zh`（参考 766e747 的教训）。vitest：`sanitizeStored.test.ts` 加 `mix→zh`、`settingsWiring.test.ts` 下拉仅两项。
- 守护：源码扫描测试 `no_mix_output_lang`（`src`、`src-tauri/src` 中不出现 `"mix"` 字面量作为 output_lang 值，`wants_style_en|wants_all_en` 零出现）。
- 风险：漏改某个 `make_request` 字面量致 CI 编译失败（历史上 3b26673/766e747 发生过）→ 必填字段让编译器兜底；用户旧设置为 mix → 归一为 zh。
- 验证：`cargo test --lib output_lang`、`cargo test --tests --no-run`、`npm test`、`npx tsc --noEmit` 全绿；守护扫描为空。
- 回滚：单提交 revert（设置归一不可逆但无害）。
- 文档：README"产出语言"→"歌词语言（中文/英文）"；CHANGELOG 注明"移除 mix（Style 恒英文）"。

## 4. 文本 rubric（8 维，0/1/2，Suno 专用）

| # | 维度 | 0 | 1 | 2 |
|---|---|---|---|---|
| R1 | Suno 语法合规 | 任一硬违规 | 1 处轻微 | 全合规 |
| R2 | 结构完整 | 缺主段/cue | cue 不全 | 主段齐、段段有 cue、收尾明确 |
| R3 | 人声层 | 无/空词 | 1 条泛化 | ≥2 条具体且分段变化 |
| R4 | 混音层 | 无 | 1 条泛化 | ≥2 条具体 |
| R5 | 具体性 | 套话 | 半可执行 | 全可执行 |
| R6 | 一致性/极性 | 风格/歌词/参数/Exclude 冲突 | 1 处轻微 | 无冲突 |
| R7 | 预算利用 | Style <25% 或超限；标签 <8 或 >20 | 25–60% 或标签有重复 | 25–90%、8–15 不重复标签 |
| R8 | 模式契合 | 违背模式目标（如 D 无前置 Hook） | 部分 | 完全契合 |

（R7 按 roo.beehiiv 2026 v6 审计建议 Style 250–600 字符 → 下限放宽到 25%。R8 取代 v1 的"跨平台可迁移"。）

## 5. 分期（修复 → 新规则 → 退旧规则）

格式：**位置 / 操作 / 风险 / 验证 / 回滚**；`[可合并:X]` 表示可与 X 同一提交/PR。

### P0 修复（不依赖新架构）

| 步 | 位置 | 操作 | 风险 | 验证 | 回滚 |
|---|---|---|---|---|---|
| P0-0 用户 Key 确认 | 用户本机 | 用户在 app 内重录 Key（若丢失）；我方只读 `security find-generic-password -s shiyi-music -a global >/dev/null; echo $?` | 无 | 输出 0 | — |
| P0-1 A-01 | `src-tauri/src/commands/keychain.rs:240-255` + RULE-KEYCHAIN-RO | 测试改隔离账户；加受保护 panic 与守护 | 隔离账户残留 → Drop guard | `security find-generic-password -s shiyi-music -a global -w \| shasum -a 256` 前后相同；`cargo test --lib keychain` 绿；`security dump-keychain \| grep -c "test:shiyi"` = 0 | revert |
| P0-2 C-04 | `src/hooks/usePipeline.ts:~664-683` | 抽 `buildRequest(kind)`，run/refine/resume 共用；`output_lang` 改必填类型 `[可合并:P0-3]` | 漏字段 → TS 必填拦截 | `tsc --noEmit`=0；新单测 resume 载荷含 output_lang | revert |
| P0-3 C-05/C-06/D-06 | `cancel.rs`、`orchestrator.rs`、`llm.rs`、`App.tsx`、`test_api` | RULE-RUN-ID（空 id 拒取消；`StreamChunk.run_id`；结束 remove） | 前端旧监听过滤失效 → `runIdWire` 测试锁 | `cargo test --lib cancel`；vitest `runIdWire` | revert |
| P0-4 C-07 | `llm.rs:583` | `Vec<u8>` 缓冲按 `\n` 切行后 `from_utf8`，残余字节留待下一 chunk `[可合并:P0-3]` | SSE 行跨 chunk | 夹具：中文 3 字节在任意切点拆分 → 无 U+FFFD | revert |
| P0-5 C-01 根因 | `package.json:10`、`vite.config.ts` | 调查：`vitest run --reporter=verbose --logHeapUsage`；分别测 `--pool=forks/threads`、`--maxWorkers=1..8`、`environment` 是否全局 jsdom；记录 `os.cpus().length` 与 worker 启动超时（`teardownTimeout/hookTimeout`）。得出根因后再写 `vitest.config.ts`（例：非 DOM 测试改 `environment:'node'`、`pool:'forks'`） | 只调并发掩盖竞态 | `npm test` 连跑 3 次 exit 0；根因写入 `docs/qa/vitest-root-cause.md` | revert config |
| P0-6 C-10 | `useSettings.ts`（`ensureInKeychain` / `PlaintextHold` / `persistNonSecrets(s, hold)`） | **F1 读回校验后才清**：明文仅当钥匙串读回值=明文时才可从 hold 释放并哨兵化；`persistNonSecrets` 对 hold 中未验证明文原样落盘；迁移失败保留明文+`secretErrors` 就地显示 `[可合并:P0-1]` | 钥匙串写入失败 → 明文保留、Key 仍可用 | vitest `useSettingsSecrets`：读回一致才清 / 读回失败保留明文 / 验证完成前写出不哨兵化 | revert |
| P0-7 C-11 | `tests/large_scale_matrix.rs:165,236` | 路径改 `env!("CARGO_MANIFEST_DIR")/../target/large-test`；测量改调 `collect_hard_issues`（orchestrator.rs:1960，改 `pub #[doc(hidden)]`——外部 tests crate 需可见）。**注：本步是过渡口径**，§5.1 #5 删除 `collect_hard_issues` 之前，矩阵必须先切到 `plan::validate`（见 #5 前置） | 读数变严 | 编译通过；离线 dry-run | revert |
| P0-8 C-13/C-14 | `CHANGELOG.md`、`docs/qa/` | 补 0.5.5 + Unreleased；删 `results_12round_final.jsonl`（与 v4 同 MD5 `0ddd1ecc…`）`[可合并:P1-J]` | 无 | `md5 docs/qa/*.jsonl` 无重复 | git revert |
| P0-9 B-02 归因（先于 IR 设计） | （历史 jsonl 已清理） | 对 11 例配器失败逐例归因，结论决定 §2.3 A 模式 `validate_arc_mode_a` 的门语义。**实施登记**：`validator::parse_section_instruments` 剥离英文 `energy:` 尾项、否定/退出项（`is_absent_item`）不计（F9 补：`double/triple stop(s)` 演奏技法除外） | 结论反复 | 结论已并入 `plan::validate`；validator 单测 | 单函数 revert |
| P0-10 C-08/C-12/D-02/D-07 最小修复（自用） | `keychain.rs`、`capabilities/default.json`、`orchestrator.rs`、`docs/security-self-use.md` | **C-08** 密钥改走 `security -i`（stdin，写后读回；格式门仅 CLI 路径，F5）；**C-12** DNS rebinding 记录为接受风险（不做 pinning）；**D-02** fs scope 收窄到 `$APPDATA/**`+`$DOWNLOAD/**`+`$DOCUMENT/**`；**D-07** 日志去掉 output_head | 导出到其他目录失败 → 对话框路径运行期授权 | 手测导出；`cargo test --lib`；`docs/security-self-use.md` 四项齐 | 逐项 revert |

### P1 新规则（新架构）

| 步 | 位置 | 操作 | 风险 | 验证 | 回滚 |
|---|---|---|---|---|---|
| P1-J 仓库清理 | §8 清单 | 删除/忽略垃圾 `[可合并:P0-8]` | 误删 → 每项先 `git ls-files`/引用 grep | `git status` 干净；构建绿 | git revert |
| P1-1 SongPlan 类型+Schema | 新 `src-tauri/src/plan/{mod,schema,suno_profile}.rs`（**不含** validate/render_suno，二者属 P1-4）；`Cargo.toml` 加 `schemars`、`jsonschema` | §2.1、RULE-SUNO-PROFILE | schema 过大使国产模型退化 → 字段精简、enum 化 | `cargo test --lib plan` ；schema 快照 | 新文件删除 |
| P1-2 结构化调用+能力探测 | `llm.rs`、新 `llm_caps.rs`、设置 UI `InputPanel/设置面板`、`types/index.ts` | §2.2 | 网关差异 → 本地校验兜底 | 离线 mock 网关三档（schema/object/none）测试；none 档明确报错提示用户换模型 | revert |
| P1-3 Host→PlanIR、Auditor→SongPlan | `orchestrator.rs`（产出口）、`prompts.rs`、`roles.rs` | 提示词改为"按 schema 填字段"；讨论 revisions 改结构化 patch | 讨论质量下降 → 40 例回放对比 | `cargo test --lib`；新快照；真模型 12 例 smoke | revert（无双轨，整批回退） |
| P1-4 validate + render_suno | `plan/validate.rs`、`plan/render_suno.rs` | §2.3；渲染：`[Label \| cue]`+歌词；`negative_local` 渲染规则；Variety 0 | Suno 对 `\|` 语法实测差 → P2 探针决定 `[Label] (cue)` 还是 `[Label \| cue]`，渲染器单点切换 | 固定 SongPlan → 渲染快照逐字节一致；预算越界负路径红 | revert |
| P1-5 vocal_mix.csv（M-07） | 新 `src-tauri/knowledge/vocal_mix.csv`、`knowledge.rs` | 9 行起步；id 被 SongPlan 引用；覆盖门 | 注入超预算 → 只注入 id+label_en | `cargo test --lib vocal_mix` | 删表 |
| P1-6 Scenario 词表（C-02） | `knowledge.rs:494-500`、`suno_rules.csv` | RULE-SCENARIO-VOCAB；17 行未知值重分类 | 重分类误判 → 逐行表入 PR 描述 | 未知值注入即红；D 只拿 douyin/all | revert |
| P1-7 D 模式新规则 | `rules.rs` 新 `DOUYIN_V2_*`、`plan/validate.rs::validate_douyin`、`prompts.rs` mode_d | §6 | 规则过严 → 先 smoke 12 例 | §6 验证 | revert |
| P1-8 C 韵脚咨询（C-03） | `plan/validate.rs::advise_rhyme`、`Cargo.toml`(`pinyin` crate) | 仅告警；README 改"字数硬校验、韵脚提示" | 多音字误判 → 只报告 | 已知押韵/不押韵样本 | revert |
| P1-9 发音提示（M-08）+ Persona 文本（M-05）+ Audio Influence 可选（M-12）+ series_context（M-11，可选） | SongPlan `notes`/`settings` | 只文本层 | 冗余 → 默认关闭 | 单测 | revert |
| P1-10 历史/检查点 v2 | `history.rs`、`checkpoint.rs`、`App.tsx:~258-280`（迁移块删除） | RULE-HISTORY-V2；存 `SongPlan`+渲染结果 | 无 | 单测：v1 文件被丢弃、v2 往返、关闭 flush | revert |
| P1-11 前端结果区 | `ResultPanel.tsx`、`types/index.ts`、`i18n.ts` | 四字段分块复制；Settings 显示 Variety 0 | 无 | vitest 组件测试 | revert |
| P1-12 D-01/D-04/D-05 | CI、`useSettings.ts:221-233`、`orchestrator.rs:712-742` | clippy -D warnings、ESLint、coverage；清除 Key 按钮（仅用户手动，删 `global` 需二次确认）；usage RAII 记账 | clippy 存量告警 → 先修（**已提前完成**，见 §5.2 A-2） | `cargo clippy --all-targets -- -D warnings`=0 | revert |
| P1-13 文本评估（M-09/M-10） | 新 `src-tauri/tests/text_eval.rs`、`docs/qa/` | 20 条冻结提示词 × 4 模式 × 2 次；judge 双模型 + 人工 40 条校准 | 成本 → 用户控制 | report 含分布 | 旁路 |

### P2 用户探针（与 P1-1 并行，P1-4 前完成）—见 §7

### P3 退役旧规则（§5.1 按序执行，每项"零调用方实证 → 删除 → 守护 → 文档"）

### 5.1 退役清单与删除顺序

零调用方实证命令统一：`grep -rn "<符号>" src-tauri/src src-tauri/tests src | grep -v "^.*//"`，结果须仅剩定义行（或为空）方可删。

| 序 | 符号 / 资产（位置） | 前置 | 守护（删后） | 文档同步 |
|---|---|---|---|---|
| 1 | `enforce_envelope`（orchestrator.rs:1102-1160）及调用点 | P1-3 | `no_legacy_envelope_symbols` 测试（源码扫描） | CHANGELOG |
| 2 | `plan_envelope_enabled`/`PLAN_ENVELOPE_CONTRACT`（rules.rs:2659,2853-2854） | 1 | 同上 | README 去掉环境变量 |
| 3 | `envelope_spec`(rules.rs:2747)、`parse_plan_sections`(:2771)、`envelope_defect_mode`(:2819)、`PlanSections`、相关测试(:3003 等) | 2 | 同上 | — |
| 4 | `check_transcription_fidelity[_impl]`（validator.rs:569-600） | `validate_fidelity` 上线 | 保真负路径测试迁移到 plan | — |
| 5 | 文本正则校验：`desc_line_length_issues`(validator.rs:157)、`param_range_issues`(:313)、`check_audio_influence`(:394)、`validate_for_mode`(:812) 的文本解析分支、`collect_hard_issues`(orchestrator.rs:1960) | P1-4 上线，且 `tests/large_scale_matrix.rs::measure` 已由 `collect_hard_issues` 切到 `plan::validate`（P0-7 的过渡口径先迁移、后删除；顺序反了矩阵即失去测量对象） | 门禁变异实验（REPORT §4 M1-M3）在新函数上重做，3/3 红 | REPORT 附注 |
| 6 | 常量：`STYLE_PROMPT_MAX_CHARS=350`(rules.rs:13)、`DESC_LINE_MAX_CHARS`、说明行相关 `${DESC_LINE_*}` 占位、`CHECKLIST_A/B/C/D`(rules.rs:671-674，含 `Audio Influence=0`)、"禁止手写裸数字"登记的 `≤350/≤200/≤80`(rules.rs:4,1257) | 5 | 手写数字守护改登记新常量 `SUNO_*` | CHECKLIST 文案 |
| 7 | 旧 D 模式常量 `DOUYIN_*`(rules.rs:33-118+, 逐段参考表 :133) | P1-7 | `douyin_v2_single_source` | prompts mode_d |
| 8 | `RULE_REGISTRY` 中指向已删执行者的条目（rules.rs:248-289） | 5-7 | 注册表"执行者调用点存在"自检（已有）保持绿 | — |
| 9 | 旧 prompt 快照 `src-tauri/tests/fixtures/prompts/mode_{a,b,c,d}.txt` → 重生成 | 1-8 | `mode_prompts_byte_identical_to_snapshot` | — |
| 10 | 前端：历史迁移块（App.tsx:~258-275）、`HISTORY_KEY/HISTORY_MIGRATED_KEY`、旧结果解析（说明行/信封展示） | P1-10/11 | vitest | — |
| 11 | `suno_rules.csv` 中与说明行/350/Audio=0 相关行；知识表中被新规则取代行 | 6 | 加载期校验 | — |
| 12 | 用户数据：`~/Library/Application Support/com.shiyi.music/history.json`、`pipeline_checkpoint_*.json` | P1-10 发布后，用户手动或首启自动丢弃 v1 | 首启日志 | — |
| 13 | 无主 `#[allow(dead_code)]`（budget.rs:22、logging.rs:112、knowledge.rs:228,235,279,506,548,1152、roles.rs:21）逐个判定：测试专用→`#[cfg(test)]`；无调用→删 | 任意 | `cargo build` 无 dead_code 告警（去掉 allow 后） | — |
| 14 | output_lang `"mix"` 值域、`wants_style_en`/`wants_all_en`(models/mod.rs:700,706) 及三态测试(:1022-1042)、前端 `mix` 选项/类型/i18n 键、orchestrator 阶段2 mix 注入 | P0-2、P1-3、§3.7 上线；按 §3.8 迁移 1→6 执行 | `no_mix_output_lang` + `style_english_rule_independent_of_output_lang` | README、CHANGELOG、设置面板文案 |

可合并：1+2+3 一提交；5+6 一提交；9 跟随最后一个改 prompt 的提交；14 可与 6 同提交。

### 5.2 蓝图修订记录

| 编号 | 日期（UTC+8） | 修订 | 理由 |
|---|---|---|---|
| A-1 | 2026-10-20 | **P1-3 与 P1-4 合并为一批实施**（Host→PlanIR、Auditor→SongPlan、`plan::validate`、`plan::render_suno` 同一提交序列落地、整批回退） | P1-3 删除 envelope/文本产出口后，终稿只能由 `render_suno` 从 SongPlan 生成、硬校验只能由 `plan::validate` 执行；单独落 P1-3 会让流水线无可用终稿，而 §3.2 规定"无双轨"，不允许保留旧文本路径过渡。真模型 40 例回放与 12 例 smoke 推迟到 Mac 同步后执行（脚本与验收标准见 `scripts/mac-verify.sh` / `docs/RELEASE.md`） |
| A-2 | 2026-10-20 | clippy `-D warnings` 门从 P1-12 **提前**：存量告警一次清零，`scripts/gates.sh` 末步 `cargo clippy --all-targets -- -D warnings`；`too_many_arguments` 阈值经 `src-tauri/clippy.toml` 暂置 11（编排重写时收拢为上下文结构体后下调）；P1-2 结构化调用函数在接线前以 `expect(dead_code, reason)` 标注（接线后 expect 失效即红，强制删除） | P0 审计：存量告警掩盖新告警（新增告警被计数淹没） |
| A-3 | 2026-10-20 | P0 审计补丁 F1–F9（明文 Key 读回校验、RAII run 守卫与 begin 前取消、格式门限 CLI 路径、run_id wire 锁、静态守卫防别名、`stop` 尾项）登记于 CHANGELOG Unreleased；§3.6 同步 | 审计发现 |

## 6. D 模式重设计（抖音，2026 调研）

### 6.1 调研结论
- 热门片段 15–20 秒，约 2 秒 intro 后直接副歌；副歌前置、几乎摒弃传统主歌；旋律"向上走"、悲伤词配轻快旋律；歌词场景开放、多用"你我他"（第一财经/腾讯新闻 2025-08，https://news.qq.com/rain/a/20250829A01YXT00）。
- 任何 >3 秒纯器乐前奏跳出率飙升；"抖音版"结构 Hook(0–15s)→短过渡→主歌片段→回 Hook；核心副歌 ≤20 字；BPM 分布 80–100(25%)/100–120(35%)/120–140(30%)；人声 2–5kHz 突出、低频勿过重、留空拍便于卡点（https://produsso.com/blog/yuanchuang-yinyue-douyin-zouhong-celue，厂商博客）。
- 副歌核心 2–3 句反复吟唱（界面新闻，https://www.jiemian.com/article/6463988.html）。
- Suno 侧：冷开副歌 / 2 小节 riser 直接进 Hook；Exclude 写 `long intro, slow build, fade out`；Variety 0（https://roo.beehiiv.com/p/best-suno-prompts-v6、https://songsmith.studio/blog/suno-7-second-hook-short-form、https://www.sun-cn.uk/en/blog/suno-music-for-short-video-15-second-hooks-to-60-second-loops/）；Hook 在 0:08 前落地（https://aitoolsguidebook.com/en/articles/tiktok-15s-hook-lyrics-prompts/）。
- 证据强度：E3（媒体/厂商/第三方），无平台官方数据；规则做成单源常量便于调整。

### 6.2 新 D 模式（替换现有 60–90s / Intro 7s / 行≤12 体系）

> **用户裁定（2026-10-20 21:35 Asia/Shanghai，覆盖下表默认值；单源 `plan::douyin::DOUYIN_V2_*`）**：成曲 **60–90 秒**；Intro 硬门 **≤3 秒**；Hook **10 秒内**落地；Hook 全曲重复 **≥2**（强化区间 2–4）；BPM 下限 80 不变。下文 45–75s / 2s / 8s / ≥3 仅为调研原始默认，以本裁定为准。
产出：**一首可完整生成的短歌（约 45–75 秒）+ 指定"可剪 15 秒片段"**。

SongPlan `mode_meta.douyin`：`{ scene: 卡点|情绪|剧情|舞蹈|国风, bpm_band, clip: {start_section, end_section, seconds:15}, hook_line, loop_tail: bool }`

| 规则 | 常量（rules.rs 新 `DOUYIN_V2_*`） | 默认 |
|---|---|---|
| 结构首段必须为 `Chorus/Hook`（冷开）或 `Intro`≤2s 紧接 Hook | `INTRO_MAX_SEC=2` | 冷开 |
| Hook 落地时间 ≤8s | `HOOK_LAND_SEC=8` | — |
| 核心 Hook 2–3 句、合计 ≤20 字，全曲重复 ≥3 次 | `HOOK_LINES=2..=3`、`HOOK_CHARS_MAX=20`、`HOOK_REPEAT_MIN=3` | — |
| 主歌可省；若有 ≤4 行 | `VERSE_MAX_LINES=4` | — |
| 每行字数 ≤12（保留旧上限，和音虚词豁免沿用） | `LINE_MAX_CHARS=12` | — |
| BPM 按场景区间 | 情绪 80–100 / 日常 100–120 / 舞蹈卡点 120–140 | — |
| 指定 15 秒片段段落可独立成立（无需上下文） | `clip` 字段必填 | — |
| 结尾：`loop_tail`（回到 Hook 首字）或硬停 `[End]` | 二选一必填 | loop |
| Exclude 必含：long intro, slow build, fade out | `DOUYIN_V2_EXCLUDE` | — |
| 混音：vocal-forward、phone-speaker friendly（低频适度） | vocal_mix id | — |
| Settings：Variety 0；Weirdness/Style Influence 区间待 P2 探针定 | provisional | — |

### 6.3 场景选择（D11）
- 场景枚举（单源 `src-tauri/src/plan/douyin.rs` `enum DouyinScene`，前端经 `get_douyin_meta` 读取，不在 TS 手抄）：`emotion` 情绪治愈 80–100 / `daily` 日常 Vlog 100–120 / `dance` 舞蹈卡点 120–140 / `story` 剧情反转 90–110 / `guofeng` 国风 90–130。BPM 区间来源 §6.1（Produsso，E3）。
- **默认**：`daily`（调研中占比最高 35%、适配面最广）。
- **推荐来源**：纯函数 `suggest_scenes(user_input) -> Vec<(DouyinScene, score, reason)>`，基于 `knowledge/douyin_scene_keywords.csv`（新，`scene,keyword,weight`）关键词打分，**不调 LLM**（即时、离线、可测）；返回前 3。无命中 → 只返回默认 `daily`。
- **UI 位置**：`src/components/InputPanel.tsx`，仅 Mode D 显示，位于灵感输入框下方一排 chip 按钮（推荐项带“推荐”角标，按得分排序；未选时 `daily` 呈默认高亮“默认”）；输入防抖 300ms 调 `suggest_douyin_scenes` 命令刷新推荐；用户点选后不再被推荐覆盖。新组件 `src/components/SceneSelector.tsx`。
- **IR 字段**：请求 `PipelineRequest.douyin_scene: Option<DouyinScene>`（缺省=Default）→ `SongPlan.mode_meta.douyin.scene`（必填，由后端写入，不让模型选）+ `scene_source: user|default`。Host prompt 注入场景对应 BPM/结构偏好；`validate_douyin` 校验 `global.bpm ∈ scene.bpm_band`。
- 测试：Rust `suggest_scenes_keywords`（“跳舞/卡点”→dance 首位；无命中→仅 daily）、`douyin_scene_default_is_daily`、`douyin_bpm_band_enforced`（越界红）、`scene_written_by_backend_not_model`；vitest `sceneSelector.test.tsx`（仅 D 显示、默认高亮、点选覆盖推荐、载荷含 douyin_scene）；`wireProtocol.test.ts` 锁字段名。
- 步骤：并入 P1-7（新增文件 `douyin.rs`、`douyin_scene_keywords.csv`、`SceneSelector.tsx`；改 `InputPanel.tsx`、`types/index.ts`、`usePipeline.ts::buildRequest`、`i18n.ts`）。风险：关键词误推 → 仅推荐不强制；回滚：revert。

验证：`cargo test --lib douyin_v2`（冷开/Hook 超字/重复不足/clip 缺失负路径各红）；真模型 12 例 smoke + 用户 P2 实听 3 首确认 Hook ≤8s。

## 7. Suno v6 用户探针清单（P2，用户手动，结果填 `docs/research/2026-10-20-suno-probe.md`）

| # | 探针 | 操作 | 记录 |
|---|---|---|---|
| S1 | Style 上限 | Advanced 模式粘贴 1,200 字符 | 计数器上限值 |
| S2 | Lyrics 上限 | 粘贴 6,000 字符 | 上限值 |
| S3 | Exclude 上限与位置 | 同上 | 上限值、是否需开关 |
| S4 | Variety | 存在否、默认值、设 0 后 Style 是否被改写（看歌曲页 Styles） | 截图 |
| S5 | 段内 cue 语法 | 同词同 Style 生成 2×3：`[Verse] (soft, close-mic)` / `[Verse \| soft, close-mic]` / 旧格式 `[Verse]`+说明行 | 哪种 cue 被唱出/被执行；是否把 cue 唱成歌词 |
| S6 | Exclude 生效 | Style 含 piano，Exclude `piano` | 是否仍有钢琴 |
| S7 | 冷开 | 首段 `[Chorus]` 无 Intro | 人声进入秒数 |
| S8 | 时长控制 | 是否有 Duration 设置；短词是否生成 <60s | 实际时长 |
| S9 | 中文咬字 | 同一中文副歌 × 拼音注释有/无 | 错字数 |
| S10 | Weirdness/Style Influence 默认值与区间 | 读 UI | 数值 |
| S11 | 负向段落改写 | `(drums drop out)` 是否生效 | 是/否 |
| S12 | Style 英文 vs 中文（D12） | 同一中文歌词，Style 分别为纯英文 / 同义中文 / 英文+1 个中文术语（如 戏腔 vs "Peking opera falsetto"）各 2 首 | 风格命中度；中文术语是否被理解；结果决定 `style_cjk_allow.csv` 入表 |

每条记日期、模型（v6/v6-wild/v6-mini）、套餐。结果回填 `suno_profile.rs` 为 `UserProbed`。

## 8. 仓库垃圾清单（只读盘点 2026-10-20）

| 项 | 证据 | 处置 |
|---|---|---|
| `graphify-out/`（13M，含 `graph.json.bak-before-v3`、cache、wiki） | 未跟踪；第三方代码图分析产物 | 删除，`.gitignore` 加 `graphify-out/` |
| 根目录 `target/large-test`、`large-test-envelope`（1.2M） | harness 输出（large_scale_matrix.rs:165 硬编码） | 删除；P0-7 后输出到 cargo target |
| `src-tauri/target`（4.9G） | 构建缓存，已忽略 | `cargo clean`（可选） |
| `dist/`（1.5M） | 构建产物，已忽略 | 删除 |
| `.DS_Store` ×5（根、docs、graphify-out、src、src-tauri） | 已忽略但存在 | 删除 |
| `.mimosa/hook-state` | 第三方扫描状态，已忽略 | 删除 |
| `docs/qa/results_12round_final.jsonl` | 与 v4 同 MD5 `0ddd1ecc…`（已跟踪） | `git rm` |
| `docs/2026-07-08 … 2026-09-14-*.md`（8 份历史计划） | 已跟踪；被新施工图取代 | **直接 `git rm`**（D10）。零引用检查：`grep -rn "2026-0[79]-[01][0-9]-" README.md CHANGELOG.md src src-tauri/src src-tauri/tests docs/qa docs/research .github` 只允许命中本施工图/REPORT；有命中先改引用再删 |
| `public/tauri.svg` / `public/vite.svg` | 无引用（模板残留） | 已删除 |
| `src-tauri/icons/android/*`、`icons/ios/*`（35 个文件） | 桌面应用无移动端目标；`tauri.conf.json:31-37` bundle.icon 只引 32x32/128x128/128x128@2x/icon.icns/icon.ico | **直接 `git rm -r`**（D10）。零引用检查：`grep -rn "android\|ios/\|AppIcon\|ic_launcher" src-tauri/tauri.conf.json src-tauri/Cargo.toml .github` 为空；删后 `npm run tauri build -- --debug`（或 CI）图标正常。|
| `src-tauri/icons/Square{30x30,44x44,71x71,89x89,107x107,142x142,150x150,284x284,310x310}Logo.png`、`StoreLogo.png` | Windows Store/UWP 图标；`tauri.conf.json:31-37` 未引用 | **直接 `git rm`**（D14）。零引用检查：`grep -rnE "Square[0-9]+x[0-9]+Logo|StoreLogo" src-tauri .github package.json` 仅命中文件自身（无配置引用）；删后 Windows CI 构建（NSIS）图标正常 |
| `src/utils/speechOrder.test.ts` | 唯一不在 `src/test/` 的测试 | 移入 `src/test/` |
| `docs/{REPORT,BLUEPRINT}.{html,pdf}`、`build.py` | 由 md 生成的派生物 | 保留 md，派生物不入库 |
| 审计/调研/记忆文档（`docs/BLUEPRINT_v2.md`、`docs/research/`、`.workbuddy/`） | `git status` | md 入库；`.workbuddy/` 加入 `.gitignore` |
| `~/Library/Application Support/com.shiyi.music/{history.json, pipeline_checkpoint_5c13cf9d-….json}` | D5 测试垃圾 | §5.1 #12 删除 |
| 无主 `#[allow(dead_code)]` 9 处 | §5.1 #13 | 逐个判定 |

## 9. 问题归位（38 条）

| ID | 去向 | ID | 去向 |
|---|---|---|---|
| A-01 | P0-1 | C-12 | P0-10（DNS rebinding **接受风险**文档声明 `docs/security-self-use.md`；不做 pinning） |
| B-01 | P1-2/P1-3 + P3-1..4 | C-13 | P0-8 |
| B-02 | P0-9 → P1-4 | C-14 | P0-8 |
| B-03 | P1-10（迁移删除） | D-01 | P1-12 |
| C-01 | P0-5 | D-02 | P0-10 |
| C-02 | P1-6 | D-03 | P1-10 |
| C-03 | P1-8 | D-04 | P1-12 |
| C-04 | P0-2 | D-05 | P1-12 |
| C-05 | P0-3 | D-06 | P0-3 |
| C-06 | P0-3 | D-07 | P0-10 |
| C-07 | P0-4 | D-08 | 关闭（自用） |
| C-08 | P0-10 | M-01..M-04 | P1-1/P1-4 |
| C-09 | P1-10 | M-05/M-08/M-11/M-12 | P1-9 |
| C-10 | P0-6 | M-06 | 关闭（D3） |
| C-11 | P0-7 | M-07 | P1-5 |
| M-09/M-10 | P1-13 | — | — |

## 10. 总验收

```bash
cd src-tauri && cargo test --lib && cargo test --test headless_offline && cargo clippy --all-targets -- -D warnings
cd .. && npm test && npm test && npm test && npx tsc --noEmit && npm run build
# Key 未被改动（只读指纹）
security find-generic-password -s shiyi-music -a global -w | shasum -a 256   # 测试前后一致
grep -rnE "envelope_|PLAN_ENVELOPE|STYLE_PROMPT_MAX_CHARS|DESC_LINE_MAX" src-tauri/src src  # 应为空
```

## 11. 开放问题

**无。** 全部已由 D1–D14 拍板：旧文档/移动端图标删除（D10）、D 模式场景（D11）、Style 恒英文（D12）、output_lang 收敛 zh/en（D13）、Windows Store 图标删除（D14）。施工中若 P2 探针结果推翻某项平台事实，按 RULE-SUNO-PROFILE 回填，不另开决策。

## 12. 一致性自检（2026-10-20 终稿）

| 项 | 读数 |
|---|---|
| 决策 | D1–D14，连续 |
| 新规则 | §3.1–§3.8，共 8 条 |
| P0 步骤 | P0-0..P0-10，共 11 步 |
| P1 步骤 | P1-J + P1-1..P1-13，共 14 步（场景选择并入 P1-7） |
| P2 | 用户探针 S1–S12，共 12 项 |
| P3 退役 | §5.1 #1–#14，共 14 项 |
| 问题归位 | 26 工程 + 12 市场 = 38；关闭不做 2（D-08、M-06） |
| 交叉引用 | §2.3→§3.7/§3.8/§6；§3.7→§3.8；§5.1#14→§3.8；§8→§5.1#12、P1-J；§6.3→P1-7；均已核对 |

# shiyi-music 架构与规则契约（v3 正式版 · v6.1.0）

> **状态**：已落地生效（v6.1.0 生产真源，与 Rust 后端 `src-tauri/` 及 React 前端 `src/` 逐字段对齐）
> **基线**：`v6.1.0`（Tauri 2 + React 19 + Rust）
> **核心目标**：将系统从旧版“多轮委员会打补丁 + 脆弱算术/正则校验”升级为**“立意五问与全局声律统筹（Intent & Prosody Blueprint） + 词曲声学一体直通管线（Direct Pipeline） + 汉语十三辙硬门与去机器味双层终审 + Suno 诚实编译器渲染”**的高内聚架构。

---

## 一、 架构总览：直通管线（Direct Pipeline）与按需靶向精修

常规生成（`pipeline_generate`）默认走**三阶确定性直通管线**，消除旧版多轮委员会空转内耗，同时完整保留增量优化（`pipeline_refine`）、轮间人工确认门（`round_gate`）与中途插话（`interject`）：

```
用户输入 (Mode A/B/C/D + 歌词语言 output_lang + 抖音场景 douyin_scene)
  │
  ├─► [阶段 0/1：词曲声学一体主创 + 初稿自愈 (`plan_flow::host_initial_plan`)]
  │     ├─ 强制确立顶层 `SongIntent`（立意五问 + `rhyme_plan` 全局十三辙声律与句式网格统筹）
  │     ├─ 同步构建 `AsymmetryPlan`（破格短句、故意不押韵刺点、末遍副歌微变、终局留白）与三维声场编配
  │     ├─ 内置初稿自愈循环（最多 2 次）：初稿立即过 `plan::validate`，若检出声韵/曲式/契约硬伤，当场注入修复提示重修
  │     └─ 经 `orchestrator::direct_pipeline_role_summary` 拆解各领域专家视角摘要（驱动前端圆桌阶段条、Q 版角色动效与详情弹窗）
  │        （注：当处于 `pipeline_refine` 增量优化、开启 `round_gate` 轮间门或存在用户 `interject` 插话时，自动运行定向角色审改轮）
  │
  ├─► [阶段 2：物理/声律硬门与去机器味双层终审 (`plan_flow::auditor_finalize`)]
  │     ├─ 执行 `plan::validate::validate`（平台物理硬门 + 汉语十三辙声律硬门 + Anti-AI-Tell 诊断）
  │     ├─ 若 0 硬违规 → 0 额外 LLM 调用直接定稿放行，并发出 `AuditStart` → `AuditResult(pass=true)` → `FinalPlan` 事件
  │     └─ 若命中 `Severity::Hard` 硬伤 → 注入当前模式核心词曲方法论（`prompt_for_mode`）与问题清单，执行最多 3 次结构化靶向返修
  │        （当收敛初稿存在歌词/声韵/格律硬伤时，自动解除 `check_fidelity` 逐行死锁，允许定稿人重写违规歌词行）
  │
  └─► [阶段 3：Suno 诚实编译器渲染 (`plan::render_suno::render` + 前端 `ResultPanel::parseEnergy`)]
        ├─ 后端 `render_suno` 按 Suno v5/v6 前置高权重顺序输出 `Style`（自动收敛至 8–14 个标签黄金区间），按 Section Delta 编译段头
        └─ 前端 `ResultPanel::parseEnergy` 与 `sunoPackage::historyPlanFields` 提取 `energy N` 渲染独立能量条，并从展示/复制/历史落盘文本中彻底剥离 `energy N`（空竖线自动收拢为 `[Label]`）
```

---

## 二、 四大模式定位与契约（共享 `SongPlan` 内核）

| 模式 | 定位与数据流向 | 上游锁死项 | 核心生成与校验契约 |
| :--- | :--- | :--- | :--- |
| **Mode A（歌词模式）** | **先词后曲（Lyrics-First）** | 用户原词 100% 锁死（`check_fidelity` multiset 零增删） | 从原词提炼 `intent` 与 `rhyme_plan` → 设计段落能量差（`≥3`）、配器差（`≥2`）、`arrangement_hook` 与 `subtraction` 减法抽空事件 |
| **Mode B（想法模式）** | **立意与声律驱动全创（Intent-First）** | 立意五问 + 单核 `anchor_object`（1 个核心锚点物三阶递进） | 强制标准流行双循环曲式（`Verse 1 → Chorus 1 → Verse 2 → Chorus 2 → Bridge → Final Chorus → Outro`）；全曲十三辙统筹与同职能段锁死 |
| **Mode C（改写模式）** | **先曲后词/格律重填（Prosody-First）** | 原词段落结构、逐行字数（`validate_lyric_fill`）與原词押韵行对位置 | 严格保持原词同辙押韵行对（`mode_c_rhyme_scheme`），严禁照搬原词韵脚尾字（`mode_c_copied_rhyme_char`），严禁非复沓句同音挤韵 |
| **Mode D（抖音模式）** | **高密度钩子先行（Hook-First）** | 场景 BPM 带（`daily/emotion/dance/story/guofeng`）、60–90s 时长、Intro ≤3s、Hook ≤10s 落地 | Hook 段 `≥2`、Verse `≤4` 行、每行 `≤12` 字、结尾 `abrupt stop`、混音必含 `M-01 + M-06`、Exclude 必含 `long intro / slow build / fade out` |

---

## 三、 结构化中间表示（`SongPlan` IR 真源字段对照）

后端唯一定义点：`src-tauri/src/plan/mod.rs`（JSON Schema 快照：`src-tauri/tests/fixtures/schema/{song_plan.v1.json, plan_ir.v1.json}`；前端镜像类型：`src/types/index.ts`）：

1. **`intent: Option<SongIntent>`（创作立意五问与声律蓝图）**
   - `plot_type: PlotType`：`attitudinal`（态度型）| `situational`（情境型）| `narrative`（叙事型）| `expository`（白描枚举型）
   - `one_thing: String`：一句话口语真话（禁修辞、禁“既…又…”）
   - `to_whom: String`：具体倾诉对象与姿态（禁写“所有人/听众”）
   - `at_what_moment: String`：具体时间、空间光影或身体状态
   - `anchor_object: String`：1 个核心可拍照锚点物（全曲围绕该物件在主歌一、主歌二、桥段发生三阶状态变化，辅以同一物理现场的环境细节）
   - `rhyme_plan: Option<String>`：全局十三辙声律与句式字数网格统筹说明
2. **`asymmetry: Option<AsymmetryPlan>`（刻意不对称与留白设计）**
   - `short_line: Option<String>`：破格短句或进副歌前的留白呼吸句（在 `prosody_meter_imbalance` 中显式豁免字数极差）
   - `deliberate_unrhymed: Option<String>`：故意不押韵的转折刺点句（在 `section_rhyme_missing` 中显式豁免末句落韵）
   - `chorus_variation: Option<String>`：末遍副歌相对于首遍副歌的热点词/人声微变说明
   - `withholding: Option<String>`：终局留白（核心痛点如何悬置而不强行圆满升华）
3. **`global: GlobalStyle`（Suno Style & Exclude 全局源）**
   - `mood: Option<String>`：置顶情绪与声学质地（编译时排最高权重位）
   - `genre: Vec<String>`、`bpm: Option<u16>`、`key: Option<String>`、`meter: Option<String>`
   - `vocal: Vec<String>`：支持 `vocal_mix.csv` 词表 ID（`V-01..V-08`）或 `≥3` 词具象英文人声短语
   - `instruments: Vec<String>`：全曲核心乐器清单（`≤7` 件）
   - `arrangement_hook: Option<String>`：贯穿全曲的标志性器乐记忆点/动机钩子
   - `arc: Option<String>`：整体能量推进描述
   - `mix: Vec<String>`：支持 `vocal_mix.csv` 词表 ID（`M-01..M-08`）或 `≥3` 词具象英文混音短语
   - `negative: Vec<String>`：渲染至 Suno `Exclude` 栏（正向短语，不带 `no/without`）
4. **`sections: Vec<Section>`（段落与局部声学变化量 Section Delta）**
   - `kind: SectionKind`、`label: String`、`motion: Option<SectionMotion>`（`push` | `stop`）
   - `positive_local: Vec<String>`：段内正向局部变化量（奏法、声场空间、人声姿态，恒英文）
   - `negative_local: Vec<String>`、`subtraction: Option<String>`：段内退出/减法抽空事件（由 `render_suno::rewrite_negative_local` 统一编译为 `<instrument> drop out` 等正向声学状态）
   - `energy: Option<u8>`：`1..=10`
   - `instruments: Vec<String>`、`minimal: bool`、`target_seconds: Option<u16>`、`lines: Vec<String>`
5. **`exemptions: Vec<String>`（显式创作破格豁免理由）**
   - 当创作者声明极简独奏编制或特殊实验结构时，配器下限、参数区间与声律硬门可通过 `Issue::hard_or_exempt` 降级为 `Severity::Warn`。

---

## 四、 双层终审规则体系（`src-tauri/src/plan/validate.rs`）

### 第一层：物理契约、曲式结构与汉语声律硬门（`Severity::Hard`）

1. **平台物理与极性/语言硬门**
   - `style_too_long`（`≤1000`）、`style_length`（`30..=1000`）、`header_too_long`（`≤200`）、`lyrics_too_long`（`≤5000`）、`exclude_too_long`（`≤1000`）、`variety`（`=0`）。
   - `polarity`：正向层禁 `no / without / 不要 / 无` 否定词，且禁与 `global.negative` 重叠。
   - `style_cjk`：`global.*` 与 `sections[].positive_local / negative_local` 恒英文（仅放行 `style_cjk_allow.csv` 已登记专名）。
   - `lyrics_lang_mismatch`：歌词行语言与请求 `output_lang`（`zh` | `en`）严格一致。
   - `lyric_bracket_leak` / `break_mark`：歌词行内禁混入 `[...]` 标签说明（`[spoken]` 除外），禁 `/` 与 `、` 断句符号，废除中文行内机械单空格切词。
   - `vocal_mix_coverage`：`global.vocal` 与 `global.mix` 各至少包含 1 个有效词表 ID（`V-01..V-08` / `M-01..M-08`）或 `≥3` 词具象英文短语。
2. **汉语十三辙声律与流行曲式双循环硬门（`check_prosody_and_rhyme`，内置多音字词尾消歧与五大家族发音部位归并）**
   - `intent_missing`：多段歌曲若填写 `intent`，其 `anchor_object` 与 `rhyme_plan` 不得为空。
   - `section_rhyme_missing`：每段以偶句（第 2、4、6 句）与末句 2 倍权重识别主导十三辙，要求有词段落必须形成押韵闭环且收尾句入主韵（除非在 `asymmetry.deliberate_unrhymed` 显式声明）。
   - `rhyme_weak_particle`：Mode B 段落末句严禁拿轻声虚词（`的/吗/呢/吧/啊/呀/啦/呗`）充当收束韵脚，必须以实词入韵。
   - `rhyme_homophone_collision`：同段内非整句复沓、非双字同尾排比的不同歌词行，严禁使用同音字挤韵（如 `间 jian / 件 jian`）或同字重韵。
   - `prosody_meter_imbalance`：同段内非 `asymmetry.short_line` 声明的对偶句，去标点与和音虚词后的字数极差不得超过 `3` 字。
   - `song_form_imbalance`：Mode B 含 `≥2` 段 `Verse` 时，`Verse 1` 与 `Verse 2` 之间必须有 `Chorus/Hook`（严禁 `Verse 1 → Verse 2` 连堆在首遍副歌前）。
   - `global_rhyme_scatter`：全曲有词段 `≥3` 段时，强制全局韵部统筹——全曲非 `Bridge` 段最多 `2` 个十三辙，含 `Bridge` 最多 `3` 个十三辙，且跨发音部位家族（前鼻音 `-n` / 后鼻音 `-ng` / 开口 `a` 系 / 齐撮 `i-e` 系 / 圆唇 `o-u` 系）绝对不超过 `2` 个。
   - `homologous_section_rhyme_drift` & `homologous_section_meter_drift`：全曲所有 `Chorus/Hook` 必须 100% 锁死同一十三辙；所有 `Pre-Chorus` 与所有 `Verse` 必须锁死同一发音部位家族，且 `Verse 1` 与 `Verse 2` 行数差 `≤1`、均长差 `≤2`。
   - `mode_c_rhyme_scheme` & `mode_c_copied_rhyme_char`：Mode C 改词必须对齐原词押韵行对同辙，且严禁直接照搬原词对应行尾字（常见语气助词除外）。

### 第二层：去机器味审美诊断（`Severity::Warn`，透出至 `AuditResult.findings`）

1. `style_empty_adjective`：拦截 `epic, emotional, beautiful, amazing, masterpiece, perfect, louder` 等无效空洞词。
2. `energy_monotonic`：检测 A/B 模式能量全程单调递增且无 `minimal` 或 `subtraction` 减法回落。
3. `chorus_static_repeat`：检测首末遍 `Chorus` 的歌词、局部提示与配器完全雷同且无 `chorus_variation` / `subtraction`。
4. `lyric_abstract_cliche`：检测歌词出现 `≥2` 个高频抽象套话词（`时光/岁月/远方/星辰/余生/救赎/宿命/深渊/彼岸/永恒`）。
5. `rhyme_char_overuse`：检测非复沓句中同一个汉字被重复当作句尾韵字 `≥4` 次。
6. `peak_closed_vowel`：检测最高能量 `Chorus/Hook` 末句收尾落在闭口韵（衣期 `i/ü`、姑苏 `u`）导致高音憋嗓。

---

## 五、 Suno 诚实编译器（`src-tauri/src/plan/render_suno.rs`）與前后端闭环

1. **`Style` 栏高权重前置与标签自动收敛（`style_tags`）**
   - 固定编译顺序：`mood → genre → bpm → key → meter → vocal → instruments → arrangement_hook → arc → mix`。
   - 当模型填写的原始标签总数 `>12` 时，编译器自动执行高权重收敛裁剪（`genre ≤ 2`、`vocal ≤ 1`、`instruments ≤ 4`、`mix ≤ 1`（Mode D 为 `2`），并过滤超长冗余散文式 `arc`），确保最终 `Style` 稳定落在 `8–14` 个标签的 Suno 黄金解析区间；对精简夹具（`≤12` 项）保持逐字节不变。
2. **`Lyrics` 段头 Section Delta 编译（`section_cues`）**
   - 当 `positive_local` 已写出完整的局部声学变化量（`≥32` 字符）时，不再向段头重复追加 `s.instruments` 裸乐器名清单。
   - `rewrite_negative_local` 将 `no X`、`without X` 以及裸乐器名 `X` 统一改写为正向声学动作 `X drop out`；当 `subtraction` 已存在时不再重复拼接 `negative_local`。
3. **前后端全链路同源保障**
   - **跨语言协议锁**：`src/test/wireProtocol.test.ts` 双向锁死 Rust `PipelineEvent`、`GateDecision`、`BackoffReason` 与前端 `src/types/index.ts`。
   - **反馈路由双端同源**：`src/utils/refineTargets.ts`（`estimateRefineTargets`）与 `src-tauri/src/commands/orchestrator.rs`（`roles_for_feedback`）统一转小写并共享相同的五类（含 v3 声律/减法/留白）关键词表。
   - **角色知识表双端同源**：`src/hooks/usePipeline.ts`（`MODE_EXPERTS`）与 `src-tauri/src/commands/roles.rs`（`role_for`）绑定完全一致的知识表清单，并在启动时由 `get_pipeline_meta` 动态同步。
   - **能量锚点展示与落盘剥离同源**：`src/components/ResultPanel.tsx`（`parseEnergy`）与 `src/utils/sunoPackage.ts`（`historyPlanFields`）统一剥离段头 `energy N` 并收拢空竖线，确保存入 `history.json`、界面展示与一键复制给 Suno 的歌词 100% 纯净无伪代码。

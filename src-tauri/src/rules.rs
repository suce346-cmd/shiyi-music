//! 规则单源（P4 / 知识质量方案 C1 共用）：阈值只定义一次。
//!
//! - 本文件是数字的唯一 Rust 真源：`validator.rs` 的硬校验必须引用此处常量，
//!   不得再手写 350/3/2/7/2/4/10/90 等裸数字（防 drift）。
//! - `checklist(mode)` 是数字的唯一 prose 载体：校验员/审改提示词中重复的
//!   参数区间数字已删，改为拼接本清单（M4）。清单文本与常量同文件手写一次，
//!   由本文件单测锁 verbatim 一致。
//! - `host_primer(mode)` 是阶段 0 地基：模式专属、每模式 ≤800 字、只读裁剪，
//!   顺序按 M9（受众→物件→约束→旋律）排段。主持人保持零 CSV 检索语义，仅加静态地基。
//! - 无新增依赖，无网络调用，无凭据改动。

/// Style Prompt 上限字符数（含空格）
pub const STYLE_PROMPT_MAX_CHARS: usize = 350;
/// Style Prompt 过短告警下限（极端缺失提示）
pub const STYLE_PROMPT_MIN_CHARS: usize = 30;
/// 结构标签最少段落数
pub const MIN_SECTION_TAGS: usize = 2;
/// 最弱 vs 最强能量差下限（0-10 标尺）
pub const MIN_ENERGY_GAP: u32 = 3;
/// 最弱 vs 最强配器件数差下限
pub const MIN_INSTRUMENT_GAP: usize = 2;
/// 全曲核心乐器上限
pub const INSTRUMENT_MAX: usize = 7;
/// 最弱段至少件数（L-3 用户决策 2026-09-06：单段下限统一按 3——旧值 2 与"单段 3-7 件"打架）
pub const MIN_INSTRUMENT_WEAK: usize = 3;
/// 最强段至少件数
pub const MIN_INSTRUMENT_STRONG: usize = 5;
/// D 模式 Hook 最少次数
pub const HOOK_MIN_COUNT: usize = 2;
/// D 模式单段 Verse 上限行数
pub const VERSE_MAX_LINES: usize = 4;
/// D 模式每行歌词上限字数（去空白与半角标点后计数）
pub const DOUYIN_LINE_MAX_CHARS: usize = 10;
/// 中文歌词 Verse 每行字数区间（与 suno_rules.csv `line_chars_verse` 同源，由
/// `lyric_chars_match_csv_structured_value` 锁定）。旧实现把 6-13 手写在 roles.rs/prompts.rs
/// 共 4 处，与 CSV 脱钩——改 CSV 值提示词不联动（审计 #21）。
pub const LYRIC_LINE_VERSE_MIN: usize = 6;
pub const LYRIC_LINE_VERSE_MAX: usize = 13;
/// 中文歌词 Chorus 每行字数区间（与 suno_rules.csv `line_chars_chorus` 同源）。
pub const LYRIC_LINE_CHORUS_MIN: usize = 4;
pub const LYRIC_LINE_CHORUS_MAX: usize = 9;
/// D 模式 BPM 下限
pub const DOUYIN_BPM_MIN: u32 = 90;
/// 全模式说明行上限字符数（含方括号、整行计）。
/// 单源：上游告知（四模式 checklist + D primer + 信封规范 + 各角色提示词经 ${DESC_LINE_MAX} 插值）
/// 与下游强制（rules::envelope_defect_mode 方案侧左移 + validator::desc_line_length_issues 终稿侧硬门）
/// 同读此值——上游输出约束与下游校验约束共用一个数字，口径不可能再分叉。
/// 旧值 80 的判废依据（2026-09-17 实测）：最强段 ≥5 件且每件带行为动词的英文说明行最简约 90-100 字符，
/// 与 80 上限在数学上不可能同时满足 → 5/5 run 全部被信封门拦下、0 次干净通过（14 条超限行 81-117 字符）。
pub const DESC_LINE_MAX_CHARS: usize = 200;
/// Mode C 尾部收尾允许行数
pub const LYRIC_FILL_TAIL_ALLOW: usize = 2;

/// 请求准入·用户输入上限字符数（user_input 与原歌词共用）。
///
/// 单源理由：这两个数原先作为**函数局部常量**写在 `models::validate_request` 体内，
/// 于是 ① 需要同一规则的另一入口无法引用，只能伪造整个请求来"蹭"校验
/// （见 `models::validate_feedback` 的说明）；② doc 注释只能手抄数字，注释与常量必然漂移。
/// 消费点：`models::validate_request`（唯一引用处，`admission_limits_are_single_sourced` 锁定）。
pub const INPUT_MAX_CHARS: usize = 20000;
/// 请求准入·优化反馈 / 中途插话上限字符数。
///
/// 消费点：`models::validate_feedback`（`validate_request` 的 feedback 分支与
/// `orchestrator::interject_feedback` 共用同一实现——上游约束与下游校验同一个数），
/// 以及 `commands::interject::push` 的**同值**兜底（见下条注释）。
pub const FEEDBACK_MAX_CHARS: usize = 2000;

/// 单 run 待消费插话的**条数**上限（内存保护：防 UI 异常循环 push 撑爆内存）。
///
/// 与 `FEEDBACK_MAX_CHARS` 的区别：一个管"单条多长"，一个管"最多几条"，都是准入规则，
/// 故同处单源。消费点：`commands::interject::push`（达上限返回 Validation 错误，不静默丢弃）
/// 与 `orchestrator::interject_feedback`（错误原样透传给前端提示）。
pub const INTERJECT_MAX_PER_RUN: usize = 10;

/// 弧线参数区间（Weirdness min/max + Style Influence min/max），与 suno_rules.csv 同源。
/// K-1 正名：CSV 仅 style_arc_high 有结构化列（85-90，arc_high_matches_csv_structured_value 锁定）；
/// 其余 9 条的 Weirdness 区间只散在 CSV 描述文本中，无结构化列，数值系 prose 共识——
/// 改这 9 条时 CSV 描述文本需同步，二者无自动约束。
/// 顺序：标准叙事 / 全程高能 / 高开低走 / 平铺氛围 / 起伏戏剧 / 阶梯上升 / 渐进爆发 / U型 / 单峰 / 回环。
pub const ARC_PARAMS: &[(&str, u32, u32, u32, u32)] = &[
    ("标准叙事", 22, 28, 78, 83),
    ("全程高能", 10, 15, 85, 90),
    ("高开低走", 25, 35, 70, 80),
    ("平铺氛围", 15, 25, 80, 90),
    ("起伏戏剧", 28, 35, 75, 82),
    ("阶梯上升", 20, 28, 78, 88),
    ("渐进爆发", 15, 25, 80, 90),
    ("U型", 25, 35, 70, 82),
    ("单峰", 20, 30, 75, 85),
    ("回环", 20, 28, 78, 85),
];
/// 抖音默认参数区间（叙事型例外与硬校验同口径：仅当结构含 ≥2 个叙事段 Verse/Pre-Chorus/Bridge 且方案写明'叙事型'归类，方可回落 A/B 弧线区间；二者缺一按本区间打回）
pub const DOUYIN_WEIRD_MIN: u32 = 12;
pub const DOUYIN_WEIRD_MAX: u32 = 20;
pub const DOUYIN_STYLE_MIN: u32 = 85;
pub const DOUYIN_STYLE_MAX: u32 = 95;
/// B 模式参数区间
pub const MODE_B_WEIRD_MIN: u32 = 20;
pub const MODE_B_WEIRD_MAX: u32 = 35;
pub const MODE_B_STYLE_MIN: u32 = 75;
pub const MODE_B_STYLE_MAX: u32 = 85;

/// C1：CHECKLIST_A 弧线区间的并集（10 条弧线 min/max 派生）——ARC_PARAMS 的执行点之一。
/// 精确到单条弧线的门需终稿带弧线类型机读标注（项目书 §6 既有问题区记录，后续组件补）。
pub fn arc_param_union() -> (u32, u32, u32, u32) {
    ARC_PARAMS.iter().fold(
        (u32::MAX, 0, u32::MAX, 0),
        |(wlo, whi, slo, shi), (_, wmin, wmax, smin, smax)| {
            (wlo.min(*wmin), whi.max(*wmax), slo.min(*smin), shi.max(*smax))
        },
    )
}

/// Style Prompt 长度门的**模式域单源**：注册表与执行者
/// （`validator::style_prompt_length_issues`）同读这两个常量——
/// 防"注册模式域 ≠ 执行模式域"漂移（第三批实测 `douyin_bpm_min` 错挂执行者即同类）。
///
/// 上限：A/B/C/D 全部承诺（CHECKLIST_A/B/C/D 均写 "Style Prompt≤350字符"）。
pub const STYLE_PROMPT_MAX_MODES: &[&str] = ALL_MODES;
/// 下限：仅 A/B/D 承诺（CHECKLIST_A/B/D 写 "≥30字符"）；C 的 STYLE 节为"氛围/流派基调一句"，
/// 上游无下限告知，故下限不对 C 执行。
pub const STYLE_PROMPT_MIN_MODES: &[&str] = &["mode_a", "mode_b", "mode_d"];

/// 全部模式名（单源）：模式域条件标签的取值域 + `STYLE_PROMPT_MAX_MODES` 等"全模式"清单别名。
/// 取值必须与 `models::Mode::to_str_name()` 逐一对应（守护测试锁定）。
pub const ALL_MODES: &[&str] = &["mode_a", "mode_b", "mode_c", "mode_d"];

/// 规则执行者所在文件的**声明式扫描面**——守护测试 `rule_registry_symbols_have_executors` 消费。
///
/// 为什么是生产侧常量、而不是测试内的字面量数组：扫描路径硬编码在测试里时，新增执行者
/// 所在文件不会自动进网（第十三批 D1 实测：准入限额的执行者在 `models`/`interject`，
/// 而扫描面只有 `validator`/`orchestrator`——三条规则注册进表也照旧"绿灯裸奔"）。
/// 登记即入网；未登记的新文件不会静默漏网，而是**直接报红**（"执行器无调用点"），
/// 因为该执行者在扫描面内找不到任何调用点。
pub const RULE_EXECUTOR_FILES: &[&str] = &[
    // 产出硬门执行者
    "commands/validator.rs",
    "commands/orchestrator.rs",
    // 准入硬门执行者（#26 新增三条规则所在处）
    "models/mod.rs",
    "commands/interject.rs",
];

/// C1/ADR-2：规则注册表——每条硬门规则一个条目，"每条规则唯一执行者"的机读清单。
/// 守护测试 `rule_registry_symbols_have_executors` 保证两件事：
/// ① `symbols` 列出的每个符号在 `RULE_EXECUTOR_FILES` 内有真实消费者（死常量在 CI 即失败）；
/// ② `executor` 在 `RULE_EXECUTOR_FILES` 内有**真实调用点**（只定义不接线同样在 CI 即失败）。
pub struct RuleSpec {
    pub id: &'static str,
    pub modes: &'static [&'static str],
    pub symbols: &'static [&'static str],
    pub executor: &'static str,
}

pub const RULE_REGISTRY: &[RuleSpec] = &[
    RuleSpec { id: "style_prompt_max", modes: STYLE_PROMPT_MAX_MODES, symbols: &["STYLE_PROMPT_MAX_CHARS"], executor: "style_prompt_length_issues" },
    RuleSpec { id: "style_prompt_min", modes: STYLE_PROMPT_MIN_MODES, symbols: &["STYLE_PROMPT_MIN_CHARS"], executor: "style_prompt_length_issues" },
    RuleSpec { id: "min_section_tags", modes: &["mode_a", "mode_b"], symbols: &["MIN_SECTION_TAGS"], executor: "validate_production" },
    RuleSpec { id: "min_energy_gap", modes: &["mode_a", "mode_b"], symbols: &["MIN_ENERGY_GAP"], executor: "validate_production" },
    RuleSpec { id: "instrument_gap_weak_strong_max", modes: &["mode_a", "mode_b"], symbols: &["MIN_INSTRUMENT_GAP", "MIN_INSTRUMENT_WEAK", "MIN_INSTRUMENT_STRONG", "INSTRUMENT_MAX"], executor: "validate_production" },
    // 极简段豁免：最弱段下限的合法放宽口，唯一执行者 = declared_minimal_sections（validator）。
    // 与上一条同域（A/B 配器件数族）；上游告知单源 = rules::minimal_section_exemption()。
    RuleSpec { id: "minimal_section_exemption", modes: &["mode_a", "mode_b"], symbols: &["MIN_INSTRUMENT_WEAK", "declared_minimal_sections"], executor: "declared_minimal_sections" },
    RuleSpec { id: "audio_influence_zero", modes: &["mode_a", "mode_b"], symbols: &[], executor: "check_audio_influence" },
    // ARC_PARAMS 经 arc_param_union() 折叠后在 validator 消费，对外消费符号登记访问器
    RuleSpec { id: "mode_a_param_arc_union", modes: &["mode_a"], symbols: &["arc_param_union"], executor: "validate_production" },
    RuleSpec { id: "mode_b_param_range", modes: &["mode_b"], symbols: &["MODE_B_WEIRD_MIN", "MODE_B_WEIRD_MAX", "MODE_B_STYLE_MIN", "MODE_B_STYLE_MAX"], executor: "validate_production" },
    RuleSpec { id: "mode_d_param_range", modes: &["mode_d"], symbols: &["DOUYIN_WEIRD_MIN", "DOUYIN_WEIRD_MAX", "DOUYIN_STYLE_MIN", "DOUYIN_STYLE_MAX"], executor: "validate_douyin" },
    RuleSpec { id: "hook_min_count", modes: &["mode_d"], symbols: &["HOOK_MIN_COUNT"], executor: "validate_douyin" },
    RuleSpec { id: "verse_max_lines", modes: &["mode_d"], symbols: &["VERSE_MAX_LINES"], executor: "validate_douyin" },
    RuleSpec { id: "douyin_line_max", modes: &["mode_d"], symbols: &["DOUYIN_LINE_MAX_CHARS"], executor: "validate_douyin" },
    // 说明行上限：方案侧信封门（envelope_defect_mode）左移拦截 + 终稿侧 A/B/C/D 全模式硬门，
    // 两侧同读 DESC_LINE_MAX_CHARS（旧 douyin_desc_line_max 仅 D 单模式 + 80 上限，已废）
    RuleSpec { id: "desc_line_max_terminal", modes: &["mode_a", "mode_b", "mode_c", "mode_d"], symbols: &["DESC_LINE_MAX_CHARS", "is_desc_line"], executor: "desc_line_length_issues" },
    // 执行者更正（第三批）：原挂 check_style_prompt_blocks（不消费 DOUYIN_BPM_MIN、只查下限），
    // 真执行者 = check_bpm_range（validator::check_bpm_range 消费 DOUYIN_BPM_MIN，经 orchestrator 接线）。
    RuleSpec { id: "douyin_bpm_min", modes: &["mode_d"], symbols: &["DOUYIN_BPM_MIN"], executor: "check_bpm_range" },
    RuleSpec { id: "lyric_fill_tail_allow", modes: &["mode_c"], symbols: &["LYRIC_FILL_TAIL_ALLOW"], executor: "validate_lyric_fill" },
    // D-Envelope：方案信封契约——解析器是"方案是否符合结构"的唯一执行者（2026-09-09）
    RuleSpec { id: "plan_envelope_contract", modes: &["mode_a", "mode_b", "mode_c", "mode_d"], symbols: &["parse_plan_sections", "envelope_spec"], executor: "enforce_envelope" },
    // -----------------------------------------------------------------------
    // 准入限额族（第十三批 D1 补登记）：与上面 16 条"产出硬门"不同，这三条是**请求准入硬门**
    // （§6.4 验收口径是"每条参数/格式规则恰有一个执行点"，未按产出/准入分类豁免）。
    // 不登记 = 规则表里没有它们，删掉执行点/换掉常量都不会有守护测试报红。
    // 执行者在 `models`/`commands::interject`（非 validator），故 `RULE_EXECUTOR_FILES` 已扩至这两个文件。
    // modes 取全模式：准入限额与模式无关（任何模式的请求都过同一道闸）。
    // 注意 `interject_max_per_run` 的执行者写作限定路径：`push` 是 `Vec::push` 同名的常见名，
    // 裸名会命中任意容器的 `push(`，守护变成假绿。
    RuleSpec { id: "input_max_chars", modes: ALL_MODES, symbols: &["INPUT_MAX_CHARS"], executor: "validate_request" },
    RuleSpec { id: "feedback_max_chars", modes: ALL_MODES, symbols: &["FEEDBACK_MAX_CHARS"], executor: "validate_feedback" },
    RuleSpec { id: "interject_max_per_run", modes: ALL_MODES, symbols: &["INTERJECT_MAX_PER_RUN"], executor: "interject::push" },
];

// ---------------------------------------------------------------------------
// 思维资产引用契约（#15/#16 修复的单源）
//
// 问题本质：角色 system_prompt 里点名"按 lyric_craft 规则逐项核查（LC-15/LC-18…）"，
// 但注入层是另一套逻辑（trigger 过滤 + 恒 0 打分 + 稳定排序截断）——上游投递的内容
// 与下游点名要求的内容出自两个不相干的来源，于是出现"被要求核查一条看不见内容的规则"。
//
// 单源口径：某角色 prompt 点名要求核查的 LC-/CC- 编号，就是该角色注入必须强制投递的编号。
// - 声明位置：本处（roles.rs 只引用常量名，不另写字面量清单）
// - 注入消费：orchestrator::inject_knowledge 透传 role.craft_refs → knowledge::render_craft_table
// - 守护测试：roles::tests::craft_refs_match_prompt_and_single_source 三向锁定
//   ① roles.rs prompt 文本中出现的全部编号集合 == 本处声明；
//   ② 每个声明编号在对应 CSV 中存在，且 trigger 通过**真门** `knowledge::craft_inject_gate`
//      （角色所在**全部模式** × 角色身份的审改上下文——条件域限定也必须命中，否则该模式下悬空）；
//   ③ 每角色声明数 ≤ orchestrator::INJECT_MAX_CRAFT_ROWS（上限不得截断必达项）。
// ---------------------------------------------------------------------------
/// 🎭 情感分析师：借体/纵深/暴露分层 + 真诚校验原文
pub const CRAFT_REFS_EMOTION: &[&str] = &["LC-13", "LC-15", "LC-18", "CC-10"];
/// 📝 作词人：画面清单/意象三筛/人称齿轮/借体/换象/黑暗出口/减法
pub const CRAFT_REFS_LYRICIST: &[&str] = &["LC-01", "LC-02", "LC-04", "LC-15", "LC-16", "LC-18", "LC-26"];
/// ✍️ 改词人：对齐约束下的三筛/借体/换象/人称落点/减法
pub const CRAFT_REFS_REVISER: &[&str] = &["LC-02", "LC-04", "LC-15", "LC-16", "LC-26"];
/// 🎤 制作人：主题不可改/民族元素表达理由/减法留白/概念先行/制约清单
pub const CRAFT_REFS_PRODUCER: &[&str] = &["CC-01", "CC-12", "CC-17", "CC-22", "CC-24"];
/// 🔥 流行风格分析师：前3秒画面/道具刻度/对话体
pub const CRAFT_REFS_STYLE_ANALYST: &[&str] = &["LC-01", "LC-10", "LC-23"];

// ---------------------------------------------------------------------------
// 思维资产 trigger 标签双域单源（#17/#18/#23/#29 修复）
//
// trigger 列是 "/" 分隔的多标签（如 "阶段0/审改"、"审改/抖音"、"全程"、"扩展位"）。
// 历史缺陷链：
// ① 子串匹配字面"审改" → "全程"行（全流程语义）被结构性排除，任何角色不可达（#17）；
// ② "扩展位"这类预留标签无单源、无登记、无告警 → 用户追加内容静默失效（#18）；
// ③ 条件修饰标签（抖音/制作/A/B/C）只登记词表、**零执行门**（#29）——声明范围与执行范围
//    不同域：LC-31 标"审改/A/B/C"却在 D 模式注入；CC-22 标"审改/制作"却进所有 craft 角色；
// ④ 阶段标签"阶段0"零消费者（#23）——注释称"仅主持 primer 用"，而 primer 是静态手写文本
//    （不读 CSV），于是 Phase0 行在 C/D 模式彻底不可达、在 A/B 只是与 primer 双载体。
//
// 单源口径（**双域模型**）：
// - 阶段域（析取适用，命中即候选）：`CRAFT_TRIGGER_HOST_ONLY`(阶段0，主持人阶段0消费者) /
//   `CRAFT_TRIGGER_ACTIVE`(审改·全程，审改上下文消费者) / `CRAFT_TRIGGER_RESERVED`(扩展位，
//   非注入，走 CRAFT_RESERVED_ROWS 登记)
// - 条件域（**限定**，叠加在阶段域之上；行内条件标签析取，任一命中当前 (模式, 角色) 即通过）：
//   `CRAFT_CONDITIONAL_TAGS`（抖音→mode_d / A/B/C→mode_a/b/c / 制作→producer）
// - 消费者上下文登记：`CRAFT_STAGE_CONSUMERS`（每个阶段标签必须至少有一个消费者，否则即红）
// - 唯一判定函数：`knowledge::craft_inject_gate`（渲染层 / 守护测试 / 角色引用契约共用）
// - 守护测试：knowledge::tests::craft_trigger_vocabulary_has_consumers_and_real_scope、
//   knowledge::tests::craft_conditional_gate_is_restrictive_gate（红灯先行）、
//   orchestrator::tests::craft_rows_reachable_in_every_real_context（死行审计 + 条件域限定 + 阶段隔离）、
//   orchestrator::tests::host_stage0_injects_only_stage0_rows（#23 锁）
// ---------------------------------------------------------------------------
/// trigger 列标签分隔符（"/" 分隔多标签）
pub const CRAFT_TRIGGER_TAG_SEP: char = '/';
/// 审改上下文可注入标签——角色在讨论/审改阶段必须看到（阶段域交集判定）。
/// "审改" = 审改轮；"全程" = 讨论/汇总全流程规则（含审改轮，故必须可注入）。
pub const CRAFT_TRIGGER_ACTIVE: &[&str] = &["审改", "全程"];
/// 主持阶段0专用标签——消费者 = `orchestrator::host_initial_system`（按本标签注入思维资产行）。
/// **不**注入角色审改上下文（角色讨论的是已产出的方案，不是阶段0规划）。
pub const CRAFT_TRIGGER_HOST_ONLY: &[&str] = &["阶段0"];
/// 预留扩展位标签——**默认不注入**；落在该位的每一行都必须在 CRAFT_RESERVED_ROWS 登记，
/// 否则守护测试失败（防用户追加内容静默失效，#18）
pub const CRAFT_TRIGGER_RESERVED: &[&str] = &["扩展位"];

/// 条件修饰标签的判定域（#29 单源）：模式域命中当前模式名、角色域命中当前角色名。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CraftConditionalScope {
    /// 模式域：取值 ∈ `ALL_MODES`（如 抖音→mode_d）
    Mode(&'static str),
    /// 角色域：取值 ∈ `PipelineRole::storage_key()`（如 制作→producer）
    Role(&'static str),
}

/// 条件修饰标签登记表（标签 → 判定域）——**限定语义**：行内条件标签析取，
/// 任一命中当前 (模式, 角色) 即通过；全部未命中则该行在当前上下文不可注入。
/// 新增条件标签 = 在此登记（并保证取值域真实，守护测试锁定）。
pub const CRAFT_CONDITIONAL_TAGS: &[(&str, CraftConditionalScope)] = &[
    ("抖音", CraftConditionalScope::Mode("mode_d")),
    ("A", CraftConditionalScope::Mode("mode_a")),
    ("B", CraftConditionalScope::Mode("mode_b")),
    ("C", CraftConditionalScope::Mode("mode_c")),
    ("制作", CraftConditionalScope::Role("producer")),
];

/// 条件修饰标签查询（未登记返回 None——未知标签由守护测试拦截，运行期按"非条件"处理）
pub fn conditional_scope(tag: &str) -> Option<CraftConditionalScope> {
    CRAFT_CONDITIONAL_TAGS
        .iter()
        .find(|(t, _)| *t == tag)
        .map(|(_, s)| *s)
}

/// 阶段标签的消费者上下文登记（#23/#29）：**每个阶段标签必须至少被一个上下文接受**，
/// 否则该标签就是"词表可见、执行零消费者"的静默死角（守护测试锁定）。
/// 运行期经 `stage_consumer(name)` 取用（登记即被消费，不是文档性清单）。
pub struct CraftStageConsumer {
    pub name: &'static str,
    pub stage_tags: &'static [&'static str],
}

/// 主持人阶段0 上下文名（消费者 = `orchestrator::host_initial_system`）
pub const CRAFT_STAGE_CONSUMER_HOST0: &str = "host_stage0";
/// 审改上下文名（消费者 = `orchestrator::execute_review` 的角色注入）
pub const CRAFT_STAGE_CONSUMER_REVIEWER: &str = "reviewer";

pub const CRAFT_STAGE_CONSUMERS: &[CraftStageConsumer] = &[
    // 主持人阶段0（写初稿前的规划上下文）：接受"阶段0"，无角色身份（角色域条件行不进主持）
    CraftStageConsumer {
        name: CRAFT_STAGE_CONSUMER_HOST0,
        stage_tags: CRAFT_TRIGGER_HOST_ONLY,
    },
    // 审改上下文（每轮角色审改）：接受"审改/全程"，带模式 + 角色身份
    CraftStageConsumer {
        name: CRAFT_STAGE_CONSUMER_REVIEWER,
        stage_tags: CRAFT_TRIGGER_ACTIVE,
    },
];

/// 按上下文名取阶段消费者（未知名返回 None——运行期调用点写错名即测试红）
pub fn stage_consumer(name: &str) -> Option<&'static CraftStageConsumer> {
    CRAFT_STAGE_CONSUMERS.iter().find(|c| c.name == name)
}

/// 预留位行登记（id, 保留理由）——与 CSV 中 trigger 含"扩展位"的行集合**必须相等**（#18 守护）。
/// 新增预留行须在此登记理由；若该行本应生效，请改 trigger 为 "审改" 或 "全程"。
pub const CRAFT_RESERVED_ROWS: &[(&str, &str)] = &[
    ("LC-29", "连载思维（留续集位/后作反转）：进阶技法，未纳入当前审改轮"),
    ("CC-05", "动机经济（同旋律家族换词复用）：进阶编曲技法，未纳入当前审改轮"),
    ("CC-19", "非乐音等权（田野录音入曲）：实验性技法，未纳入当前审改轮"),
    ("CC-25", "印象集迭代（独立小样循环反馈）：工作流技法，未纳入当前审改轮"),
];

// ---------------------------------------------------------------------------
// 关键词表可达性契约（#22 修复的单源）
//
// 问题本质：emotions/cliches/hooks/style_genre 四张关键词表的注入门 = "行的检索键字面
// 出现在当前方案中"（orchestrator::matching_keywords）。该门本身是按需检索设计，但门内
// 存在两个**静默**缺口：
// ① 检索键短于 `KEYWORD_MIN_CHARS` 的候选被直接丢弃——用户追加一条单字关键词（如 cliche
//    "夜"）即永久休眠，无日志、无测试、无登记（现状实测：cliches 的 家/雨/夜/光 4 行死行）；
// ② 表路由（表名→检索键列）与条数上限散落在 orchestrator 的 match 分支 + 两个独立常量 +
//    注释文档四处，新增/改表无守护（此处为单源后，路由与上限只在本表定义）。
//
// 单源口径：
// - 检索键列 + 条数上限：本处 `KeywordTableReachability`（orchestrator 只查表，不写字面量）
// - 键长下限：`KEYWORD_MIN_CHARS`（orchestrator::matching_keywords 与守护测试共读）
// - 可达性守护：knowledge::tests::keyword_table_keys_meet_reachability_policy
//   （任一行的检索键为空或短于下限即红——死行不再静默）
// - 运行期告警：knowledge::warn_reachability_violations（覆盖用户覆盖目录这一编译期测试覆盖不到的场景）
// ---------------------------------------------------------------------------
/// 关键词表的可达性规格：表名 → （检索键列, 单表条数上限）。
/// `key_col` 既是候选词来源也是过滤条件——该列取值字面出现在方案中即注入。
pub struct KeywordTableReachability {
    pub table: &'static str,
    /// 关键词门：该列取值字面出现在方案中即注入；能量门：能量列见 ENERGY_GATE_COLUMNS
    pub key_col: &'static str,
    /// 单表注入条数上限（0 = 不限，走全量表路径）
    pub max_rows: usize,
}

/// 检索键最小字数（单源）——短于此值的候选会被丢弃（避免单字误命中：方案含"深夜"不该命中
/// 关键词"夜"）。因此**数据侧必须满足本下限**，否则该行永久不可达。
pub const KEYWORD_MIN_CHARS: usize = 2;
/// 关键词表单表命中条数上限（主词 1-3 个 + 近邻，"少而准"）
pub const KEYWORD_MAX_ROWS: usize = 6;
/// 流派表命中条数上限（方案通常命中 1-2 个流派）
pub const STYLE_GENRE_MAX_ROWS: usize = 3;
/// 关键词表可达性规格（新增关键词表 = 在本表加一行并在角色 knowledge_tables 绑定，无第二处改动）
pub const KEYWORD_TABLES: &[KeywordTableReachability] = &[
    KeywordTableReachability { table: "emotions", key_col: "emotion", max_rows: KEYWORD_MAX_ROWS },
    KeywordTableReachability { table: "cliches", key_col: "cliche", max_rows: KEYWORD_MAX_ROWS },
    KeywordTableReachability { table: "hooks", key_col: "hook_type", max_rows: KEYWORD_MAX_ROWS },
    KeywordTableReachability { table: "style_genre", key_col: "genre", max_rows: STYLE_GENRE_MAX_ROWS },
];
/// 乐器表可达性：行必须携带可解析的 (energy_min, energy_max) 数值对且 min ≤ max，
/// 否则 `render_instruments_by_energy` 永不命中（同族静默失效，故纳入可达性守护）。
pub const ENERGY_GATE_COLUMNS: (&str, &str) = ("energy_min", "energy_max");

/// 按表名取可达性规格（orchestrator 注入路由的唯一查表口）
pub fn keyword_table(table: &str) -> Option<&'static KeywordTableReachability> {
    KEYWORD_TABLES.iter().find(|s| s.table == table)
}

// ---------------------------------------------------------------------------
// suno_rules 规则职责单源（#19 修复）
//
// 问题本质：suno_rules.csv 的 40 条规则里，只有校验员（全量）与制作人（22 条参数/配器子集）
// 被绑定为知识源——情感分析师/作词人/改词人/风格分析师的 system_prompt 明确要求核查
// 参数弧线（"A 对照校验清单弧线区间"）、歌词格式（"每行 ${LYRIC_CHARS_VERSE} 字"）、
// 抖音传播（"Hook ≥${HOOK_MIN} 次/Verse ≤${VERSE_MAX_LINES} 行"）等，但这些规则行的
// **取值依据与适用场景文本**在注入上下文里不可达（与 #16 悬空引用同族：被要求核查看不见的规则）。
//
// 单源口径：规则 → 职责角色（owner）。角色的 suno_rules 行子集 = 本处对应常量；
// 守护测试 roles::tests::suno_rules_bindings_match_role_ownership 双向锁定，并保证
// **CSV 每条规则至少归属一个职责角色**（新增规则未分配职责即红）。
// 校验员保持全量（终稿格式端口必须全见），不列入 owner 表。
// ---------------------------------------------------------------------------
/// 🎭 情感分析师：参数准绳 + 弧线区间 + 能量落差 + 转折点（弧线与参数终裁在其人设）
pub const SUNO_RULES_FOR_EMOTION: &[&str] = &[
    "weirdness",
    "style_influence",
    "min_energy_gap",
    "turn_point",
    "style_arc_dry",
    "style_arc_high",
    "style_arc_highlow",
    "style_arc_flat",
    "style_arc_drama",
    "style_arc_stair",
    "style_arc_buildup",
    "style_arc_u",
    "style_arc_singlepeak",
    "style_arc_loop",
];
/// 📝 作词人：歌词格式/结构标签/断句标记/创作类（Verse2 增量、意象统一、反套话、Hook 保护）
pub const SUNO_RULES_FOR_LYRICIST: &[&str] = &[
    "line_chars_verse",
    "line_chars_chorus",
    "line_max_chars",
    "tag_list",
    "line_break_rule",
    "no_slash",
    "mark_system",
    "verse2_new",
    "imagery_unity",
    "anti_cliche",
    "hook_protect",
];
/// ✍️ 改词人：对齐类（行字数/断句/标记）+ 创作类核对（转译下的意象统一/反套话/Verse2/Hook 保护）
pub const SUNO_RULES_FOR_REVISER: &[&str] = &[
    "line_chars_verse",
    "line_chars_chorus",
    "tag_list",
    "line_break_rule",
    "no_slash",
    "mark_system",
    "verse2_new",
    "imagery_unity",
    "anti_cliche",
    "hook_protect",
];
/// 🎤 制作人：参数/配器/流派/BPM/说明行（配器与说明行终裁在其人设）。
/// 相对迁移前的 22 条内嵌清单**补入 `desc_line_max`**——制作人 prompt 引用 `${DESC_LINE_MAX}`
/// 并承担说明行终裁定稿（R-4），却拿不到该规则行的取值与依据（#19 同类缺口）。
pub const SUNO_RULES_FOR_PRODUCER: &[&str] = &[
    "weirdness",
    "style_influence",
    "audio_influence",
    "style_prompt_max_chars",
    "min_energy_gap",
    "min_instrument_gap",
    "instrument_max",
    "min_instrument_weak",
    "min_instrument_strong",
    "desc_line_max",
    "style_arc_dry",
    "style_arc_high",
    "style_arc_highlow",
    "style_arc_flat",
    "style_arc_drama",
    "style_arc_stair",
    "style_arc_buildup",
    "style_arc_u",
    "style_arc_singlepeak",
    "style_arc_loop",
    "no_full_band",
    "bpm_match",
    "era_words",
];
/// 🔥 流行风格分析师：抖音传播类（Hook 次数/前3秒/Verse 行数/行字数/骤停/重复标记/BPM 区间）
pub const SUNO_RULES_FOR_STYLE_ANALYST: &[&str] = &[
    "hook_min_count",
    "verse_max_lines",
    "line_max_chars",
    "hook_first",
    "stop_mark",
    "hook_protect",
    "repeat_mark",
    "bpm_match",
];

/// suno_rules 职责归属注册表（规则名 → 职责角色）：CSV 每条规则必须出现在本表，
/// 且各角色的 `SUNO_RULES_FOR_*` 常量 == 本表中归属该角色的规则集合（双向锁定）。
pub const SUNO_RULE_OWNERS: &[(&str, &[&str])] = &[
    ("weirdness", &["情感分析师", "制作人"]),
    ("style_influence", &["情感分析师", "制作人"]),
    ("audio_influence", &["制作人"]),
    ("style_prompt_max_chars", &["制作人"]),
    ("min_energy_gap", &["情感分析师", "制作人"]),
    ("min_instrument_gap", &["制作人"]),
    ("instrument_max", &["制作人"]),
    ("min_instrument_weak", &["制作人"]),
    ("min_instrument_strong", &["制作人"]),
    ("hook_min_count", &["流行风格分析师"]),
    ("verse_max_lines", &["流行风格分析师"]),
    ("line_max_chars", &["作词人", "流行风格分析师"]),
    ("line_chars_verse", &["作词人", "改词人"]),
    ("line_chars_chorus", &["作词人", "改词人"]),
    ("style_arc_dry", &["情感分析师", "制作人"]),
    ("style_arc_high", &["情感分析师", "制作人"]),
    ("style_arc_highlow", &["情感分析师", "制作人"]),
    ("style_arc_flat", &["情感分析师", "制作人"]),
    ("style_arc_drama", &["情感分析师", "制作人"]),
    ("style_arc_stair", &["情感分析师", "制作人"]),
    ("style_arc_buildup", &["情感分析师", "制作人"]),
    ("style_arc_u", &["情感分析师", "制作人"]),
    ("style_arc_singlepeak", &["情感分析师", "制作人"]),
    ("style_arc_loop", &["情感分析师", "制作人"]),
    ("hook_first", &["流行风格分析师"]),
    ("stop_mark", &["流行风格分析师"]),
    ("desc_line_max", &["制作人"]),
    ("no_full_band", &["制作人"]),
    ("tag_list", &["作词人", "改词人"]),
    ("line_break_rule", &["作词人", "改词人"]),
    ("no_slash", &["作词人", "改词人"]),
    ("mark_system", &["作词人", "改词人"]),
    ("verse2_new", &["作词人", "改词人"]),
    ("imagery_unity", &["作词人", "改词人"]),
    ("anti_cliche", &["作词人", "改词人"]),
    ("hook_protect", &["作词人", "改词人", "流行风格分析师"]),
    ("turn_point", &["情感分析师"]),
    ("bpm_match", &["制作人", "流行风格分析师"]),
    ("era_words", &["制作人"]),
    ("repeat_mark", &["流行风格分析师"]),
];

/// 模式校验清单（**唯一入口**，返回已插值的文本——清单原文只写 `${占位符}`）。
/// 入参为 `Mode::to_str_name()`（mode_a/mode_b/mode_c/mode_d），未知模式回退通用清单。
/// 返回 `String` 而非 `&'static str`：占位符必须在出口解析，调用点拿到的一律是常量派生值
/// （旧实现返回原文，改常量文案不联动 = #21 家族残留的"上游告知与常量脱钩"）。
pub fn checklist(mode: &str) -> String {
    let raw = match mode {
        "mode_a" => CHECKLIST_A,
        // R-1：B 模式独立清单——参数准绳是 B 专属区间（20-35/75-85），弧线区间不再混入 B 清单
        "mode_b" => CHECKLIST_B,
        "mode_c" => CHECKLIST_C,
        "mode_d" => CHECKLIST_D,
        _ => CHECKLIST_A,
    };
    interpolate(raw)
}

/// 四模式清单：**全数值走 `${占位符}` 派生**（原文不得出现规则阈值字面量，
/// 由 `no_handwritten_rule_numbers_in_prose_carriers` 扫描 `PROSE_CARRIERS` 锁定）——
/// 旧实现手写全文数字，与常量/`ARC_PARAMS` 脱钩（#21 家族残留：`CHECKLIST_A` 的
/// 10 条弧线区间整段复刻 `ARC_PARAMS`、`CHECKLIST_D` 的抖音区间无常量锁）。
const CHECKLIST_A: &str = "【校验清单 A·单源】Style Prompt≤${STYLE_PROMPT_MAX}字符且≥${STYLE_PROMPT_MIN}字符；结构标签≥2段；能量差≥${MIN_ENERGY_GAP}级（0-10）；配器差≥${MIN_INSTRUMENT_GAP}件、单段${INSTRUMENT_RANGE}件（最弱段即下限${MIN_INSTRUMENT_WEAK}件，声明'极简段'的段落豁免下限）、最强段≥${MIN_INSTRUMENT_STRONG}件；说明行≤${DESC_LINE_MAX}字符（含方括号整行计，上限内须保乐器+行为动词）；弧线参数：${ARC_INLINE}；Audio Influence=0；断句单空格、禁/与、标点全半角。";
const CHECKLIST_B: &str = "【校验清单 B·单源】Style Prompt≤${STYLE_PROMPT_MAX}字符且≥${STYLE_PROMPT_MIN}字符；结构标签≥2段；能量差≥${MIN_ENERGY_GAP}级（0-10）；配器差≥${MIN_INSTRUMENT_GAP}件、单段${INSTRUMENT_RANGE}件（最弱段即下限${MIN_INSTRUMENT_WEAK}件，声明'极简段'的段落豁免下限）、最强段≥${MIN_INSTRUMENT_STRONG}件；说明行≤${DESC_LINE_MAX}字符（含方括号整行计，上限内须保乐器+行为动词）；参数固定区间：Weirdness ${MODE_B_WEIRD_RANGE}、Style Influence ${MODE_B_STYLE_RANGE}（B 专属区间为准，弧线区间不适用 B）；Audio Influence=0；断句单空格、禁/与、标点全半角。";
const CHECKLIST_C: &str = "【校验清单 C·单源】逐行等字数（差一字即失败，尾部≤${LYRIC_FILL_TAIL}行收尾）；行数与原歌词一致；段落结构与原歌词一致（禁新增Hook/Chorus段）；韵脚位置与模式保留；说明行带方括号且≤${DESC_LINE_MAX}字符（整行计）；Style Prompt≤${STYLE_PROMPT_MAX}字符；断句单空格、禁/与、标点全半角。";
const CHECKLIST_D: &str = "【校验清单 D·单源】Hook≥${HOOK_MIN}次；单段Verse≤${VERSE_MAX_LINES}行；每行≤${DOUYIN_LINE_MAX}字；结尾骤停（一刀切，含abruptly/cut标识）；BPM≥${DOUYIN_BPM_MIN}；Style Prompt≤${STYLE_PROMPT_MAX}字符且≥${STYLE_PROMPT_MIN}字符；说明行≤${DESC_LINE_MAX}字符（含方括号整行计，上限内须保乐器+行为动词）；参数抖音${DOUYIN_WEIRD_RANGE}/${DOUYIN_STYLE_RANGE}（仅当含≥2叙事段Verse/Pre-Chorus/Bridge且方案写明'叙事型'归类，方可回落A/B弧线区间，二者缺一打回）；Audio Influence=0；断句单空格、禁/与、标点全半角。";

/// 阶段 0 地基 primer（模式专属，每模式≤800字；M9 顺序：受众→物件→约束→旋律）。
/// 定位 = **工序顺序锚点**（primer 独有内容：意象家族/声学映射/Verse2 新增信息/借体覆盖/弧线能量标尺），
/// 与 `trigger=阶段0` 的思维资产行**同时注入**主持人（见 `orchestrator::host_craft_injection`）——
/// 二者是同一阶段的两个载体：**规则条文与数值权威在 CSV**（如 LC-01 物件清单下限），
/// primer 只是压缩重申；凡 primer 重申的数值必须有同源锁（`primer_ab_stage0_threshold_matches_lyric_craft_row`），
/// 否则改 CSV 不联动 primer 即复发"上游告知 ≠ 知识库"的降级环。缺失回退无 primer（旧行为）。
pub fn host_primer(mode: &str) -> Option<&'static str> {
    match mode {
        "mode_a" | "mode_b" => Some(PRIMER_AB),
        "mode_c" => Some(PRIMER_C),
        "mode_d" => Some(PRIMER_D),
        _ => None,
    }
}

// A/B：受众一句话 + 画面清单模板 + 借体要求 + 弧线能量标尺。
// 数值规则：常量派生项写 `${占位符}`；**物件清单下限（≥5件具体物）数值权威在知识库 CSV**
// （`lyric_craft::LC-01`），无常量可派，故保留字面量并在 `PROSE_CARRIERS` 登记
// 「CSV 同源锁」`primer_ab_stage0_threshold_matches_lyric_craft_row`（登记即受控，不是漏网）。
const PRIMER_AB: &str = "【阶段0地基·A/B】受众：先一句话定对象阅历与时机（写给谁听、何时听），不到不写。物件：先建时空物件清单（≥5件具体物，禁抽象词开局），再定意象家族（一首歌一个系统）与声学映射（每个意象写出乐器/音色对应）。约束：声调服从旋律走向（硬门），韵脚密度为软优化；Verse2须新增信息；借体覆盖：每个抽象词配具体物象，禁裸奔。旋律骨架：先定弧线10选1与能量标尺（0-2静止/3-4铺垫/5-6推进/7-8爆发/9-10用尽全力），弱强差≥${MIN_ENERGY_GAP}级，配器差≥${MIN_INSTRUMENT_GAP}件，全曲≤${INSTRUMENT_MAX}件。";
// C：对齐铁律 + 节奏保留 + 借体。
const PRIMER_C: &str = "【阶段0地基·C】对齐铁律：逐行等字数（差一字即失败）、行数与原歌词一致、段落结构与原歌词一致（禁新增段）。节奏保留：词组切分与原歌词一致（3+4、2+2+3等），呼吸点位置一致，韵脚位置与模式保留。借体：新意象转译原意象叙事功能（非替换），意象家族统一，抽象词每个有借体，套话具体化。";
// D：前3秒画面 + 道具刻度 + 骤停提醒 + 字数提醒。
const PRIMER_D: &str = "【阶段0地基·D】前3秒：开场3秒内建钩子或强画面，直接进内容不慢铺垫。道具刻度：核心情感绑有世俗重量的具体物（物作刻度），金句短、魔性、可独立传播，Hook≥${HOOK_MIN}次。约束：Verse≤${VERSE_MAX_LINES}行，每行≤${DOUYIN_LINE_MAX}字，说明行≤${DESC_LINE_MAX}字符（含方括号整行计），BPM≥${DOUYIN_BPM_MIN}。收尾：结尾骤停一刀切（不渐弱），动态标签用对，骤停后无乐器残留。";

/// 上游告知载体（`&'static str`，直接进 LLM 上下文的 prose）注册表——**守护网扫描面单源**。
///
/// 旧守护网 `no_handwritten_rule_numbers_in_prompt_sources` 只扫 `prompts.rs`/`roles.rs`
/// 两个文件，把同类载体 `CHECKLIST_A–D` / `PRIMER_AB/C/D`（就在本文件）漏在网外——
/// 于是"改常量改 CSV 不联动文案"的 #21 家族缺陷在本文件内继续存活。现在扫描面由本表声明：
/// 新增载体必须登记，登记即被 `no_handwritten_rule_numbers_in_prose_carriers` 扫描。
///
/// `csv_locked` = 显式豁免登记：字面量片段的数值权威在**知识库 CSV**（无常量可派生），
/// 必须写明锁定它的测试名——登记条目不成立（测试名不存在/豁免未覆盖实际命中）即红。
pub struct ProseCarrier {
    pub name: &'static str,
    pub text: &'static str,
    pub csv_locked: &'static [(&'static str, &'static str)],
}

pub const PROSE_CARRIERS: &[ProseCarrier] = &[
    ProseCarrier { name: "CHECKLIST_A", text: CHECKLIST_A, csv_locked: &[] },
    ProseCarrier { name: "CHECKLIST_B", text: CHECKLIST_B, csv_locked: &[] },
    ProseCarrier { name: "CHECKLIST_C", text: CHECKLIST_C, csv_locked: &[] },
    ProseCarrier { name: "CHECKLIST_D", text: CHECKLIST_D, csv_locked: &[] },
    // ≥5件具体物：数值权威在 lyric_craft::LC-01（CSV），无常量可派生，由专锁跨载体同源
    ProseCarrier {
        name: "PRIMER_AB",
        text: PRIMER_AB,
        csv_locked: &[("≥5件具体物", "primer_ab_stage0_threshold_matches_lyric_craft_row")],
    },
    ProseCarrier { name: "PRIMER_C", text: PRIMER_C, csv_locked: &[] },
    ProseCarrier { name: "PRIMER_D", text: PRIMER_D, csv_locked: &[] },
];

/// C3/D3：prompt 数值单源插值——`${NAME}` 占位符替换为常量派生值。
/// prompts.rs/roles.rs 的静态文本不得手写注册表辖域数字（守护测试锁定）；
/// 无占位符文本原样返回（幂等——用户 override 文件可不写占位符）。
pub fn interpolate(text: &str) -> String {
    let mut out = text.to_string();
    for (key, value) in placeholder_pairs() {
        out = out.replace(key, &value);
    }
    out
}

/// 占位符→值映射表（单源：全部由 rules 常量派生，无手写数字）。
fn placeholder_pairs() -> Vec<(&'static str, String)> {
    vec![
        ("${STYLE_PROMPT_MAX}", STYLE_PROMPT_MAX_CHARS.to_string()),
        ("${STYLE_PROMPT_MIN}", STYLE_PROMPT_MIN_CHARS.to_string()),
        ("${DOUYIN_WEIRD_RANGE}", format!("{}-{}", DOUYIN_WEIRD_MIN, DOUYIN_WEIRD_MAX)),
        ("${DOUYIN_STYLE_RANGE}", format!("{}-{}", DOUYIN_STYLE_MIN, DOUYIN_STYLE_MAX)),
        ("${MODE_B_WEIRD_RANGE}", format!("{}-{}", MODE_B_WEIRD_MIN, MODE_B_WEIRD_MAX)),
        ("${MODE_B_STYLE_RANGE}", format!("{}-{}", MODE_B_STYLE_MIN, MODE_B_STYLE_MAX)),
        ("${DESC_LINE_MAX}", DESC_LINE_MAX_CHARS.to_string()),
        ("${DOUYIN_LINE_MAX}", DOUYIN_LINE_MAX_CHARS.to_string()),
        ("${LYRIC_CHARS_VERSE}", format!("{}-{}", LYRIC_LINE_VERSE_MIN, LYRIC_LINE_VERSE_MAX)),
        ("${LYRIC_CHARS_CHORUS}", format!("{}-{}", LYRIC_LINE_CHORUS_MIN, LYRIC_LINE_CHORUS_MAX)),
        ("${LYRIC_LINE_VERSE_MAX}", LYRIC_LINE_VERSE_MAX.to_string()),
        ("${DOUYIN_BPM_MIN}", DOUYIN_BPM_MIN.to_string()),
        ("${HOOK_MIN}", HOOK_MIN_COUNT.to_string()),
        ("${VERSE_MAX_LINES}", VERSE_MAX_LINES.to_string()),
        ("${LYRIC_FILL_TAIL}", LYRIC_FILL_TAIL_ALLOW.to_string()),
        ("${MIN_ENERGY_GAP}", MIN_ENERGY_GAP.to_string()),
        ("${MIN_INSTRUMENT_GAP}", MIN_INSTRUMENT_GAP.to_string()),
        ("${MIN_INSTRUMENT_WEAK}", MIN_INSTRUMENT_WEAK.to_string()),
        ("${MIN_INSTRUMENT_STRONG}", MIN_INSTRUMENT_STRONG.to_string()),
        ("${INSTRUMENT_MAX}", INSTRUMENT_MAX.to_string()),
        ("${INSTRUMENT_RANGE}", format!("{}-{}", MIN_INSTRUMENT_WEAK, INSTRUMENT_MAX)),
        ("${MINIMAL_SECTION_EXEMPT}", minimal_section_exemption()),
        ("${ARC_TABLE}", arc_table_lines()),
        ("${ARC_INLINE}", arc_inline_list()),
        ("${TERRITORY_RULES}", territory_rules_text()),
    ]
}

/// 弧线参数表（从 ARC_PARAMS 运行时生成，供模式 prompt 的弧线十选一表插值）。
fn arc_table_lines() -> String {
    ARC_PARAMS
        .iter()
        .map(|(name, wmin, wmax, smin, smax)| {
            let disp = if name.ends_with("型") { name.to_string() } else { format!("{}型", name) };
            format!("- {}：Weirdness {}-{} | Style Influence {}-{}", disp, wmin, wmax, smin, smax)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 弧线区间**紧凑串**（`标准叙事22-28/78-83、全程高能10-15/85-90、…`）——供 `CHECKLIST_A` 内联。
/// 单源 = `ARC_PARAMS`：旧实现清单里整段手写复刻这 10 组区间（改 `ARC_PARAMS` 不联动）。
fn arc_inline_list() -> String {
    ARC_PARAMS
        .iter()
        .map(|(name, wmin, wmax, smin, smax)| format!("{}{}-{}/{}-{}", name, wmin, wmax, smin, smax))
        .collect::<Vec<_>>()
        .join("、")
}

/// C4/D4：领地终裁表——冲突裁决的唯一结构化真源（依据, 领域, 终裁者）；
/// 与 roles.rs 各角色领地声明文字同源（双向锁定测试）。
pub const TERRITORY_RULES: &[(&str, &str, &str)] = &[
    ("R-2", "金句/Hook 文字形态", "作词人"),
    ("R-2", "Hook 次数/位置/骤停/传播动态", "流行风格分析师"),
    ("R-3", "人声设计", "制作人"),
    ("R-3", "参数与弧线匹配", "情感分析师"),
    // D-Envelope：说明行此前三方（制作人配器/情感质地/主持人能量）各自往一行塞内容导致
    // 80 字上限 9/10-79% 违规率——收敛为制作人单一所有者，其他角色只提交素材；
    // 上限值不在本表复述（数字单源 see DESC_LINE_MAX_CHARS，本表只管归属）
    ("R-4", "说明行最终形态（限长合成）", "制作人"),
];

/// C4/D4：领地声明文本（`TERRITORY_RULES` 的 prose 形态，单源）。
/// **两个载体同读这一份**：主持人整合职责条款（`roles::host()` 的 `${TERRITORY_RULES}`）
/// 与冲突裁决指引（`territory_adjudication_text`）——#14 根因之一就是这两处各写各的口径：
/// 人设说"只做整合、不自己改细节"，user 侧却要求"按领地声明裁决"，同一职责两个口径，
/// 冲突裁决被 system 人设抵消（详见 `ADJUDICATION_KEYS` 守护测试）。
pub fn territory_rules_text() -> String {
    TERRITORY_RULES
        .iter()
        .map(|(id, domain, owner)| format!("{}：{}归{}", id, domain, owner))
        .collect::<Vec<_>>()
        .join("；")
}

/// C4/D4：冲突裁决指引文本（注入主持人汇总输入的冲突条目前）。
/// 内容由 TERRITORY_RULES 生成——表改这里自动跟随，不允许手写裁决清单。
pub fn territory_adjudication_text(role_a: &str, role_b: &str, target: &str) -> String {
    format!(
        "\n⚠️ 冲突：{} 与 {} 同时修订了 {}。\n请按领地声明裁决（{}），并在方案后注明取舍理由。\n",
        role_a,
        role_b,
        target,
        territory_rules_text()
    )
}

/// #14 冲突裁决措辞单源（守护测试用）：主持人**人设**与**冲突裁决指引**是两个载体，
/// 二者必须都命中这三个语义锚点——否则"人设禁止改细节 / 指引要求裁决"的对立口径复发。
/// 载体：`roles::host().system_prompt`（经 interpolate）与 `territory_adjudication_text`。
pub const ADJUDICATION_KEYS: &[&str] = &["领地声明", "裁决", "取舍理由"];

/// 生产段提取（**测试专用**共享工具：本文件与 `commands/orchestrator.rs` 的文本级守护共用）。
///
/// 语义：删掉每个 `#[cfg(test)]` 属性行及其后到**列 0 的收尾 `}`** 为止的测试条目，返回真正的生产代码。
///
/// 为什么不能用"取第一个 `#[cfg(test)]` 之前"（第十三批 D5 实测缺陷，属"绿灯空转"）：
/// `models/mod.rs` 的测试模块位于文件**中段**（L68），其后的 `too_long`/`validate_feedback`/
/// `validate_request` 全在测试模块**之后**——旧写法下 models 的"生产段"只剩前 67 行，
/// 于是"不得复述限额数值""不得出现凭据形状字面量"两条断言对 models **完全空转**
/// （实测：往 `too_long` 上加一行"上限 2000"注释，断言照旧绿灯）。
/// 判据用 rustfmt 的固定形态：`#[cfg(test)]` 行与顶层测试模块的收尾 `}` 都在列 0，
/// 模块内嵌套项的收尾 `}` 一律带缩进——故"列 0 的 `}`"即模块结束。
/// 提取正确性由 `production_segment_excludes_mid_file_test_module` 双向自证
/// （必须含生产函数、必须不含测试函数）——提取逻辑一旦失效即报红，不会静默空转。
#[cfg(test)]
pub(crate) fn production_segment(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut skipping = false;
    for line in src.split_inclusive('\n') {
        let body = line.trim_end_matches('\n');
        if skipping {
            // 顶层测试模块的收尾：列 0 的 `}`
            if body == "}" {
                skipping = false;
            }
            continue;
        }
        if body.trim_start().starts_with("#[cfg(test)]") {
            skipping = true;
            continue;
        }
        out.push_str(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolate_resolves_param_ranges() {
        let t = "抖音 ${DOUYIN_WEIRD_RANGE}/${DOUYIN_STYLE_RANGE}；B ${MODE_B_WEIRD_RANGE}/${MODE_B_STYLE_RANGE}；上限 ${STYLE_PROMPT_MAX}；说明行 ${DESC_LINE_MAX}";
        let r = interpolate(t);
        assert!(r.contains(&format!("{}-{}", DOUYIN_WEIRD_MIN, DOUYIN_WEIRD_MAX)), "抖音区间: {}", r);
        assert!(r.contains(&format!("{}-{}", DOUYIN_STYLE_MIN, DOUYIN_STYLE_MAX)), "抖音 Style: {}", r);
        assert!(r.contains(&format!("{}-{}", MODE_B_WEIRD_MIN, MODE_B_WEIRD_MAX)), "B 区间: {}", r);
        assert!(r.contains(&format!("{}-{}", MODE_B_STYLE_MIN, MODE_B_STYLE_MAX)), "B Style: {}", r);
        assert!(r.contains(&STYLE_PROMPT_MAX_CHARS.to_string()), "350: {}", r);
        assert!(r.contains(&DESC_LINE_MAX_CHARS.to_string()), "说明行上限: {}", r);
        assert!(!r.contains("${"), "占位符必须全部解析: {}", r);
    }

    #[test]
    fn interpolate_arc_table_generated_from_arc_params() {
        let r = interpolate("${ARC_TABLE}");
        assert_eq!(r.lines().count(), ARC_PARAMS.len(), "弧线表行数=弧线数");
        for (name, wmin, wmax, smin, smax) in ARC_PARAMS {
            let disp = if name.ends_with("型") { name.to_string() } else { format!("{}型", name) };
            let frag = format!("- {}：Weirdness {}-{} | Style Influence {}-{}", disp, wmin, wmax, smin, smax);
            assert!(r.contains(&frag), "弧线表缺 {}", frag);
        }
    }

    #[test]
    fn interpolate_plain_text_unchanged() {
        assert_eq!(interpolate("无占位符文本，Weirdness 12-20 保持原样"), "无占位符文本，Weirdness 12-20 保持原样");
    }

    /// C3 守护：prompts.rs/roles.rs 非注释非测试代码不得手写注册表辖域数字——
    /// 数字只从 rules.rs 常量经 interpolate 流入提示词（防"常量改了文案没改"复发）。
    ///
    /// ⚠️ 分工（第八批单源化）：本测试只负责**两个源文件**这一半扫描面；另一半是
    /// `rules.rs` 内的 `PROSE_CARRIERS`（`CHECKLIST_A–D`/`PRIMER_AB/C/D`），由
    /// `no_handwritten_rule_numbers_in_prose_carriers` 扫描。两者合起来才是完整守护网——
    /// 新增上游告知载体时，若它不在两个源文件里，就必须登记进 `PROSE_CARRIERS`。
    ///
    /// 2026-09-18 升级（审计 #21 + R1 延伸）：旧实现是**手写枚举**禁止表，漏网严重——
    /// 实测 `prompts.rs` 中「最弱段至少 2 件」「配器部分至少 2 件乐器」两处残留 **旧值 2**
    /// （2026-09-06 已判废为 3），与硬校验 `最弱段≥3 件` 直接对立 = 现役降级源。
    /// 现改为「显式字面量 + 从常量派生的『数值+单位』扫描」双网：
    /// 派生网按常量值 + 单位自动生成（新增常量即自动纳入），并跳过 `5-6件`/`3-4件` 这类
    /// 区间的右端（前一字符为 `-` 或数字视为区间写法，不属裸阈值）。
    fn handwritten_rule_number_hits(line: &str) -> Vec<String> {
        /// 阈值语境窗口（字符数）：数值前后各看多少字符内是否出现阈值词
        const THRESHOLD_WINDOW: usize = 12;
        let mut hits: Vec<String> = Vec::new();
        // 网一：区间与符号写法（裸数字无法由常量派生，手写枚举）
        const LITERALS: &[&str] = &[
            // 弧线区间片段（10 弧）+ 模式参数区间（4）
            "22-28", "10-15", "25-35", "70-80", "15-25", "80-90", "28-35", "75-82", "20-28", "78-88",
            "20-30", "78-83", "85-90", "70-82", "78-85", "12-20", "85-95", "20-35", "75-85",
            // 件数/行数/字数区间
            "3-7", "6-13", "4-9",
            // 上限符号写法（console/半角混写都要拦）
            "≤350", "≤200", "≤80", "≤30", "≤10", "≤4",
            "<=350", "<=200", "<=80", "<=10", "<= 10", "<= 4",
            "BPM>=90", "BPM≥90",
        ];
        for lit in LITERALS {
            if line.contains(lit) {
                hits.push((*lit).to_string());
            }
        }
        // 网二：从常量派生的「数值 + 单位」——新增规则常量自动纳入，不再靠人记得加进表
        let derived: &[(usize, &str)] = &[
            (MIN_INSTRUMENT_WEAK, "件"),
            (MIN_INSTRUMENT_STRONG, "件"),
            (MIN_INSTRUMENT_GAP, "件"),
            (INSTRUMENT_MAX, "件"),
            (MIN_ENERGY_GAP as usize, "级"),
            (HOOK_MIN_COUNT, "次"),
            (VERSE_MAX_LINES, "行"),
            (DOUYIN_LINE_MAX_CHARS, "字"),
            (LYRIC_LINE_VERSE_MAX, "字"),
            (STYLE_PROMPT_MIN_CHARS, "字符"),
            (STYLE_PROMPT_MAX_CHARS, "字符"),
            (DESC_LINE_MAX_CHARS, "字符"),
            (LYRIC_FILL_TAIL_ALLOW, "行"),
        ];
        for (num, unit) in derived {
            for needle in [format!("{}{}", num, unit), format!("{} {}", num, unit)] {
                let mut from = 0usize;
                while let Some(rel) = line[from..].find(&needle) {
                    let at = from + rel;
                    // 区间的右端（前一字符是 `-`、数字或 `.`）不算裸阈值，如 `5-6件`、`3-4件`
                    let prev = line[..at].chars().next_back();
                    let is_range = match prev {
                        Some('-') | Some('–') | Some('~') | Some('.') => true,
                        Some(c) => c.is_ascii_digit(),
                        None => false,
                    };
                    // 阈值语境才算违规：`至少/最多/不超/≥/≤/>=/<=/以内/下限/上限` 等，且必须
                    // **紧邻**该数值（前后各 THRESHOLD_WINDOW 字符窗口内）才算。旧实现判"整行是否
                    // 含阈值词"——同一段密度表文案在 prompts.rs（拆成多行）不报、在 roles.rs（挤成
                    // 一行）全报，误报由行宽决定，网不可信。改窗口后与行宽无关。
                    // 逐段密度表的裸件数（"Bridge=7，2件"）是规划示例而非阈值常量，不计违规；
                    // 其与 ≥3 下限的关系由"极简段豁免"条款约束（见 prompts.rs 密度表注记）。
                    // 取"整行 - 该数值之前"的末尾 12 字（**保持原序**——反转会打断"至少/不超过"这类多字词）
                    let head: Vec<char> = line[..at].chars().collect();
                    let before: String = head[head.len().saturating_sub(THRESHOLD_WINDOW)..].iter().collect();
                    let after: String = line[at + needle.len()..].chars().take(THRESHOLD_WINDOW).collect();
                    let is_threshold = THRESHOLD_WORDS
                        .iter()
                        .any(|w| before.contains(w) || after.contains(w));
                    if !is_range && is_threshold {
                        hits.push(needle.clone());
                    }
                    from = at + needle.len();
                }
            }
        }
        hits
    }

    /// 阈值语境词表（判定"数值+单位"是否在讲规则阈值，而非讲规划示例）
    const THRESHOLD_WORDS: &[&str] = &[
        "至少", "最少", "最多", "不超过", "不超", "以上", "以下", "以内",
        "≥", "≤", ">=", "<=", ">", "<", "下限", "上限", "起",
    ];

    #[test]
    fn no_handwritten_rule_numbers_in_prompt_sources() {
        let mut violations: Vec<String> = Vec::new();
        for file in ["src/commands/prompts.rs", "src/commands/roles.rs"] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
            let src = std::fs::read_to_string(&path).unwrap();
            let mut depth_in_tests: i32 = -1; // -1 = 不在测试区
            for (i, raw) in src.lines().enumerate() {
                let trimmed = raw.trim_start();
                if trimmed.starts_with("//") {
                    continue; // 注释白名单
                }
                if depth_in_tests < 0 && trimmed.contains("mod tests") {
                    depth_in_tests = (raw.chars().take_while(|c| *c == ' ').count() / 4) as i32;
                    continue;
                }
                if depth_in_tests >= 0 {
                    let d = (raw.chars().take_while(|c| *c == ' ').count() / 4) as i32;
                    if d <= depth_in_tests && trimmed.starts_with('}') {
                        depth_in_tests = -1; // 测试区结束
                    }
                    continue; // 测试区白名单
                }
                // `${...}` 占位符行天然合法：占位符名不含数字，无需排除
                for hit in handwritten_rule_number_hits(raw) {
                    violations.push(format!("{}:{} \"{}\"", file, i + 1, hit));
                }
            }
        }
        assert!(
            violations.is_empty(),
            "手写规则数字 {} 处——请改用 ${{占位符}} 经 rules::interpolate 注入：\n{}",
            violations.len(),
            violations.join("\n")
        );
    }

    /// 新网自检（红灯先行）：旧实现漏网的两条真实现役降级源必须被网二抓到——
    /// mode_d「最弱段至少 2 件」与「配器部分至少 2 件乐器」，旧值 2 与硬校验 ≥3 直接对立。
    #[test]
    fn handwritten_guard_catches_known_leaks() {
        for leak in [
            "配器最弱段至少 2 件，最强段至少 5 件最多不超过 7 件。",
            "- 三要素（配器+动态+人声）必须完整，配器部分至少 2 件乐器",
            "最弱 vs 最强差 >= 3 级（0-10）",
            "- 全局核心乐器 3-7 件，全曲不超 7 件",
            "每行 6-13 字为宜，Chorus 更短（4-9 字）",
            "- 每行 <= 10 字（一屏能装下）",
        ] {
            let hits = handwritten_rule_number_hits(leak);
            assert!(
                !hits.is_empty(),
                "升级后的守护网必须抓到该漏网写法：{} → 命中 {:?}",
                leak,
                hits
            );
        }
        // 区间右端与合法占位符不得误报（`5-6件`/`1-2件` 是密度表区间，不是阈值）
        for ok in [
            "· Hook：全程高位=9，3-4件，持续高压；先压后炸=9-10，5件，突然爆发",
            "先压后炸=3-4，1-2件，制造反差",
            "最弱段用至少 ${MIN_INSTRUMENT_WEAK} 件乐器",
            "每行 ${LYRIC_CHARS_VERSE} 字为宜",
        ] {
            assert!(
                handwritten_rule_number_hits(ok).is_empty(),
                "误报：{} → {:?}",
                ok,
                handwritten_rule_number_hits(ok)
            );
        }
    }

    /// 审计 #21 家族残留（第八批）：**上游告知载体的守护网单源化**。
    ///
    /// 旧网只扫 `prompts.rs`/`roles.rs` 两个文件；同类载体 `CHECKLIST_A–D`、`PRIMER_AB/C/D`
    /// 就在本文件（rules.rs）却漏在网外——实测 `CHECKLIST_A` 整段复刻 `ARC_PARAMS` 的 10 组
    /// 弧线区间、`CHECKLIST_D` 手写抖音区间、`PRIMER_D` 手写 Hook/Verse/字数/BPM 四阈值，
    /// 全靠零散 verbatim 断言兜底（覆盖不全），改常量即静默漂移。
    /// 现扫描面由 `PROSE_CARRIERS` 声明（新增载体必须登记），并对"CSV 同源豁免"做双向校验：
    /// 未登记的字面量即红；登记了却无实际命中的豁免即红（防无主豁免随时间腐化）。
    #[test]
    fn no_handwritten_rule_numbers_in_prose_carriers() {
        assert!(PROSE_CARRIERS.len() >= 7, "载体注册表条目数异常: {}", PROSE_CARRIERS.len());
        let mut violations: Vec<String> = Vec::new();
        for c in PROSE_CARRIERS {
            let hits = handwritten_rule_number_hits(c.text);
            for hit in &hits {
                let exempt = c
                    .csv_locked
                    .iter()
                    .any(|(lit, lock)| lit.contains(hit.as_str()) && !lock.is_empty());
                if !exempt {
                    violations.push(format!(
                        "{} 手写规则数字 \"{}\"——请改用 ${{占位符}} 经 rules::interpolate 派生，\
                         或（数值权威在 CSV 时）在 PROSE_CARRIERS 登记 csv_locked 并写明锁定测试名",
                        c.name, hit
                    ));
                }
            }
            // 反向：登记的豁免必须真的覆盖到命中（否则是无主豁免）
            for (lit, lock) in c.csv_locked {
                assert!(!lock.is_empty(), "{} 的 CSV 豁免 \"{}\" 未写锁定测试名", c.name, lit);
                assert!(
                    hits.iter().any(|h| lit.contains(h.as_str())),
                    "{} 登记了 CSV 豁免 \"{}\"（锁 {}）但扫描网未命中——豁免已失效/表述已改，请清理登记",
                    c.name, lit, lock
                );
            }
        }
        assert!(
            violations.is_empty(),
            "上游告知载体手写规则数字 {} 处：\n{}",
            violations.len(),
            violations.join("\n")
        );
    }

    /// 载体守护网自检（红灯先行）：把 `CHECKLIST_D` 的占位符还原成手写字面量必须被网住，
    /// 且 `PRIMER_AB` 的 CSV 同源豁免必须恰好覆盖「≥5件」而不掩盖其他数字。
    #[test]
    fn prose_carrier_guard_catches_handwritten_restore() {
        // 模拟"回退到旧写法"：抖音区间 + Hook/Verse 阈值全部手写
        let legacy = "Hook≥2次；单段Verse≤4行；每行≤10字；BPM≥90；参数抖音12-20/85-95；说明行≤200字符。";
        let hits = handwritten_rule_number_hits(legacy);
        for want in ["≤200", "12-20", "85-95", "2次", "4行", "10字", "BPM≥90"] {
            assert!(hits.iter().any(|h| h == want), "红灯先行：旧写法 {} 未被网住（命中 {:?}）", want, hits);
        }
        // 现役 CHECKLIST_D 已全部占位符化：原文不得有命中
        assert!(
            handwritten_rule_number_hits(CHECKLIST_D).is_empty(),
            "CHECKLIST_D 原文仍含手写阈值: {:?}",
            handwritten_rule_number_hits(CHECKLIST_D)
        );
        // PRIMER_AB 的豁免精确性：命中集合应为 {5件}，且被登记覆盖
        let ab_hits = handwritten_rule_number_hits(PRIMER_AB);
        assert_eq!(ab_hits, vec!["5件".to_string()], "PRIMER_AB 命中集合变化，需同步 csv_locked 登记: {:?}", ab_hits);
    }

    /// 判定 `name` 在源码中存在**真实调用点**（排除 `fn name(` 定义与注释行）。
    ///
    /// 第三批升级（红灯先行）：旧实现用 `src.contains(name)` 文件全文文本匹配——
    /// 函数"存在但无人调用"、或"名字被注释/字符串提及"都能过关。实测 `check_style_prompt_blocks`
    /// 只查下限却登记为 `douyin_bpm_min`（不消费 DOUYIN_BPM_MIN）的执行者，靠人工核对才发现。
    /// 调用点判定：出现 `name(` 且其前紧邻非 `fn`（即非定义），且不在注释行内。
    fn has_call_site(src: &str, name: &str) -> bool {
        let needle = format!("{}(", name);
        let mut from = 0usize;
        while let Some(rel) = src[from..].find(&needle) {
            let at = from + rel;
            // 定义排除：前方紧邻（跳过空白）为 `fn`（含 `pub fn` / `async fn`）
            if !src[..at].trim_end().ends_with("fn") {
                // 注释排除：调用点所在行以 `//` 开头
                let line_start = src[..at].rfind('\n').map(|p| p + 1).unwrap_or(0);
                let line = src[line_start..].trim_start();
                if !line.starts_with("//") {
                    return true;
                }
            }
            from = at + needle.len();
        }
        false
    }

    /// 判定 `n` 的十进制写法在源码中作为**独立数字 token** 出现（前后紧邻字符都不是数字）。
    ///
    /// 旧实现直接 `src.contains(&n.to_string())`：`2000` 会命中 `12000` 的**子串**，
    /// 把无关数字误报成"复述限额数值"。误报的代价不是安全，而是**守护网被改松**
    /// （被误伤的人只会去删断言），故改为 token 级判定。
    fn contains_standalone_number(src: &str, n: usize) -> bool {
        let needle = n.to_string();
        let bytes = src.as_bytes();
        let mut from = 0usize;
        while let Some(rel) = src[from..].find(needle.as_str()) {
            let at = from + rel;
            let end = at + needle.len();
            let prev_is_digit = at > 0 && bytes[at - 1].is_ascii_digit();
            let next_is_digit = end < bytes.len() && bytes[end].is_ascii_digit();
            if !prev_is_digit && !next_is_digit {
                return true;
            }
            from = end;
        }
        false
    }

    /// D2 守护：注册表每条规则的执行器必须在 `RULE_EXECUTOR_FILES` 中**存在真实调用点**
    /// （而非仅同名符号出现），且每个登记的消费符号在同一扫描面内有消费者——
    /// 删掉执行点/只定义不接线即红，防止常量退化成"只有承诺没有执行"的死常量
    /// （诊断铁证：参数区间零执行；第三批铁证：style_prompt_max 在 C/D 无执行者、
    /// douyin_bpm_min 错挂不消费其常量的函数）。
    ///
    /// 第十三批 D1：扫描面由**测试内硬编码的两个文件**改为生产侧声明常量 `RULE_EXECUTOR_FILES`。
    /// 旧写法下新增执行者所在文件不会自动入网，规则注册了也照旧绿灯（准入限额族实测）；
    /// 现写法下"执行者不在扫描面"会直接报红，逼出登记动作——漏登记不再静默。
    #[test]
    fn rule_registry_symbols_have_executors() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let files: Vec<(&str, String)> = RULE_EXECUTOR_FILES
            .iter()
            .map(|rel| {
                let body = std::fs::read_to_string(src.join(rel))
                    .unwrap_or_else(|e| panic!("读扫描面文件 {} 失败（路径写错？）: {}", rel, e));
                (*rel, body)
            })
            .collect();
        assert!(RULE_REGISTRY.len() >= 15, "注册表条目数异常: {}", RULE_REGISTRY.len());
        for rule in RULE_REGISTRY {
            assert!(
                files.iter().any(|(_, s)| has_call_site(s, rule.executor)),
                "规则 {} 的执行器 {} 在扫描面 {:?} 中无调用点（只定义不接线；若执行者在新文件，请登记进 rules::RULE_EXECUTOR_FILES）",
                rule.id, rule.executor, RULE_EXECUTOR_FILES
            );
            for sym in rule.symbols {
                assert!(
                    files.iter().any(|(_, s)| s.contains(sym)),
                    "死常量回归：{} 被规则 {} 注册但扫描面内无消费者",
                    sym, rule.id
                );
            }
        }
    }

    /// #26 准入限额单源锁：input / feedback / 插话三个限额的唯一真源在本文件。
    ///
    /// 旧实现把前两个数写成 `models::validate_request` 的**函数局部常量**，后果有三：
    /// ① doc 注释只能手抄数值（注释与常量必然漂移，实际已手抄了一份）；
    /// ② 需要同一规则的中途插话入口无法引用它，只能伪造整个请求去"蹭"校验——
    /// 生产代码因此出现凭据字段 + 占位域名的字面量（Mimosa CWE-798 命中面）；
    /// ③ 下游执行侧（`interject::push`）另写了**更小**的一份私有限额，与准入侧不同源，
    /// 于是"介于两者之间"的意见通过校验后被静默丢弃 = 回报成功却没进槽的假成功。
    /// 本锁五件事：消费点必须引用常量 / 局部常量与旧标识符不得回归 /
    /// 生产段注释与文案不得复述限额数值 / 生产段不得出现凭据形状字面量 /
    /// 越界边界必须由常量派生（+1 越界、等值通过）。
    #[test]
    fn admission_limits_are_single_sourced() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let read = |rel: &str| std::fs::read_to_string(src.join(rel)).expect(rel);
        let models = read("models/mod.rs");
        let orch = read("commands/orchestrator.rs");
        let ij = read("commands/interject.rs");
        // ① 真消费：规则实现唯一，两个入口（整请求准入 / 中途插话）共用
        assert!(models.contains("pub fn validate_feedback("), "models 缺 feedback 规则实现");
        assert!(models.contains("INPUT_MAX_CHARS"), "models 未引用输入上限常量");
        assert!(models.contains("FEEDBACK_MAX_CHARS"), "models 未引用反馈上限常量");
        assert!(orch.contains("models::validate_feedback("), "插话入口未接单源校验");
        assert!(ij.contains("crate::rules::FEEDBACK_MAX_CHARS"), "插话槽未引用单条长度单源常量");
        assert!(ij.contains("crate::rules::INTERJECT_MAX_PER_RUN"), "插话槽未引用条数单源常量");
        // ② 旧声明不得回归（函数局部常量 = 第二份真源；`const ` 前缀避免撞上常量名的子串）
        for legacy in ["const MAX_FEEDBACK", "const MAX_INPUT", "const MAX_TEXT_CHARS", "const MAX_PER_RUN"] {
            assert!(
                !models.contains(legacy) && !ij.contains(legacy) && !orch.contains(legacy),
                "{} 局部常量不得回归",
                legacy
            );
        }
        // ③ 越界夹具必须由常量派生（手抄数字的夹具会在常量变更时静默失真）
        assert!(models.contains("INPUT_MAX_CHARS + 1"), "输入越界夹具须由常量派生");
        assert!(models.contains("FEEDBACK_MAX_CHARS + 1"), "反馈越界夹具须由常量派生");
        assert!(ij.contains("FEEDBACK_MAX_CHARS + 1"), "插话槽越界夹具须由常量派生");
        assert!(orch.contains("FEEDBACK_MAX_CHARS + 1"), "插话入口越界夹具须由常量派生");
        // ④⑤ 生产段（剔除全部 `#[cfg(test)]` 条目）：不复述限额数值，且无凭据形状字面量。
        // 扫描针分片拼装——否则断言自己就成了新的"占位域名/凭据字段"字面量。
        let phantom_host = format!("placeholder{}", ".invalid");
        let credential_assign = format!("api_key{} \"", ":");
        for (name, full, prod_fn, test_fn) in [
            ("models/mod.rs", &models, "pub fn validate_request(", "fn validate_feedback_boundaries("),
            ("commands/orchestrator.rs", &orch, "pub async fn interject_feedback", "fn interject_feedback_enforces_single_sourced_limit("),
            ("commands/interject.rs", &ij, "pub(crate) fn push(", "fn interject_limits_error_instead_of_silent_drop("),
        ] {
            let prod = production_segment(full);
            // 双向自证：提取逻辑一旦失效（如把中段测试模块之后的代码也剔掉），下面两条断言会**静默空转**
            // ——D5 实测过这个失败模式，故先钉住提取正确性：必须含生产函数、必须不含测试函数。
            assert!(prod.contains(prod_fn), "{} 生产段提取异常：缺生产函数 {}", name, prod_fn);
            assert!(!prod.contains(test_fn), "{} 生产段提取异常：未剔除测试函数 {}", name, test_fn);
            for v in [INPUT_MAX_CHARS, FEEDBACK_MAX_CHARS] {
                assert!(
                    !contains_standalone_number(&prod, v),
                    "{} 生产段复述了限额数值 {}——数值唯一真源在本文件，请引用常量",
                    name, v
                );
            }
            assert!(!prod.contains(&phantom_host), "{} 生产段出现占位域名字面量（CWE-798 命中面）", name);
            assert!(!prod.contains(&credential_assign), "{} 生产段出现凭据字段字面量（CWE-798 命中面）", name);
        }
    }

    /// 调用点判定自检（红灯先行）：定义不算调用点、注释提及不算调用点、真实调用才算。
    #[test]
    fn has_call_site_distinguishes_definition_from_invocation() {
        assert!(!has_call_site("pub fn foo(a: u32) -> u32 { a }", "foo"), "定义不得算调用点");
        assert!(!has_call_site("// foo( 只是注释\npub fn bar() {}", "foo"), "注释提及不得算调用点");
        assert!(!has_call_site("fn foo() {}\nlet x = 1;", "foo"), "仅定义无调用应为 false");
        assert!(has_call_site("fn foo() {}\nlet x = foo(1);", "foo"), "真实调用应为 true");
        assert!(has_call_site("async fn bar() { baz(1).await; }", "baz"), "普通调用应为 true");
        assert!(has_call_site("validator::foo(&x)", "foo"), "限定路径调用应为 true");
    }

    /// 生产段提取自证（第十三批 D5 红灯先行）：中段测试模块之后的生产代码必须保留，
    /// 测试代码必须剔除——否则文本级断言会静默空转（D5 实测的失败模式）。
    #[test]
    fn production_segment_excludes_mid_file_test_module() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let models = std::fs::read_to_string(src.join("models/mod.rs")).expect("读 models/mod.rs 失败");
        let prod = production_segment(&models);
        assert!(prod.contains("pub fn validate_request("), "中段测试模块之后的生产代码必须保留");
        assert!(prod.contains("pub(crate) fn too_long("), "中段测试模块之后的生产代码必须保留");
        assert!(!prod.contains("fn validate_feedback_boundaries("), "测试函数必须被剔除");
        assert!(!prod.contains("#[cfg(test)]"), "测试属性行必须被剔除");
        // 提取后不得塌成"只剩文件头"：生产段必须显著长于测试段之外的残片
        assert!(prod.lines().count() > 200, "生产段长度异常（{} 行）——提取逻辑可能剔多了", prod.lines().count());
    }

    /// 独立数字 token 判定自检（第十三批 D3 红灯先行）：子串不得误报，真值必须命中。
    #[test]
    fn standalone_number_ignores_substrings() {
        assert!(contains_standalone_number("上限 = 2000;", 2000), "独立 token 应命中");
        assert!(contains_standalone_number("上限 = 20000;", 20000), "独立 token 应命中");
        assert!(
            !contains_standalone_number("上限 = 12000;", 2000),
            "12000 里的 2000 是子串，不得误报"
        );
        assert!(
            !contains_standalone_number("上限 = 20000;", 2000),
            "20000 里的 2000 是子串，不得误报（两个限额常量互不误伤）"
        );
        assert!(
            !contains_standalone_number("上限 = 20001;", 20000),
            "20001 里的 20000 是子串，不得误报"
        );
        assert!(!contains_standalone_number("上限 = 200;", 2000), "短数字不得命中");
        assert!(!contains_standalone_number("无数字", 2000), "无命中应为 false");
    }

    #[test]
    fn primer_within_800_chars_per_mode() {
        for m in ["mode_a", "mode_b", "mode_c", "mode_d"] {
            // 字数按**插值后**计（primer 原文含 `${占位符}`，未解析文本不是真送达内容）
            let p = interpolate(host_primer(m).expect("四模式 primer 缺失"));
            let n = p.chars().count();
            assert!(n <= 800, "{} primer {} 字超 800", m, n);
            assert!(n > 30, "{} primer 过短", m);
            assert!(!p.contains("${"), "{} primer 占位符未解析: {}", m, p);
        }
    }

    #[test]
    fn primer_m9_order_ab() {
        // M9：受众→物件→约束→旋律
        let p = PRIMER_AB;
        let i_aud = p.find("受众").expect("缺受众段");
        let i_obj = p.find("物件").expect("缺物件段");
        let i_con = p.find("约束").expect("缺约束段");
        let i_arc = p.find("弧线").expect("缺旋律/弧线段");
        assert!(i_aud < i_obj && i_obj < i_con && i_con < i_arc, "primer 段落顺序须为受众→物件→约束→旋律");
    }

    /// #23 修复后的**双载体同源锁**：primer 与「清单」同族，都是阶段0 上游告知载体
    /// （对照 `checklist_contains_single_source_numbers` 对 PRIMER_D 的常量锁）。
    /// #23 让 `trigger=阶段0` 的行真正注入主持人后，`PRIMER_AB` 的「物件：先建时空物件清单
    /// （≥N 件具体物…）」与 `lyric_craft::LC-01`（check 列「清单≥N件具体物…」）成为同一阈值的
    /// 两处载体——数值权威在 CSV，故锁死两处数字相等：只改 CSV 不改 primer 即红，
    /// 防复发"上游告知 ≠ 知识库"的降级环。
    #[test]
    fn primer_ab_stage0_threshold_matches_lyric_craft_row() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("knowledge");
        let kb = crate::knowledge::KnowledgeBase::load(&dir).unwrap();
        let table = kb.table("lyric_craft").expect("lyric_craft 缺失");
        let id_idx = table.header_index("id").unwrap();
        let check_idx = table.header_index("check").unwrap();
        let row = table
            .rows
            .iter()
            .find(|r| r.get(id_idx).map(|s| s.as_str()) == Some("LC-01"))
            .expect("lyric_craft 缺 LC-01 行（#23 阶段0 物件清单条文）");
        let check = &row[check_idx];
        let n: usize = check
            .split('≥')
            .nth(1)
            .map(|s| s.chars().take_while(|c| c.is_ascii_digit()).collect::<String>())
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| panic!("LC-01 check 列缺「≥N件」阈值表述（primer 同源锁的取值基准）: {}", check));
        assert!(
            interpolate(PRIMER_AB).contains(&format!("≥{}件", n)),
            "PRIMER_AB 物件清单下限与 LC-01 双载体不同源（CSV={}，primer 须含「≥{}件」）: {}",
            n,
            n,
            interpolate(PRIMER_AB)
        );
    }

    #[test]
    fn checklist_contains_single_source_numbers() {
        // 清单数字必须与常量一致（verbatim 锁）
        let ab = checklist("mode_a");
        assert!(ab.contains(&format!("≤{}", STYLE_PROMPT_MAX_CHARS)), "缺350");
        assert!(ab.contains(&format!("≥{}", MIN_ENERGY_GAP)), "缺能量差3");
        assert!(ab.contains(&format!("≥{}件", MIN_INSTRUMENT_GAP)), "缺配器差2");
        // L-3：单段下限统一为 3 后，清单不得残留"最弱段≥2件"旧口径
        assert!(ab.contains("单段3-7件"), "缺单段3-7");
        assert!(!ab.contains("最弱段≥2件"), "残留最弱段2件旧口径");
        // R-1：B 清单参数准绳是 B 专属区间，弧线区间不适用 B
        let b = checklist("mode_b");
        assert!(b.contains(&format!("{}-{}", MODE_B_WEIRD_MIN, MODE_B_WEIRD_MAX)), "缺B区间20-35");
        assert!(b.contains(&format!("{}-{}", MODE_B_STYLE_MIN, MODE_B_STYLE_MAX)), "缺B区间75-85");
        assert!(b.contains("B 专属区间为准"), "缺B优先级声明");
        assert!(!b.contains("22-28/78-83"), "B 清单不得混入弧线区间表");
        assert!(!b.contains("弧线参数："), "B 清单不得含弧线参数段");
        assert!(ab.contains("7件"), "缺7件上限");
        let d = checklist("mode_d");
        assert!(d.contains(&format!("≥{}次", HOOK_MIN_COUNT)), "缺Hook2");
        assert!(d.contains(&format!("≤{}行", VERSE_MAX_LINES)), "缺Verse4");
        assert!(d.contains(&format!("≤{}字", DOUYIN_LINE_MAX_CHARS)), "缺10字");
        assert!(d.contains(&format!("≥{}", DOUYIN_BPM_MIN)), "缺BPM90");
        let c = checklist("mode_c");
        assert!(c.contains(&format!("≤{}行", LYRIC_FILL_TAIL_ALLOW)), "缺尾部2行");
        // 说明行上限（2026-09-18：80→200）：四模式清单都告知同一数字——上游告知与下游门同源，
        // 任一模式漏告知即"上游不知道、下游硬拦"的降级源（旧实现 A/B/C 三模式零告知）
        for m in ["mode_a", "mode_b", "mode_c", "mode_d"] {
            let cl = checklist(m);
            assert!(cl.contains(&format!("≤{}", DESC_LINE_MAX_CHARS)), "{} 清单缺说明行上限 {}", m, DESC_LINE_MAX_CHARS);
            assert!(cl.contains("说明行"), "{} 清单缺说明行条目", m);
        }
        // A/B 极简段豁免是下游实装（validator::declared_minimal_sections）的放宽口，上游必须告知
        for m in ["mode_a", "mode_b"] {
            assert!(checklist(m).contains("极简段"), "{} 清单缺极简段豁免告知", m);
        }
        // D primer 与常量同口径（primer 与清单同为上游告知载体；primer 原文已占位符化，
// 数值断言一律针对**插值后**文本——"原文写法"由 no_handwritten_rule_numbers_in_prose_carriers 管）
        assert!(interpolate(PRIMER_D).contains(&format!("说明行≤{}字符", DESC_LINE_MAX_CHARS)), "D primer 说明行上限与常量不一致");
        // 全部上游告知载体的插值结果不得残留未解析占位符
        for m in ["mode_a", "mode_b", "mode_c", "mode_d"] {
            assert!(!checklist(m).contains("${"), "{} 清单占位符未解析", m);
        }
    }

    /// 极简段豁免：单源（`minimal_section_exemption()`）+ 模式域与执行者严格一致（A/B）。
    /// 上游缺告知（旧版 mode_b 零告知）或与执行者对立（旧版 A/B 都写"Intro/Outro 不豁免"，
    /// 而执行者 `declared_minimal_sections` 对任何声明的段落都豁免）都会打回降级，
    /// 故锁死三件事：条款数字与常量同源、A/B 必须引用、C/D 不得引用。
    #[test]
    fn minimal_section_exemption_is_ab_only_and_single_sourced() {
        let clause = minimal_section_exemption();
        assert!(clause.contains("极简段"), "条款缺标记词");
        assert!(clause.contains(&format!("{} 件", MIN_INSTRUMENT_WEAK)), "条款缺最弱段下限 {}", MIN_INSTRUMENT_WEAK);
        assert!(clause.contains(&format!("≥{} 件", MIN_INSTRUMENT_GAP)), "条款缺配器差 {}", MIN_INSTRUMENT_GAP);
        assert!(clause.contains(&format!("≥{} 件", MIN_INSTRUMENT_STRONG)), "条款缺最强段下限 {}", MIN_INSTRUMENT_STRONG);
        assert!(clause.contains(&format!("≤{} 件", INSTRUMENT_MAX)), "条款缺全曲上限 {}", INSTRUMENT_MAX);
        // 注册表：豁免是 A/B 专属规则，执行者唯一（"每条硬门规则一个条目"）
        let rule = RULE_REGISTRY
            .iter()
            .find(|r| r.id == "minimal_section_exemption")
            .expect("极简段豁免规则未登记");
        assert_eq!(rule.modes.to_vec(), vec!["mode_a", "mode_b"], "豁免规则模式域应为 A/B");
        assert_eq!(rule.executor, "declared_minimal_sections", "豁免执行者唯一");
        // A/B：prompt 与清单必须都告知（上游无告知 = 下游硬拦）
        for (m, p) in [
            ("mode_a", crate::commands::prompts::mode_a_system_prompt()),
            ("mode_b", crate::commands::prompts::mode_b_system_prompt()),
        ] {
            assert!(p.contains("${MINIMAL_SECTION_EXEMPT}"), "{} prompt 未引用豁免条款单源", m);
            assert!(checklist(m).contains("极简段"), "{} 清单未告知豁免", m);
        }
        // C/D：无执行者（validate_lyric_fill / validate_douyin 均不查配器件数），不得告知
        for (m, p) in [
            ("mode_c", crate::commands::prompts::mode_c_system_prompt()),
            ("mode_d", crate::commands::prompts::mode_d_system_prompt()),
        ] {
            assert!(!p.contains("${MINIMAL_SECTION_EXEMPT}"), "{} 无执行者却引用豁免条款（承诺没人执行的门）", m);
            assert!(!checklist(m).contains("极简段"), "{} 无执行者却在清单告知豁免", m);
        }
    }

    /// 上游承诺必须与下游执行者同域（R1 根因的镜像）：mode_d 的配器/能量硬门没有执行者
    /// （`validate_for_mode("mode_d")` → `validate_douyin` 不查件数与能量差），故其 prompt
    /// 不得引用这些 A/B 专属阈值占位符——引用了就是"上游立下没人执行的承诺"，与"上游零告知"
    /// 一样制造降级环（已改写为带参考件数的设计方向）。反向断言防误删把 A/B 门一起削弱。
    #[test]
    fn mode_d_prompt_has_no_unenforced_instrument_or_energy_placeholders() {
        let d = crate::commands::prompts::mode_d_system_prompt();
        for ph in [
            "${MIN_INSTRUMENT_WEAK}",
            "${MIN_INSTRUMENT_STRONG}",
            "${MIN_INSTRUMENT_GAP}",
            "${INSTRUMENT_MAX}",
            "${INSTRUMENT_RANGE}",
            "${MIN_ENERGY_GAP}",
        ] {
            assert!(!d.contains(ph), "mode_d 引用了无执行者的阈值占位符 {}（A/B 专属硬门）", ph);
        }
        for p in [
            crate::commands::prompts::mode_a_system_prompt(),
            crate::commands::prompts::mode_b_system_prompt(),
        ] {
            assert!(p.contains("${INSTRUMENT_RANGE}"), "A/B prompt 丢了配器区间门");
        }
    }

    #[test]
    fn arc_params_cover_ten_arcs() {
        assert_eq!(ARC_PARAMS.len(), 10, "弧线须10种");
        let ab = checklist("mode_a");
        for (name, wmin, wmax, smin, smax) in ARC_PARAMS {
            assert!(ab.contains(name), "清单缺弧线 {}", name);
            // K-2：断言完整 "W-W/S-S" 片段——只查 Weirdness 段时 Style Influence 数字漂移漏检
            let frag = format!("{}-{}/{}-{}", wmin, wmax, smin, smax);
            assert!(ab.contains(&frag), "清单缺完整弧线区间 {}", frag);
        }
    }

    #[test]
    fn arc_high_matches_csv_structured_value() {
        // Q1真源锁：全程高能 Style Influence 上限以 CSV style_arc_high 为准（85-90）。
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("knowledge");
        let kb = crate::knowledge::KnowledgeBase::load(&dir).unwrap();
        let table = kb.table("suno_rules").expect("suno_rules 缺失");
        let rule_idx = table.header_index("rule").expect("缺 rule 列");
        let min_idx = table.header_index("value_min").expect("缺 value_min 列");
        let max_idx = table.header_index("value_max").expect("缺 value_max 列");
        let row = table.rows.iter().find(|r| r.get(rule_idx).map(|s| s.as_str()) == Some("style_arc_high")).expect("缺 style_arc_high 行");
        let csv_min: u32 = row[min_idx].parse().expect("value_min 非数字");
        let csv_max: u32 = row[max_idx].parse().expect("value_max 非数字");
        let (_, _, _, smin, smax) = ARC_PARAMS.iter().find(|(n, _, _, _, _)| *n == "全程高能").expect("缺全程高能");
        assert_eq!((*smin, *smax), (csv_min, csv_max), "全程高能须与 CSV style_arc_high 同源");
    }

    /// R1 真源锁：suno_rules.csv 的 desc_line_max 结构化值必须与 DESC_LINE_MAX_CHARS 一致。
    /// CSV 会渲染进校验员/制作人提示词——它是同一个上限的第三处载体，
    /// 不同步即复发"上游告知 80 / 下游门 200"的必然降级。
    #[test]
    fn desc_line_max_matches_csv_structured_value() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("knowledge");
        let kb = crate::knowledge::KnowledgeBase::load(&dir).unwrap();
        let table = kb.table("suno_rules").expect("suno_rules 缺失");
        let rule_idx = table.header_index("rule").expect("缺 rule 列");
        let max_idx = table.header_index("value_max").expect("缺 value_max 列");
        let desc_idx = table.header_index("description").expect("缺 description 列");
        let row = table.rows.iter().find(|r| r.get(rule_idx).map(|s| s.as_str()) == Some("desc_line_max")).expect("缺 desc_line_max 行");
        let csv_max: usize = row[max_idx].parse().expect("value_max 非数字");
        assert_eq!(csv_max, DESC_LINE_MAX_CHARS, "desc_line_max 须与 DESC_LINE_MAX_CHARS 同源");
        assert!(
            row[desc_idx].contains(&DESC_LINE_MAX_CHARS.to_string()),
            "desc_line_max 描述须写明上限（该文本会渲染进提示词）: {}",
            row[desc_idx]
        );
    }

    /// 审计 #21：歌词行字数族（Verse 6-13 / Chorus 4-9）曾手写在 roles.rs+prompts.rs 共 4 处，
    /// 与 CSV 脱钩——改 CSV 提示词不联动。现锁死：CSV 结构化列↔常量↔描述文本三向一致，
    /// 且占位符渲染值必须等于 CSV 区间（上游告知的数字 = 知识库的数字）。
    #[test]
    fn lyric_chars_match_csv_structured_value() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("knowledge");
        let kb = crate::knowledge::KnowledgeBase::load(&dir).unwrap();
        let table = kb.table("suno_rules").expect("suno_rules 缺失");
        let rule_idx = table.header_index("rule").expect("缺 rule 列");
        let min_idx = table.header_index("value_min").expect("缺 value_min 列");
        let max_idx = table.header_index("value_max").expect("缺 value_max 列");
        let desc_idx = table.header_index("description").expect("缺 description 列");
        for (rule, lo, hi) in [
            ("line_chars_verse", LYRIC_LINE_VERSE_MIN, LYRIC_LINE_VERSE_MAX),
            ("line_chars_chorus", LYRIC_LINE_CHORUS_MIN, LYRIC_LINE_CHORUS_MAX),
        ] {
            let row = table
                .rows
                .iter()
                .find(|r| r.get(rule_idx).map(|s| s.as_str()) == Some(rule))
                .unwrap_or_else(|| panic!("suno_rules 缺 {} 行", rule));
            assert_eq!(row[min_idx].parse::<usize>().unwrap(), lo, "{} value_min 须与常量同源", rule);
            assert_eq!(row[max_idx].parse::<usize>().unwrap(), hi, "{} value_max 须与常量同源", rule);
            assert!(
                row[desc_idx].contains(&format!("{}-{}", lo, hi)),
                "{} 描述须写明区间（该文本会渲染进提示词）: {}",
                rule,
                row[desc_idx]
            );
        }
        // 占位符渲染值 = CSV 区间（有人手改 CSV 忘了改常量，或反之，都红）
        let rendered = interpolate("${LYRIC_CHARS_VERSE}|${LYRIC_CHARS_CHORUS}|${LYRIC_LINE_VERSE_MAX}|${DOUYIN_LINE_MAX}");
        assert_eq!(
            rendered,
            format!(
                "{}-{}|{}-{}|{}|{}",
                LYRIC_LINE_VERSE_MIN, LYRIC_LINE_VERSE_MAX,
                LYRIC_LINE_CHORUS_MIN, LYRIC_LINE_CHORUS_MAX,
                LYRIC_LINE_VERSE_MAX, DOUYIN_LINE_MAX_CHARS
            )
        );
    }
}

// ===========================================================================
// D-Envelope（2026-09-09）：方案信封契约——收敛方案的文档结构单一真源
// ===========================================================================
// 根因（40 例大型实测）：主持人自由 markdown 输出（```围栏/表格/元话语混入方案正文），
// 保真校验靠启发式分类器"猜"哪些行是歌词——猜错 212/263（81%）。
// 彻底修复：方案必须用固定信封分节，下游按节处理，"猜"这个动作退役。
// 开关 plan_envelope_enabled()（默认开）：OFF 回退自由格式 + 启发式分类（旧行为）。

/// 信封节标记（顺序固定；NOTES 可省略）
pub const ENV_LYRICS: &str = "<<<LYRICS>>>";
pub const ENV_STYLE: &str = "<<<STYLE>>>";
pub const ENV_PARAMS: &str = "<<<PARAMS>>>";
pub const ENV_NOTES: &str = "<<<NOTES>>>";

/// 信封规范正文（不含说明行契约行——该行带数字，由 `envelope_spec()` 拼接，防手写漂移）。
/// 节序 NOTES-first：顺应模型"先思考后产出"天性（40 例实测分析散文前置的固有习惯），
/// 方法论四步的产出全部归属 NOTES 节，格式宪法与方法论不再冲突。
const ENVELOPE_SPEC_BASE: &str = "\
【方案信封契约（最高优先级，覆盖一切格式化冲动）】你的方案必须且只能按以下节输出，节标记独立成行、一字不差：
<<<NOTES>>>
（方法论要求的逐项分析、情感翻译、质量审查结论、裁决理由、整合说明——全部且只能写进本节）
<<<LYRICS>>>
（歌词正文：结构标签行 + 说明行 + 歌词行。Style Prompt 行与参数行不得写进本节）
<<<STYLE>>>
（Style Prompt 行，含\"Style Prompt:\"标签）
<<<PARAMS>>>
（参数行，如 Weirdness=18|StyleInfluence=92|AudioInfluence=0）
节标记之外不得输出任何文字；禁止使用 markdown 代码围栏（```）、表格（|---|）、标题（#）等任何额外格式，NOTES 节内的结构化内容用缩进排版。";

/// 说明行契约全文（单源，含数字）：上游告知与下游强制同读一段。
///
/// 三条格式形态条款（2026-09-18 补，审计 #20 剩余）：乐器主次排列、人声映射序列、能量标注
/// 书写格式——旧版只写在 `roles::auditor()`（下游格式端口）里，上游主持人完全看不见，
/// 于是主持人产出不合下游规范的排版 → 校验员按自己那份规范改写 → 与转写契约"逐字保留"
/// 冲突 → TRANSCRIPTION_ISSUE 回炉。格式契约只在下游 = 必然降级环，现上移单源。
///
/// 注入策略（见 `host_system_with`）：信封开关开 → 随 `envelope_spec()` 注入；
/// 开关关 → 由 `host_system_with` 单独注入本段，保证"上游告知"不随开关消失。
pub fn desc_line_contract() -> String {
    format!(
        "【说明行契约（全模式统一）】每段说明行形如 [乐器1+行为, 乐器2+行为, …, 空间/力度, 人声状态]（A/B 模式另在行尾标 能量:X），整行含方括号 ≤{} 字符。上限宽松：不得为缩短而删乐器或行为动词（最强段 ≥5 件、单段 3-7 件照常执行），也不得靠堆修饰词占满；超过上限的说明行会被信封门与终稿硬校验拦下。\n\
1. 乐器主次：乐器逐个列名 + 行为动词，按主次排列（主奏在前、支撑次之、色彩点缀最后）；禁 full band 等笼统写法。\n\
2. 人声映射：每段说明行的人声状态必须能映射到 Style Prompt 的人声描述序列（高能段用开放真声形态、低能段用气声/假声形态），不得自造与 Style Prompt 无关的人声描述。\n\
3. 能量标注：A/B 模式每段说明行末尾必须标 能量:X（0-10）——必须用中文'能量:X'格式、X 为 0-10 单值，禁英文 energy、禁区间写法（如 8-9）。",
        DESC_LINE_MAX_CHARS
    )
}

/// 极简段豁免条款（单源；**仅 A/B 适用**）：下游执行者 = `validator::declared_minimal_sections`
/// 过滤 + `validate_production` 的 mode_a/mode_b 最弱段下限检查。
///
/// 上下游同域修复（2026-09-18，审计 #20 延伸）：旧版此条款只写在 `prompts.rs` 的 mode_a 一处
/// （还额外限死"Bridge/Intro"与"1-2 件"），mode_b **零告知**，而 A/B 两模式的说明行规则又写着
/// "Intro/Outro 不豁免"——与执行者"任何含'极简段'标记的段落（含 Intro/Outro）均豁免下限"
/// 正好对立。上游告知与下游执行器不一致 = 必然打回降级。现单源定义，A/B 同读一个占位符。
///
/// 适用边界（与执行者严格一致，改执行者必须同改此处）：
/// - 只有 mode_a / mode_b 可引用；`mode_d` 无逐段件数硬门（`validate_douyin` 不查配器件数）、
///   `mode_c` 亦不查配器——二者不得告知本豁免（`minimal_section_exemption_is_ab_only` 锁定）。
/// - 豁免对象只有"最弱段下限"一条；配器差、最强段下限、全曲上限照常校验。
/// - 声明方式：说明行内含"极简段"标记 + NOTES 写明理由（可审计）。
pub fn minimal_section_exemption() -> String {
    format!(
        "【极简段豁免（模式 A/B）】配器按段硬查：每段说明行都计入（含 Intro/Outro），最弱段下限 {} 件；\
若某段音乐上确需少于 {} 件（剥离型 Bridge/Intro、反差段落），必须在该段说明行内加\"极简段\"标记\
（如 [felt piano, rain room, 极简段]）并在 NOTES 写明理由——声明后该段豁免最弱段下限；\
配器差 ≥{} 件、最强段 ≥{} 件、全曲 ≤{} 件照常校验。未声明而低于下限的段落会被硬校验打回。",
        MIN_INSTRUMENT_WEAK,
        MIN_INSTRUMENT_WEAK,
        MIN_INSTRUMENT_GAP,
        MIN_INSTRUMENT_STRONG,
        INSTRUMENT_MAX
    )
}

/// 信封规范完整文本（单源：阶段 0/汇总/纠错重写三个注入点 + 方案侧说明行门同读）。
/// 说明行契约（R1 根因修复）：旧版全文不提说明行上限，而上限又硬门在方案侧——
/// 上游零告知、下游强制 = 必然降级；现把上限连同"不得为缩短删乐器"的边界写进规范。
pub fn envelope_spec() -> String {
    format!("{}\n{}", ENVELOPE_SPEC_BASE, desc_line_contract())
}

/// 解析结果：四个节（NOTES 可为空串）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSections {
    pub lyrics: String,
    pub style: String,
    pub params: String,
    pub notes: String,
}

impl PlanSections {
    /// LYRICS 节的非空行（保留原始顺序与内容——保真比对与转写指令共用）
    pub fn lyrics_lines(&self) -> Vec<&str> {
        self.lyrics.lines().map(|l| l.trim_end()).filter(|l| !l.trim().is_empty()).collect()
    }
}

/// 解析方案信封（顺序无关容错版）。
/// 实测模型（40 例+探测）会颠倒数序、漏 PARAMS、在标记外写分析散文——严格顺序版全部误杀。
/// 规则：LYRICS 节必须存在（保真正源）；其余节按标记提取（缺失=空串，由 envelope_defect
/// 出缺陷清单逼主持人补齐）；标记前后的游离内容（分析散文）自然落节外被忽略。
pub fn parse_plan_sections(plan: &str) -> Option<PlanSections> {
    let markers = [ENV_LYRICS, ENV_STYLE, ENV_PARAMS, ENV_NOTES];
    let mut pos: Vec<(usize, usize)> = Vec::new(); // (marker_idx, line_idx)
    for (li, line) in plan.lines().enumerate() {
        let t = line.trim();
        if let Some(mi) = markers.iter().position(|m| *m == t) {
            if pos.iter().any(|(m, _)| *m == mi) {
                return None; // 同一标记出现两次
            }
            pos.push((mi, li));
        }
    }
    if !pos.iter().any(|(m, _)| *m == 0) {
        return None; // LYRICS 节是保真正源，必须存在
    }
    let line_of = |mi: usize| -> Option<usize> {
        pos.iter().find(|(m, _)| *m == mi).map(|(_, l)| *l)
    };
    // 每节内容 = 自身标记行下一行 → 按行号排序的下一个标记行（任何类型）或文末
    let mut sorted: Vec<(usize, usize)> = pos.clone();
    sorted.sort_by_key(|(_, l)| *l);
    let sec = |mi: usize| -> String {
        let Some(start) = line_of(mi) else { return String::new() };
        let start = start + 1;
        let end = sorted.iter().find(|(_, l)| *l >= start).map(|(_, l)| *l).unwrap_or(plan.lines().count());
        plan.lines().skip(start).take(end.saturating_sub(start)).collect::<Vec<_>>().join("\n")
    };
    Some(PlanSections {
        lyrics: sec(0),
        style: sec(1),
        params: sec(2),
        notes: sec(3),
    })
}

/// 说明行行型判定（单源）：方括号整行包裹 + 含逗号（三要素分隔）。
/// 方案侧信封门（envelope_defect_mode）与终稿侧硬门（validator::desc_line_length_issues）共用同一谓词，
/// 且两侧都先 trim——旧实现方案侧不 trim，缩进过的超长说明行能绕过方案侧门却被终稿侧拦下，
/// 属"同一契约两套判法"的降级源，已收口。
pub fn is_desc_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('[') && t.ends_with(']') && t.contains(',')
}

/// 信封缺陷清单（纯函数可测）：None=合规；Some=人类可读违规明细（供纠错重写）。
/// 覆盖：结构（LYRICS 缺失/重复）、节缺失、说明行长度。
/// 模式感知：LYRICS 全模式必须；STYLE 全模式必须（C 由格式阶段排版但方案仍需提供）；
/// PARAMS 仅 a/b/d（C 无参数规则，方案不含参数节）。
pub fn envelope_defect_mode(plan: &str, mode: &str) -> Option<String> {
    let mut defects: Vec<String> = Vec::new();
    let sections = match parse_plan_sections(plan) {
        Some(s) => s,
        None => return Some("缺少 <<<LYRICS>>>/<<<STYLE>>>/<<<PARAMS>>> 信封分节（LYRICS 节为保真正源必须存在），或节标记重复/有误；不得使用 ``` 围栏、表格等禁止格式。".to_string()),
    };
    if sections.style.trim().is_empty() {
        defects.push("缺少 <<<STYLE>>> 节（Style Prompt 行）".to_string());
    }
    if mode != "mode_c" && sections.params.trim().is_empty() {
        defects.push("缺少 <<<PARAMS>>> 节（参数行）".to_string());
    }
    let over: Vec<String> = sections
        .lyrics_lines()
        .into_iter()
        .filter(|l| is_desc_line(l))
        .filter(|l| l.chars().count() > DESC_LINE_MAX_CHARS)
        .map(|l| format!("[{}]…（{} 字符）", l.chars().take(40).collect::<String>(), l.chars().count()))
        .collect();
    if !over.is_empty() {
        defects.push(format!(
            "说明行超过 {} 字符上限 {} 行，必须压缩到限内（保乐器+行为动词，删修饰词/重复空间描述；不得删乐器——配器 3-7 件与最强段 ≥5 件照常校验）：\n- {}",
            DESC_LINE_MAX_CHARS,
            over.len(),
            over.join("\n- ")
        ));
    }
    if defects.is_empty() {
        None
    } else {
        Some(defects.join("\n"))
    }
}

/// 信封开关（默认开；env PLAN_ENVELOPE_CONTRACT=0 一键回退自由格式 + 启发式分类旧行为）
pub fn plan_envelope_enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("PLAN_ENVELOPE_CONTRACT").ok().as_deref() != Some("0"))
}

#[cfg(test)]
mod envelope_tests {
    use super::*;

    const OK_PLAN: &str = "<<<LYRICS>>>\n[Intro]\n[clean pad, 能量:2]\n凌晨 两点半\n<<<STYLE>>>\nStyle Prompt: dark trap, 140BPM\n<<<PARAMS>>>\nWeirdness=18|StyleInfluence=92|AudioInfluence=0\n<<<NOTES>>>\n裁决理由：略";

    #[test]
    fn parse_valid_envelope_all_sections() {
        let s = parse_plan_sections(OK_PLAN).expect("合规信封应解析成功");
        assert!(s.lyrics.contains("凌晨 两点半"));
        assert!(s.style.contains("dark trap"));
        assert!(s.params.contains("Weirdness=18"));
        assert!(s.notes.contains("裁决理由"));
    }

    #[test]
    fn parse_notes_optional() {
        let plan = "<<<LYRICS>>>\n歌词行\n<<<STYLE>>>\nStyle Prompt: x\n<<<PARAMS>>>\nW=1";
        let s = parse_plan_sections(plan).expect("NOTES 可省略");
        assert!(s.notes.is_empty());
        assert_eq!(s.lyrics.trim(), "歌词行");
    }

    #[test]
    fn parse_requires_lyrics_only() {
        assert!(parse_plan_sections("歌词直接开写没有信封").is_none(), "无 LYRICS 节应拒绝");
        // 缺 STYLE/PARAMS 解析放行（节=空串），由 envelope_defect 出缺陷清单逼主持人补齐
        let s = parse_plan_sections("<<<LYRICS>>>\n歌词\n<<<PARAMS>>>\nW=1").expect("缺 STYLE 容错");
        assert!(s.style.is_empty() && s.params.contains("W=1"));
        assert!(envelope_defect_mode("<<<LYRICS>>>\n歌词\n<<<PARAMS>>>\nW=1", "mode_a").unwrap().contains("STYLE"));
    }

    #[test]
    fn parse_tolerates_order_and_prefix_prose() {
        // 实测形态：分析散文在标记外 + STYLE/LYRICS 顺序颠倒——容错版应正确提取
        let plan = "### Step 1 灵感分析\n（大量分析散文……）\n<<<STYLE>>>\nStyle Prompt: dark trap\n<<<LYRICS>>>\n[Intro]\n[pad, 能量:2]\n凌晨 两点半\n<<<PARAMS>>>\nWeirdness=18|StyleInfluence=92|AudioInfluence=0";
        let s = parse_plan_sections(plan).expect("顺序颠倒+前置散文应容错解析");
        assert!(s.lyrics.contains("凌晨 两点半") && !s.lyrics.contains("分析散文"));
        assert!(s.style.contains("dark trap"));
        assert!(s.params.contains("Weirdness=18"));
    }

    #[test]
    fn envelope_defect_flags_missing_sections() {
        // 模型漏 PARAMS 节的实测形态——缺陷清单必须点名，逼主持人补齐
        let plan = "<<<LYRICS>>>\n[Intro]\n[pad, 能量:2]\n凌晨\n<<<STYLE>>>\nStyle Prompt: x";
        let defect = envelope_defect_mode(&plan, "mode_a").expect("缺 PARAMS 节应报缺陷");
        assert!(defect.contains("PARAMS"), "应点名缺 PARAMS 节: {}", defect);
        assert!(!defect.contains("说明行超过"), "无超长说明行不应报");
    }

    #[test]
    fn parse_rejects_duplicate() {
        let dup = "<<<LYRICS>>>\n词\n<<<LYRICS>>>\n词2\n<<<STYLE>>>\nx\n<<<PARAMS>>>\nW=1";
        assert!(parse_plan_sections(dup).is_none(), "重复标记应拒绝");
    }

    #[test]
    fn parse_fences_and_tables_stay_outside_sections() {
        // 主持人把围栏/表格写在 NOTES 里——合法，且不污染 LYRICS
        let plan = "<<<LYRICS>>>\n[Intro]\n[pad, 能量:1]\n凌晨\n<<<STYLE>>>\nStyle Prompt: x\n<<<PARAMS>>>\nW=1\n<<<NOTES>>>\n```markdown\n|参数|值|\n|---|---|\n```";
        let s = parse_plan_sections(plan).expect("NOTES 中的 markdown 应合法");
        assert!(!s.lyrics.contains("```"));
        assert!(!s.lyrics.contains("|参数|"));
        assert!(s.notes.contains("```"));
    }

    #[test]
    fn envelope_defect_detects_overlong_desc_lines() {
        let long_desc = format!("[{}, soft pad, wide hall, 能量:3]", "a".repeat(DESC_LINE_MAX_CHARS));
        let plan = format!("<<<LYRICS>>>\n[Intro]\n{}\n凌晨\n<<<STYLE>>>\nStyle Prompt: x\n<<<PARAMS>>>\nW=1", long_desc);
        let defect = envelope_defect_mode(&plan, "mode_a").expect("超长说明行应报缺陷");
        assert!(defect.contains("说明行超过"), "缺陷文案应说明行超长: {}", defect);
        assert!(defect.contains("不得删乐器"), "纠错文案须写明边界（删修饰词而非删乐器）: {}", defect);
    }

    /// 边界：恰好等于上限的说明行必须放行（上限口径=含方括号整行字符数），超 1 字符即拦
    #[test]
    fn envelope_desc_line_boundary_at_limit() {
        let mk = |extra: usize| format!("[{}, room]", "a".repeat(DESC_LINE_MAX_CHARS - 8 + extra));
        let plan_of = |l: &str| format!("<<<LYRICS>>>\n[Intro]\n{}\n凌晨\n<<<STYLE>>>\nStyle Prompt: x\n<<<PARAMS>>>\nW=1", l);
        let exact = mk(0);
        assert_eq!(exact.chars().count(), DESC_LINE_MAX_CHARS, "夹具须恰好等于上限: {}", exact.chars().count());
        assert!(envelope_defect_mode(&plan_of(&exact), "mode_a").is_none(), "恰好等于上限应放行");
        assert!(envelope_defect_mode(&plan_of(&mk(1)), "mode_a").is_some(), "超 1 字符应拦");
    }

    /// 边界：全中文超长说明行不得 panic（旧实现按字节切片 &l[..40] 会在多字节边界崩），
    /// 且缩进过的超长说明行同样被方案侧拦下（与终稿侧 trim 口径一致）
    #[test]
    fn envelope_desc_line_cjk_overlong_no_panic() {
        let cjk = format!("[{}, 能量:3]", "钢琴铺底".repeat(60)); // 240 汉字 + 尾项，远超上限
        assert!(cjk.chars().count() > DESC_LINE_MAX_CHARS);
        let plan = format!("<<<LYRICS>>>\n[Intro]\n    {}\n凌晨\n<<<STYLE>>>\nStyle Prompt: x\n<<<PARAMS>>>\nW=1", cjk);
        let defect = envelope_defect_mode(&plan, "mode_a").expect("缩进中文超长说明行应报缺陷");
        assert!(defect.contains("说明行超过"), "{}", defect);
    }

    /// R1 数学相容锁：上限必须容得下"最强段 ≥5 件且每件带行为动词"的合法说明行。
    /// 旧值 80 在此必然失败（5 件×行为动词 ≈90-100 字符）——"5/5 run 全被信封门拦"的根因。
    #[test]
    fn desc_line_limit_fits_strongest_section_contract() {
        let five = "[acoustic guitar fingerpicked, cello dark bowing, warm piano cushions, light brushed drums, intimate room, voice open earnest, 能量:8]";
        let six = "[acoustic guitar fingerpicked, cello dark bowing, warm piano cushions, light brushed drums, deep bass pulses, intimate room, voice open earnest, 能量:9]";
        for (name, l) in [("最强段5件", five), ("六件强配", six)] {
            let n = l.chars().count();
            assert!(n <= DESC_LINE_MAX_CHARS, "{} 合法说明行 {} 字符超上限 {}", name, n, DESC_LINE_MAX_CHARS);
        }
    }

    /// 说明行示例锁：prompts.rs/roles.rs 里给模型看的示例与模板行本身不得超过上限
    /// （旧 80 时代示例 111-126 字符即违规——上游示例与下游门自相矛盾，属"示例违法"降级源）
    #[test]
    fn desc_line_examples_within_limit() {
        let keywords = ["乐器", "voice", "人声", "room", "hall"];
        let mut checked = 0usize;
        let mut violations: Vec<String> = Vec::new();
        for file in ["src/commands/prompts.rs", "src/commands/roles.rs"] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
            let src = std::fs::read_to_string(&path).unwrap();
            for (i, raw) in src.lines().enumerate() {
                let mut rest = raw;
                while let Some(open) = rest.find('[') {
                    let after = &rest[open..];
                    let Some(close) = after.find(']') else { break };
                    let frag = &after[..=close];
                    if frag.contains(',') && keywords.iter().any(|k| frag.contains(k)) {
                        let rendered = interpolate(frag);
                        checked += 1;
                        let n = rendered.chars().count();
                        if n > DESC_LINE_MAX_CHARS {
                            violations.push(format!("{}:{} 「{}」{} 字符", file, i + 1, frag, n));
                        }
                    }
                    rest = &after[close + 1..];
                }
            }
        }
        assert!(checked >= 10, "说明行示例扫描数量异常（应≥10，实际 {}）——扫描逻辑失效即锁失效", checked);
        assert!(violations.is_empty(), "说明行示例超过 {} 字符上限：\n{}", DESC_LINE_MAX_CHARS, violations.join("\n"));
    }

    /// 上游告知锁：信封规范必须写明说明行上限与"不得为缩短删乐器"的边界（否则又是零告知强制）
    #[test]
    fn envelope_spec_states_desc_line_limit() {
        let spec = envelope_spec();
        assert!(spec.contains("说明行契约"), "信封规范缺说明行契约行");
        assert!(spec.contains(&format!("≤{}", DESC_LINE_MAX_CHARS)), "信封规范缺上限数字");
        assert!(spec.contains("不得为缩短而删乐器"), "信封规范缺防误删边界的表述");
        for m in [ENV_LYRICS, ENV_STYLE, ENV_PARAMS, ENV_NOTES] {
            assert!(spec.contains(m), "信封规范缺节标记 {}", m);
        }
    }

    /// 行型谓词单源：缩进/未缩进同判（旧方案侧不 trim → 缩进行漏检，与终稿侧判法分叉）
    #[test]
    fn is_desc_line_trim_invariant() {
        assert!(is_desc_line("[pad, 能量:2]"));
        assert!(is_desc_line("   [pad, 能量:2]  "));
        assert!(!is_desc_line("[Chorus]"));
        assert!(!is_desc_line("凌晨 两点半, 就是这样"));
    }

    #[test]
    fn envelope_defect_clean_plan_passes() {
        assert!(envelope_defect_mode(OK_PLAN, "mode_a").is_none());
    }

    #[test]
    fn territory_r4_desc_line_owner_is_producer() {
        assert!(TERRITORY_RULES.iter().any(|(id, domain, owner)| *id == "R-4" && domain.contains("说明行") && *owner == "制作人"));
        let producer_prompt = crate::commands::roles::producer().system_prompt;
        assert!(producer_prompt.contains("领地声明（R-4）"), "制作人提示词应含 R-4 领地声明");
    }

    /// #14 冲突裁决措辞**双载体**守护（红灯先行）。
    ///
    /// 根因：D4 已在汇总 user prompt 注入 `territory_adjudication_text`（要求"按领地声明裁决、
    /// 注明取舍理由"），但主持人**人设** system_prompt 同时写着"只做整合、不自己改细节"——
    /// 同一职责两个对立口径，裁决指引被 system 抵消，冲突实际无人裁决。
    /// 修法：人设改读 `${TERRITORY_RULES}` 单源 + 显式写下"必须裁决/注明取舍理由"。
    /// 本测试锁死两个载体都必须命中 `ADJUDICATION_KEYS` 三个语义锚点，缺一即对立口径复发。
    #[test]
    fn adjudication_language_single_sourced_across_host_persona_and_injection() {
        let persona = interpolate(crate::commands::roles::host().system_prompt);
        let injection = territory_adjudication_text("情感分析师", "制作人", "params");
        for key in ADJUDICATION_KEYS {
            assert!(
                persona.contains(key),
                "主持人**人设**缺裁决语义锚点 \"{}\"——人设禁止改细节 / 指引要求裁决 的对立口径复发\n{}",
                key, persona
            );
            assert!(
                injection.contains(key),
                "冲突裁决**注入**缺语义锚点 \"{}\"\n{}",
                key, injection
            );
        }
        // 反向：人设不得残留与裁决职责对立的旧口径（"只做整合"/"不自己改细节"）
        assert!(
            !persona.contains("只做整合") && !persona.contains("不自己改细节"),
            "主持人 人设残留旧的\"只做整合/不自己改细节\"口径——与冲突裁决职责对立\n{}",
            persona
        );
        // 人设的领地声明必须与注入同源（插值后逐字一致，非各自手写复刻）
        assert!(
            persona.contains(&territory_rules_text()),
            "主持人 人设的领地声明与 territory_rules_text() 不同源（手写复刻会随表腐化）"
        );
    }
}

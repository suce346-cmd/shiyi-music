//! 知识库加载器：从 `knowledge/*.csv` 加载专家知识表。
//!
//! 每张表在启动时加载进内存，按需渲染成 markdown 表格文本，
//! 注入对应专家的 system prompt（「CSV 注入 prompt」方案）。

use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

/// 汉字判定（CJK 统一表意文字主区 + 扩展 A）——思维资产相关性打分的字元域。
fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF)
}

/// 方案文本的 CJK 二字组集合（跳过标点/空白/ASCII，天然滤掉格式噪声）。
fn plan_cjk_bigrams(plan: &str) -> HashSet<(char, char)> {
    let chars: Vec<char> = plan.chars().collect();
    let mut set = HashSet::new();
    for w in chars.windows(2) {
        if is_cjk(w[0]) && is_cjk(w[1]) {
            set.insert((w[0], w[1]));
        }
    }
    set
}

/// 思维资产行相关性：行语义列（rule/check/negative）的**不同** CJK 二字组在方案中出现的个数。
///
/// 旧实现（#15 根因）用过滤候选词本身在方案中的出现次数打分：思维资产的过滤条件是
/// trigger 标签"审改"，而方案文本永远不含该标签 → 每行恒 0 分 → "分数高的在前"的
/// 稳定排序退化为 CSV 行序 → take(8) 只取到文件前 8 条，第 9 条以后（含用户追加行）
/// 永不生效。现口径改为"规则正文 ↔ 当前方案的字面重合度"：方案谈韵脚/声调，声律类
/// 规则上浮；谈人称/代入，画面类规则上浮。取**不同**二字组以消除行长度偏差。
fn craft_overlap_score(row: &[String], cols: &[usize], plan_bigrams: &HashSet<(char, char)>) -> i32 {
    let mut seen: HashSet<(char, char)> = HashSet::new();
    let mut score = 0i32;
    for &ci in cols {
        if let Some(text) = row.get(ci) {
            let chars: Vec<char> = text.chars().collect();
            for w in chars.windows(2) {
                if is_cjk(w[0]) && is_cjk(w[1]) && plan_bigrams.contains(&(w[0], w[1])) && seen.insert((w[0], w[1])) {
                    score += 1;
                }
            }
        }
    }
    score
}

/// trigger 列标签集合解析（"/" 分隔，去空白，忽略空段）——#17/#18 的标签**精确**匹配基础。
///
/// 旧实现用 `rv.contains("审改")` 子串匹配：`trigger="全程"`（全流程语义，含审改轮）因此被漏掉，
/// 任何角色不可达（#17）；且子串匹配无法与单源词表对齐，未知标签也不会被发现（#18）。
fn trigger_tags(value: &str) -> HashSet<&str> {
    value
        .split(crate::rules::CRAFT_TRIGGER_TAG_SEP)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// 阶段域子判定（标签集合 ∩ 给定标签集合 ≠ ∅）——供预留位登记检查与守护测试使用。
/// **思维资产注入的完整判定不在此**（条件域限定见 `craft_inject_gate`），
/// 运行期渲染走 `render_craft_table`（内部调 `craft_inject_gate`），避免两处口径分叉。
pub fn trigger_tags_intersect(value: &str, active: &[&str]) -> bool {
    let tags = trigger_tags(value);
    active.iter().any(|t| tags.contains(*t))
}

/// 思维资产注入上下文（#23/#29 单源）：消费者标识 + 阶段标签域 + 条件域取值。
/// - `stage_tags` 取自 `rules::CRAFT_STAGE_CONSUMERS`（宿主阶段0 = 阶段0；审改 = 审改/全程）
/// - `mode`：当前模式名（`Mode::to_str_name()`），参与模式域条件判定
/// - `role`：当前角色名（`PipelineRole::storage_key()`）；`None` = 无角色身份
///   （主持人阶段0），此时角色域条件行**不满足**（如"阶段0/制作"不进主持）
pub struct CraftInjectCtx<'a> {
    pub consumer: &'a str,
    pub stage_tags: &'a [&'a str],
    pub mode: &'a str,
    pub role: Option<&'a str>,
}

/// 思维资产行注入门（**唯一判定函数**：渲染层 / 守护测试 / 角色引用契约共用，
/// 保证"测试口径 == 运行口径"）。双域语义：
/// 1. 阶段域：行标签集合 ∩ ctx.stage_tags ≠ ∅（标签精确匹配，非子串；#17）
/// 2. 预留位：行含"扩展位"即不可注入（非注入标记，走 `rules::CRAFT_RESERVED_ROWS` 登记；#18）
/// 3. 条件域（**限定**，叠加在阶段域之上；#29）：行内条件标签析取——任一命中 ctx 的
///    模式/角色域即通过；全部未命中则该行在此上下文不可注入；无条件标签 = 不限
/// 4. 条件域对 ctx 不可满足的域（role=None 遇角色域条件）按"未命中"处理（fail-closed）
pub fn craft_inject_gate(trigger: &str, ctx: &CraftInjectCtx) -> bool {
    let tags = trigger_tags(trigger);
    if tags.is_empty() {
        return false;
    }
    if tags
        .iter()
        .any(|t| crate::rules::CRAFT_TRIGGER_RESERVED.contains(t))
    {
        return false;
    }
    if !ctx.stage_tags.iter().any(|t| tags.contains(*t)) {
        return false;
    }
    let mut has_conditional = false;
    for tag in &tags {
        let scope = match crate::rules::conditional_scope(tag) {
            Some(s) => s,
            None => continue,
        };
        has_conditional = true;
        let hit = match scope {
            crate::rules::CraftConditionalScope::Mode(m) => m == ctx.mode,
            crate::rules::CraftConditionalScope::Role(r) => {
                ctx.role.map(|v| v == r).unwrap_or(false)
            }
        };
        if hit {
            return true;
        }
    }
    !has_conditional
}

/// 预留位告警去重（每 (表, 行集合) 每进程一次）——避免每角色每轮重复刷屏。
static RESERVED_WARNED: std::sync::OnceLock<std::sync::Mutex<HashSet<String>>> =
    std::sync::OnceLock::new();

/// 预留扩展位行**不静默**：命中即告警（#18）。
/// 运行期覆盖目录（`warm_knowledge` 的用户 knowledge 目录）里的新增预留行无法被编译期守护测试覆盖，
/// 只能靠此处告警提示用户"该行未注入，如需生效请改 trigger 为 审改/全程 或在 rules::CRAFT_RESERVED_ROWS 登记"。
fn warn_reserved_rows_once(table: &str, reserved_ids: &[String]) {
    if reserved_ids.is_empty() {
        return;
    }
    let set = RESERVED_WARNED.get_or_init(|| std::sync::Mutex::new(HashSet::new()));
    let mut guard = match set.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if guard.insert(format!("{}|{}", table, reserved_ids.join(","))) {
        tracing::warn!(
            table = %table,
            rows = %reserved_ids.join(","),
            "预留扩展位行未注入（如需生效请改 trigger 为 审改/全程，或在 rules::CRAFT_RESERVED_ROWS 登记保留理由）"
        );
    }
}

/// 可达性违规告警去重（每 (表, 违规集合) 每进程一次）——避免每角色每轮重复刷屏。
static REACHABILITY_WARNED: std::sync::OnceLock<std::sync::Mutex<HashSet<String>>> =
    std::sync::OnceLock::new();

// ---------------------------------------------------------------------------
// 引用必达的按表结算（#30 修复）
//
// 问题本质：角色 `craft_refs`（`rules::CRAFT_REFS_*`）是**跨表并集**（LC-* 与 CC-* 混排，
// 如情感分析师 = LC-13/LC-15/LC-18/CC-10），而 `inject_knowledge` 把它原样透传给**每张**
// 思维资产表。旧实现（#30）在渲染末尾拿全量 refs 逐表核对"是否出现在本表注入结果里"——
// 每张表都会把**别表编号**报成"引用必达行未命中（不存在或未过注入门）"（假告警：
// 投递链路本身没坏，但日志被噪声淹没，真缺陷被掩盖；2026-09-18 GUI 实跑 12 条 WARN 中 8 条为假）。
//
// 单源口径：**结算按表**——只有本表 id 列真实存在的编号（carried，本表承运）才属本表核对范围；
// 其中未出现在本表注入结果里的才是真缺陷（unmatched，行存在但未过注入门）。
// 别表编号在本表不进任何集合；任何表都不存在的编号 = 悬空引用（数据缺陷），由
// `dangling_craft_refs` 跨 `rules::CRAFT_TABLES` 单独审计并告警。
// 守护测试：knowledge::tests::craft_ref_settlement_scopes_refs_to_carrier_table（含旧口径复演红灯）、
// roles::tests::craft_ref_ids_belong_to_exactly_one_craft_table（归属唯一性 + 悬空=0）。
// ---------------------------------------------------------------------------

/// 引用必达的**按表结算**结果（#30）：carried = 本表承运的必达编号；unmatched = 承运但未过注入门。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CraftRefSettlement {
    /// 本表 id 列承载的必达编号（按 `required_rows` 声明序）——本表需要核对的集合
    pub carried: Vec<String>,
    /// 本表承运但未出现在注入结果里的编号（真缺陷：prompt 点名核查但注入里没有）
    pub unmatched: Vec<String>,
}

/// 引用必达按表结算（**纯函数**：渲染层与守护测试共用，保证"测试口径 == 运行口径"）。
/// `table_ids` = 本表 id 列全集（`Table::id_set`）；`matched_ids` = 本表注入结果里的编号集。
pub fn settle_craft_refs(
    table_ids: &HashSet<&str>,
    matched_ids: &HashSet<&str>,
    required_rows: &[&str],
) -> CraftRefSettlement {
    let carried: Vec<String> = required_rows
        .iter()
        .filter(|id| table_ids.contains(*id))
        .map(|s| s.to_string())
        .collect();
    let unmatched: Vec<String> =
        carried.iter().filter(|id| !matched_ids.contains(id.as_str())).cloned().collect();
    CraftRefSettlement { carried, unmatched }
}

/// 引用必达真未命中告警去重（每 (表, 编号) 每进程一次）——避免每角色每轮重复刷屏。
static CRAFT_REF_MISS_WARNED: std::sync::OnceLock<std::sync::Mutex<HashSet<String>>> =
    std::sync::OnceLock::new();

/// 引用必达真未命中（**行存在但未过注入门**）告警（#30 语义精确化）：
/// 只有本表承运的编号才算本表缺陷；别表编号不会走到这里（按表结算已过滤）。
fn warn_craft_ref_unmatched_once(table: &str, id: &str) {
    let set = CRAFT_REF_MISS_WARNED.get_or_init(|| std::sync::Mutex::new(HashSet::new()));
    let mut guard = match set.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if guard.insert(format!("{}|{}", table, id)) {
        tracing::warn!(
            table = %table,
            id = %id,
            "引用必达行存在但未过注入门（本表承运、注入结果缺失）——该行 trigger/条件域需修正"
        );
    }
}

/// 悬空引用告警去重（每编号每进程一次）。
static DANGLING_REF_WARNED: std::sync::OnceLock<std::sync::Mutex<HashSet<String>>> =
    std::sync::OnceLock::new();

/// 一张 CSV 表：表头 + 行数据
#[derive(Debug, Clone)]
pub struct Table {
    pub name: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// 加载期跳过的坏行号（1-based 含表头偏移，供日志与测试断言；生产渲染不读）
    #[allow(dead_code)] // 非测试构建下生产渲染不读此字段
    pub skipped_rows: Vec<usize>,
}

impl Table {
    /// 按列名过滤行后渲染（conditions: 列名 -> 值片段匹配）
    /// 注：当前 v1 全量注入；为未来按模式差异化筛选预留
    #[allow(dead_code)]
    pub fn render_filtered(&self, conditions: &[(&str, &str)], max_rows: Option<usize>) -> String {
        let col_idx: HashMap<&str, usize> = self
            .headers
            .iter()
            .enumerate()
            .map(|(i, h)| (h.as_str(), i))
            .collect();
        let matched: Vec<Vec<String>> = self
            .rows
            .iter()
            .filter(|row| {
                conditions.iter().all(|(col, val)| {
                    match col_idx.get(*col) {
                        Some(&i) => row.get(i).map(|v| v.contains(val)).unwrap_or(false),
                        None => false,
                    }
                })
            })
            .cloned()
            .collect();
        let mut out = String::new();
        out.push_str(&format!("## {} 知识库（筛选: {:?}）\n\n", self.name, conditions));
        out.push_str("| ");
        out.push_str(&self.headers.join(" | "));
        out.push_str(" |\n");
        out.push_str("|");
        for _ in &self.headers {
            out.push_str("---|");
        }
        out.push('\n');
        let rows: Vec<&Vec<String>> = match max_rows {
            Some(n) => matched.iter().take(n).collect(),
            None => matched.iter().collect(),
        };
        for row in rows {
            out.push_str("| ");
            out.push_str(&row.join(" | "));
            out.push_str(" |\n");
        }
        out
    }

    /// 表头索引（供校验用）
    #[allow(dead_code)]
    pub fn header_index(&self, header: &str) -> Option<usize> {
        self.headers.iter().position(|h| h == header)
    }

    /// id 列全部取值集合（思维资产表的编号全集）——供引用必达按表结算（#30）
    /// 与悬空引用审计共用；无 id 列返回空集（非思维资产表即无承运编号）。
    pub fn id_set(&self) -> HashSet<&str> {
        self.header_index("id")
            .map(|i| self.rows.iter().filter_map(|r| r.get(i).map(|s| s.as_str())).collect())
            .unwrap_or_default()
    }
}

/// 列投影辅助：返回（投影后表头, 投影后全部行）。None=全列；投影列不存在时报错。
/// 注：投影只影响展示层（表头+行），调用方基于原表做过滤——保证过滤列即使被裁掉仍可用。
fn project_table<'a>(
    table: &'a Table,
    cols: Option<&'a [&'a str]>,
) -> Result<(Vec<String>, Vec<Vec<String>>), String> {
    match cols {
        None => Ok((table.headers.clone(), table.rows.clone())),
        Some(cs) => {
            let idxs: Vec<usize> = cs
                .iter()
                .map(|c| {
                    table
                        .header_index(c)
                        .ok_or_else(|| format!("{} 表缺少列: {}", table.name, c))
                })
                .collect::<Result<_, _>>()?;
            Ok((
                cs.iter().map(|s| s.to_string()).collect(),
                table
                    .rows
                    .iter()
                    .map(|r| idxs.iter().map(|&i| r[i].clone()).collect())
                    .collect(),
            ))
        }
    }
}

/// 从方案文本提取能量范围（min/max；无能量标注返回 None）。
/// 实现统一委托 energy.rs（原先本函数内嵌一份逐字符扫描，与 validator 重复）。
/// 本薄壳保留函数名——orchestrator 的 plan_energy_range 与 render_filtered_any 调用点零改动。
pub(crate) fn plan_energy_range_str(plan: &str) -> Option<(u32, u32)> {
    crate::energy::plan_energy_range(plan)
}

/// 从方案文本提取 BPM（"120BPM" / "120 BPM" / "BPM 90" 等显式书写）。
/// 只信任显式 "BPM" 标注——无 BPM 字样返回 None（原 fallback 会从任意 60-200
/// 数字猜值，"80年代" 等年代词是误报源）。BPM 合理性归制作人审查兜底（validator.rs
/// 顶部哲学），代码只对明确标注报错。
/// 提取值限 30-300（防 "能量:8 BPM 范围说明" 这类邻近数字误命中）。
/// char_indices/chars 全字符遍历——禁止字节索引切片（多字节字符内部切片会 panic）。
pub(crate) fn plan_bpm_value(plan: &str) -> Option<u32> {
    let upper = plan.to_uppercase();
    let pos = upper.find("BPM")?;
    // "120BPM" / "120 BPM"：BPM 前的最后一个数字段（trim_end 允许 "68 BPM" 的间隔空格）
    if let Some(v) = trailing_digits(upper[..pos].trim_end()) {
        return sanity_bpm(v);
    }
    // "BPM 90"：BPM 后的第一个数字段
    leading_digits(upper[pos + 3..].trim_start()).and_then(sanity_bpm)
}

/// 合理性过滤：BPM 取值限 30-300（音乐常见区间外视为误命中）
fn sanity_bpm(v: u32) -> Option<u32> {
    (30..=300).contains(&v).then_some(v)
}

fn trailing_digits(s: &str) -> Option<u32> {
    let mut out = String::new();
    for c in s.chars().rev() {
        if c.is_ascii_digit() {
            out.insert(0, c);
        } else {
            break;
        }
    }
    if out.is_empty() { None } else { out.parse::<u32>().ok() }
}

fn leading_digits(s: &str) -> Option<u32> {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            out.push(c);
        } else {
            break;
        }
    }
    if out.is_empty() { None } else { out.parse::<u32>().ok() }
}

/// bpm_range 列（"60-75" / "110-170" 格式）是否包含给定 BPM
fn bpm_range_contains(range: &str, bpm: u32) -> bool {
    let parts: Vec<&str> = range.split('-').collect();
    if parts.len() != 2 {
        return false;
    }
    match (parts[0].trim().parse::<u32>(), parts[1].trim().parse::<u32>()) {
        (Ok(lo), Ok(hi)) => lo <= bpm && bpm <= hi,
        _ => false,
    }
}

/// 知识库集合
#[derive(Debug, Clone, Default)]
pub struct KnowledgeBase {
    tables: HashMap<String, Table>,
}

/// 进程级共享缓存（一次生成触发 8~16 次加载，解析一次够用）。
/// OnceLock 线程安全；初始化失败 panic——嵌入数据损坏属构建期错误。
static SHARED_KB: std::sync::OnceLock<KnowledgeBase> = std::sync::OnceLock::new();

/// 知识库来源（日志用；embedded=嵌入版，override=用户覆盖目录）
static KB_SOURCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// 取共享缓存（生产路径；测试直调 load/load_embedded）
pub fn shared_knowledge() -> &'static KnowledgeBase {
    SHARED_KB.get_or_init(|| {
        let kb = load_embedded_internal().expect("嵌入知识库损坏（构建期错误）");
        kb.warn_reachability_violations();
        kb
    })
}

/// 知识库来源描述（setup 时预热后可查；缺省 embedded）
pub fn knowledge_source() -> &'static str {
    KB_SOURCE.get().map(|s| s.as_str()).unwrap_or("embedded")
}

/// setup 预热——覆盖目录整组有效则替换缓存源，否则嵌入版。
/// 按组切换（单文件覆盖易致表间不一致）；损坏回退嵌入版 + 日志。
/// 幂等：OnceLock 已初始化则跳过（测试直调 load 不受影响）。
pub fn warm_knowledge(app_data_dir: &std::path::Path) {
    let dir = app_data_dir.join("knowledge");
    // 目录不存在 → 嵌入版（最常见路径，静默）
    if !dir.is_dir() {
        let _ = KB_SOURCE.set("embedded".to_string());
        let _ = SHARED_KB.get_or_init(|| {
            load_embedded_internal().expect("嵌入知识库损坏（构建期错误）")
        });
        return;
    }
    match KnowledgeBase::load(&dir) {
        Ok(kb) if kb.table_names().len() == 8 => {
            // #22 运行期通道：覆盖目录里的结构性不可达行必须呐喊（编译期守护测试覆盖不到用户目录）
            kb.warn_reachability_violations();
            let _ = SHARED_KB.get_or_init(|| kb.clone());
            // get_or_init 已初始化时上面的 clone 白做但无害；来源标记尝试设置
            let _ = KB_SOURCE.set("override".to_string());
            tracing::info!(dir = %dir.display(), "知识库来源：用户覆盖目录");
        }
        Ok(kb) => {
            tracing::warn!(
                tables = kb.table_names().len(),
                "覆盖目录表不全（需 8 张），回退嵌入版"
            );
            let _ = KB_SOURCE.set("embedded".to_string());
            let _ = SHARED_KB.get_or_init(|| {
                load_embedded_internal().expect("嵌入知识库损坏（构建期错误）")
            });
        }
        Err(e) => {
            tracing::warn!(error = %e, "覆盖目录加载失败，回退嵌入版");
            let _ = KB_SOURCE.set("embedded".to_string());
            let _ = SHARED_KB.get_or_init(|| {
                load_embedded_internal().expect("嵌入知识库损坏（构建期错误）")
            });
        }
    }
}

/// 嵌入加载内部实现（load_embedded 与 shared_knowledge 共用）
fn load_embedded_internal() -> Result<KnowledgeBase, String> {
    let mut kb = KnowledgeBase::default();
    // 名称必须与 knowledge/ 目录下文件名一致（不含扩展名）
    let files: [(&str, &str); 8] = [
        ("instruments", include_str!("../knowledge/instruments.csv")),
        ("emotions", include_str!("../knowledge/emotions.csv")),
        ("style_genre", include_str!("../knowledge/style_genre.csv")),
        ("suno_rules", include_str!("../knowledge/suno_rules.csv")),
        ("cliches", include_str!("../knowledge/cliches.csv")),
        ("hooks", include_str!("../knowledge/hooks.csv")),
        // P0 思维资产表（8 Skill 去指纹全量融合，不压缩）
        ("lyric_craft", include_str!("../knowledge/lyric_craft.csv")),
        ("compose_craft", include_str!("../knowledge/compose_craft.csv")),
    ];
    let mut loaded = 0usize;
    for (name, content) in files {
        match parse_csv(name, content) {
            Ok(table) => {
                kb.tables.insert(name.to_string(), table);
                loaded += 1;
            }
            Err(e) => {
                tracing::warn!(table = %name, error = %e, "知识库跳过坏表");
            }
        }
    }
    if loaded == 0 {
        return Err("知识库嵌入数据全部不可用".to_string());
    }
    Ok(kb)
}

/// 场景列与模式匹配单源（用户拍板 2026-09-19）：suno_rules 的 scenario 列
/// （"all"/"抖音"/"中文"）决定规则行注入哪个模式——根治 D 模式作词人同时被喂
/// "≤12 字"（抖音）与"6-13 字"（普通模式）的矛盾输入（全链路核查 发现 1）。
/// "all" 恒注入；"抖音" 仅 mode_d；"中文"（普通模式）仅 mode_a/b/c；
/// 未知场景保守注入（宁可多给知识，不静默丢失；调用方打日志）。
pub fn scenario_allows_mode(scenario: &str, mode: &str) -> bool {
    match scenario {
        "抖音" => mode == "mode_d",
        "中文" => mode != "mode_d",
        _ => true,
    }
}


impl KnowledgeBase {
    /// 从目录加载所有 `.csv` 文件（测试与动态加载场景用；生产走 load_embedded）。
    /// 单表解析失败降级——跳过该表并打印警告，不阻断其他表；全失败才报错。
    #[allow(dead_code)]
    pub fn load(dir: &Path) -> Result<KnowledgeBase, String> {
        let mut kb = KnowledgeBase::default();
        let entries = fs::read_dir(dir)
            .map_err(|e| format!("无法读取知识库目录 {}: {}", dir.display(), e))?;
        let mut loaded = 0usize;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("csv") {
                continue;
            }
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unnamed")
                .to_string();
            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    // 读失败也降级跳过（与坏表一致，不阻断其他表）
                    tracing::warn!(table = %name, error = %e, "知识库跳过不可读表");
                    continue;
                }
            };
            match parse_csv(&name, &content) {
                Ok(table) => {
                    kb.tables.insert(name.clone(), table);
                    loaded += 1;
                }
                Err(e) => {
                    tracing::warn!(table = %name, error = %e, "知识库跳过坏表");
                }
            }
        }
        if loaded == 0 {
            return Err(format!("知识库目录 {} 下没有可用的 CSV 文件", dir.display()));
        }
        Ok(kb)
    }

    /// 编译期嵌入加载（打包后亦可用，不依赖运行时文件路径）。
    /// 生产走 shared_knowledge 缓存；测试直调本函数；分发态 dev 目录不存在时兜底。
    #[allow(dead_code)] // 生产走缓存，测试+兜底保留
    /// 单表解析失败降级——跳过该表并打印警告，其余表照常可用。
    /// shared_knowledge() 缓存调用内部实现（OnceLock 只初始化一次）。
    pub fn load_embedded() -> Result<KnowledgeBase, String> {
        crate::knowledge::load_embedded_internal()
    }

    /// 结构性不可达行审计（#22/#31）：列出**永远无法注入**的行及其原因。
    /// - 关键词表（`rules::KEYWORD_TABLES`）：该行**全部检索列**（多列析取）按单源分词
    ///   （`rules::keyword_tokens`）拆出的 token 都短于 `rules::KEYWORD_MIN_CHARS`
    ///   → `orchestrator::matching_keywords` 全部丢弃，该行永久休眠（旧行为：无日志无测试）
    /// - 乐器表：`energy_min/energy_max` 缺失/非数值/min>max → `render_instruments_by_energy` 永不命中
    /// 语义：**可达 = 行具备被注入的必要条件**（是否真被命中取决于当前方案文本，属按需检索设计）。
    pub fn reachability_violations(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for spec in crate::rules::KEYWORD_TABLES {
            let table = match self.table(spec.table) {
                Ok(t) => t,
                Err(_) => {
                    out.push(format!("{}：可达性规格声明的表在知识库中不存在", spec.table));
                    continue;
                }
            };
            // 检索列存在性：**每一列**都必须存在（列名写错 = 该列候选恒空，检索面静默收窄）
            let mut idxs: Vec<usize> = Vec::with_capacity(spec.key_cols.len());
            let mut missing: Option<&str> = None;
            for col in spec.key_cols {
                match table.header_index(col) {
                    Some(i) => idxs.push(i),
                    None => {
                        missing = Some(col);
                        break;
                    }
                }
            }
            if let Some(col) = missing {
                out.push(format!("{}.{}：可达性规格声明的检索列不存在", spec.table, col));
                continue;
            }
            for (ri, row) in table.rows.iter().enumerate() {
                // 行可达 = 全部检索列里至少有一个 token 达到长度门（分词口径与运行期同源）
                let cells: Vec<&str> = idxs
                    .iter()
                    .map(|&i| row.get(i).map(|s| s.as_str()).unwrap_or(""))
                    .collect();
                let max_token_len = cells
                    .iter()
                    .flat_map(|c| crate::rules::keyword_tokens(c))
                    .map(|t| t.chars().count())
                    .max()
                    .unwrap_or(0);
                if max_token_len < crate::rules::KEYWORD_MIN_CHARS {
                    out.push(format!(
                        "{}.{:?} 第 {} 行：全部检索列的 token 都短于下限 {}（检索键 {:?} 长度 {}），该行永久不可注入",
                        spec.table,
                        spec.key_cols,
                        ri + 1,
                        crate::rules::KEYWORD_MIN_CHARS,
                        cells,
                        max_token_len
                    ));
                }
            }
        }
        let (min_col, max_col) = crate::rules::ENERGY_GATE_COLUMNS;
        if let Ok(table) = self.table("instruments") {
            let (lo, hi) = (table.header_index(min_col), table.header_index(max_col));
            match (lo, hi) {
                (Some(lo), Some(hi)) => {
                    for (ri, row) in table.rows.iter().enumerate() {
                        let parse = |i: usize| {
                            row.get(i).and_then(|v| v.trim().parse::<u32>().ok())
                        };
                        match (parse(lo), parse(hi)) {
                            (Some(a), Some(b)) if a <= b => {}
                            (Some(a), Some(b)) => out.push(format!(
                                "instruments 第 {} 行：能量区间 {}-{} 倒置（min > max），该行永不命中",
                                ri + 1,
                                a,
                                b
                            )),
                            _ => out.push(format!(
                                "instruments 第 {} 行：能量区间缺失或非数值（{}={:?}, {}={:?}），该行永不命中",
                                ri + 1,
                                min_col,
                                row.get(lo),
                                max_col,
                                row.get(hi)
                            )),
                        }
                    }
                }
                _ => out.push(format!(
                    "instruments：能量门列 {} / {} 不存在，整表不可注入",
                    min_col, max_col
                )),
            }
        }
        out
    }

    /// 可达性违规的运行期告警（每 (表, 违规集合) 每进程一次）。
    /// 编译期守护测试覆盖不到用户**覆盖目录**里的新增死行，故运行期必须呐喊而不是静默。
    pub fn warn_reachability_violations(&self) {
        let violations = self.reachability_violations();
        if violations.is_empty() {
            return;
        }
        let set = REACHABILITY_WARNED.get_or_init(|| std::sync::Mutex::new(HashSet::new()));
        let mut guard = match set.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if guard.insert(violations.join("|")) {
            tracing::warn!(
                count = violations.len(),
                violations = %violations.join("；"),
                "知识库存在结构性不可达行：这些行的检索键/能量区间不满足注入门，永远不会被注入（请修正数据或改检索键）"
            );
        }
    }

    /// 悬空引用审计（#30）：`required_rows` 中在 `rules::CRAFT_TABLES` **全部**思维资产表里
    /// 都不存在的编号（真数据缺陷：prompt 点名核查一个 CSV 里查无此行的编号）。
    /// 与按表结算分工：本表承运 → 本表核对；全部表皆无 → 此处审计。
    pub fn dangling_craft_refs(&self, required_rows: &[&str]) -> Vec<String> {
        let mut all_ids: HashSet<&str> = HashSet::new();
        for t in crate::rules::CRAFT_TABLES {
            if let Ok(table) = self.table(t) {
                all_ids.extend(table.id_set());
            }
        }
        required_rows
            .iter()
            .filter(|id| !all_ids.contains(*id))
            .map(|s| s.to_string())
            .collect()
    }

    /// 悬空引用告警（每编号每进程一次）：真数据缺陷必须呐喊，不得静默（旧实现把它混在
    /// 每表"未命中"噪声里，无法区分"别表编号"与"查无此行"）。
    pub fn warn_dangling_craft_refs(&self, required_rows: &[&str]) {
        let dangling = self.dangling_craft_refs(required_rows);
        if dangling.is_empty() {
            return;
        }
        let set = DANGLING_REF_WARNED.get_or_init(|| std::sync::Mutex::new(HashSet::new()));
        let mut guard = match set.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        for id in &dangling {
            if guard.insert(id.clone()) {
                tracing::warn!(
                    id = %id,
                    tables = ?crate::rules::CRAFT_TABLES,
                    "引用必达编号悬空：任何思维资产表中都不存在该编号（prompt 点名核查的行查无此条，请修正数据或编号）"
                );
            }
        }
    }

    /// 关键词/规则表的按需检索渲染（不含思维资产表——那些表走 `render_craft_table`，
    /// 因为它们的注入门是"阶段域 + 条件域"，不是候选词 contains）。
    pub fn render_filtered_any(
        &self,
        name: &str,
        conditions: &[(&str, &[&str])],
        cols: Option<&[&str]>,
        plan: &str,
        max_rows: Option<usize>,
        required_rows: &[&str],
    ) -> Result<String, String> {
        self.render_with_gate(name, conditions, cols, plan, max_rows, required_rows, None)
    }

    /// 思维资产表（lyric_craft / compose_craft）渲染入口（#23/#29）：
    /// 注入门 = `craft_inject_gate`（阶段域 + 条件域限定），**必须**提供上下文，
    /// 否则报错（不允许"无门注入"这种静默路径存在）。
    pub fn render_craft_table(
        &self,
        name: &str,
        ctx: &CraftInjectCtx,
        cols: Option<&[&str]>,
        plan: &str,
        max_rows: Option<usize>,
        required_rows: &[&str],
    ) -> Result<String, String> {
        self.render_with_gate(name, &[], cols, plan, max_rows, required_rows, Some(ctx))
    }

    /// 渲染实现单点（过滤 + 排序 + 引用必达 + 条数上限 + 表格输出）。
    /// `conditions`：关键词/规则表的检索条件；**思维资产表忽略本参数**，由 `craft_ctx` 单源门决定。
    fn render_with_gate(
        &self,
        name: &str,
        conditions: &[(&str, &[&str])],
        cols: Option<&[&str]>,
        plan: &str,
        max_rows: Option<usize>,
        required_rows: &[&str],
        craft_ctx: Option<&CraftInjectCtx>,
    ) -> Result<String, String> {
        let table = self.table(name)?;
        let (headers, rows_all) = project_table(table, cols)?;
        let col_idx: HashMap<&str, usize> = table
            .headers
            .iter()
            .enumerate()
            .map(|(i, h)| (h.as_str(), i))
            .collect();
        // 过滤基于原表行（行序与投影后 rows_all 一一对应），投影列缺失不影响过滤
        // 命中后按表路由多维排序：候选词命中数（×3）+ 表特定分（emotions 能量距离+core 权重 / style_genre BPM 匹配）
        let e_idx = table.header_index("energy_min");
        let lv_idx = table.header_index("emotion_level");
        let bpm_idx = table.header_index("bpm_range");
        let plan_e = plan_energy_range_str(plan);
        let plan_bpm = plan_bpm_value(plan);
        // 思维资产表（#15）：相关性打分读语义列（rule/check/negative），不读 id/module/trigger 元数据
        let craft_table = crate::rules::is_craft_table(name);
        // 思维资产表必须带注入上下文（#23/#29）：无上下文 = 阶段/条件门无法判定，直接失败
        if craft_table && craft_ctx.is_none() {
            return Err(format!(
                "思维资产表 {} 注入缺少 CraftInjectCtx（阶段域/条件域无法判定）——请改用 render_craft_table",
                name
            ));
        }
        let craft_cols: Vec<usize> = ["rule", "check", "negative"]
            .iter()
            .filter_map(|c| table.header_index(c))
            .collect();
        let plan_bigrams = if craft_table { plan_cjk_bigrams(plan) } else { HashSet::new() };
        // 引用必达集合：行 id 在 required_rows 中的位置（None = 非必达行）
        let id_idx = table.header_index("id");
        let required_pos = |row: &[String]| -> Option<usize> {
            id_idx
                .and_then(|i| row.get(i))
                .and_then(|id| required_rows.iter().position(|r| r == id))
        };
        // trigger 标签列（仅思维资产表有）——#17：标签精确匹配，不用子串
        let trigger_idx = table.header_index("trigger");
        // 预留位行**不静默**告警（#18）：与可注入无关，只看标签是否落"扩展位"
        if craft_table {
            let reserved_ids: Vec<String> = table
                .rows
                .iter()
                .filter(|row| {
                    trigger_idx
                        .and_then(|i| row.get(i))
                        .map(|v| trigger_tags_intersect(v, crate::rules::CRAFT_TRIGGER_RESERVED))
                        .unwrap_or(false)
                })
                .filter_map(|row| row.first().cloned())
                .collect();
            warn_reserved_rows_once(name, &reserved_ids);
        }
        let mut matched_idx: Vec<(usize, i32)> = rows_all
            .iter()
            .enumerate()
            .filter(|(ri, _)| {
                let row = match table.rows.get(*ri) {
                    Some(r) => r,
                    None => return false,
                };
                // 思维资产（#17/#29）：trigger 是 "/" 分隔的多标签列，且**同一标签可叠加**
                // （如 "阶段0/审改"、"审改/抖音"）——判定走**唯一门函数** `craft_inject_gate`：
                // 阶段域（标签精确匹配，非子串）+ 条件域（模式/角色限定）。
                // 旧实现只做阶段域交集 → 条件标签形同虚设（LC-31 在 D 也注入、CC-22 进所有角色，#29）。
                if craft_table {
                    let tv = match trigger_idx.and_then(|i| row.get(i)) {
                        Some(v) => v.as_str(),
                        None => return false,
                    };
                    let ctx = craft_ctx.expect("思维资产表已在入口校验必带注入上下文");
                    return craft_inject_gate(tv, ctx);
                }
                conditions.iter().any(|(col, vals)| {
                    col_idx
                        .get(*col)
                        .map(|&i| {
                            row.get(i)
                                .map(|rv| vals.iter().any(|v| rv.contains(v)))
                                .unwrap_or(false)
                        })
                        .unwrap_or(false)
                })
            })
            .map(|(ri, _)| {
                let row = table.rows.get(ri).cloned().unwrap_or_default();
                // 引用必达（#16）：prompt 点名核查的编号行置顶（分数域撑满，按声明序排）
                if let Some(pos) = required_pos(&row) {
                    return (ri, i32::MAX - pos as i32);
                }
                // 思维资产（#15）：按"规则正文 ↔ 当前方案"的字面重合度排序。
                // 旧路径的候选词打分在此恒 0（过滤候选是 trigger 标签"审改"，方案永不含），
                // 会把排序退化为 CSV 行序、使尾部行永不生效——故此处改走内容相关性。
                if craft_table {
                    return (ri, craft_overlap_score(&row, &craft_cols, &plan_bigrams));
                }
                // 候选词命中数（同一行命中多个候选词说明相关度更高）
                // 命中权重：行字段匹配的候选词 × 其在方案中的出现次数（反复出现的情绪/主题词权重更高）
                let mut score: i32 = conditions
                    .iter()
                    .filter_map(|(col, vals)| {
                        col_idx.get(*col).map(|&i| {
                            vals.iter()
                                .filter(|v| row.get(i).map(|rv| rv.contains(*v)).unwrap_or(false))
                                .map(|v| plan.matches(v).count() as i32)
                                .sum::<i32>()
                                * 3
                        })
                    })
                    .sum::<i32>();
                match table.name.as_str() {
                    "emotions" => {
                        // 能量距离：方案能量中心 vs 情绪区间中心（方案无能量标注时跳过该维）
                        if let Some((e_min, e_max)) = plan_e {
                            if let (Some(lo), Some(hi)) = (
                                e_idx.and_then(|i| row.get(i)).and_then(|v| v.parse::<u32>().ok()),
                                e_idx.map(|i| i + 1).and_then(|i| row.get(i)).and_then(|v| v.parse::<u32>().ok()),
                            ) {
                                let center = (e_min + e_max) / 2;
                                let ic = (lo + hi) / 2;
                                score += 10 - (ic as i32 - center as i32).abs().min(10);
                            }
                        }
                        // 核心情绪权重：core 优先（主情绪决定弧线）
                        if let Some(i) = lv_idx {
                            if row.get(i).map(|v| v == "core").unwrap_or(false) {
                                score += 2;
                            }
                        }
                    }
                    "style_genre" => {
                        // BPM 匹配：方案含 BPM 且命中流派 bpm_range 区间则加分
                        if let (Some(bpm), Some(i)) = (plan_bpm, bpm_idx) {
                            if let Some(v) = row.get(i) {
                                if bpm_range_contains(v, bpm) {
                                    score += 2;
                                }
                            }
                        }
                    }
                    _ => {}
                }
                (ri, score)
            })
            .collect();
        // 稳定排序：分数高的在前（同分保持表顺序）
        matched_idx.sort_by(|a, b| b.1.cmp(&a.1));
        let matched: Vec<Vec<String>> = matched_idx
            .iter()
            .map(|(ri, _)| rows_all.get(*ri).cloned().unwrap_or_default())
            .collect();
        let hit = !matched.is_empty();
        // 引用必达按表结算（#30）：carried = 本表 id 列承运的编号；unmatched = 承运但未过注入门。
        // 旧实现拿**跨表并集** refs 逐表核对 → 每张表都把别表编号报成"未命中"（假告警刷屏）。
        let table_ids = table.id_set();
        let matched_ids: HashSet<&str> = matched_idx
            .iter()
            .filter_map(|(ri, _)| table.rows.get(*ri))
            .filter_map(|row| id_idx.and_then(|i| row.get(i)).map(|s| s.as_str()))
            .collect();
        let settlement = settle_craft_refs(&table_ids, &matched_ids, required_rows);
        for id in &settlement.unmatched {
            warn_craft_ref_unmatched_once(&table.name, id);
        }
        // 引用必达行数（声明集合中真实命中门的行）：上限对它让步——承诺必达的规则不得被条数上限截掉。
        // 基于**原表行**计数（不受列投影影响：投影裁掉 id 列时按原表索引读仍是本行编号）。
        let required_hits = matched_idx
            .iter()
            .filter(|(ri, _)| table.rows.get(*ri).map(|r| required_pos(r).is_some()).unwrap_or(false))
            .count();
        // M18：删随机兜底——未命中时不塞前 3 行，明确标注“无命中”，由调用方走确定性默认。
        // 自矛盾指令“禁止照搬”同步删除，换成“按功能选用并说明理由”（M18，roles.rs 护栏同步）。
        let rows: Vec<&Vec<String>> = if hit {
            let cap = max_rows.unwrap_or(usize::MAX).max(required_hits);
            matched.iter().take(cap).collect()
        } else {
            Vec::new()
        };
        let mut out = String::new();
        out.push_str(&format!(
            "## {} 知识库（{}）\n\n",
            table.name,
            if hit {
                let shown = rows.len();
                let must_note = if required_hits > 0 {
                    format!("，含引用必达 {} 条", required_hits)
                } else {
                    String::new()
                };
                if matched.len() > shown {
                    format!("按需命中 {} 条{}，以上展示前 {} 条", matched.len(), must_note, shown)
                } else if required_hits > 0 {
                    format!("按需命中 {} 条{}", matched.len(), must_note)
                } else {
                    format!("按需命中 {} 条", matched.len())
                }
            } else {
                "未命中关键词（无示例；请按功能选用并说明理由）".to_string()
            }
        ));
        out.push_str("| ");
        out.push_str(&headers.join(" | "));
        out.push_str(" |\n");
        out.push_str("|");
        for _ in &headers {
            out.push_str("---|");
        }
        out.push('\n');
        for row in rows {
            out.push_str("| ");
            out.push_str(&row.join(" | "));
            out.push_str(" |\n");
        }
        Ok(out)
    }

    /// instruments 按能量区间过滤（数值比较）：energy_min ≤ e_max 且 energy_max ≥ e_min
    /// （与方案能量范围 [e_min, e_max] 有交集——覆盖情绪弧线两端，弱段低能量/强段高能量）。
    /// 无交集时零行+无示例标注（M18）。cols：列投影（None=全列；能量过滤基于原表列）。
    pub fn render_instruments_by_energy(
        &self,
        e_min: u32,
        e_max: u32,
        cols: Option<&[&str]>,
        plan: &str,
        max_rows: Option<usize>,
    ) -> Result<String, String> {
        let table = self.table("instruments")?;
        let (headers, rows_all) = project_table(table, cols)?;
        let (i_min, i_max) = match (table.header_index("energy_min"), table.header_index("energy_max")) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err("instruments 缺少 energy_min/energy_max 列".to_string()),
        };
        // 倒置区间视为无效：直接兜底（避免跨越缝隙误命中）
        if e_min > e_max {
            let rows: Vec<&Vec<String>> = rows_all.iter().take(3).collect();
            return Ok(format!(
                "## instruments 知识库（能量区间无效 {}~{}，无示例；请按功能选用并说明理由）\n\n| {} |\n|{}|\n",
                e_min,
                e_max,
                headers.join(" | "),
                headers.iter().map(|_| "---|").collect::<String>()
            ) + &rows
                .iter()
                .map(|r| format!("| {} |", r.join(" | ")))
                .collect::<Vec<_>>()
                .join("\n"));
        }
        // 过滤基于原表行（行序与投影后 rows_all 一一对应），命中后按多维相关度排序：
        // ① 能量距离（乐器区间中心 vs 方案区间中心越近越高）② 风格匹配（方案词命中 style_tags）
        // ③ 主次权重（lead 主奏优先，color 色彩靠后）——保证截断保留最相关、主奏不丢
        let style_idx = table.header_index("style_tags");
        let prio_idx = table.header_index("priority");
        let plan_center = (e_min + e_max) / 2;
        let mut matched_idx: Vec<(usize, i32)> = rows_all
            .iter()
            .enumerate()
            .filter_map(|(ri, _)| {
                let row = table.rows.get(ri).cloned().unwrap_or_default();
                let lo = row.get(i_min).and_then(|v| v.parse::<u32>().ok());
                let hi = row.get(i_max).and_then(|v| v.parse::<u32>().ok());
                match (lo, hi) {
                    (Some(lo), Some(hi)) if lo <= e_max && hi >= e_min => {
                        let inst_center = (lo + hi) / 2;
                        let dist = (inst_center as i32 - plan_center as i32).abs();
                        let mut score = 10 - dist.min(10); // 能量接近分 0-10
                        // 风格匹配：方案文本命中 style_tags 词（空格/顿号分词）
                        if let Some(si) = style_idx {
                            for tag in row.get(si).map(|s| s.as_str()).unwrap_or("").split(|ch| ch == ' ' || ch == '、') {
                                if !tag.is_empty() && plan.contains(tag) {
                                    score += 3;
                                }
                            }
                        }
                        // 主次权重：lead 主奏优先，color 色彩靠后
                        if let Some(pi) = prio_idx {
                            match row.get(pi).map(|s| s.as_str()).unwrap_or("") {
                                "lead" => score += 2,
                                "color" => score -= 1,
                                _ => {}
                            }
                        }
                        Some((ri, score))
                    }
                    _ => None, // 无交集或数值解析失败
                }
            })
            .collect();
        // 稳定排序：分数高的在前（同分保持表顺序）
        matched_idx.sort_by(|a, b| b.1.cmp(&a.1));
        let matched: Vec<Vec<String>> = matched_idx
            .iter()
            .map(|(ri, _)| rows_all.get(*ri).cloned().unwrap_or_default())
            .collect();
        let hit = !matched.is_empty();
        // M18：删随机兜底——未命中时行数归零，只留标注；调用方走确定性默认
        let rows: Vec<&Vec<String>> = if hit {
            matched.iter().take(max_rows.unwrap_or(usize::MAX)).collect()
        } else {
            Vec::new()
        };
        let mut out = String::new();
        out.push_str(&format!(
            "## instruments 知识库（{}）\n\n",
            if hit {
                let shown = rows.len();
                if matched.len() > shown {
                    format!("按能量区间 {}~{} 命中 {} 件，以上展示前 {} 件", e_min, e_max, matched.len(), shown)
                } else {
                    format!("按能量区间 {}~{} 命中 {} 件", e_min, e_max, matched.len())
                }
            } else {
                "未命中能量区间（无示例；请按功能选用并说明理由）".to_string()
            }
        ));
        out.push_str("| ");
        out.push_str(&headers.join(" | "));
        out.push_str(" |\n");
        out.push_str("|");
        for _ in &headers {
            out.push_str("---|");
        }
        out.push('\n');
        for row in rows {
            out.push_str("| ");
            out.push_str(&row.join(" | "));
            out.push_str(" |\n");
        }
        Ok(out)
    }

    /// 按表名获取（带错误信息，供校验调用）
    pub fn table(&self, name: &str) -> Result<&Table, String> {
        self.tables
            .get(name)
            .ok_or_else(|| format!("知识库中没有表: {}（可用: {:?}）", name, self.tables.keys().collect::<Vec<_>>()))
    }

    /// 渲染整张表（cols：列投影，None=全列；mode：Some=按 scenario 列过滤规则行）。
    /// 产出语言三态配套（全链路核查 发现 1 根治）：mode_d 只注入场景="抖音"的行，
    /// mode_a/b/c 只注入场景="中文"（普通模式）的行——矛盾规则不再同时进 prompt。
    /// 无 scenario 列的表不受影响；未知场景保守注入。
    pub fn render_table(
        &self,
        name: &str,
        cols: Option<&[&str]>,
        max_rows: Option<usize>,
        mode: Option<&str>,
    ) -> Result<String, String> {
        let owned: Table;
        let table: &Table = match mode {
            Some(m) => {
                let t = self.table(name)?;
                owned = match t.header_index("scenario") {
                    Some(idx) => Table {
                        rows: t
                            .rows
                            .iter()
                            .filter(|r| {
                                scenario_allows_mode(r.get(idx).map(|s| s.as_str()).unwrap_or("all"), m)
                            })
                            .cloned()
                            .collect(),
                        ..t.clone()
                    },
                    None => t.clone(),
                };
                &owned
            }
            None => self.table(name)?,
        };
        let (headers, rows_all) = project_table(table, cols)?;
        let mut out = String::new();
        out.push_str(&format!("## {} 知识库\n\n", table.name));
        out.push_str("| ");
        out.push_str(&headers.join(" | "));
        out.push_str(" |\n");
        out.push_str("|");
        for _ in &headers {
            out.push_str("---|");
        }
        out.push('\n');
        let rows: Vec<&Vec<String>> = match max_rows {
            Some(n) => rows_all.iter().take(n).collect(),
            None => rows_all.iter().collect(),
        };
        for row in rows {
            out.push_str("| ");
            out.push_str(&row.join(" | "));
            out.push_str(" |\n");
        }
        // 截断标记（防止 LLM 误以为知识库只有这些条目）
        if let Some(n) = max_rows {
            if rows_all.len() > n {
                out.push_str(&format!("\n（共 {} 条，以上展示前 {} 条）\n", rows_all.len(), n));
            }
        }
        Ok(out)
    }

    /// 所有表名
    #[allow(dead_code)]
    pub fn table_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tables.keys().cloned().collect();
        names.sort();
        names
    }
}

/// 简易 CSV 解析：支持双引号包裹字段与转义 `""`。
/// 不做完整 RFC 4180（无跨行字段），我们的表都是简单表格。
fn parse_csv(name: &str, content: &str) -> Result<Table, String> {
    // BOM 剥离（Windows 记事本存 CSV 常见，首列名会 mismatch）
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut lines = content.lines().filter(|l| !l.trim().is_empty());
    let header_line = lines
        .next()
        .ok_or_else(|| format!("CSV {} 为空", name))?;
    let headers = split_csv_line(header_line);
    if headers.len() < 2 {
        return Err(format!("CSV {} 表头列数不足（{}）", name, headers.len()));
    }
    let mut rows = Vec::new();
    let mut skipped_rows: Vec<usize> = Vec::new();
    let mut total = 0usize;
    for (i, line) in lines.enumerate() {
        total += 1;
        let fields = split_csv_line(line);
        if fields.len() != headers.len() {
            // 坏行跳过（行号 1-based 含表头偏移 i+2），不废整表
            skipped_rows.push(i + 2);
            continue;
        }
        rows.push(fields);
    }
    if rows.is_empty() {
        return Err(format!("CSV {} 没有数据行", name));
    }
    // 半残表拒绝——坏行占比超 10% 视为表损坏，走相关表级降级（跳过该表）
    if skipped_rows.len() * 10 > total {
        return Err(format!(
            "CSV {} 坏行过多（{}/{}），整表拒绝",
            name,
            skipped_rows.len(),
            total
        ));
    }
    if !skipped_rows.is_empty() {
        tracing::warn!(table = %name, skipped = ?skipped_rows, "知识库跳过坏行");
    }
    Ok(Table {
        name: name.to_string(),
        headers,
        rows,
        skipped_rows,
    })
}

/// 切分一行 CSV：支持 `"a,b"` 引号包裹（内部逗号不切分）、`""` 转义
fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cur.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            fields.push(cur.trim().to_string());
            cur = String::new();
        } else {
            cur.push(c);
        }
    }
    fields.push(cur.trim().to_string());
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主持人阶段0 注入上下文（#23 真消费者）：阶段标签取 `CRAFT_STAGE_CONSUMERS` 登记，
    /// 无角色身份（角色域条件行在此不满足）。
    fn host_ctx(mode: &str) -> CraftInjectCtx<'_> {
        let c = crate::rules::stage_consumer(crate::rules::CRAFT_STAGE_CONSUMER_HOST0)
            .expect("阶段消费者登记缺失：host_stage0");
        CraftInjectCtx { consumer: c.name, stage_tags: c.stage_tags, mode, role: None }
    }

    /// 审改注入上下文：阶段标签取 `CRAFT_STAGE_CONSUMERS` 登记，带模式 + 角色身份。
    fn reviewer_ctx<'a>(mode: &'a str, role: Option<&'a str>) -> CraftInjectCtx<'a> {
        let c = crate::rules::stage_consumer(crate::rules::CRAFT_STAGE_CONSUMER_REVIEWER)
            .expect("阶段消费者登记缺失：reviewer");
        CraftInjectCtx { consumer: c.name, stage_tags: c.stage_tags, mode, role }
    }

    /// load() 整组有效则可用作覆盖源（8 张 mini 表）；调用方按组切换
    #[test]
    fn load_override_dir_semantics() {
        let dir = std::env::temp_dir().join(format!("kb_override_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["instruments", "emotions", "style_genre", "suno_rules", "cliches", "hooks", "lyric_craft", "compose_craft"] {
            std::fs::write(dir.join(format!("{}.csv", name)), "a,b\n1,2\n").unwrap();
        }
        let kb = KnowledgeBase::load(&dir).unwrap();
        assert_eq!(kb.table_names().len(), 8);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_simple_csv() {
        let content = "a,b,c\n1,2,3\n4,5,6\n";
        let t = parse_csv("test", content).unwrap();
        assert_eq!(t.headers, vec!["a", "b", "c"]);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[1], vec!["4", "5", "6"]);
    }

    #[test]
    fn parses_quoted_fields() {
        let content = "name,desc\npiano,\"warm, soft keys\"\n";
        let t = parse_csv("test", content).unwrap();
        assert_eq!(t.rows[0][1], "warm, soft keys");
    }

    /// 单数据行全坏（坏行比 100% > 10%）→ 整表拒绝（旧语义保留）
    #[test]
    fn rejects_column_mismatch() {
        let content = "a,b\n1,2,3\n";
        assert!(parse_csv("test", content).is_err());
    }

    /// 多行中 1 坏行（占比 <10%）→ 跳过该行 + 记录行号，好行保留
    #[test]
    fn skips_single_bad_row_keeps_good_ones() {
        let mut content = String::from("a,b\n");
        for i in 0..20 {
            content.push_str(&format!("{}, {}\n", i, i));
        }
        content.push_str("bad,extra,col\n"); // 1 坏行（1/21 < 10%）
        let t = parse_csv("test", &content).unwrap();
        assert_eq!(t.rows.len(), 20);
        assert_eq!(t.skipped_rows, vec![22]);
    }

    /// BOM 前缀剥离（Windows 记事本存 CSV 常见）
    #[test]
    fn strips_utf8_bom() {
        let content = "\u{feff}a,b\n1,2\n";
        let t = parse_csv("test", content).unwrap();
        assert_eq!(t.headers, vec!["a", "b"]);
        assert_eq!(t.rows.len(), 1);
    }

    #[test]
    fn rejects_empty() {
        assert!(parse_csv("test", "").is_err());
        assert!(parse_csv("test", "a,b\n").is_err());
    }

    #[test]
    fn render_produces_markdown_table() {
        let t = parse_csv("test", "a,b\n1,2\n").unwrap();
        let kb = KnowledgeBase { tables: [("test".to_string(), t)].into_iter().collect() };
        let out = kb.render_table("test", None, None, None).unwrap();
        assert!(out.contains("| 1 | 2 |"));
        assert!(out.contains("---|"));
    }

    #[test]
    fn render_filtered_matches_column() {
        let t = parse_csv("test", "a,b\nx,1\ny,2\n").unwrap();
        let out = t.render_filtered(&[("a", "y")], None);
        assert!(out.contains("| y | 2 |"));
        assert!(!out.contains("| x | 1 |"));
    }

    #[test]
    fn loads_from_directory() {
        // 使用项目真实 knowledge 目录（相对于 crate root）
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("knowledge");
        let kb = KnowledgeBase::load(&dir).unwrap();
        let names = kb.table_names();
        assert!(names.contains(&"instruments".to_string()));
        assert!(names.contains(&"emotions".to_string()));
        assert!(names.contains(&"style_genre".to_string()));
        assert!(names.contains(&"suno_rules".to_string()));
        assert!(names.contains(&"cliches".to_string()));
        // P0 思维资产表（8 Skill 去指纹全量融合）
        assert!(names.contains(&"lyric_craft".to_string()));
        assert!(names.contains(&"compose_craft".to_string()));
    }

    #[test]
    fn unknown_table_returns_error() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("knowledge");
        let kb = KnowledgeBase::load(&dir).unwrap();
        assert!(kb.table("nope").is_err());
        assert!(kb.render_table("nope", None, None, None).is_err());
    }

    #[test]
    fn load_missing_directory_errors() {
        let kb = KnowledgeBase::load(Path::new("/nonexistent/dir"));
        assert!(kb.is_err());
    }

    #[test]
    fn rejects_single_column_header() {
        let content = "only
1
";
        assert!(parse_csv("t", content).is_err());
    }

    #[test]
    fn parses_escaped_quotes() {
        let content = "a,b\n\"say \"\"hi\"\"\",2\n";
        let t = parse_csv("t", content).unwrap();
        assert_eq!(t.rows[0][0], "say \"hi\"");
        assert_eq!(t.rows[0][1], "2");
    }

    #[test]
    fn render_filtered_no_match_still_renders_header() {
        let t = parse_csv("t", "a,b\nx,1\n").unwrap();
        let out = t.render_filtered(&[("a", "zzz")], None);
        assert!(out.contains("| a | b |"));
        assert!(!out.contains("| x |"));
    }

    #[test]
    fn table_names_sorted() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("knowledge");
        let kb = KnowledgeBase::load(&dir).unwrap();
        let names = kb.table_names();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        assert!(names.contains(&"instruments".to_string()));
    }

    /// P0 验收门：两思维资产表行数 + 严格指纹零命中 + 可检索。
    /// 行数：lyric_craft 32 行（LC-01..32），compose_craft 30 行（CC-01..30），不压缩。
    /// 指纹：人物姓名零命中（宽泛词如留白/概念先行是通用中文词，不在门内）。
    #[test]
    fn craft_tables_row_counts_and_fingerprint_clean() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("knowledge");
        let kb = KnowledgeBase::load(&dir).unwrap();
        let lyric = kb.table("lyric_craft").expect("lyric_craft 表缺失");
        let compose = kb.table("compose_craft").expect("compose_craft 表缺失");
        assert_eq!(lyric.rows.len(), 32, "lyric_craft 应为 32 行，实际 {}", lyric.rows.len());
        assert_eq!(compose.rows.len(), 30, "compose_craft 应为 30 行，实际 {}", compose.rows.len());
        // id 连续性
        assert_eq!(lyric.rows[0][0], "LC-01");
        assert_eq!(lyric.rows[31][0], "LC-32");
        assert_eq!(compose.rows[0][0], "CC-01");
        assert_eq!(compose.rows[29][0], "CC-30");
        // 严格指纹：人物姓名零命中
        let person_names = [
            "方文山", "林夕", "黄霑", "黄伟文", "罗大佑", "陈其钢",
            "坂本龙一", "坂本", "久石让", "久石",
        ];
        for table in [&lyric, &compose] {
            for row in &table.rows {
                for cell in row {
                    for name in &person_names {
                        assert!(
                            !cell.contains(name),
                            "思维资产表含人物指纹 [{}]：{}",
                            name,
                            cell.chars().take(60).collect::<String>()
                        );
                    }
                }
            }
        }
        // 可检索：按 module 列过滤有结果
        let out = lyric.render_filtered(&[("module", "画面")], None);
        assert!(out.contains("LC-01"), "lyric_craft 应可按 module 检索：{}", &out[..out.chars().count().min(200)]);
    }

    #[test]
    fn header_index_lookup() {
        let t = parse_csv("t", "a,b\nx,1\n").unwrap();
        assert_eq!(t.header_index("b"), Some(1));
        assert_eq!(t.header_index("z"), None);
    }

    /// render_table 的 max_rows=Some(n) 分支（截断）
    #[test]
    fn render_truncates_with_max_rows() {
        let t = parse_csv("t", "a,b\nx,1\ny,2\nz,3\n").unwrap();
        let kb = KnowledgeBase { tables: [("t".to_string(), t)].into_iter().collect() };
        let out = kb.render_table("t", None, Some(2), None).unwrap();
        assert!(out.contains("| x | 1 |"));
        assert!(!out.contains("| z | 3 |"));
        assert!(out.contains("共 3 条，以上展示前 2 条"), "应有截断标注: {}", out);
    }

    /// filtered 条件列不存在 → 该条件视为不匹配
    #[test]
    fn render_filtered_unknown_column_no_match() {
        let t = parse_csv("t", "a,b\nx,1\n").unwrap();
        let out = t.render_filtered(&[("nope", "x")], None);
        assert!(!out.contains("| x | 1 |"));
    }

    /// 目录里只有非 csv 文件 → 报"没有可用的 CSV 文件"
    #[test]
    fn load_errors_when_no_csv_files() {
        let dir = std::env::temp_dir().join(format!("kb_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("readme.txt"), "not csv").unwrap();
        let err = KnowledgeBase::load(&dir).unwrap_err();
        assert!(err.contains("CSV 文件"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 目录不存在 → 报"无法读取知识库目录"
    #[test]
    fn load_missing_directory_message() {
        let err = KnowledgeBase::load(Path::new("/nonexistent/dir")).unwrap_err();
        assert!(err.contains("无法读取知识库目录"));
    }

    /// 坏表降级——目录里坏表被跳过，好表照常加载
    #[test]
    fn load_skips_bad_table_keeps_good_ones() {
        let dir = std::env::temp_dir().join(format!("kb_skip_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("good.csv"), "a,b\n1,2\n").unwrap();
        std::fs::write(dir.join("bad.csv"), "a,b\n1,2,3\n").unwrap(); // 列数不符
        let kb = KnowledgeBase::load(&dir).unwrap();
        let names = kb.table_names();
        assert!(names.contains(&"good".to_string()), "好表应加载: {:?}", names);
        assert!(!names.contains(&"bad".to_string()), "坏表应被跳过: {:?}", names);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 全坏 → 报错（无可用表不静默返回空库）
    #[test]
    fn load_all_bad_errors() {
        let dir = std::env::temp_dir().join(format!("kb_allbad_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad1.csv"), "a,b\n1,2,3\n").unwrap();
        std::fs::write(dir.join("bad2.csv"), "a\n1\n").unwrap(); // 单列表头
        let err = KnowledgeBase::load(&dir).unwrap_err();
        assert!(err.contains("CSV 文件"), "got: {}", err);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn embedded_load_matches_directory() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("knowledge");
        let from_dir = KnowledgeBase::load(&dir).unwrap();
        let embedded = KnowledgeBase::load_embedded().unwrap();
        assert_eq!(from_dir.table_names(), embedded.table_names());
        // 各表行数一致
        for name in from_dir.table_names() {
            assert_eq!(
                from_dir.table(&name).unwrap().rows.len(),
                embedded.table(&name).unwrap().rows.len(),
                "表 {} 行数不一致",
                name
            );
        }
    }

    #[test]
    fn table_has_required_columns() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("knowledge");
        let kb = KnowledgeBase::load(&dir).unwrap();
        let inst = kb.table("instruments").unwrap();
        for col in ["instrument", "family", "energy_min", "energy_max", "note", "priority"] {
            assert!(inst.header_index(col).is_some(), "缺少列: {}", col);
        }
        let rules = kb.table("suno_rules").unwrap();
        for col in ["rule", "value_min", "value_max"] {
            assert!(rules.header_index(col).is_some(), "缺少列: {}", col);
        }
        // 颗粒度对齐：层级/模式列必须存在且已标注
        let emo = kb.table("emotions").unwrap();
        assert!(emo.header_index("emotion_level").is_some(), "emotions 缺 emotion_level 列");
        let hooks = kb.table("hooks").unwrap();
        assert!(hooks.header_index("mode_fit").is_some(), "hooks 缺 mode_fit 列");
        let has_core = emo.rows.iter().any(|r| r.get(13).map(|v| v == "core").unwrap_or(false));
        assert!(has_core, "emotions 无 core 标注");
        let has_douyin = hooks.rows.iter().any(|r| r.get(10).map(|v| v == "douyin").unwrap_or(false));
        assert!(has_douyin, "hooks 无 douyin 标注");
    }

    /// 产出语言三态配套（全链路核查 发现 1 根治）：suno_rules 场景过滤——
    /// mode_d 只吃"抖音"行（line_max_chars ≤12），mode_b 只吃"中文"行（line_chars 6-13），
    /// 矛盾规则不再同时进同一 prompt。
    #[test]
    fn rules_injection_respects_scenario_mode() {
        let t = parse_csv("suno_rules", "rule,scenario,value_min,value_max,unit,description\n\
            line_max_chars,抖音,0,12,字,抖音每行不超过 12 字：短行易跟唱卡点清晰\n\
            line_chars_verse,中文,6,13,字,普通模式 Verse 每行 6-13 字：太短信息量不足，太长难唱\n").unwrap();
        let kb = KnowledgeBase { tables: [("suno_rules".to_string(), t)].into_iter().collect() };
        let d = kb
            .render_table("suno_rules", None, None, Some("mode_d"))
            .unwrap();
        assert!(d.contains("line_max_chars"), "mode_d 须含抖音行字数规则");
        assert!(!d.contains("line_chars_verse"), "mode_d 不得注入普通模式行字数规则: {}", d);
        let b = kb
            .render_table("suno_rules", None, None, Some("mode_b"))
            .unwrap();
        assert!(b.contains("line_chars_verse"), "mode_b 须含普通模式行字数规则");
        assert!(!b.contains("line_max_chars"), "普通模式不得注入抖音行字数规则: {}", b);
    }

    /// 微观②：render_filtered_any 任一候选命中即保留（OR 语义）
    #[test]
    fn render_filtered_any_matches_any_keyword() {
        let t = parse_csv("t", "emotion,energy_words\n孤独,冷 空 钝\n愤怒,锋利 燥热\n温柔,暖 软\n").unwrap();
        let kb = KnowledgeBase { tables: [("t".to_string(), t)].into_iter().collect() };
        let out = kb.render_filtered_any("t", &[("emotion", &["孤独", "愤怒"])], None, "", None, &[]).unwrap();
        assert!(out.contains("按需命中 2 条"), "got: {}", out);
        assert!(out.contains("孤独"));
        assert!(out.contains("愤怒"));
        assert!(!out.contains("温柔"));
    }

    /// #16 修复锁：引用必达——被点名的行必须在投递集合内，且优先于条数上限（不得被截掉）。
    /// 旧实现只按 CSV 行序 take(max_rows)，点名行一旦落在第 9 行之后即永远不投递。
    #[test]
    fn craft_required_rows_force_injected_beyond_cap() {
        let t = parse_csv(
            "lyric_craft",
            "id,module,rule,trigger,check,negative\n\
             LC-01,画面,无关内容一,审改,无关检查一,无关禁用一\n\
             LC-02,画面,无关内容二,审改,无关检查二,无关禁用二\n\
             LC-03,画面,无关内容三,审改,无关检查三,无关禁用三\n",
        )
        .unwrap();
        let kb = KnowledgeBase { tables: [("lyric_craft".to_string(), t)].into_iter().collect() };
        // 上限 2 条，但点名的是第 3 行 → 必须投递（必达优先于上限）
        let out = kb
            .render_craft_table("lyric_craft", &reviewer_ctx("mode_a", Some("lyricist")), None, "", Some(2), &["LC-03"])
            .unwrap();
        assert!(out.contains("| LC-03 |"), "引用必达行被上限截掉: {}", out);
        assert!(out.contains("含引用必达 1 条"), "注入头未标注必达数: {}", out);
        assert!(out.contains("| LC-01 |"), "上限内仍应按序补足: {}", out);
        assert!(!out.contains("| LC-02 |"), "上限外的非必达行应被截断: {}", out);
    }

    /// #15 修复锁：思维资产排序按"规则正文 ↔ 方案"的内容相关性，而非 CSV 行序——
    /// 尾部行（含用户追加行）在方案谈及其内容时能被注入，不再"永不生效"。
    #[test]
    fn craft_rows_ranked_by_plan_overlap_not_csv_order() {
        let t = parse_csv(
            "lyric_craft",
            "id,module,rule,trigger,check,negative\n\
             LC-01,画面,先建时空物件清单,审改,清单齐全,禁抽象开局\n\
             LC-02,声律,韵脚律动优先语义,审改,韵脚闭环,禁拗口\n\
             LC-30,减法,自检三问语义密度,审改,三问有结论,禁跳过自检\n",
        )
        .unwrap();
        let kb = KnowledgeBase { tables: [("lyric_craft".to_string(), t)].into_iter().collect() };
        // 方案谈韵脚：LC-02 上浮到唯一名额（旧实现恒 0 分 → 永远是 CSV 首行 LC-01）
        let out = kb
            .render_craft_table("lyric_craft", &reviewer_ctx("mode_a", Some("lyricist")), None, "韵脚 押韵 声律", Some(1), &[])
            .unwrap();
        assert!(out.contains("| LC-02 |"), "相关性命中行未入选: {}", out);
        assert!(!out.contains("| LC-01 |"), "无关行不应挤掉相关行: {}", out);
        // 尾部行（模拟用户追加）同样可凭相关性上浮
        let out2 = kb
            .render_craft_table("lyric_craft", &reviewer_ctx("mode_a", Some("lyricist")), None, "语义密度 自检", Some(1), &[])
            .unwrap();
        assert!(out2.contains("| LC-30 |"), "尾部行应可凭相关性注入: {}", out2);
    }

    /// #17 阶段域子判定（纯函数）：标签**精确**匹配——"审改"/"全程"落在审改阶段域内，
    /// "阶段0"（主持阶段域）、"扩展位"（非注入）不落在内，且不会像子串匹配那样把"非审改"误判为命中。
    /// 完整注入判定（叠加条件域）见 `craft_inject_gate` 的守护测试。
    #[test]
    fn craft_trigger_gate_is_tag_exact_not_substring() {
        let active = crate::rules::CRAFT_TRIGGER_ACTIVE;
        assert!(trigger_tags_intersect("审改", active));
        assert!(trigger_tags_intersect("阶段0/审改", active), "多标签叠加：审改应命中");
        assert!(trigger_tags_intersect("审改/抖音", active));
        assert!(trigger_tags_intersect("全程", active), "#17：全流程规则必须可注入");
        assert!(trigger_tags_intersect(" 全程 ", active), "标签应去空白");
        assert!(!trigger_tags_intersect("阶段0", active), "阶段0 属主持阶段域（消费者 host_stage0），不在审改阶段域");
        assert!(!trigger_tags_intersect("扩展位", active), "扩展位为已登记预留位");
        assert!(!trigger_tags_intersect("非审改", active), "标签精确：子串不得误命中");
        assert!(!trigger_tags_intersect("", active), "空 trigger 不可注入");
    }

    /// #17 修复锁（红灯先行）：trigger="全程" 的行必须可达——旧子串匹配下 CC-23/CC-28 任何角色不可达。
    /// 端到端断言：单源门判定通过 + 实际渲染文本中出现该行。
    #[test]
    fn craft_full_process_rows_are_injectable() {
        let kb = KnowledgeBase::load_embedded().unwrap();
        let t = kb.table("compose_craft").unwrap();
        let id_idx = t.header_index("id").unwrap();
        let trg_idx = t.header_index("trigger").unwrap();
        let full_process: Vec<String> = t
            .rows
            .iter()
            .filter(|row| row.get(trg_idx).map(|v| v == "全程").unwrap_or(false))
            .filter_map(|row| row.get(id_idx).cloned())
            .collect();
        assert!(
            !full_process.is_empty(),
            "compose_craft 应存在 trigger=全程 的行（#17 反例集不应为空）"
        );
        for id in &full_process {
            let row = t
                .rows
                .iter()
                .find(|r| r.get(id_idx).map(|v| v == id).unwrap_or(false))
                .unwrap();
            assert!(
                craft_inject_gate(&row[trg_idx], &reviewer_ctx("mode_a", Some("producer"))),
                "{} trigger=全程 未通过唯一注入门（#17 回归）",
                id
            );
        }
        // 渲染实测：全量（不设上限）下"全程"行必须在注入文本内
        let out = kb
            .render_craft_table("compose_craft", &reviewer_ctx("mode_a", Some("producer")), None, "（占位方案）", None, &[])
            .unwrap();
        for id in &full_process {
            assert!(out.contains(&format!("| {} |", id)), "{} 未出现在注入文本中（#17 回归）: {}", id, out);
        }
    }

    /// #18 修复锁：CSV 中 trigger 含"扩展位"的行集合 == rules::CRAFT_RESERVED_ROWS 登记集合。
    /// 新增预留行未登记即红——强制"预留"成为显式决策，杜**用户追加内容静默失效**。
    #[test]
    fn craft_reserved_rows_match_registry() {
        let kb = KnowledgeBase::load_embedded().unwrap();
        let mut actual: Vec<String> = Vec::new();
        for name in ["lyric_craft", "compose_craft"] {
            let t = kb.table(name).unwrap();
            let id_idx = t.header_index("id").unwrap();
            let trg_idx = t.header_index("trigger").unwrap();
            for row in &t.rows {
                if trigger_tags_intersect(&row[trg_idx], crate::rules::CRAFT_TRIGGER_RESERVED) {
                    actual.push(row[id_idx].clone());
                }
            }
        }
        actual.sort();
        let mut declared: Vec<String> = crate::rules::CRAFT_RESERVED_ROWS
            .iter()
            .map(|(id, _)| (*id).to_string())
            .collect();
        declared.sort();
        assert_eq!(
            actual, declared,
            "扩展位行与 CRAFT_RESERVED_ROWS 登记不一致（CSV={:?} 登记={:?}）——\
             新增预留行须在 rules::CRAFT_RESERVED_ROWS 登记理由；若本应生效请改 trigger 为 审改/全程",
            actual, declared
        );
        for (id, reason) in crate::rules::CRAFT_RESERVED_ROWS {
            assert!(!reason.trim().is_empty(), "{} 的预留登记缺理由", id);
        }
    }

    /// #17/#18/#23/#29 修复锁：trigger 词表**有消费者且域取值真实**——三向守护：
    /// ① CSV 标签 ⊆ 词表（防新标签静默丢弃）；
    /// ② 每个**阶段标签**至少有一个消费者上下文（`CRAFT_STAGE_CONSUMERS`），且消费者的
    ///    阶段标签只能取自阶段域词表——防"词表可见、执行零消费者"（#23 的"阶段0"零消费者）；
    /// ③ 每个**条件标签**的判定域取值真实：模式域 ∈ `ALL_MODES`、角色域 ∈ `PipelineRole::storage_key()`，
    ///    且该标签确实被至少一行 CSV 使用（防登记即死条目）。
    #[test]
    fn craft_trigger_vocabulary_has_consumers_and_real_scope() {
        let conditional_keys: Vec<&str> =
            crate::rules::CRAFT_CONDITIONAL_TAGS.iter().map(|(t, _)| *t).collect();
        let vocab: HashSet<&str> = crate::rules::CRAFT_TRIGGER_ACTIVE
            .iter()
            .chain(crate::rules::CRAFT_TRIGGER_HOST_ONLY)
            .chain(crate::rules::CRAFT_TRIGGER_RESERVED)
            .chain(conditional_keys.iter())
            .copied()
            .collect();
        let kb = KnowledgeBase::load_embedded().unwrap();
        let mut used_tags: HashSet<&str> = HashSet::new();
        for name in ["lyric_craft", "compose_craft"] {
            let t = kb.table(name).unwrap();
            let id_idx = t.header_index("id").unwrap();
            let trg_idx = t.header_index("trigger").unwrap();
            for row in &t.rows {
                assert!(
                    !trigger_tags(&row[trg_idx]).is_empty(),
                    "{} 缺 trigger 标签",
                    row[id_idx]
                );
                for tag in trigger_tags(&row[trg_idx]) {
                    assert!(
                        vocab.contains(tag),
                        "{} trigger 含词表外标签 {:?}（词表：{:?}）——请登记到 rules::CRAFT_TRIGGER_* / CRAFT_CONDITIONAL_TAGS 后再用",
                        row[id_idx],
                        tag,
                        vocab
                    );
                    used_tags.insert(tag);
                }
            }
        }
        // ② 阶段标签必须各有消费者；消费者只能消费阶段域标签（不得把条件标签当阶段标签）
        let stage_vocab: HashSet<&str> = crate::rules::CRAFT_TRIGGER_ACTIVE
            .iter()
            .chain(crate::rules::CRAFT_TRIGGER_HOST_ONLY)
            .copied()
            .collect();
        for tag in &stage_vocab {
            assert!(
                crate::rules::CRAFT_STAGE_CONSUMERS
                    .iter()
                    .any(|c| c.stage_tags.contains(tag)),
                "阶段标签 {:?} 无消费者上下文（= 零消费者静默死角，#23）",
                tag
            );
            assert!(
                used_tags.contains(tag),
                "阶段标签 {:?} 在 CSV 中零使用（词表条目即死条目）",
                tag
            );
        }
        for c in crate::rules::CRAFT_STAGE_CONSUMERS {
            for tag in c.stage_tags {
                assert!(
                    stage_vocab.contains(tag),
                    "消费者 {} 声明的阶段标签 {:?} 不在阶段域词表内",
                    c.name,
                    tag
                );
            }
            assert!(!c.stage_tags.is_empty(), "消费者 {} 的阶段标签为空", c.name);
        }
        // ③ 条件标签域取值真实 + 确实被使用
        let all_modes: HashSet<&str> = crate::rules::ALL_MODES.iter().copied().collect();
        let all_roles: HashSet<&str> =
            crate::models::PipelineRole::all().iter().map(|r| r.storage_key()).collect();
        for (tag, scope) in crate::rules::CRAFT_CONDITIONAL_TAGS {
            match scope {
                crate::rules::CraftConditionalScope::Mode(m) => assert!(
                    all_modes.contains(m),
                    "条件标签 {:?} 的模式域取值 {:?} 不在 ALL_MODES（{:?}）",
                    tag,
                    m,
                    crate::rules::ALL_MODES
                ),
                crate::rules::CraftConditionalScope::Role(r) => assert!(
                    all_roles.contains(r),
                    "条件标签 {:?} 的角色域取值 {:?} 不在 PipelineRole::storage_key()（{:?}）",
                    tag,
                    r,
                    all_roles
                ),
            }
            assert!(
                used_tags.contains(tag),
                "条件标签 {:?} 在 CSV 中零使用（登记即死条目）",
                tag
            );
        }
    }

    /// #29 修复锁（**红灯先行**）：条件标签是**限定门**，不是装饰——同一行在不同
    /// (模式, 角色) 上下文下命中与否必须分化。旧实现只做阶段域交集 → 下列 assert 全红。
    #[test]
    fn craft_conditional_gate_is_restrictive_gate() {
        let lyricist = Some("lyricist");
        // 抖音限定（LC-10/LC-24 = 审改/抖音）：仅 mode_d 生效
        assert!(
            !craft_inject_gate("审改/抖音", &reviewer_ctx("mode_a", lyricist)),
            "#29：抖音限定行不得进 mode_a"
        );
        assert!(craft_inject_gate("审改/抖音", &reviewer_ctx("mode_d", lyricist)));
        // 模式域条件（LC-31 = 审改/A/B/C）：D 不生效、C 生效（旧实现 D 也注入 = 红）
        assert!(
            !craft_inject_gate("审改/A/B/C", &reviewer_ctx("mode_d", lyricist)),
            "#29：审改/A/B/C 行不得进 mode_d"
        );
        for m in ["mode_a", "mode_b", "mode_c"] {
            assert!(
                craft_inject_gate("审改/A/B/C", &reviewer_ctx(m, lyricist)),
                "#29：审改/A/B/C 行应在 {} 生效",
                m
            );
        }
        // 角色域条件（CC-22 = 审改/制作）：只进制作人（旧实现所有 craft 角色都注入 = 红）
        assert!(craft_inject_gate("审改/制作", &reviewer_ctx("mode_a", Some("producer"))));
        for r in ["emotion", "lyricist", "reviser", "style_analyst"] {
            assert!(
                !craft_inject_gate("审改/制作", &reviewer_ctx("mode_a", Some(r))),
                "#29：制作限定行不得进 {}",
                r
            );
        }
        // 阶段域与条件域是"与"：条件命中但阶段域不符 → 不注入
        assert!(
            !craft_inject_gate("阶段0/制作", &reviewer_ctx("mode_a", Some("producer"))),
            "阶段域：阶段0 行不得进审改上下文"
        );
        // 角色域条件遇无角色身份（主持人阶段0）按未命中（fail-closed）
        assert!(
            !craft_inject_gate("阶段0/制作", &host_ctx("mode_a")),
            "#29：角色域条件在 role=None 上下文必须不满足"
        );
        assert!(craft_inject_gate("阶段0", &host_ctx("mode_a")));
        // 无条件标签 = 不限模式/角色：全程行在任意审改上下文均可注入
        assert!(craft_inject_gate("全程", &reviewer_ctx("mode_d", Some("style_analyst"))));
        // 但阶段域仍要满足：阶段0 上下文只消费"阶段0"标签（全程/审改行不进主持初稿）
        assert!(
            !craft_inject_gate("全程", &host_ctx("mode_a")),
            "阶段域隔离：全程行不进主持人阶段0"
        );
        assert!(!craft_inject_gate("审改", &host_ctx("mode_a")), "阶段域隔离：审改行不进主持人阶段0");
        // 预留位 / 空 trigger 恒不可注入
        assert!(!craft_inject_gate("扩展位", &reviewer_ctx("mode_a", lyricist)));
        assert!(!craft_inject_gate("审改/扩展位", &reviewer_ctx("mode_a", lyricist)));
        assert!(!craft_inject_gate("", &reviewer_ctx("mode_a", lyricist)));
        // 未知标签按"非条件"处理（不构成限定），阶段域命中即通过
        assert!(craft_inject_gate("审改/未知标签", &reviewer_ctx("mode_a", lyricist)));
    }

    /// #23/#29 修复锁：思维资产表**必须**带注入上下文——无上下文的注入路径直接失败，
    /// 不允许"无门注入"（阶段域/条件域都无法判定，正是历史缺陷的温床）。
    #[test]
    fn craft_render_requires_inject_context() {
        let kb = KnowledgeBase::load_embedded().unwrap();
        let err = kb
            .render_filtered_any("lyric_craft", &[("trigger", &["审改"])], None, "", None, &[])
            .unwrap_err();
        assert!(err.contains("CraftInjectCtx"), "错误信息须指明缺上下文: {}", err);
        // 非思维资产表不受影响
        assert!(kb
            .render_filtered_any("suno_rules", &[("trigger", &["审改"])], None, "", None, &[])
            .is_ok());
    }

    /// #30 修复锁（引用必达按表结算）：角色的 craft_refs 是**跨表并集**，渲染每张思维资产表时
    /// 只能核对**本表承运**的编号；别表编号不得进入本表未命中集合。
    /// 红灯先行：旧口径（拿全量 refs 逐表核对）必然把 CC-10 报成 lyric_craft 未命中——本测试
    /// 先复演旧口径并断言其非空（证明假告警真实存在），再断言新口径结算结果为空。
    #[test]
    fn craft_ref_settlement_scopes_refs_to_carrier_table() {
        let kb = KnowledgeBase::load_embedded().unwrap();
        let refs = crate::rules::CRAFT_REFS_EMOTION; // LC-13/LC-15/LC-18 + CC-10（跨表并集）
        let plan = "（未填写的方案占位）";
        let ctx = reviewer_ctx("mode_a", Some("emotion"));
        let out = kb
            .render_craft_table(
                "lyric_craft",
                &ctx,
                None,
                plan,
                Some(crate::commands::orchestrator::INJECT_MAX_CRAFT_ROWS),
                refs,
            )
            .unwrap();
        // 旧口径复演（红）：全量 refs 逐表核对 → lyric_craft 缺 CC-10，报"未命中"
        let old_missing: Vec<&&str> = refs
            .iter()
            .filter(|id| !out.contains(&format!("| {} |", id)))
            .collect();
        assert!(
            !old_missing.is_empty(),
            "旧口径必须能复现跨表假告警（否则本锁无法证明修复必要性）"
        );
        // 本表承运的编号必须全部在注入结果里（真必达不落空）
        let table = kb.table("lyric_craft").unwrap();
        let id_idx = table.header_index("id").unwrap();
        let matched_ids: HashSet<&str> = table
            .rows
            .iter()
            .filter_map(|r| r.get(id_idx).map(|s| s.as_str()))
            .filter(|id| out.contains(&format!("| {} |", id)))
            .collect();
        // 新口径（绿①）：本表结算 = 只认承运编号，且无一未命中
        let s = settle_craft_refs(&table.id_set(), &matched_ids, refs);
        assert_eq!(s.carried, vec!["LC-13", "LC-15", "LC-18"], "本表承运集合错");
        assert!(s.unmatched.is_empty(), "本表真未命中不得有：{:?}", s.unmatched);
        // 新口径（绿②）：同一 refs 结算 compose_craft → 承运 = CC-10，跨表编号不进任何集合
        let cc = kb.table("compose_craft").unwrap();
        let cc_id_idx = cc.header_index("id").unwrap();
        let cc_out = kb
            .render_craft_table(
                "compose_craft",
                &ctx,
                None,
                plan,
                Some(crate::commands::orchestrator::INJECT_MAX_CRAFT_ROWS),
                refs,
            )
            .unwrap();
        let cc_matched: HashSet<&str> = cc
            .rows
            .iter()
            .filter_map(|r| r.get(cc_id_idx).map(|s| s.as_str()))
            .filter(|id| cc_out.contains(&format!("| {} |", id)))
            .collect();
        let cc_settle = settle_craft_refs(&cc.id_set(), &cc_matched, refs);
        assert_eq!(cc_settle.carried, vec!["CC-10"], "compose_craft 只承运 CC-10");
        assert!(cc_settle.unmatched.is_empty(), "CC-10 必须过门：{:?}", cc_settle.unmatched);
        // 悬空审计：真实角色 refs 无悬空（任何表都不存在的编号）
        for role in crate::models::PipelineRole::all() {
            let r = crate::commands::roles::role_for(role);
            assert!(
                kb.dangling_craft_refs(r.craft_refs).is_empty(),
                "{} 的引用存在悬空编号：{:?}",
                r.name,
                kb.dangling_craft_refs(r.craft_refs)
            );
        }
    }

    /// #22 修复锁（① 现状）：真实知识库中不得存在**结构性不可达行**——
    /// 关键词表全部检索列（多列析取）的 token 必须至少有一个 ≥ rules::KEYWORD_MIN_CHARS
    /// （短于此值的候选被 orchestrator::matching_keywords 静默丢弃 → 该行永久休眠），
    /// 乐器表能量区间必须可解析且 min ≤ max。
    /// 旧状态实测：cliches 的 家/雨/夜/光 4 行是死行（单字键被丢弃，无日志无测试）。
    #[test]
    fn keyword_tables_have_no_structurally_unreachable_rows() {
        let kb = KnowledgeBase::load_embedded().unwrap();
        let v = kb.reachability_violations();
        assert!(v.is_empty(), "存在结构性不可达行（永久休眠且无告警）：{:?}", v);
    }

    /// #22/#31 修复锁（② 红灯先行）：审计本身必须真能抓到死行——单字检索键、多列全短 token、
/// 检索列缺失与倒置能量区间。若本测试不红而真实数据"全绿"，说明守护网是假的。
    #[test]
    fn reachability_audit_detects_single_char_key_and_bad_energy() {
        let mut kb = KnowledgeBase::default();
        kb.tables.insert(
            "cliches".to_string(),
            parse_csv("cliches", "cliche,replacement\n夜,写具体场景\n黑夜,写具体场景\n").unwrap(),
        );
        // #31：多列检索面的死行——每一列的 token 都短于下限（单列口径只会看 genre，漏判）
        kb.tables.insert(
            "style_genre".to_string(),
            parse_csv(
                "style_genre",
                "genre,base,aliases\n电,a,b\n摇滚,rock,摇滚乐\n",
            )
            .unwrap(),
        );
        kb.tables.insert(
            "instruments".to_string(),
            parse_csv("instruments", "instrument,energy_min,energy_max\nfelt piano,7,2\n").unwrap(),
        );
        let v = kb.reachability_violations();
        assert!(
            v.iter().any(|s| s.contains("夜") && s.contains("长度 1")),
            "单字检索键未被审计抓出: {:?}",
            v
        );
        assert!(
            v.iter().any(|s| s.contains("style_genre") && s.contains("第 1 行")),
            "多列全短 token 的死行未被审计抓出: {:?}",
            v
        );
        assert!(
            !v.iter().any(|s| s.contains("第 2 行")),
            "第 2 行经 base/aliases 可达，不得误报: {:?}",
            v
        );
        assert!(v.iter().any(|s| s.contains("倒置")), "能量区间倒置未被审计抓出: {:?}", v);
        // 检索列缺失（列名写错 = 该列候选恒空）必须单独报出，而不是静默收窄
        kb.tables.insert(
            "style_genre".to_string(),
            parse_csv("style_genre", "genre,base\n摇滚,rock\n").unwrap(),
        );
        let v2 = kb.reachability_violations();
        assert!(
            v2.iter().any(|s| s.contains("aliases") && s.contains("不存在")),
            "缺失的检索列未被审计抓出: {:?}",
            v2
        );
    }

    /// 微观②改写（M18）：全部未命中 → 零行 + 无示例标注，由调用方走确定性默认
    #[test]
    fn render_filtered_any_no_match_fallback() {
        let t = parse_csv("t", "a,b\nx,1\ny,2\nz,3\nw,4\n").unwrap();
        let kb = KnowledgeBase { tables: [("t".to_string(), t)].into_iter().collect() };
        let out = kb.render_filtered_any("t", &[("a", &["zzz"])], None, "", None, &[]).unwrap();
        assert!(out.contains("未命中关键词"), "got: {}", out);
        assert!(out.contains("无示例"), "未命中应明确无示例: {}", out);
        assert!(out.contains("按功能选用并说明理由"), "未命中应给新指令: {}", out);
        assert!(!out.contains("| x | 1 |"), "M18 后未命中不再塞随机行: {}", out);
    }

    /// 微观②：instruments 能量区间过滤（数值比较 + 上限）
    #[test]
    fn render_instruments_by_energy_filters_range() {
        let t = parse_csv("t", "instrument,energy_min,energy_max\na,1,3\nb,4,6\nc,7,9\nd,8,10\n").unwrap();
        let kb = KnowledgeBase { tables: [("instruments".to_string(), t)].into_iter().collect() };
        // 方案能量范围 [3, 9]：与 [1,3][4,6][7,9][8,10] 均有交集 → 4 件；上限 3
        let out = kb.render_instruments_by_energy(3, 9, None, "", Some(3)).unwrap();
        assert!(out.contains("命中 4 件"), "got: {}", out);
        // 无交集：方案能量 [0,0] 与全部区间无交集 → 兜底
        let out2 = kb.render_instruments_by_energy(0, 0, None, "", None).unwrap();
        assert!(out2.contains("未命中能量区间"), "got: {}", out2);
    }

    /// 主次深化：多维打分排序——能量接近、风格命中、lead 主奏优先
    #[test]
    fn instruments_ranked_by_energy_style_and_priority() {
        let t = parse_csv(
            "instruments",
            "instrument,energy_min,energy_max,style_tags,priority\na,4,6,rock,lead\nb,8,10,metal,color\nc,7,9,rock,lead\nd,1,3,folk,support\n",
        )
        .unwrap();
        let kb = KnowledgeBase { tables: [("instruments".to_string(), t)].into_iter().collect() };
        // 方案能量 [1,10] 全命中 + 含 rock：a(中心5,命中rock,lead) 最相关；c(中心8,命中rock,lead) 次之；
        // d(中心2,folk,support) 再次；b(中心9,metal,color) 最后（风格不命中且色彩靠后）
        let out = kb.render_instruments_by_energy(1, 10, None, "rock 愤怒", None).unwrap();
        let idx_a = out.find("| a |").expect("a 应命中");
        let idx_c = out.find("| c |").expect("c 应命中");
        let idx_b = out.find("| b |").expect("b 应命中");
        let idx_d = out.find("| d |").expect("d 应命中");
        assert!(idx_a < idx_c, "能量更近+风格命中的 a 应排 c 前");
        assert!(idx_c < idx_d, "风格命中+lead 的 c 应排 support 的 d 前");
        assert!(idx_d < idx_b, "color 且风格不命中的 b 应最后");
    }

    /// 颗粒度对齐：emotions 命中后按能量距离 + core 权重排序
    #[test]
    fn emotions_ranked_by_energy_and_level() {
        let t = parse_csv(
            "emotions",
            "emotion,energy_min,energy_max,emotion_level\n愤怒,7,9,core\n悲伤,2,5,core\n麻木,1,3,secondary\n喜悦,6,9,core\n忐忑,3,6,secondary\n",
        )
        .unwrap();
        let kb = KnowledgeBase { tables: [("emotions".to_string(), t)].into_iter().collect() };
        // 分数：愤怒=能量9+命中3+core2=14 > 喜悦=能量10+core2=12 > 悲伤=能量6+core2=8 > 忐忑=能量7=7 > 麻木=能量5=5
        let out = kb
            .render_filtered_any("emotions", &[("emotion", &["愤怒", "悲伤", "麻木", "喜悦", "忐忑"])], None, "愤怒 能量:6 到 能量:9", None, &[])
            .unwrap();
        let i_x = out.find("| 喜悦 |").expect("喜悦应命中");
        let i_f = out.find("| 愤怒 |").expect("愤怒应命中");
        let i_t = out.find("| 忐忑 |").expect("忐忑应命中");
        let i_s = out.find("| 悲伤 |").expect("悲伤应命中");
        let i_m = out.find("| 麻木 |").expect("麻木应命中");

        assert!(i_f < i_x, "命中+core 的愤怒应排喜悦前");
        assert!(i_x < i_s, "core+能量近的喜悦应排悲伤前");
        assert!(i_s < i_t, "core 的悲伤应排 secondary 的忐忑前");
        assert!(i_t < i_m, "能量近的忐忑应排麻木前");
    }

    /// 颗粒度对齐：style_genre BPM 匹配加分（方案含 BPM 时命中区间流派优先）
    #[test]
    fn style_genre_ranked_by_bpm_match() {
        let t = parse_csv(
            "style_genre",
            "genre,bpm_range\n深夜室内民谣,60-75\nfestival EDM,120-140\n抒情流行,70-90\n",
        )
        .unwrap();
        let kb = KnowledgeBase { tables: [("style_genre".to_string(), t)].into_iter().collect() };
        // 方案含 130BPM：EDM(120-140 命中) 优先于民谣(60-75 不命中)
        let out = kb
            .render_filtered_any("style_genre", &[("genre", &["深夜室内民谣", "festival EDM", "抒情流行"])], None, "130BPM 4/4 全场高能", None, &[])
            .unwrap();
        let i_edm = out.find("| festival EDM |").expect("EDM 应命中");
        let i_folk = out.find("| 深夜室内民谣 |").expect("民谣应命中");
        assert!(i_edm < i_folk, "BPM 命中的 EDM 应排民谣前");
    }

    /// 能量范围提取：四种格式（能量:/能量 /energy:/energy ）全部兼容，min/max 正确
    #[test]
    fn plan_energy_range_supports_four_formats() {
        // 中文冒号 + 中文空格 + 英文冒号 + 英文空格混合
        let plan = "Intro 能量:2，Verse 能量 4，Chorus energy:7，Final energy 9";
        assert_eq!(plan_energy_range_str(plan), Some((2, 9)));
        // 无能量标注 → None
        assert_eq!(plan_energy_range_str("BPM 120 深夜民谣"), None);
    }

    /// BPM 提取：数字前紧邻中文/全角字符时不得 panic（char 边界安全），BPM 值正确
    #[test]
    fn plan_bpm_value_char_boundary_safe() {
        // 触发场景：before 中数字前的非数字字符是多字节中文（旧实现 rfind+1 落在字符内部 panic）
        let plan = "4/4拍 速度120BPM 慢速深夜民谣";
        assert_eq!(plan_bpm_value(plan), Some(120));
        // 常规：BPM 前带空格
        assert_eq!(plan_bpm_value("深夜民谣 68 BPM"), Some(68));
        // 无 BPM后不再猜值，直接 None
        assert_eq!(plan_bpm_value("拍号 4/4 节奏"), None);
    }

    /// 年代词不再误判——只信任显式 BPM 标注，无 BPM 字样返回 None
    #[test]
    fn plan_bpm_value_ignores_era_words() {
        // "80年代" 的 80 不得被当成 BPM（旧 fallback 会取首个 60-200 数字 → 80）
        assert_eq!(plan_bpm_value("80年代复古Disco, 125BPM"), Some(125));
        assert_eq!(plan_bpm_value("80年代Disco 4/4拍"), None);
        assert_eq!(plan_bpm_value("90s hip hop"), None);
        // 合理性过滤：邻近的 0-10 数值（如能量标注）不得被当成 BPM
        assert_eq!(plan_bpm_value("能量:8 BPM 范围说明"), None);
        // "BPM 90" 前置书写同样支持
        assert_eq!(plan_bpm_value("BPM 90 起步"), Some(90));
    }

    /// 能量范围提取：段号不误算（"Verse 1" 的 1 跳过），energy 词尾数字提取
    #[test]
    fn plan_energy_range_skips_section_numbers() {
        let plan = "能量轨迹：Verse 1 energy 3，Pre-Chorus 2 energy 8";
        assert_eq!(plan_energy_range_str(plan), Some((3, 8)));
        // 单值区间（只标注一个能量）
        let single = "说明行 能量:5";
        assert_eq!(plan_energy_range_str(single), Some((5, 5)));
        // 大写形态
        let upper = "能量轨迹：Verse 1 Energy 6，Chorus ENERGY 9";
        assert_eq!(plan_energy_range_str(upper), Some((6, 9)));
    }

    /// 列投影：只渲染指定列（表头与行都裁剪），过滤仍基于原表全列
    #[test]
    fn render_filtered_any_with_cols_projects_columns() {
        let t = parse_csv(
            "t",
            "cliche,example,banned_formula,replacement\n孤独,孤单一个人,禁直接使用,写独处现场感\n温柔,你的温柔,禁空洞用温柔,写具体言行\n",
        )
        .unwrap();
        let kb = KnowledgeBase { tables: [("t".to_string(), t)].into_iter().collect() };
        let out = kb
            .render_filtered_any(
                "t",
                &[("cliche", &["孤独"])],
                Some(&["cliche", "banned_formula", "replacement"]),
                "",
                None,
                &[],
            )
            .unwrap();
        assert!(out.contains("| cliche | banned_formula | replacement |"), "got: {}", out);
        assert!(out.contains("孤独"), "got: {}", out);
        assert!(!out.contains("孤单一个人"), "example 列应被裁掉: {}", out);
        assert!(!out.contains("| example |"), "表头不应含 example: {}", out);
    }

    /// 列投影：投影列不存在 → 报错（防止角色绑定写错列名静默失败）
    #[test]
    fn render_with_cols_missing_column_errors() {
        let t = parse_csv("t", "a,b\nx,1\n").unwrap();
        let kb = KnowledgeBase { tables: [("t".to_string(), t)].into_iter().collect() };
        let err = kb.render_filtered_any("t", &[("a", &["x"])], Some(&["a", "nope"]), "", None, &[]).unwrap_err();
        assert!(err.contains("缺少列: nope"), "got: {}", err);
    }
}

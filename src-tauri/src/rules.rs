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
/// D 模式 BPM 下限
pub const DOUYIN_BPM_MIN: u32 = 90;
/// D 模式说明行上限字符数（Q2 起代码已硬门：validator.rs 说明行检查；checklist 同口径）
pub const DOUYIN_DESC_LINE_MAX_CHARS: usize = 80;
/// Mode C 尾部收尾允许行数
pub const LYRIC_FILL_TAIL_ALLOW: usize = 2;

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

/// C1/ADR-2：规则注册表——每条硬门规则一个条目，"每条规则唯一执行者"的机读清单。
/// 守护测试 `rule_registry_constants_have_executors` 保证：constants 列出的每个常量
/// 必须在执行文件中有真实消费者（死常量在 CI 即失败）。
pub struct RuleSpec {
    pub id: &'static str,
    pub modes: &'static [&'static str],
    pub symbols: &'static [&'static str],
    pub executor: &'static str,
}

pub const RULE_REGISTRY: &[RuleSpec] = &[
    RuleSpec { id: "style_prompt_max", modes: &["mode_a", "mode_b"], symbols: &["STYLE_PROMPT_MAX_CHARS"], executor: "validate_production" },
    RuleSpec { id: "style_prompt_min", modes: &["mode_a", "mode_b", "mode_d"], symbols: &["STYLE_PROMPT_MIN_CHARS"], executor: "check_style_prompt_blocks" },
    RuleSpec { id: "min_section_tags", modes: &["mode_a", "mode_b"], symbols: &["MIN_SECTION_TAGS"], executor: "validate_production" },
    RuleSpec { id: "min_energy_gap", modes: &["mode_a", "mode_b"], symbols: &["MIN_ENERGY_GAP"], executor: "validate_production" },
    RuleSpec { id: "instrument_gap_weak_strong_max", modes: &["mode_a", "mode_b"], symbols: &["MIN_INSTRUMENT_GAP", "MIN_INSTRUMENT_WEAK", "MIN_INSTRUMENT_STRONG", "INSTRUMENT_MAX"], executor: "validate_production" },
    RuleSpec { id: "audio_influence_zero", modes: &["mode_a", "mode_b"], symbols: &[], executor: "check_audio_influence" },
    // ARC_PARAMS 经 arc_param_union() 折叠后在 validator 消费，对外消费符号登记访问器
    RuleSpec { id: "mode_a_param_arc_union", modes: &["mode_a"], symbols: &["arc_param_union"], executor: "validate_production" },
    RuleSpec { id: "mode_b_param_range", modes: &["mode_b"], symbols: &["MODE_B_WEIRD_MIN", "MODE_B_WEIRD_MAX", "MODE_B_STYLE_MIN", "MODE_B_STYLE_MAX"], executor: "validate_production" },
    RuleSpec { id: "mode_d_param_range", modes: &["mode_d"], symbols: &["DOUYIN_WEIRD_MIN", "DOUYIN_WEIRD_MAX", "DOUYIN_STYLE_MIN", "DOUYIN_STYLE_MAX"], executor: "validate_douyin" },
    RuleSpec { id: "hook_min_count", modes: &["mode_d"], symbols: &["HOOK_MIN_COUNT"], executor: "validate_douyin" },
    RuleSpec { id: "verse_max_lines", modes: &["mode_d"], symbols: &["VERSE_MAX_LINES"], executor: "validate_douyin" },
    RuleSpec { id: "douyin_line_max", modes: &["mode_d"], symbols: &["DOUYIN_LINE_MAX_CHARS"], executor: "validate_douyin" },
    RuleSpec { id: "douyin_desc_line_max", modes: &["mode_d"], symbols: &["DOUYIN_DESC_LINE_MAX_CHARS"], executor: "validate_douyin" },
    RuleSpec { id: "douyin_bpm_min", modes: &["mode_d"], symbols: &["DOUYIN_BPM_MIN"], executor: "check_style_prompt_blocks" },
    RuleSpec { id: "lyric_fill_tail_allow", modes: &["mode_c"], symbols: &["LYRIC_FILL_TAIL_ALLOW"], executor: "validate_lyric_fill" },
];

/// 模式校验清单（数字唯一 prose 载体；与上方常量同文件维护）。
/// 入参为 `Mode::to_str_name()`（mode_a/mode_b/mode_c/mode_d），未知模式回退通用清单。
pub fn checklist(mode: &str) -> &'static str {
    match mode {
        "mode_a" => CHECKLIST_A,
        // R-1：B 模式独立清单——参数准绳是 B 专属区间（20-35/75-85），弧线区间不再混入 B 清单
        "mode_b" => CHECKLIST_B,
        "mode_c" => CHECKLIST_C,
        "mode_d" => CHECKLIST_D,
        _ => CHECKLIST_A,
    }
}

const CHECKLIST_A: &str = "【校验清单 A·单源】Style Prompt≤350字符且≥30字符；结构标签≥2段；能量差≥3级（0-10）；配器差≥2件、单段3-7件（最弱段即下限3件）、最强段≥5件；弧线参数：标准叙事22-28/78-83、全程高能10-15/85-90、高开低走25-35/70-80、平铺氛围15-25/80-90、起伏戏剧28-35/75-82、阶梯上升20-28/78-88、渐进爆发15-25/80-90、U型25-35/70-82、单峰20-30/75-85、回环20-28/78-85；Audio Influence=0；断句单空格、禁/与、标点全半角。";
const CHECKLIST_B: &str = "【校验清单 B·单源】Style Prompt≤350字符且≥30字符；结构标签≥2段；能量差≥3级（0-10）；配器差≥2件、单段3-7件（最弱段即下限3件）、最强段≥5件；参数固定区间：Weirdness 20-35、Style Influence 75-85（B 专属区间为准，弧线区间不适用 B）；Audio Influence=0；断句单空格、禁/与、标点全半角。";
const CHECKLIST_C: &str = "【校验清单 C·单源】逐行等字数（差一字即失败，尾部≤2行收尾）；行数与原歌词一致；段落结构与原歌词一致（禁新增Hook/Chorus段）；韵脚位置与模式保留；说明行带方括号；Style Prompt≤350字符；断句单空格、禁/与、标点全半角。";
const CHECKLIST_D: &str = "【校验清单 D·单源】Hook≥2次；单段Verse≤4行；每行≤10字；结尾骤停（一刀切，含abruptly/cut标识）；BPM≥90；Style Prompt≤350字符且≥30字符；说明行≤80字符；参数抖音12-20/85-95（仅当含≥2叙事段Verse/Pre-Chorus/Bridge且方案写明'叙事型'归类，方可回落A/B弧线区间，二者缺一打回）；Audio Influence=0；断句单空格、禁/与、标点全半角。";

/// 阶段 0 地基 primer（模式专属，每模式≤800字；M9 顺序：受众→物件→约束→旋律）。
/// 主持人零 CSV 语义不变，仅拼接本静态文本；缺失回退无 primer（旧行为）。
pub fn host_primer(mode: &str) -> Option<&'static str> {
    match mode {
        "mode_a" | "mode_b" => Some(PRIMER_AB),
        "mode_c" => Some(PRIMER_C),
        "mode_d" => Some(PRIMER_D),
        _ => None,
    }
}

// A/B：受众一句话 + 画面清单模板 + 借体要求 + 弧线能量标尺。
const PRIMER_AB: &str = "【阶段0地基·A/B】受众：先一句话定对象阅历与时机（写给谁听、何时听），不到不写。物件：先建时空物件清单（≥5件具体物，禁抽象词开局），再定意象家族（一首歌一个系统）与声学映射（每个意象写出乐器/音色对应）。约束：声调服从旋律走向（硬门），韵脚密度为软优化；Verse2须新增信息；借体覆盖：每个抽象词配具体物象，禁裸奔。旋律骨架：先定弧线10选1与能量标尺（0-2静止/3-4铺垫/5-6推进/7-8爆发/9-10用尽全力），弱强差≥3级，配器差≥2件，全曲≤7件。";
// C：对齐铁律 + 节奏保留 + 借体。
const PRIMER_C: &str = "【阶段0地基·C】对齐铁律：逐行等字数（差一字即失败）、行数与原歌词一致、段落结构与原歌词一致（禁新增段）。节奏保留：词组切分与原歌词一致（3+4、2+2+3等），呼吸点位置一致，韵脚位置与模式保留。借体：新意象转译原意象叙事功能（非替换），意象家族统一，抽象词每个有借体，套话具体化。";
// D：前3秒画面 + 道具刻度 + 骤停提醒 + 字数提醒。
const PRIMER_D: &str = "【阶段0地基·D】前3秒：开场3秒内建钩子或强画面，直接进内容不慢铺垫。道具刻度：核心情感绑有世俗重量的具体物（物作刻度），金句短、魔性、可独立传播，Hook≥2次。约束：Verse≤4行，每行≤10字，说明行≤80字符，BPM≥90。收尾：结尾骤停一刀切（不渐弱），动态标签用对，骤停后无乐器残留。";

#[cfg(test)]
mod tests {
    use super::*;

    /// D2 守护：注册表每条规则的执行器必须在 validator/orchestrator 源码中存在，
    /// 且每个登记的消费符号在 rules.rs 之外有消费者——删掉执行点即红，
    /// 防止常量再次退化成"只有承诺没有执行"的死常量（诊断铁证：参数区间零执行）。
    #[test]
    fn rule_registry_symbols_have_executors() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let validator = std::fs::read_to_string(src.join("commands/validator.rs")).expect("读 validator.rs 失败");
        let orchestrator = std::fs::read_to_string(src.join("commands/orchestrator.rs")).expect("读 orchestrator.rs 失败");
        assert!(RULE_REGISTRY.len() >= 15, "注册表条目数异常: {}", RULE_REGISTRY.len());
        for rule in RULE_REGISTRY {
            assert!(
                validator.contains(rule.executor) || orchestrator.contains(rule.executor),
                "规则 {} 的执行器 {} 未在 validator/orchestrator 中实现",
                rule.id, rule.executor
            );
            for sym in rule.symbols {
                assert!(
                    validator.contains(sym) || orchestrator.contains(sym),
                    "死常量回归：{} 被规则 {} 注册但 rules.rs 之外无消费者",
                    sym, rule.id
                );
            }
        }
    }

    #[test]
    fn primer_within_800_chars_per_mode() {
        for m in ["mode_a", "mode_b", "mode_c", "mode_d"] {
            let p = host_primer(m).expect("四模式 primer 缺失");
            let n = p.chars().count();
            assert!(n <= 800, "{} primer {} 字超 800", m, n);
            assert!(n > 30, "{} primer 过短", m);
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
}

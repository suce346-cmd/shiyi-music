//! P1-13 文本评估（LLM-as-Judge，M-09/M-10）——**旁路**，默认关闭。
//!
//! - 开关：`PipelineRequest.text_eval_judge` / 设置 `textEvalJudge`（缺省 false）
//! - Judge 模型：与圆桌同 key + 同 model（用户拍板 same_as_roundtable）
//! - 报告恒标「小样本」直至人工校准 ≥40 条且一致率 ≥80%（见 `docs/qa/text-eval-rubric.md`）

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::budget::SharedBudget;
use crate::commands::llm;
use crate::errors::AppError;
use crate::models::GenerationConfig;

/// 七维评分卡（与 docs/qa/2026-09-09-large-scale-test-plan.md 产品维度冻结同口径）
pub const DIMENSIONS: &[&str] = &[
    "arrangement_norm",
    "arrangement_diversity",
    "coherence",
    "non_cliche",
    "rhyme",
    "lyric_norm",
    "hook_quality",
];

/// 单维 0–2
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DimScore {
    pub id: String,
    pub score: u8,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JudgeScores {
    pub dimensions: Vec<DimScore>,
    /// 七维合计 0–14
    pub total: u8,
    /// 恒为 true 直至人工校准达标（R-10）
    pub small_sample: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DistributionReport {
    pub n: usize,
    pub mean_total: f64,
    pub per_dim_mean: Vec<(String, f64)>,
    pub small_sample: bool,
}

/// 冻结提示词池（20 条；扩展自 docs/qa/test-prompts.md 的 10 条）
pub const FROZEN_PROMPTS_20: &[&str] = &[
    "深夜加班打工人的心酸和坚持，要炸，要洗脑循环",
    "夏夜暴雨后巷口的烧烤摊，烟火气加一点孤独感",
    "健身房撸铁逆袭的热血，重拍炸裂，荷尔蒙爆棚",
    "早八人挤地铁的麻木与自嘲，鬼畜循环，魔性上头",
    "江南梅雨季的青石板巷，油纸伞下欲说还休的告别，国风叙事",
    "疫情后第一次回家过年，站台父子无言的拥抱，温暖催泪",
    "深海孤独症：社会性死亡后的自我放逐，电子冷感，意识流",
    "高考落榜生在天台看日出，从失落走向释怀，摇滚燃向",
    "异地恋第七年，视频里笑着说没事挂了电话哭，城市流行苦情",
    "老房子拆迁前夜，三代人记忆的墙面剥落，民谣叙事",
    // P11–P20：扩至 20（风格×情感×形式补洞）
    "毕业季天台大合唱，青春散场又热血，大合唱氛围",
    "赛博朋克夜市霓虹下的独行侠，电子迷幻，冷酷拽感",
    "咖啡店窗边写不出论文的焦虑，Lo-fi 轻拍，自嘲疏解",
    "老家田埂上外婆的蒲扇与蝉鸣，民谣慢板，怀旧温暖",
    "电竞决赛最后一波团战，电子鼓点炸裂，热血短 Hook",
    "雨夜便利店偶遇前任，城市流行，克制不煽情",
    "登山者在风口的沉默与坚持，后摇铺陈，力量感",
    "孩子第一次独自上学，童谣质感，小心翼翼的勇敢",
    "加班后的凌晨公交，说唱碎拍，麻木里的幽默",
    "古镇灯笼倒映水面，国风电子融合，留白与回响",
];

pub fn build_judge_prompt(mode: &str, user_input: &str, final_text: &str) -> String {
    format!(
        "你是提示词文本质量评审（不是听感评审）。按七维各打 0/1/2 分。\n\
模式: {mode}\n用户输入:\n{user_input}\n\n终稿文本:\n{final_text}\n\n\
维度与标准：\n\
- arrangement_norm: 0 违反硬线 / 1 贴线 / 2 全在区间\n\
- arrangement_diversity: 0 配方重复感强 / 1 部分重叠 / 2 独立配方\n\
- coherence: 0 配器能量自相矛盾 / 1 局部矛盾 / 2 全曲自洽\n\
- non_cliche: 0 结构金句套路 / 1 部分套路 / 2 独立结构\n\
- rhyme: 0 押韵混乱 / 1 部分押韵 / 2 一韵到底或有意换韵\n\
- lyric_norm: 0 字数行数违规 / 1 贴线 / 2 全部合规\n\
- hook_quality: 0 无记忆点 / 1 有 Hook 无差异 / 2 记忆点+变化\n\n\
只输出 JSON：{{\"dimensions\":[{{\"id\":\"arrangement_norm\",\"score\":0,\"note\":\"...\"}},...]}}\n\
必须覆盖全部七维 id；score 只能是 0、1 或 2。"
    )
}

/// 解析 Judge 模型输出（容错：剥离围栏 / 取首个 JSON 对象）
pub fn parse_judge_response(raw: &str) -> Result<JudgeScores, String> {
    let trimmed = raw.trim();
    let json_str = extract_json_object(trimmed).ok_or_else(|| "无 JSON 对象".to_string())?;
    let v: Value = serde_json::from_str(json_str).map_err(|e| format!("JSON 解析失败: {e}"))?;
    let arr = v
        .get("dimensions")
        .and_then(|x| x.as_array())
        .ok_or_else(|| "缺 dimensions 数组".to_string())?;
    let mut dimensions = Vec::new();
    for id in DIMENSIONS {
        let found = arr.iter().find(|d| d.get("id").and_then(|x| x.as_str()) == Some(*id));
        let Some(item) = found else {
            return Err(format!("缺维度 {id}"));
        };
        let score = item
            .get("score")
            .and_then(|x| x.as_u64())
            .ok_or_else(|| format!("{id} 缺 score"))?;
        if score > 2 {
            return Err(format!("{id} score={score} 越界（须 0–2）"));
        }
        let note = item
            .get("note")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        dimensions.push(DimScore {
            id: (*id).to_string(),
            score: score as u8,
            note,
        });
    }
    let total = dimensions.iter().map(|d| d.score).sum();
    Ok(JudgeScores {
        dimensions,
        total,
        small_sample: true, // 未校准前恒小样本
    })
}

fn extract_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let mut depth = 0i32;
    for (i, ch) in s[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..start + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// 人工 vs Judge：逐维完全一致比例（校准用）
pub fn agreement_rate(human: &JudgeScores, judge: &JudgeScores) -> f64 {
    let mut ok = 0usize;
    let mut n = 0usize;
    for id in DIMENSIONS {
        let h = human.dimensions.iter().find(|d| d.id == *id).map(|d| d.score);
        let j = judge.dimensions.iter().find(|d| d.id == *id).map(|d| d.score);
        if let (Some(a), Some(b)) = (h, j) {
            n += 1;
            if a == b {
                ok += 1;
            }
        }
    }
    if n == 0 {
        0.0
    } else {
        ok as f64 / n as f64
    }
}

/// 一致率达标（≥80%）且样本量 ≥40 才可取消小样本标记
pub fn calibration_trusted(n_pairs: usize, agreement: f64) -> bool {
    n_pairs >= 40 && agreement >= 0.80
}

pub fn aggregate_report(scores: &[JudgeScores]) -> DistributionReport {
    let n = scores.len();
    if n == 0 {
        return DistributionReport {
            n: 0,
            mean_total: 0.0,
            per_dim_mean: DIMENSIONS.iter().map(|id| ((*id).to_string(), 0.0)).collect(),
            small_sample: true,
        };
    }
    let mean_total = scores.iter().map(|s| f64::from(s.total)).sum::<f64>() / n as f64;
    let per_dim_mean = DIMENSIONS
        .iter()
        .map(|id| {
            let sum: f64 = scores
                .iter()
                .filter_map(|s| s.dimensions.iter().find(|d| d.id == *id))
                .map(|d| f64::from(d.score))
                .sum();
            ((*id).to_string(), sum / n as f64)
        })
        .collect();
    DistributionReport {
        n,
        mean_total,
        per_dim_mean,
        small_sample: scores.iter().all(|s| s.small_sample),
    }
}

/// 请求是否启用 Judge（显式 true 才开；缺省/false 永不调用）
pub fn judge_enabled(flag: Option<bool>) -> bool {
    flag == Some(true)
}

/// 调用圆桌同配置模型打分（失败不阻断主流程——由调用方决定）
pub async fn run_judge(
    base_url: &str,
    api_key: &str,
    model: &str,
    mode: &str,
    user_input: &str,
    final_text: &str,
    budget: &SharedBudget,
    gen: &GenerationConfig,
    run_id: &str,
) -> Result<JudgeScores, AppError> {
    let prompt = build_judge_prompt(mode, user_input, final_text);
    let messages = vec![
        json!({"role":"system","content":"你只输出合法 JSON，不要 Markdown 围栏。"}),
        json!({"role":"user","content":prompt}),
    ];
    let resp = llm::call_llm_silent(
        base_url,
        api_key,
        model,
        messages,
        1024,
        false, // Judge 关闭思考以控成本/延迟
        budget,
        gen,
        run_id,
        llm::FINAL_STAGE_RESERVE,
        None,
    )
    .await?;
    parse_judge_response(&resp.raw).map_err(|m| {
        AppError::new(
            crate::errors::ErrorKind::Parse,
            format!("Judge 输出无法解析: {m}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_pool_is_20() {
        assert_eq!(FROZEN_PROMPTS_20.len(), 20);
        let mut set = std::collections::BTreeSet::new();
        for p in FROZEN_PROMPTS_20 {
            assert!(set.insert(*p), "重复提示词: {p}");
        }
    }

    #[test]
    fn dimensions_are_seven() {
        assert_eq!(DIMENSIONS.len(), 7);
    }

    #[test]
    fn parse_valid_json() {
        let raw = r#"{"dimensions":[
            {"id":"arrangement_norm","score":2,"note":"ok"},
            {"id":"arrangement_diversity","score":1,"note":""},
            {"id":"coherence","score":2,"note":""},
            {"id":"non_cliche","score":1,"note":""},
            {"id":"rhyme","score":0,"note":"乱"},
            {"id":"lyric_norm","score":2,"note":""},
            {"id":"hook_quality","score":1,"note":""}
        ]}"#;
        let s = parse_judge_response(raw).unwrap();
        assert_eq!(s.total, 9);
        assert!(s.small_sample);
    }

    #[test]
    fn parse_rejects_oob_score() {
        let raw = r#"{"dimensions":[
            {"id":"arrangement_norm","score":3,"note":""},
            {"id":"arrangement_diversity","score":1,"note":""},
            {"id":"coherence","score":1,"note":""},
            {"id":"non_cliche","score":1,"note":""},
            {"id":"rhyme","score":1,"note":""},
            {"id":"lyric_norm","score":1,"note":""},
            {"id":"hook_quality","score":1,"note":""}
        ]}"#;
        assert!(parse_judge_response(raw).is_err());
    }

    #[test]
    fn parse_strips_fence() {
        let raw = "```json\n{\"dimensions\":[\n{\"id\":\"arrangement_norm\",\"score\":2,\"note\":\"\"},\n{\"id\":\"arrangement_diversity\",\"score\":2,\"note\":\"\"},\n{\"id\":\"coherence\",\"score\":2,\"note\":\"\"},\n{\"id\":\"non_cliche\",\"score\":2,\"note\":\"\"},\n{\"id\":\"rhyme\",\"score\":2,\"note\":\"\"},\n{\"id\":\"lyric_norm\",\"score\":2,\"note\":\"\"},\n{\"id\":\"hook_quality\",\"score\":2,\"note\":\"\"}\n]}\n```";
        let s = parse_judge_response(raw).unwrap();
        assert_eq!(s.total, 14);
    }

    #[test]
    fn agreement_and_calibration_gate() {
        let mk = |scores: [u8; 7]| JudgeScores {
            dimensions: DIMENSIONS
                .iter()
                .zip(scores)
                .map(|(id, score)| DimScore {
                    id: (*id).to_string(),
                    score,
                    note: String::new(),
                })
                .collect(),
            total: scores.iter().sum(),
            small_sample: true,
        };
        let a = mk([2, 2, 2, 2, 2, 2, 2]);
        let b = mk([2, 2, 2, 2, 2, 2, 1]);
        assert!((agreement_rate(&a, &b) - 6.0 / 7.0).abs() < 1e-9);
        assert!(!calibration_trusted(39, 0.99));
        assert!(!calibration_trusted(40, 0.79));
        assert!(calibration_trusted(40, 0.80));
    }

    #[test]
    fn judge_enabled_default_off() {
        assert!(!judge_enabled(None));
        assert!(!judge_enabled(Some(false)));
        assert!(judge_enabled(Some(true)));
    }

    #[test]
    fn aggregate_distribution() {
        let s = parse_judge_response(
            r#"{"dimensions":[
            {"id":"arrangement_norm","score":2,"note":""},
            {"id":"arrangement_diversity","score":2,"note":""},
            {"id":"coherence","score":2,"note":""},
            {"id":"non_cliche","score":2,"note":""},
            {"id":"rhyme","score":2,"note":""},
            {"id":"lyric_norm","score":2,"note":""},
            {"id":"hook_quality","score":2,"note":""}
        ]}"#,
        )
        .unwrap();
        let r = aggregate_report(&[s.clone(), s]);
        assert_eq!(r.n, 2);
        assert!((r.mean_total - 14.0).abs() < 1e-9);
        assert!(r.small_sample);
    }
}

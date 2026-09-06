//! 知识库加载器：从 `knowledge/*.csv` 加载专家知识表。
//!
//! 每张表在启动时加载进内存，按需渲染成 markdown 表格文本，
//! 注入对应专家的 system prompt（「CSV 注入 prompt」方案）。

use std::collections::HashMap;
use std::fs;
use std::path::Path;

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
        load_embedded_internal().expect("嵌入知识库损坏（构建期错误）")
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

    /// 按"列名 → 候选值列表"过滤渲染（任一列任一候选 contains 命中即保留该行）。
    /// 微观② 按需检索：命中时注入命中条目（max_rows 上限）；全部未命中时兜底注入前 3 条并标注。
    /// cols：列投影（None=全列；角色裁剪字段用，过滤仍基于原表全列）。
    /// plan：当前方案文本——命中后按表路由多维排序（候选词命中数 + 能量距离/BPM/层级权重），
    /// 保证截断保留最相关条目（与 instruments 多维排序同一颗粒度）。
    pub fn render_filtered_any(
        &self,
        name: &str,
        conditions: &[(&str, &[&str])],
        cols: Option<&[&str]>,
        plan: &str,
        max_rows: Option<usize>,
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
        let mut matched_idx: Vec<(usize, i32)> = rows_all
            .iter()
            .enumerate()
            .filter(|(ri, _)| {
                conditions.iter().any(|(col, vals)| {
                    col_idx
                        .get(*col)
                        .map(|&i| {
                            table
                                .rows
                                .get(*ri)
                                .and_then(|row| row.get(i))
                                .map(|rv| vals.iter().any(|v| rv.contains(v)))
                                .unwrap_or(false)
                        })
                        .unwrap_or(false)
                })
            })
            .map(|(ri, _)| {
                let row = table.rows.get(ri).cloned().unwrap_or_default();
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
        let rows: Vec<&Vec<String>> = if hit {
            matched.iter().take(max_rows.unwrap_or(usize::MAX)).collect()
        } else {
            // 兜底：示例特征（最多 3 条），明确标注未命中
            rows_all.iter().take(3).collect()
        };
        let mut out = String::new();
        out.push_str(&format!(
            "## {} 知识库（{}）\n\n",
            table.name,
            if hit {
                let shown = rows.len();
                if matched.len() > shown {
                    format!("按需命中 {} 条，以上展示前 {} 条", matched.len(), shown)
                } else {
                    format!("按需命中 {} 条", matched.len())
                }
            } else {
                "未命中关键词，以下为示例特征（禁止照搬）".to_string()
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
    /// 无交集时兜底前 3 条并标注。cols：列投影（None=全列；能量过滤基于原表列）。
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
                "## instruments 知识库（能量区间无效 {}~{}，以下为示例特征（禁止照搬））\n\n| {} |\n|{}|\n",
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
        let rows: Vec<&Vec<String>> = if hit {
            matched.iter().take(max_rows.unwrap_or(usize::MAX)).collect()
        } else {
            rows_all.iter().take(3).collect()
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
                "未命中能量区间，以下为示例特征（禁止照搬）".to_string()
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

    /// 渲染整张表（cols：列投影，None=全列）
    pub fn render_table(
        &self,
        name: &str,
        cols: Option<&[&str]>,
        max_rows: Option<usize>,
    ) -> Result<String, String> {
        let table = self.table(name)?;
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
        let out = kb.render_table("test", None, None).unwrap();
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
        assert!(kb.render_table("nope", None, None).is_err());
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
        let out = kb.render_table("t", None, Some(2)).unwrap();
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

    /// 微观②：render_filtered_any 任一候选命中即保留（OR 语义）
    #[test]
    fn render_filtered_any_matches_any_keyword() {
        let t = parse_csv("t", "emotion,energy_words\n孤独,冷 空 钝\n愤怒,锋利 燥热\n温柔,暖 软\n").unwrap();
        let kb = KnowledgeBase { tables: [("t".to_string(), t)].into_iter().collect() };
        let out = kb.render_filtered_any("t", &[("emotion", &["孤独", "愤怒"])], None, "", None).unwrap();
        assert!(out.contains("按需命中 2 条"), "got: {}", out);
        assert!(out.contains("孤独"));
        assert!(out.contains("愤怒"));
        assert!(!out.contains("温柔"));
    }

    /// 微观②：全部未命中 → 兜底前 3 条 + 未命中标注
    #[test]
    fn render_filtered_any_no_match_fallback() {
        let t = parse_csv("t", "a,b\nx,1\ny,2\nz,3\nw,4\n").unwrap();
        let kb = KnowledgeBase { tables: [("t".to_string(), t)].into_iter().collect() };
        let out = kb.render_filtered_any("t", &[("a", &["zzz"])], None, "", None).unwrap();
        assert!(out.contains("未命中关键词"), "got: {}", out);
        assert!(out.contains("| x | 1 |"));
        assert!(!out.contains("| w | 4 |"), "兜底最多 3 条");
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
            .render_filtered_any("emotions", &[("emotion", &["愤怒", "悲伤", "麻木", "喜悦", "忐忑"])], None, "愤怒 能量:6 到 能量:9", None)
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
            .render_filtered_any("style_genre", &[("genre", &["深夜室内民谣", "festival EDM", "抒情流行"])], None, "130BPM 4/4 全场高能", None)
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
        let err = kb.render_filtered_any("t", &[("a", &["x"])], Some(&["a", "nope"]), "", None).unwrap_err();
        assert!(err.contains("缺少列: nope"), "got: {}", err);
    }
}

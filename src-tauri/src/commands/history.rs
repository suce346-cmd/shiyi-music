//! 历史记录文件持久化 + 导出（RULE-HISTORY-V2 / P1-10）。
//! 路径：{app_data}/history.json（原子写 .tmp+rename）。
//! 文件格式：`{ "schema_version": 2, "entries": [...] }`。
//! 读到无版本 / v1 裸数组 / 损坏 → 备份 .bak 后丢弃（D5），返回空。
//! 写入截断至 HISTORY_MAX_ENTRIES；命令串行（Mutex）防并发覆盖。

use crate::errors::{AppError, ErrorKind};
use serde::{Deserialize, Serialize};
use tauri::Manager;
use tokio::sync::Mutex;

/// RULE-HISTORY-V2：当前文件 schema
pub const HISTORY_SCHEMA_VERSION: u32 = 2;
/// 历史上限（D-03）
pub const HISTORY_MAX_ENTRIES: usize = 200;

/// 前端 ChatTurn 对齐
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTurn {
    pub role: String,
    pub content: String,
    pub timestamp: i64,
    #[serde(default)]
    pub speaker: Option<Speaker>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Speaker {
    pub id: String,
    pub emoji: String,
    pub name: String,
}

/// 前端 TokenUsage 对齐
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
}

/// P1-10：渲染四栏快照（与 plan::render_suno::Rendered 字段对齐，供历史回放）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryRendered {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub style: String,
    #[serde(default)]
    pub lyrics: String,
    #[serde(default)]
    pub exclude: String,
    #[serde(default)]
    pub settings: String,
}

/// 前端 HistoryEntry 对齐
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub mode: String,
    pub input: String,
    pub output: String,
    #[serde(default)]
    pub conversation: Option<Vec<ChatTurn>>,
    #[serde(default)]
    pub usage: Option<Usage>,
    pub timestamp: i64,
    #[serde(default)]
    pub degraded: Option<Vec<String>>,
    #[serde(default)]
    pub validation_passed: Option<bool>,
    /// P1-10 / RULE-HISTORY-V2：SongPlan JSON（缺省 null=旧记录或未接线）
    #[serde(default)]
    pub song_plan: Option<serde_json::Value>,
    /// P1-10：渲染结果四栏
    #[serde(default)]
    pub rendered: Option<HistoryRendered>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoryFile {
    schema_version: u32,
    entries: Vec<HistoryEntry>,
}

/// 写串行锁（C-09：并发 save 不得交错覆盖）
static HISTORY_WRITE_LOCK: Mutex<()> = Mutex::const_new(());

fn history_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("应用数据目录不可用: {}", e)))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("创建数据目录失败: {}", e)))?;
    Ok(dir.join("history.json"))
}

fn backup_corrupt(path: &std::path::Path, reason: &str) {
    let bak = path.with_extension("json.bak");
    let _ = std::fs::rename(path, &bak);
    tracing::warn!(backup = ?bak, reason, "历史文件已按 RULE-HISTORY-V2 丢弃并备份");
}

/// 纯函数：解码文件内容。v1/无版本/损坏 → 空（调用方负责备份）。
pub fn decode_history_file(content: &str) -> Result<Vec<HistoryEntry>, &'static str> {
    let v: serde_json::Value = serde_json::from_str(content).map_err(|_| "json_parse")?;
    if let Some(arr) = v.as_array() {
        // v1 裸数组 → 丢弃
        let _ = arr;
        return Err("v1_array");
    }
    let file: HistoryFile = serde_json::from_value(v).map_err(|_| "shape")?;
    if file.schema_version != HISTORY_SCHEMA_VERSION {
        return Err("bad_schema_version");
    }
    Ok(file.entries)
}

/// 纯函数：编码为 v2 文件；截断至上限（保留前 N 条=最新优先）。
pub fn encode_history_file(entries: &[HistoryEntry]) -> String {
    let capped: Vec<HistoryEntry> = entries.iter().take(HISTORY_MAX_ENTRIES).cloned().collect();
    let file = HistoryFile {
        schema_version: HISTORY_SCHEMA_VERSION,
        entries: capped,
    };
    serde_json::to_string(&file).expect("HistoryFile 可序列化")
}

fn write_atomic(path: &std::path::Path, content: &str) -> Result<(), AppError> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, content)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("写入历史失败: {}", e)))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("历史落盘失败: {}", e)))?;
    Ok(())
}

/// 读取全部历史（不存在 → 空；v1/损坏 → 备份后空）
#[tauri::command]
pub async fn history_load(app: tauri::AppHandle) -> Result<Vec<HistoryEntry>, AppError> {
    let path = history_path(&app)?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("读取历史失败: {}", e)))?;
    match decode_history_file(&content) {
        Ok(entries) => Ok(entries),
        Err(reason) => {
            backup_corrupt(&path, reason);
            Ok(vec![])
        }
    }
}

/// 全量写回（串行 + 原子写 + 上限截断）
#[tauri::command]
pub async fn history_save(app: tauri::AppHandle, entries: Vec<HistoryEntry>) -> Result<(), AppError> {
    let _guard = HISTORY_WRITE_LOCK.lock().await;
    let path = history_path(&app)?;
    let content = encode_history_file(&entries);
    write_atomic(&path, &content)
}

/// P1-10 / C-09：关闭前 flush——与 save 同路径（串行锁），供前端 bypass 防抖立即落盘。
#[tauri::command]
pub async fn history_flush(app: tauri::AppHandle, entries: Vec<HistoryEntry>) -> Result<(), AppError> {
    history_save(app, entries).await
}

/// 导出单条记录为 txt/markdown
#[tauri::command]
pub async fn history_export_text(
    app: tauri::AppHandle,
    id: String,
    format: String,
) -> Result<String, AppError> {
    let entries = history_load(app).await?;
    let e = entries
        .iter()
        .find(|x| x.id == id)
        .ok_or_else(|| AppError::new(ErrorKind::Internal, "记录不存在（可能已被删除）"))?;
    Ok(render_export(e, &format))
}

fn honesty_declaration(e: &HistoryEntry) -> String {
    let degraded_part = match &e.degraded {
        Some(flags) if !flags.is_empty() => format!("本次生成存在降级：{}", flags.join("；")),
        _ => "本次生成全环节正常".to_string(),
    };
    let gate_part = match e.validation_passed {
        Some(true) => "硬校验 通过".to_string(),
        Some(false) => "硬校验 未完全通过（详见对话记录）".to_string(),
        None => "硬校验 结论未知".to_string(),
    };
    format!(
        "> 完整度声明：{}，{}，请据此评估方案可信度。\n\n",
        degraded_part, gate_part
    )
}

fn render_export(e: &HistoryEntry, format: &str) -> String {
    let mut out = String::new();
    if format == "md" {
        out.push_str(&format!("# {} · {}\n\n", mode_label(&e.mode), e.id));
        out.push_str(&honesty_declaration(e));
        out.push_str(&format!("> 输入：{}\n\n", e.input));
        if let Some(conv) = &e.conversation {
            for t in conv {
                let who = match t.role.as_str() {
                    "user" => "🧑 用户",
                    "assistant" => "🤖 方案",
                    _ => "🎙️ 专家",
                };
                out.push_str(&format!("## {}\n\n{}\n\n", who, t.content));
            }
        } else {
            out.push_str(&format!("## 🤖 方案\n\n{}\n", e.output));
        }
        if let Some(u) = &e.usage {
            out.push_str(&format!(
                "\n---\n_tokens: 输入 {} / 输出 {}_\n",
                u.prompt_tokens, u.completion_tokens
            ));
        }
    } else {
        out.push_str(&honesty_declaration(e));
        out.push_str(&format!(
            "【{}】{}\n\n输入：{}\n\n",
            mode_label(&e.mode),
            e.id,
            e.input
        ));
        if let Some(conv) = &e.conversation {
            for t in conv {
                out.push_str(&format!("--- {} ---\n{}\n\n", t.role, t.content));
            }
        } else {
            out.push_str(&format!("--- 方案 ---\n{}\n", e.output));
        }
    }
    out
}

fn mode_label(mode: &str) -> &str {
    match mode {
        "mode_a" => "Mode A",
        "mode_b" => "Mode B",
        "mode_c" => "Mode C",
        "mode_d" => "Mode D",
        _ => "未知模式",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_entry(id: &str) -> HistoryEntry {
        HistoryEntry {
            id: id.into(),
            mode: "mode_b".into(),
            input: "i".into(),
            output: "o".into(),
            conversation: None,
            usage: None,
            timestamp: 1,
            degraded: None,
            validation_passed: None,
            song_plan: None,
            rendered: None,
        }
    }

    #[test]
    fn v1_array_is_discarded() {
        let v1 = r#"[{"id":"1","mode":"mode_d","input":"灵感","output":"方案","timestamp":1}]"#;
        assert_eq!(decode_history_file(v1).unwrap_err(), "v1_array");
    }

    #[test]
    fn missing_or_wrong_schema_discarded() {
        let no_ver = r#"{"entries":[]}"#;
        assert!(decode_history_file(no_ver).is_err());
        let v1file = r#"{"schema_version":1,"entries":[]}"#;
        assert_eq!(
            decode_history_file(v1file).unwrap_err(),
            "bad_schema_version"
        );
    }

    #[test]
    fn v2_roundtrip_and_cap() {
        let mut entries: Vec<_> = (0..250).map(|i| sample_entry(&format!("id{i}"))).collect();
        let json = encode_history_file(&entries);
        let back = decode_history_file(&json).unwrap();
        assert_eq!(back.len(), HISTORY_MAX_ENTRIES);
        assert_eq!(back[0].id, "id0");
        // song_plan / rendered 往返
        entries[0].song_plan = Some(serde_json::json!({"schema_version": 1, "title": "雨巷"}));
        entries[0].rendered = Some(HistoryRendered {
            title: Some("雨巷".into()),
            style: "folk".into(),
            lyrics: "雨落".into(),
            exclude: "edm".into(),
            settings: "Weirdness: 30".into(),
        });
        let json2 = encode_history_file(&entries[..1]);
        let back2 = decode_history_file(&json2).unwrap();
        assert_eq!(back2[0].song_plan.as_ref().unwrap()["title"], "雨巷");
        assert_eq!(back2[0].rendered.as_ref().unwrap().style, "folk");
    }

    #[test]
    fn history_entry_optional_fields_default() {
        let old = serde_json::json!({
            "id": "1", "mode": "mode_d", "input": "灵感", "output": "方案", "timestamp": 1
        });
        let e: HistoryEntry = serde_json::from_value(old).unwrap();
        assert!(e.conversation.is_none());
        assert!(e.song_plan.is_none());
        assert!(e.rendered.is_none());
    }

    #[test]
    fn render_export_formats() {
        let e = HistoryEntry {
            id: "x".into(),
            mode: "mode_d".into(),
            input: "灵感".into(),
            output: "方案".into(),
            conversation: None,
            usage: Some(Usage {
                prompt_tokens: 5,
                completion_tokens: 6,
            }),
            timestamp: 0,
            degraded: None,
            validation_passed: None,
            song_plan: None,
            rendered: None,
        };
        let md = render_export(&e, "md");
        assert!(md.contains("# Mode D"), "got: {}", &md[..md.len().min(120)]);
        assert!(md.contains("方案"));
        assert!(md.contains("输入 5 / 输出 6"));
        let txt = render_export(&e, "txt");
        assert!(txt.contains("【Mode D】"));
        assert!(txt.contains("--- 方案 ---"));
    }

    #[test]
    fn render_export_honesty_declaration() {
        let base = sample_entry("x");
        let e = HistoryEntry {
            degraded: None,
            validation_passed: Some(true),
            mode: "mode_a".into(),
            input: "灵感".into(),
            output: "方案".into(),
            ..base.clone()
        };
        let md = render_export(&e, "md");
        assert!(md.contains("完整度声明：本次生成全环节正常"));
        assert!(md.contains("硬校验 通过"));
        let e2 = HistoryEntry {
            degraded: Some(vec![
                "call_degraded：作词人输出解析失败".into(),
                "gate_degraded：硬校验打回耗尽".into(),
            ]),
            validation_passed: Some(false),
            ..base
        };
        let md2 = render_export(&e2, "md");
        assert!(md2.contains("存在降级：call_degraded：作词人输出解析失败；gate_degraded：硬校验打回耗尽"));
        assert!(md2.contains("硬校验 未完全通过（详见对话记录）"));
        let e3 = HistoryEntry {
            validation_passed: None,
            ..e2
        };
        assert!(render_export(&e3, "md").contains("硬校验 结论未知"));
    }
}

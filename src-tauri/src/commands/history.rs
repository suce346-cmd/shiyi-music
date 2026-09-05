//! F3：历史记录文件持久化 + 导出（替代 localStorage 5MB 上限）。
//! 路径约定：{app_data}/history.json（原子写 .tmp+rename）；损坏自动备份 .bak 不丢数据。
//! HistoryEntry 与前端 types/index.ts 对齐（serde default 兼容旧记录缺字段）。

use crate::errors::{AppError, ErrorKind};
use tauri::Manager;
use serde::{Deserialize, Serialize};

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

/// 前端 HistoryEntry 对齐（全部 Option 字段 serde default，旧记录兼容）
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
}

fn history_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("应用数据目录不可用: {}", e)))?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("创建数据目录失败: {}", e)))?;
    Ok(dir.join("history.json"))
}

/// 读取全部历史（文件不存在 → 空数组；损坏 → 备份 .bak 后返回空数组，不丢数据）
#[tauri::command]
pub async fn history_load(app: tauri::AppHandle) -> Result<Vec<HistoryEntry>, AppError> {
    let path = history_path(&app)?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("读取历史失败: {}", e)))?;
    match serde_json::from_str::<Vec<HistoryEntry>>(&content) {
        Ok(entries) => Ok(entries),
        Err(e) => {
            // 损坏备份：history.json → history.json.bak（覆盖旧备份），然后返回空
            let bak = path.with_extension("json.bak");
            let _ = std::fs::rename(&path, &bak);
            tracing::warn!(backup = ?bak, error = %e, "历史文件损坏已备份");
            Ok(vec![])
        }
    }
}

/// 全量写回（原子写：.tmp + rename，防崩溃写半截）
#[tauri::command]
pub async fn history_save(app: tauri::AppHandle, entries: Vec<HistoryEntry>) -> Result<(), AppError> {
    let path = history_path(&app)?;
    let tmp = path.with_extension("json.tmp");
    let content = serde_json::to_string(&entries)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("历史序列化失败: {}", e)))?;
    std::fs::write(&tmp, content)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("写入历史失败: {}", e)))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("历史落盘失败: {}", e)))?;
    Ok(())
}

/// 导出单条记录为 txt/markdown 纯文本（供 dialog 保存用，前端拿到文本后调 dialog 保存）
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

fn render_export(e: &HistoryEntry, format: &str) -> String {
    let mut out = String::new();
    if format == "md" {
        out.push_str(&format!("# {} · {}\n\n", mode_label(&e.mode), e.id));
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
        out.push_str(&format!("【{}】{}\n\n输入：{}\n\n", mode_label(&e.mode), e.id, e.input));
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

    /// HistoryEntry 与前端类型对齐：旧记录（无 conversation/usage）兼容解析
    #[test]
    fn history_entry_backward_compatible() {
        let old = serde_json::json!({
            "id": "1", "mode": "mode_d", "input": "灵感", "output": "方案", "timestamp": 1
        });
        let e: HistoryEntry = serde_json::from_value(old).unwrap();
        assert!(e.conversation.is_none());
        assert!(e.usage.is_none());
        // 往返：新记录序列化后旧逻辑仍可读（字段只增不减）
        let full = HistoryEntry {
            id: "2".into(),
            mode: "mode_b".into(),
            input: "i".into(),
            output: "o".into(),
            conversation: Some(vec![ChatTurn { role: "user".into(), content: "hi".into(), timestamp: 1, speaker: None }]),
            usage: Some(Usage { prompt_tokens: 10, completion_tokens: 20 }),
            timestamp: 2,
        };
        let back: HistoryEntry = serde_json::from_str(&serde_json::to_string(&full).unwrap()).unwrap();
        assert_eq!(back.usage.unwrap().prompt_tokens, 10);
    }

    /// 导出渲染：md 含标题/对话/tokens；txt 含分隔线
    #[test]
    fn render_export_formats() {
        let e = HistoryEntry {
            id: "x".into(),
            mode: "mode_d".into(),
            input: "灵感".into(),
            output: "方案".into(),
            conversation: None,
            usage: Some(Usage { prompt_tokens: 5, completion_tokens: 6 }),
            timestamp: 0,
        };
        let md = render_export(&e, "md");
        assert!(md.contains("# Mode D"), "got: {}", &md[..md.len().min(120)]);
        assert!(md.contains("方案"));
        assert!(md.contains("输入 5 / 输出 6"));
        let txt = render_export(&e, "txt");
        assert!(txt.contains("【Mode D】"));
        assert!(txt.contains("--- 方案 ---"));
    }
}

import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { writeTextFile } from "@tauri-apps/plugin-fs";
import { IconX, IconTrash, IconHistory, IconFileText, IconBrandTiktok, IconEdit, IconStars, IconDownload } from "@tabler/icons-react";
import type { HistoryEntry, Mode } from "../types";
import { MODE_LABELS } from "../types";

interface Props {
  entries: HistoryEntry[];
  allEntriesCount: number;
  filter: Mode | "all";
  onFilterChange: (f: Mode | "all") => void;
  onDelete: (id: string) => void;
  onClear: () => void;
  onSelect: (entry: HistoryEntry) => void;
  onClose: () => void;
}

/** F3：导出单条记录（后端拼文本 → dialog 选路径 → 写文件） */
async function exportEntry(id: string, format: "txt" | "md"): Promise<string | null> {
  const text = await invoke<string>("history_export_text", { id, format });
  const filePath = await save({
    defaultPath: `shiyi-${id.slice(0, 8)}.${format}`,
    filters: [{ name: format === "md" ? "Markdown" : "Text", extensions: [format] }],
  });
  if (!filePath) return null; // 用户取消
  await writeTextFile(filePath, text);
  return filePath;
}

const modeIcon: Record<Mode, typeof IconFileText> = { mode_a: IconFileText, mode_b: IconStars, mode_c: IconEdit, mode_d: IconBrandTiktok };

const FILTER_TABS: { label: string; value: Mode | "all" }[] = [
  { label: "全部", value: "all" },
  { label: "A", value: "mode_a" },
  { label: "B", value: "mode_b" },
  { label: "C", value: "mode_c" },
  { label: "D", value: "mode_d" },
];

function outputSummary(entry: HistoryEntry): string {
  const text = entry.output || "";
  return text.length > 60 ? text.slice(0, 60) + "..." : text;
}

export default function HistoryPanel({ entries, allEntriesCount, filter, onFilterChange, onDelete, onClear, onSelect, onClose }: Props) {
  const [confirmClear, setConfirmClear] = useState(false);
  /** F3：导出中 id（按钮 loading 态）/ 导出结果提示 */
  const [exportingId, setExportingId] = useState<string | null>(null);
  const [exportMsg, setExportMsg] = useState("");

  const handleExport = async (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    if (exportingId) return;
    setExportingId(id);
    setExportMsg("");
    try {
      // 默认导出 md（对话结构保留最好）；用户要 txt 可二次保存时改后缀
      const p = await exportEntry(id, "md");
      setExportMsg(p ? `已导出：${p}` : "");
    } catch (err) {
      setExportMsg(`导出失败：${err instanceof Error ? err.message : String(err)}`);
    } finally {
      setExportingId(null);
      setTimeout(() => setExportMsg(""), 4000);
    }
  };

  return (
    <div className="fixed inset-0 oklch(0.50 0.02 55 / 0.35) backdrop-blur-md z-50 flex items-start justify-center pt-20
                    animate-[fade_150ms_ease]">
      <div className="glass-panel rounded-2xl border border-border/40 w-full max-w-md mx-4
                      max-h-[70vh] flex flex-col overflow-hidden">
        <div className="flex items-center justify-between px-4 py-3 border-b border-border/50">
          <div className="flex items-center gap-2">
            <IconHistory size={16} className="text-brand-400" />
            <h2 className="text-[13px] font-semibold text-text-1">历史记录</h2>
            <span className="text-[11px] text-text-muted">({allEntriesCount})</span>
          </div>
          <div className="flex items-center gap-1">
            {allEntriesCount > 0 && (
              <button onClick={() => confirmClear ? (onClear(), setConfirmClear(false)) : setConfirmClear(true)}
                className="text-[11px] text-text-muted hover:text-danger px-2 py-1 rounded-lg
                           hover:bg-danger/8 transition-colors duration-150">
                <IconTrash size={13} className="inline mr-0.5" />
                {confirmClear ? "确认?" : "清空"}
              </button>
            )}
            <button onClick={onClose}
              className="p-1 rounded-lg text-text-muted hover:text-text-1 hover:bg-surface-3 transition-colors">
              <IconX size={15} />
            </button>
          </div>
        </div>

        <div className="flex gap-1 px-4 py-2 border-b border-border/50">
          {FILTER_TABS.map(tab => (
            <button key={tab.value} onClick={() => onFilterChange(tab.value)}
              className={`px-2 py-1 rounded-lg text-[11px] font-medium transition-all duration-150
                ${filter === tab.value
                  ? "bg-brand-500/15 text-brand-400"
                  : "text-text-muted hover:text-text-2 hover:bg-surface-3"}`}>
              {tab.label}
            </button>
          ))}
        </div>
        {/* F3：导出结果提示 */}
        {exportMsg && (
          <div className="px-4 py-1.5 border-b border-border/50 text-[10px] text-text-muted truncate">
            {exportMsg}
          </div>
        )}

        <div className="flex-1 overflow-y-auto p-2 space-y-0.5">
          {entries.length === 0 && (
            <div className="flex flex-col items-center justify-center py-10 text-text-muted">
              <IconHistory size={28} className="opacity-30 mb-2" />
              <p className="text-[12px]">{filter === "all" ? "暂无记录" : "该模式下暂无记录"}</p>
            </div>
          )}
          {entries.map((entry) => {
            const Icon = modeIcon[entry.mode];
            return (
              <div key={entry.id}
                className="group flex items-center gap-3 p-2.5 rounded-xl cursor-pointer
                           hover:bg-surface-2/50 transition-all duration-150"
                onClick={() => onSelect(entry)}>
                <div className="w-7 h-7 rounded-lg bg-surface-3 flex items-center justify-center shrink-0
                                group-hover:bg-border transition-colors">
                  <Icon size={14} className="text-text-2" />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-1.5">
                    <span className="text-[11px] font-medium text-brand-400">{MODE_LABELS[entry.mode]}</span>
                    <span className="text-[10px] text-text-muted">
                      {new Date(entry.timestamp).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}
                    </span>
                  </div>
                  <p className="text-[12px] text-text-2 truncate">{entry.input}</p>
                  {entry.output && (
                    <p className="text-[11px] text-text-muted truncate">{outputSummary(entry)}</p>
                  )}
                </div>
                <div className="flex items-center gap-1 shrink-0">
                  <button onClick={(e) => handleExport(e, entry.id)}
                    title="导出为 Markdown 文件"
                    className="p-1 rounded-md text-text-muted opacity-0 group-hover:opacity-100
                               hover:text-brand-400 hover:bg-brand-500/10 transition-all duration-150 disabled:opacity-50">
                    <IconDownload size={13} className={exportingId === entry.id ? "animate-pulse" : ""} />
                  </button>
                  <button onClick={(e) => { e.stopPropagation(); onDelete(entry.id); }}
                    className="p-1 rounded-md text-text-muted opacity-0 group-hover:opacity-100
                               hover:text-danger hover:bg-danger/8 transition-all duration-150">
                    <IconX size={13} />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

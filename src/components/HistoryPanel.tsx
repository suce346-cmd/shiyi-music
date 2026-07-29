import { useState } from "react";
import { IconX, IconTrash, IconHistory, IconFileText, IconBrandTiktok, IconEdit, IconStars } from "@tabler/icons-react";
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

const modeIcon: Record<Mode, typeof IconFileText> = { mode_a: IconFileText, mode_b: IconStars, mode_c: IconEdit, mode_d: IconBrandTiktok };

const FILTER_TABS: { label: string; value: Mode | "all" }[] = [
  { label: "全部", value: "all" },
  { label: "Mode A", value: "mode_a" },
  { label: "Mode B", value: "mode_b" },
  { label: "Mode C", value: "mode_c" },
  { label: "Mode D", value: "mode_d" },
];

/** 截取输出摘要，最多 80 字符 */
function outputSummary(entry: HistoryEntry): string {
  const text = entry.output || "";
  return text.length > 80 ? text.slice(0, 80) + "..." : text;
}

export default function HistoryPanel({ entries, allEntriesCount, filter, onFilterChange, onDelete, onClear, onSelect, onClose }: Props) {
  const [confirmClear, setConfirmClear] = useState(false);

  return (
    <div className="fixed inset-0 bg-black/30 backdrop-blur-sm z-50 flex items-start justify-center pt-16
                    animate-[fade_150ms_ease]">
      <div className="bg-surface-2 rounded-2xl border border-border shadow-2xl w-full max-w-lg mx-4
                      max-h-[75vh] flex flex-col overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-4 border-b border-border/50">
          <div className="flex items-center gap-2.5">
            <div className="w-8 h-8 rounded-lg bg-brand-500/10 flex items-center justify-center">
              <IconHistory size={16} className="text-brand-600" />
            </div>
            <h2 className="text-sm font-semibold text-text-1">历史记录</h2>
            <span className="text-xs text-text-muted">({allEntriesCount})</span>
          </div>
          <div className="flex items-center gap-1">
            {allEntriesCount > 0 && (
              <button onClick={() => confirmClear ? (onClear(), setConfirmClear(false)) : setConfirmClear(true)}
                className="text-xs text-text-muted hover:text-danger px-2.5 py-1.5 rounded-lg
                           hover:bg-danger/8 transition-colors duration-150">
                <IconTrash size={14} className="inline mr-1" />
                {confirmClear ? "确认清空？" : "清空全部"}
              </button>
            )}
            <button onClick={onClose}
              className="p-1.5 rounded-lg text-text-muted hover:text-text-1 hover:bg-border transition-colors">
              <IconX size={16} />
            </button>
          </div>
        </div>

        {/* Filter tabs */}
        <div className="flex gap-1 px-5 py-2.5 border-b border-border/50 overflow-x-auto">
          {FILTER_TABS.map(tab => (
            <button key={tab.value} onClick={() => onFilterChange(tab.value)}
              className={`px-2.5 py-1 rounded-lg text-xs font-medium whitespace-nowrap transition-all duration-150
                ${filter === tab.value
                  ? "bg-brand-500/15 text-brand-600"
                  : "text-text-muted hover:text-text-2 hover:bg-surface-0"}`}>
              {tab.label}
            </button>
          ))}
        </div>

        {/* List */}
        <div className="flex-1 overflow-y-auto p-3 space-y-1">
          {entries.length === 0 && (
            <div className="flex flex-col items-center justify-center py-12 text-text-muted">
              <IconHistory size={32} className="opacity-30 mb-3" />
              <p className="text-sm">{filter === "all" ? "暂无历史记录" : "该模式下暂无记录"}</p>
            </div>
          )}
          {entries.map((entry) => {
            const Icon = modeIcon[entry.mode];
            return (
              <div key={entry.id}
                className="group flex items-center gap-3 p-3 rounded-xl cursor-pointer
                           hover:bg-surface-0 border border-transparent hover:border-border/50
                           transition-all duration-150"
                onClick={() => onSelect(entry)}>
                <div className="w-8 h-8 rounded-lg bg-surface-0 flex items-center justify-center shrink-0
                                group-hover:bg-border transition-colors">
                  <Icon size={15} className="text-text-2" />
                </div>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2 mb-0.5">
                    <span className="text-xs font-medium text-brand-600">{MODE_LABELS[entry.mode]}</span>
                    <span className="text-xs text-text-muted">
                      {new Date(entry.timestamp).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}
                    </span>
                  </div>
                  <p className="text-xs text-text-2 truncate">{entry.input}</p>
                  {entry.output && (
                    <p className="text-xs text-text-muted truncate mt-0.5">{outputSummary(entry)}</p>
                  )}
                </div>
                <button onClick={(e) => { e.stopPropagation(); onDelete(entry.id); }}
                  className="p-1 rounded-md text-text-muted opacity-0 group-hover:opacity-100
                             hover:text-danger hover:bg-danger/8 transition-all duration-150">
                  <IconX size={14} />
                </button>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

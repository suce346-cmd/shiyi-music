import { IconX, IconTrash, IconList } from "@tabler/icons-react";
import { MODE_LABELS } from "../types";
import type { Locale } from "../types";
import { t } from "../i18n";
import type { QueueItem } from "../hooks/useQueue";

interface Props {
  queue: QueueItem[];
  /** 当前运行项 id（高亮用；无则 null） */
  runningId: string | null;
  onRemove: (id: string) => void;
  onClear: () => void;
  onSelect: (id: string) => void;
  /** F8：界面语言（缺省中文） */
  locale?: Locale;
}

/** F8：状态文案 key（locale 运行时解析） */
const STATUS_KEY: Record<QueueItem["status"], string> = {
  queued: "queue.st.queued",
  running: "queue.st.running",
  done: "queue.st.done",
  error: "queue.st.error",
  cancelled: "queue.st.cancelled",
};

/** F9：生成队列面板（等待项列表；当前运行项高亮；完成/失败项点击查看对应历史） */
export default function QueuePanel({ queue, runningId, onRemove, onClear, onSelect, locale }: Props) {
  const waiting = queue.filter((q) => q.status === "queued" || q.status === "running");
  if (queue.length === 0) return null;
  return (
    <div className="mx-4 mt-2 px-3 py-2 rounded-xl glass-panel border border-border/30 text-[12px]">
      <div className="flex items-center justify-between mb-1.5">
        <span className="flex items-center gap-1.5 text-[12px] font-medium text-text-1">
          <IconList size={13} className="text-brand-400" />
          {t(locale, "queue.title")}
          <span className="text-[10px] text-text-muted">({waiting.length} {t(locale, "queue.waiting")})</span>
        </span>
        {waiting.length > 0 && (
          <button onClick={onClear}
            className="flex items-center gap-0.5 text-[10px] text-text-muted hover:text-danger transition-colors">
            <IconTrash size={11} /> {t(locale, "queue.clear")}
          </button>
        )}
      </div>
      <div className="space-y-1 max-h-32 overflow-y-auto">
        {queue.map((q) => (
          <div key={q.id}
            onClick={() => (q.status === "done" || q.status === "error") && onSelect(q.id)}
            className={`flex items-center gap-2 px-2 py-1.5 rounded-lg transition-colors ${
              q.id === runningId
                ? "bg-brand-500/10 border border-brand-500/30"
                : q.status === "done" || q.status === "error"
                  ? "cursor-pointer hover:bg-surface-2/60 border border-transparent"
                  : "border border-transparent"
            }`}>
            <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${
              q.status === "running" ? "bg-brand-400 animate-pulse"
              : q.status === "queued" ? "bg-text-muted/50"
              : q.status === "done" ? "bg-success"
              : q.status === "error" ? "bg-danger"
              : "bg-text-muted/30"
            }`} />
            <span className="text-[10px] text-brand-400 shrink-0">{MODE_LABELS[q.mode]}</span>
            <span className="flex-1 min-w-0 truncate text-text-2">{q.label}</span>
            <span className="text-[10px] text-text-muted shrink-0">{t(locale, STATUS_KEY[q.status])}</span>
            {(q.status === "queued" || q.status === "done" || q.status === "error" || q.status === "cancelled") && (
              <button onClick={(e) => { e.stopPropagation(); onRemove(q.id); }}
                className="p-0.5 rounded text-text-muted hover:text-danger transition-colors shrink-0">
                <IconX size={12} />
              </button>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

import { IconLoader, IconCheck, IconAlertTriangle, IconRefresh } from "@tabler/icons-react";
import type { LLMStatus } from "../types";

interface Props {
  status: LLMStatus;
  errorMessage?: string;
  onRetry?: () => void;
  /** B3：生成中显示"停止"按钮 */
  onCancel?: () => void;
}

const config: Record<LLMStatus, { text: string; icon: typeof IconLoader; color: string; bg: string }> = {
  idle: { text: "", icon: IconLoader, color: "", bg: "" },
  loading: { text: "连接中...", icon: IconLoader, color: "text-brand-400", bg: "bg-brand-500/8" },
  streaming: { text: "接收中...", icon: IconLoader, color: "text-brand-400", bg: "bg-brand-500/8" },
  done: { text: "完成", icon: IconCheck, color: "text-success", bg: "bg-success/8" },
  error: { text: "出错", icon: IconAlertTriangle, color: "text-danger", bg: "bg-danger/8" },
};

export default function StatusIndicator({ status, errorMessage, onRetry, onCancel }: Props) {
  if (status === "idle") return null;
  const c = config[status];
  const Icon = c.icon;

  return (
    <div className={`flex items-center gap-2 mx-4 mt-3 px-3 py-2 rounded-xl glass-panel border border-border/30 ${c.bg} ${c.color} text-[13px]`}>
      <Icon size={14} className={status === "loading" || status === "streaming" ? "animate-spin" : ""} />
      <span className="font-medium">{c.text}</span>
      {status === "error" && errorMessage && (
        <span className="text-[11px] opacity-70 truncate flex-1">{errorMessage}</span>
      )}
      {status === "error" && onRetry && (
        <button onClick={onRetry}
          className="flex items-center gap-1 px-2 py-0.5 rounded-lg text-[11px] font-medium
                     bg-danger/10 hover:bg-danger/20 border border-danger/20
                     transition-all duration-150 active:scale-95 shrink-0">
          <IconRefresh size={12} />
          重试
        </button>
      )}
      {onCancel && (status === "loading" || status === "streaming") && (
        <button onClick={onCancel}
          className="flex items-center gap-1 px-2 py-0.5 rounded-lg text-[11px] font-medium
                     bg-surface-3 hover:bg-border border border-border/50 text-text-2
                     transition-all duration-150 active:scale-95 shrink-0">
          停止
        </button>
      )}
      {(status === "loading" || status === "streaming") && (
        <div className="flex-1 h-[2px] rounded-full bg-surface-3 overflow-hidden ml-2">
          <div className="h-full w-1/4 rounded-full bg-current/40"
            style={{ animation: "streamBar 1.2s ease-in-out infinite" }} />
        </div>
      )}
    </div>
  );
}

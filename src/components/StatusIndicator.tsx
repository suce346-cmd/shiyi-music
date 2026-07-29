import { IconLoader, IconCheck, IconAlertTriangle, IconPencil, IconRefresh } from "@tabler/icons-react";
import type { LLMStatus } from "../types";

interface Props {
  status: LLMStatus;
  errorMessage?: string;
  onRetry?: () => void;
}

const config: Record<LLMStatus, { text: string; icon: typeof IconLoader; color: string; bg: string }> = {
  idle: { text: "", icon: IconLoader, color: "", bg: "" },
  loading: { text: "正在连接 LLM...", icon: IconLoader, color: "text-brand-600", bg: "bg-brand-500/8" },
  streaming: { text: "正在接收...", icon: IconPencil, color: "text-brand-600", bg: "bg-brand-500/8" },
  done: { text: "生成完成", icon: IconCheck, color: "text-success", bg: "bg-success/8" },
  error: { text: "生成出错", icon: IconAlertTriangle, color: "text-danger", bg: "bg-danger/8" },
};

export default function StatusIndicator({ status, errorMessage, onRetry }: Props) {
  if (status === "idle") return null;
  const c = config[status];
  const Icon = c.icon;

  return (
    <div className={`flex items-center gap-2.5 my-4 px-4 py-3 rounded-xl border ${c.bg} ${c.color} text-sm`}>
      <Icon size={16} className={status === "loading" || status === "streaming" ? "animate-spin" : ""} />
      <span className="font-medium">{c.text}</span>
      {status === "error" && errorMessage && (
        <span className="text-xs opacity-70 truncate ml-1 flex-1">{errorMessage}</span>
      )}
      {status === "error" && onRetry && (
        <button onClick={onRetry}
          className="flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-medium
                     bg-danger/10 hover:bg-danger/20 border border-danger/20
                     transition-all duration-150 active:scale-95 shrink-0">
          <IconRefresh size={13} />
          重试
        </button>
      )}
      {(status === "loading" || status === "streaming") && (
        <span className="flex gap-1 ml-1">
          <span className="w-1 h-1 rounded-full bg-current animate-bounce" style={{ animationDelay: "0ms" }} />
          <span className="w-1 h-1 rounded-full bg-current animate-bounce" style={{ animationDelay: "150ms" }} />
          <span className="w-1 h-1 rounded-full bg-current animate-bounce" style={{ animationDelay: "300ms" }} />
        </span>
      )}
    </div>
  );
}

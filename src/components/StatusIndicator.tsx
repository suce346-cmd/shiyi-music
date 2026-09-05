import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { IconLoader, IconCheck, IconAlertTriangle, IconRefresh, IconMessagePlus } from "@tabler/icons-react";
import type { LLMStatus } from "../types";
import { errText } from "../types";

interface Props {
  status: LLMStatus;
  errorMessage?: string;
  onRetry?: () => void;
  /** B3：生成中显示"停止"按钮 */
  onCancel?: () => void;
  /** A9：当前 run_id 读取（插话命令定向用） */
  getRunId?: () => string;
}

const config: Record<LLMStatus, { text: string; icon: typeof IconLoader; color: string; bg: string }> = {
  idle: { text: "", icon: IconLoader, color: "", bg: "" },
  loading: { text: "连接中...", icon: IconLoader, color: "text-brand-400", bg: "bg-brand-500/8" },
  streaming: { text: "接收中...", icon: IconLoader, color: "text-brand-400", bg: "bg-brand-500/8" },
  done: { text: "完成", icon: IconCheck, color: "text-success", bg: "bg-success/8" },
  error: { text: "出错", icon: IconAlertTriangle, color: "text-danger", bg: "bg-danger/8" },
};

export default function StatusIndicator({ status, errorMessage, onRetry, onCancel, getRunId }: Props) {
  /** F10：插话输入展开态 + 发送中 + 结果提示 */
  const [showInterject, setShowInterject] = useState(false);
  const [note, setNote] = useState("");
  const [sending, setSending] = useState(false);
  const [msg, setMsg] = useState("");
  if (status === "idle") return null;
  const c = config[status];
  const Icon = c.icon;

  /** F10/A9：发送插话（按 run_id 定向，非阻塞存入后端槽，轮边界消费；超长由后端 Validation 拦截） */
  const sendInterject = async () => {
    const text = note.trim();
    if (!text || sending) return;
    setSending(true);
    setMsg("");
    try {
      await invoke("interject_feedback", { runId: getRunId?.() ?? "", note: text });
      setNote("");
      setShowInterject(false);
      setMsg("已送达，将在下一轮讨论中纳入");
    } catch (e) {
      setMsg(`发送失败：${errText(e)}`);
    } finally {
      setSending(false);
      setTimeout(() => setMsg(""), 4000);
    }
  };

  const running = status === "loading" || status === "streaming";

  return (
    <div className={`mx-4 mt-3 px-3 py-2 rounded-xl glass-panel border border-border/30 ${c.bg} ${c.color} text-[13px]`}>
      <div className="flex items-center gap-2">
        <Icon size={14} className={running ? "animate-spin" : ""} />
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
        {onCancel && running && (
          <button onClick={onCancel}
            className="flex items-center gap-1 px-2 py-0.5 rounded-lg text-[11px] font-medium
                       bg-surface-3 hover:bg-border border border-border/50 text-text-2
                       transition-all duration-150 active:scale-95 shrink-0">
            停止
          </button>
        )}
        {/* F10：运行中插入意见（非阻塞，轮边界消费） */}
        {running && (
          <button onClick={() => setShowInterject(!showInterject)}
            className="flex items-center gap-1 px-2 py-0.5 rounded-lg text-[11px] font-medium
                       bg-brand-500/10 hover:bg-brand-500/20 border border-brand-500/30 text-brand-400
                       transition-all duration-150 active:scale-95 shrink-0">
            <IconMessagePlus size={12} />
            插话
          </button>
        )}
        {running && (
          <div className="flex-1 h-[2px] rounded-full bg-surface-3 overflow-hidden ml-2">
            <div className="h-full w-1/4 rounded-full bg-current/40"
              style={{ animation: "streamBar 1.2s ease-in-out infinite" }} />
          </div>
        )}
      </div>
      {/* F10：插话输入区 */}
      {showInterject && running && (
        <div className="mt-2 flex gap-1.5">
          <input value={note} onChange={e => setNote(e.target.value)}
            onKeyDown={e => { if (e.key === "Enter" && !e.nativeEvent.isComposing) sendInterject(); }}
            placeholder="比如：副歌再炸一点…（下一轮讨论纳入，不中断当前）"
            disabled={sending}
            className="flex-1 bg-surface-0 border border-border/60 rounded-lg px-2.5 py-1.5 text-[12px]
                       text-text-1 placeholder:text-text-muted/30 focus:outline-none
                       focus:border-brand-500/40 transition-all duration-150 disabled:opacity-50" />
          <button onClick={sendInterject} disabled={!note.trim() || sending}
            className="px-2.5 py-1.5 rounded-lg text-[12px] font-medium brand-gradient-btn text-white
                       disabled:opacity-40 disabled:cursor-not-allowed transition-all duration-150 active:scale-95 shrink-0">
            {sending ? <IconLoader size={12} className="animate-spin" /> : "发送"}
          </button>
        </div>
      )}
      {msg && (
        <p className="mt-1.5 text-[10px] text-text-muted">{msg}</p>
      )}
    </div>
  );
}

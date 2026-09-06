import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { IconLoader, IconCheck, IconAlertTriangle, IconRefresh, IconMessagePlus } from "@tabler/icons-react";
import type { LLMStatus, Locale } from "../types";
import { errText } from "../types";
import { t } from "../i18n";

interface Props {
  status: LLMStatus;
  errorMessage?: string;
  onRetry?: () => void;
  /** R3：从上次继续（检查点续跑；无检查点时后端明确报错） */
  onResume?: () => void;
  /** 生成中显示"停止"按钮 */
  onCancel?: () => void;
  /** 当前 run_id 读取（插话命令定向用） */
  getRunId?: () => string;
  /** 界面语言（缺省中文） */
  locale?: Locale;
}

/** 状态文案 key（locale 运行时解析） */
const STATUS_TEXT_KEY: Record<LLMStatus, string> = {
  idle: "",
  loading: "status.connecting",
  streaming: "status.streaming",
  done: "status.done",
  error: "status.error",
};

const config: Record<LLMStatus, { icon: typeof IconLoader; color: string; bg: string }> = {
  idle: { icon: IconLoader, color: "", bg: "" },
  loading: { icon: IconLoader, color: "text-brand-400", bg: "bg-brand-500/8" },
  streaming: { icon: IconLoader, color: "text-brand-400", bg: "bg-brand-500/8" },
  done: { icon: IconCheck, color: "text-success", bg: "bg-success/8" },
  error: { icon: IconAlertTriangle, color: "text-danger", bg: "bg-danger/8" },
};

export default function StatusIndicator({ status, errorMessage, onRetry, onResume, onCancel, getRunId, locale }: Props) {
  /** 插话输入展开态 + 发送中 + 结果提示 */
  const [showInterject, setShowInterject] = useState(false);
  const [note, setNote] = useState("");
  const [sending, setSending] = useState(false);
  const [msg, setMsg] = useState("");
  if (status === "idle") return null;
  const c = config[status];
  const Icon = c.icon;

  /** 发送插话（按 run_id 定向，非阻塞存入后端槽，轮边界消费；超长由后端 Validation 拦截） */
  const sendInterject = async () => {
    const text = note.trim();
    if (!text || sending) return;
    setSending(true);
    setMsg("");
    try {
      await invoke("interject_feedback", { runId: getRunId?.() ?? "", note: text });
      setNote("");
      setShowInterject(false);
      setMsg(t(locale, "interject.sent"));
    } catch (e) {
      setMsg(`${t(locale, "interject.fail")}${errText(e)}`);
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
        <span className="font-medium">{STATUS_TEXT_KEY[status] ? t(locale, STATUS_TEXT_KEY[status]) : ""}</span>
        {status === "error" && errorMessage && (
          <span className="text-[11px] opacity-70 truncate flex-1">{errorMessage}</span>
        )}
        {status === "error" && onRetry && (
          <button onClick={onRetry}
            className="flex items-center gap-1 px-2 py-0.5 rounded-lg text-[11px] font-medium
                       bg-danger/10 hover:bg-danger/20 border border-danger/20
                       transition-all duration-150 active:scale-95 shrink-0">
            <IconRefresh size={12} />
            {t(locale, "status.retry")}
          </button>
        )}
        {/* R3：从上次继续（不断点重跑讨论轮，直接进终稿） */}
        {status === "error" && onResume && (
          <button onClick={onResume}
            className="flex items-center gap-1 px-2 py-0.5 rounded-lg text-[11px] font-medium
                       bg-brand-500/10 hover:bg-brand-500/20 border border-brand-500/30 text-brand-400
                       transition-all duration-150 active:scale-95 shrink-0">
            <IconRefresh size={12} />
            {t(locale, "status.resume")}
          </button>
        )}
        {onCancel && running && (
          <button onClick={onCancel}
            className="flex items-center gap-1 px-2 py-0.5 rounded-lg text-[11px] font-medium
                       bg-surface-3 hover:bg-border border border-border/50 text-text-2
                       transition-all duration-150 active:scale-95 shrink-0">
            {t(locale, "status.stop")}
          </button>
        )}
        {/* 运行中插入意见（非阻塞，轮边界消费） */}
        {running && (
          <button onClick={() => setShowInterject(!showInterject)}
            className="flex items-center gap-1 px-2 py-0.5 rounded-lg text-[11px] font-medium
                       bg-brand-500/10 hover:bg-brand-500/20 border border-brand-500/30 text-brand-400
                       transition-all duration-150 active:scale-95 shrink-0">
            <IconMessagePlus size={12} />
            {t(locale, "status.interject")}
          </button>
        )}
        {running && (
          <div className="flex-1 h-[2px] rounded-full bg-surface-3 overflow-hidden ml-2">
            <div className="h-full w-1/4 rounded-full bg-current/40"
              style={{ animation: "streamBar 1.2s ease-in-out infinite" }} />
          </div>
        )}
      </div>
      {/* 插话输入区 */}
      {showInterject && running && (
        <div className="mt-2 flex gap-1.5">
          <input value={note} onChange={e => setNote(e.target.value)}
            onKeyDown={e => { if (e.key === "Enter" && !e.nativeEvent.isComposing) sendInterject(); }}
            placeholder={t(locale, "interject.ph")}
            disabled={sending}
            className="flex-1 bg-surface-0 border border-border/60 rounded-lg px-2.5 py-1.5 text-[12px]
                       text-text-1 placeholder:text-text-muted/30 focus:outline-none
                       focus:border-brand-500/40 transition-all duration-150 disabled:opacity-50" />
          <button onClick={sendInterject} disabled={!note.trim() || sending}
            className="px-2.5 py-1.5 rounded-lg text-[12px] font-medium brand-gradient-btn text-white
                       disabled:opacity-40 disabled:cursor-not-allowed transition-all duration-150 active:scale-95 shrink-0">
            {sending ? <IconLoader size={12} className="animate-spin" /> : t(locale, "interject.send")}
          </button>
        </div>
      )}
      {msg && (
        <p className="mt-1.5 text-[10px] text-text-muted">{msg}</p>
      )}
    </div>
  );
}

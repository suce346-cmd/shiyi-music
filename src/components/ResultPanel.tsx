import { useState } from "react";
import { IconCopy, IconCheck, IconRefresh, IconUser, IconRobot } from "@tabler/icons-react";
import type { ChatTurn, LLMStatus } from "../types";

interface Props {
  conversation: ChatTurn[];
  streamText: string;
  status: LLMStatus;
  onRefine: (feedback: string) => void;
  readOnly?: boolean;
}

export default function ResultPanel({ conversation, streamText, status, onRefine, readOnly }: Props) {
  const [copied, setCopied] = useState(false);
  const [feedback, setFeedback] = useState("");

  const handleCopy = async () => {
    const allAssistant = conversation
      .filter(t => t.role === "assistant")
      .map(t => t.content)
      .join("\n\n---\n\n");
    try {
      await navigator.clipboard.writeText(allAssistant);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // clipboard API 不可用时静默失败
    }
  };

  // 构建显示列表：已有对话 + 正在流式输出的临时 assistant 轮
  const turns: ChatTurn[] = [...conversation];
  if (streamText && status !== "done") {
    turns.push({ role: "assistant", content: streamText, timestamp: Date.now() });
  }

  if (turns.length === 0 && !streamText) return null;

  return (
    <div className="mt-4 animate-[fade_200ms_ease] space-y-3">
      {/* 对话流 */}
      {turns.map((turn, i) => {
        const isUser = turn.role === "user";
        const isStreaming = i === turns.length - 1 && !isUser && status === "streaming";
        const Icon = isUser ? IconUser : IconRobot;

        return (
          <div key={i} className={`flex gap-3 ${isUser ? "flex-row-reverse" : ""}`}>
            <div className={`w-8 h-8 rounded-lg flex items-center justify-center shrink-0
              ${isUser ? "bg-surface-0 border border-border" : "bg-brand-500/10"}`}>
              <Icon size={15} className={isUser ? "text-text-2" : "text-brand-600"} />
            </div>
            <div className={`flex-1 min-w-0 ${isUser ? "max-w-[85%]" : ""}`}>
              <div className={`text-xs font-medium mb-1 ${isUser ? "text-text-muted text-right" : "text-brand-600"}`}>
                {isUser ? "用户" : "AI"}
              </div>
              <div className={`rounded-2xl px-4 py-3 text-xs leading-relaxed whitespace-pre-wrap font-mono
                ${isUser
                  ? "bg-surface-0 border border-border text-text-2"
                  : "bg-surface-2 border border-border text-text-1"}
                max-h-80 overflow-y-auto selection:bg-brand-500/20`}>
                {turn.content}
                {isStreaming && (
                  <span className="inline-block w-1.5 h-4 bg-brand-500 ml-0.5 animate-pulse rounded-sm align-middle" />
                )}
              </div>
            </div>
          </div>
        );
      })}

      {/* 复制按钮 — 只在有 AI 回复时显示 */}
      {conversation.some(t => t.role === "assistant") && (
        <div className="flex justify-end">
          <button onClick={handleCopy}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs text-text-2
                       bg-surface-2 hover:bg-border border border-border/50
                       transition-all duration-150 active:scale-95">
            {copied ? <IconCheck size={14} className="text-success" /> : <IconCopy size={14} />}
            {copied ? "已复制" : "复制全部"}
          </button>
        </div>
      )}

      {/* 优化输入 — 仅在 done 且非只读时显示 */}
      {status === "done" && !readOnly && (
        <div className="bg-surface-2 rounded-2xl border border-border shadow-sm p-5 space-y-3">
          <div className="flex items-center gap-2">
            <IconRefresh size={16} className="text-text-muted" />
            <h3 className="text-sm font-medium text-text-1">优化方向</h3>
          </div>
          <p className="text-xs text-text-muted">指出不满意的地方，AI 会基于上下文针对性优化</p>
          <textarea value={feedback} onChange={e => setFeedback(e.target.value)}
            className="w-full h-20 bg-surface-0 border border-border rounded-xl p-3.5 text-sm
                       text-text-1 placeholder:text-text-muted/40 resize-y
                       focus:outline-none focus:ring-2 focus:ring-brand-500/30 focus:border-brand-500
                       transition-all duration-150"
            placeholder="比如：唢呐不够炸、人声太软、洗脑循环不明显..." />
          <button onClick={() => { onRefine(feedback); setFeedback(""); }}
            disabled={!feedback.trim()}
            className="w-full py-2.5 rounded-xl text-sm font-medium
                       bg-surface-0 border border-border text-text-1
                       hover:bg-border hover:shadow-sm
                       disabled:opacity-40 disabled:cursor-not-allowed
                       transition-all duration-150 active:scale-[0.98]">
            <IconRefresh size={15} className="inline mr-1.5 -mt-0.5" />
            重新生成（基于反馈优化）
          </button>
        </div>
      )}
    </div>
  );
}

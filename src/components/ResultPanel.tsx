import { useState } from "react";
import { IconCopy, IconCheck, IconRefresh, IconChevronDown } from "@tabler/icons-react";
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
  const [showRefine, setShowRefine] = useState(false);

  const handleCopy = async () => {
    const allAssistant = conversation.filter(t => t.role === "assistant").map(t => t.content).join("\n\n---\n\n");
    try {
      await navigator.clipboard.writeText(allAssistant);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch { /* */ }
  };

  const turns: ChatTurn[] = [...conversation];
  if (streamText && status !== "done") {
    turns.push({ role: "assistant", content: streamText, timestamp: Date.now() });
  }
  if (turns.length === 0 && !streamText) return null;

  return (
    <div className="animate-[slideUp_300ms_ease] space-y-3">
      {turns.map((turn, i) => {
        const isUser = turn.role === "user";
        const isStreaming = i === turns.length - 1 && !isUser && status === "streaming";

        if (isUser) {
          return (
            <div key={i} className="flex justify-end">
            <div className="max-w-[80%] rounded-2xl rounded-tr-md px-3.5 py-2.5 text-[13px] leading-relaxed
                bg-surface-3/70 backdrop-blur-sm text-text-2 whitespace-pre-wrap">
                {turn.content}
              </div>
            </div>
          );
        }

        return (
          <div key={i} className="relative">
            <div className="rounded-2xl rounded-tl-md px-4 py-3 text-[13px] leading-relaxed
              glass-panel border-l-2 border-brand-500/50 text-text-1 whitespace-pre-wrap
              selection:bg-brand-500/20">
              {turn.content}
              {isStreaming && (
                <span className="inline-block w-[2px] h-3.5 bg-brand-400 ml-0.5 animate-pulse align-middle" />
              )}
            </div>
            {isStreaming && (
              <div className="mt-1.5 h-[2px] rounded-full bg-surface-3 overflow-hidden">
                <div className="h-full w-1/3 rounded-full bg-brand-400/60"
                  style={{ animation: "streamBar 1.2s ease-in-out infinite" }} />
              </div>
            )}
          </div>
        );
      })}

      {conversation.some(t => t.role === "assistant") && status === "done" && !readOnly && (
        <div className="flex items-center gap-2 pt-1">
          <button onClick={handleCopy}
            className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-[12px] text-text-2
              bg-surface-2/60 hover:bg-surface-3/80 border border-border/30 transition-all duration-150 active:scale-95">
            {copied ? <IconCheck size={13} className="text-success" /> : <IconCopy size={13} />}
            {copied ? "已复制" : "复制"}
          </button>
          <button onClick={() => setShowRefine(!showRefine)}
            className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-[12px] text-text-2
              bg-surface-2/60 hover:bg-surface-3/80 border border-border/30 transition-all duration-150 active:scale-95">
            <IconRefresh size={13} />
            优化
            <IconChevronDown size={12} className={`transition-transform duration-150 ${showRefine ? "rotate-180" : ""}`} />
          </button>
        </div>
      )}

      {readOnly && conversation.some(t => t.role === "assistant") && (
        <div className="flex items-center gap-2 pt-1">
          <button onClick={handleCopy}
            className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-[12px] text-text-2
              bg-surface-2/60 hover:bg-surface-3/80 border border-border/30 transition-all duration-150 active:scale-95">
            {copied ? <IconCheck size={13} className="text-success" /> : <IconCopy size={13} />}
            {copied ? "已复制" : "复制"}
          </button>
        </div>
      )}

      {showRefine && status === "done" && !readOnly && (
        <div className="glass-panel rounded-2xl border border-border/30 p-3.5 space-y-2.5 animate-[fade_200ms_ease]">
          <textarea value={feedback} onChange={e => setFeedback(e.target.value)}
            className="w-full h-16 bg-surface-1 border border-border/60 rounded-xl px-3 py-2.5 text-[13px]
              text-text-1 placeholder:text-text-muted/30 resize-y focus:outline-none
              focus:border-brand-500/40 focus:ring-1 focus:ring-brand-500/20 transition-all duration-150"
            placeholder="比如：唢呐不够炸、人声太软、洗脑循环不明显..." />
          <button onClick={() => { onRefine(feedback); setFeedback(""); setShowRefine(false); }}
            disabled={!feedback.trim()}
            className="w-full py-2 rounded-xl text-[13px] font-medium
              bg-surface-3 text-text-1 hover:bg-border
              disabled:opacity-40 disabled:cursor-not-allowed
              transition-all duration-150 active:scale-[0.98]">
            <IconRefresh size={14} className="inline mr-1.5 -mt-0.5" />
            重新生成
          </button>
        </div>
      )}
    </div>
  );
}

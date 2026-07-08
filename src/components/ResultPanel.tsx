import { useState } from "react";
import { IconCopy, IconCheck, IconRefresh } from "@tabler/icons-react";
import type { LLMResponse, LLMStatus } from "../types";

interface Props {
  result: LLMResponse | null;
  streamText: string;
  status: LLMStatus;
  onRefine: (feedback: string) => void;
}

export default function ResultPanel({ result, streamText, status, onRefine }: Props) {
  const displayText = result?.raw || streamText || "";
  const [copied, setCopied] = useState(false);
  const [feedback, setFeedback] = useState("");

  const handleCopy = async () => {
    await navigator.clipboard.writeText(displayText);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  if (!displayText) return null;

  return (
    <div className="mt-4 animate-[fade_200ms_ease]">
      {/* Result Card */}
      <div className="bg-surface-2 rounded-2xl border border-border shadow-sm overflow-hidden">
        <div className="flex items-center justify-between px-5 py-3.5 border-b border-border/50">
          <h2 className="text-sm font-medium text-text-1">生成结果</h2>
          <button onClick={handleCopy}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs text-text-2
                       bg-surface-0 hover:bg-border border border-border/50
                       transition-all duration-150 active:scale-95">
            {copied ? <IconCheck size={14} className="text-success" /> : <IconCopy size={14} />}
            {copied ? "已复制" : "复制全部"}
          </button>
        </div>
        <pre className="text-xs leading-relaxed whitespace-pre-wrap font-mono
                        text-text-1 p-5 max-h-96 overflow-y-auto
                        selection:bg-brand-500/20">
          {displayText}
          {status === "streaming" && <span className="inline-block w-1.5 h-4 bg-brand-500 ml-0.5 animate-pulse rounded-sm" />}
        </pre>
      </div>

      {/* Iteration — only when done */}
      {status === "done" && (
        <div className="mt-4 bg-surface-2 rounded-2xl border border-border shadow-sm p-5 space-y-3">
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
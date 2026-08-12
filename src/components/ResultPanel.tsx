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

interface EnergySection {
  label: string;
  energy: number | null;
}

/// 从方案文本提取每段说明行的"能量:X"标注，返回清洗后的展示文本 + 每段能量列表。
/// 说明行是 [乐器+行为, 空间, 人声, 能量:X] 的扩展格式——能量为内部校验锚点，
/// Suno 不识别数值标注，显示/复制时从说明行摘出，作为独立能量板块展示。
/// 结构标签行（[Verse 1]/[Chorus 2] 等，支持编号）作为段落名关联到后续说明行
const TAG_RE = /^\[(Final Chorus|Verse|Pre-Chorus|Chorus|Bridge|Outro|Hook|Drop|Intro|Interlude|Beat Switch)( \d+)?\]/;
/// 能量标注：吃掉前导逗号/空格（", 能量:2" / " 能量:2" / "energy 3-4" / "energy:8" 都干净移除）。
/// 兼容中文/英文与单值/区间——LLM 在纯英文说明行时可能输出 "energy X-Y"。
const ENERGY_RE = /[,，\s]*(?:能量[:：]?\s*|energy\s*[:：]?\s*)(\d{1,2})(?:\s*-\s*(\d{1,2}))?/i;

export function parseEnergy(text: string): { display: string; sections: EnergySection[] } {
  const sections: EnergySection[] = [];
  let currentLabel = "开场";
  const display = text
    .split("\n")
    .map((line) => {
      const tag = line.match(TAG_RE);
      if (tag) {
        // 保留段号（[Verse 2] → "Verse2"），多个同标签段能量条可区分
        currentLabel = tag[2] ? `${tag[1]}${tag[2].trim()}` : tag[1];
        return line;
      }
      const em = line.match(ENERGY_RE);
      if (em && line.trim().startsWith("[")) {
        // 单值取 X；区间（energy 3-4）取上限作为段落能量强度
        const energy = em[2] ? parseInt(em[2], 10) : parseInt(em[1], 10);
        sections.push({ label: currentLabel, energy });
        // 移除能量标注（含前导逗号/空格），说明行保持干净（Suno 友好）
        return line.replace(ENERGY_RE, "");
      }
      return line;
    })
    .join("\n");
  return { display, sections };
}

/** 能量条板块：每段一行，0-10 共 10 格 */
function EnergyBars({ sections }: { sections: EnergySection[] }) {
  if (sections.length === 0) return null;
  return (
    <div className="mt-2.5 pt-2 border-t border-border/30 space-y-1.5">
      {sections.map((s, i) => (
        <div key={i} className="flex items-center gap-2 text-[11px]">
          <span className="text-text-muted w-20 shrink-0">{s.label}</span>
          <div className="flex gap-[3px] flex-1">
            {Array.from({ length: 10 }, (_, k) => (
              <div
                key={k}
                className={`h-2 flex-1 rounded-[3px] ${
                  s.energy !== null && k < s.energy ? "bg-brand-500/80" : "bg-surface-3"
                }`}
              />
            ))}
          </div>
          <span className="text-text-muted w-7 text-right shrink-0">{s.energy ?? "-"}</span>
        </div>
      ))}
    </div>
  );
}

export default function ResultPanel({ conversation, streamText, status, onRefine, readOnly }: Props) {
  const [copied, setCopied] = useState(false);
  const [feedback, setFeedback] = useState("");
  const [showRefine, setShowRefine] = useState(false);

  const handleCopy = async () => {
    // 复制 Suno 友好文本：能量标注已从说明行摘出（内部校验锚点，Suno 不识别）
    const allAssistant = conversation
      .filter(t => t.role === "assistant")
      .map(t => parseEnergy(t.content).display)
      .join("\n\n---\n\n");
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
        const isExpert = turn.role === "expert";
        const isStreaming = i === turns.length - 1 && !isUser && !isExpert && status === "streaming";

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

        // 专家发言：角色头像 + 可读摘要（浅色卡片，与最终回复区分）
        if (isExpert) {
          return (
            <div key={i} className="flex items-start gap-2">
              <span className="shrink-0 w-6 h-6 rounded-full bg-surface-2/80 border border-border/40 flex items-center justify-center text-[12px] mt-0.5">
                {turn.speaker?.emoji ?? "🎙️"}
              </span>
              <div className="max-w-[85%] rounded-2xl rounded-tl-md px-3.5 py-2.5 text-[12px] leading-relaxed
                bg-surface-2/40 border border-border/30 text-text-2 whitespace-pre-wrap">
                <span className="block text-[10px] font-medium text-text-muted mb-0.5">
                  {turn.speaker?.name ?? "专家"}
                </span>
                {turn.content}
              </div>
            </div>
          );
        }

        return (
          <div key={i} className="relative">
            {(() => {
              // 能量:X 从说明行摘出 → 独立能量板块（Suno 不识别数值标注，展示保持干净）
              const { display, sections } = parseEnergy(turn.content);
              return (
                <>
                  <div className="rounded-2xl rounded-tl-md px-4 py-3 text-[13px] leading-relaxed
                    glass-panel border-l-2 border-brand-500/50 text-text-1 whitespace-pre-wrap
                    selection:bg-brand-500/20">
                    {display}
                    {isStreaming && (
                      <span className="inline-block w-[2px] h-3.5 bg-brand-400 ml-0.5 animate-pulse align-middle" />
                    )}
                  </div>
                  <EnergyBars sections={sections} />
                </>
              );
            })()}
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

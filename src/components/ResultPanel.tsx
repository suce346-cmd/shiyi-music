import { useState } from "react";
import { IconCopy, IconCheck, IconRefresh, IconChevronDown, IconSparkles, IconAlertTriangle } from "@tabler/icons-react";
import type { ChatTurn, LLMStatus, Mode, Locale } from "../types";
import { t } from "../i18n";
import { estimateRefineTargets, REFINE_TARGET_NAMES } from "../utils/refineTargets";

interface Props {
  conversation: ChatTurn[];
  streamText: string;
  status: LLMStatus;
  /** 双模式优化——feedback + 模式（fast=增量/full=全量） */
  onRefine: (feedback: string, refineMode: "fast" | "full") => void;
  /** 预估展示用（当前模式；缺省不展示预估） */
  mode?: Mode;
  /** 界面语言（缺省中文） */
  locale?: Locale;
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

export default function ResultPanel({ conversation, streamText, status, onRefine, mode, locale }: Props) {
  const [copied, setCopied] = useState(false);
  const [feedback, setFeedback] = useState("");
  const [showRefine, setShowRefine] = useState(false);
  /** 优化模式（fast=增量/full=全量），默认快速优化 */
  const [refineMode, setRefineMode] = useState<"fast" | "full">("fast");
  /** 增量预估参跑角色（前端镜像，后端为准；无命中则后端回落全量） */
  const estimated = mode && refineMode === "fast" && feedback.trim()
    ? estimateRefineTargets(feedback, mode)
    : [];

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

  // #10/#11 操作条的**唯一判定处**（此前散落写死 status === "done"，失败/取消时整条消失）：
  // - hasOutput：有可复制/可优化的产出（含失败时固化的半成品 turn；流式中断留下的流式文本也算）
  // - busy：生成/接收中——在途产出不提供操作（避免对旧结果误操作）
  // - lastIsPartial：**最后一条** assistant 是半成品（后续成功优化会产生新的完整 turn，标记自然失效）
  const hasOutput = conversation.some(t => t.role === "assistant") || streamText.trim() !== "";
  const busy = status === "loading" || status === "streaming";
  const lastAssistant = [...conversation].reverse().find(t => t.role === "assistant");
  const lastIsPartial = lastAssistant?.partial === true;
  /** 有产出 + 非在途 → 操作条（复制 + 优化）。历史条目同样可优化（#9 起）
   *  ——旧的 readOnly 分支已下线：该 prop 早已无人传（历史也能反馈），留着只会误导"历史只读" */
  const showActions = hasOutput && !busy;

  // 结果区"显示什么"的**唯一判定处**：App 外层此前复制了一份"有无产出"的判定来决定
  // 渲染结果区还是新手引导——错误且无产出时会把用户丢回新手引导（像什么都没发生）。
  // 三种空态各有明确语义，互不冒充：
  // - 失败且无产出 → 失败态（指向重试/续跑，错误正文在上方状态条）
  // - 从未开始（无任何轮次）→ 新手引导
  // - 跑过/取消但无产出 → 诚实说明（不再提"重试"——该态没有重试入口）
  if (!hasOutput && !busy) {
    if (status === "error") {
      return (
        <div className="h-full flex flex-col items-center justify-center text-text-muted px-8">
          <div className="w-14 h-14 rounded-2xl glass-panel flex items-center justify-center mb-3">
            <IconAlertTriangle size={24} className="text-danger/60" />
          </div>
          <p className="text-[13px] text-text-2">{t(locale, "result.failed")}</p>
          <p className="text-[11px] mt-1.5 text-center">{t(locale, "result.failed.hint")}</p>
        </div>
      );
    }
    if (conversation.length === 0) {
      return (
        <div className="h-full flex flex-col items-center justify-center text-text-muted px-8">
          <div className="w-16 h-16 rounded-2xl glass-panel flex items-center justify-center mb-3">
            <IconSparkles size={28} className="text-brand-400/50" />
          </div>
          <p className="text-[13px] mb-4">{t(locale, "empty.hint")}</p>
          <div className="w-full max-w-[420px] glass-panel rounded-xl border border-border/40 p-4 space-y-2 text-[11px] leading-relaxed">
            <p className="text-text-2 font-medium">{t(locale, "empty.flow")}</p>
            <p className="flex items-center gap-1.5"><span className="w-1.5 h-1.5 rounded-full bg-brand-400" /> {t(locale, "empty.s1")}</p>
            <p className="flex items-center gap-1.5"><span className="w-1.5 h-1.5 rounded-full bg-brand-400" /> {t(locale, "empty.s2")}</p>
            <p className="flex items-center gap-1.5"><span className="w-1.5 h-1.5 rounded-full bg-brand-400" /> {t(locale, "empty.s3")}</p>
          </div>
        </div>
      );
    }
    return (
      <div className="h-full flex flex-col items-center justify-center text-text-muted px-8">
        <p className="text-[12px]">{t(locale, "result.none")}</p>
      </div>
    );
  }

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
                  {/* #10 半成品角标：生成中断固化下来的产出必须显式标注（内容可能被截断） */}
                  {turn.partial && (
                    <div className="flex items-center gap-1.5 mb-1 text-[10px] text-warning">
                      <span className="px-1.5 py-0.5 rounded-md bg-warning/10 border border-warning/30 font-medium">
                        {t(locale, "result.partial")}
                      </span>
                      <span className="text-text-muted">{t(locale, "result.partial.hint")}</span>
                    </div>
                  )}
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

      {/* #11 可发现性：操作条改为**滚动容器内的常驻条**（sticky bottom-0）——
          此前它是结果流最后一个子元素，输出越长越"藏得深"（用户"找了八遍"）。
          自带玻璃底 + brand 描边，滚动到任何位置都看得见，且一眼可辨"这里能操作"。 */}
      {showActions && (
        <div className="sticky bottom-0 z-10 pt-2 pb-0.5">
          <div className="glass-panel rounded-xl border border-brand-500/40 bg-surface-1/85 backdrop-blur-md px-2 py-1.5
            flex items-center gap-2 flex-wrap">
            <button onClick={handleCopy}
              className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-[12px] text-text-2
                bg-surface-2/60 hover:bg-surface-3/80 border border-border/30 transition-all duration-150 active:scale-95">
              {copied ? <IconCheck size={13} className="text-success" /> : <IconCopy size={13} />}
              {copied ? t(locale, "result.copied") : t(locale, "result.copy")}
            </button>
            <button onClick={() => setShowRefine(!showRefine)}
              className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-[12px] font-medium
                border transition-all duration-150 active:scale-95
                ${showRefine
                  ? "bg-brand-500/20 border-brand-500/50 text-brand-400"
                  : "bg-brand-500/12 hover:bg-brand-500/22 border-brand-500/35 text-brand-400"}`}>
              <IconRefresh size={13} />
              {lastIsPartial ? t(locale, "result.refine.partial") : t(locale, "result.refine")}
              <IconChevronDown size={12} className={`transition-transform duration-150 ${showRefine ? "rotate-180" : ""}`} />
            </button>
            <span className="text-[10px] text-text-muted ml-auto pr-1">
              {lastIsPartial ? t(locale, "result.partial.ready") : t(locale, "result.actions.hint")}
            </span>
          </div>
        </div>
      )}

      {showRefine && !busy && hasOutput && (
        <div className="glass-panel rounded-2xl border border-border/30 p-3.5 space-y-2.5 animate-[fade_200ms_ease]">
          <textarea value={feedback} onChange={e => setFeedback(e.target.value)}
            className="w-full h-16 bg-surface-1 border border-border/60 rounded-xl px-3 py-2.5 text-[13px]
              text-text-1 placeholder:text-text-muted/30 resize-y focus:outline-none
              focus:border-brand-500/40 focus:ring-1 focus:ring-brand-500/20 transition-all duration-150"
            placeholder="比如：唢呐不够炸、人声太软、洗脑循环不明显..." />
          {/* #10 以半成品为基础时显式告知用户（内容可能被截断，后端也会收到同源告知） */}
          {lastIsPartial && (
            <p className="text-[10px] text-warning leading-relaxed">
              {t(locale, "result.partial.panel")}
            </p>
          )}
          {/* 双模式优化——快速（增量）/深度（全量） */}
          <div className="flex gap-2">
            {(["fast", "full"] as const).map((m) => (
              <button key={m} onClick={() => setRefineMode(m)}
                className={`flex-1 py-1.5 rounded-lg text-[12px] font-medium border transition-all duration-150 ${
                  refineMode === m
                    ? "bg-brand-500/15 border-brand-500/40 text-brand-400"
                    : "bg-surface-2/60 border-border/40 text-text-muted hover:text-text-2"
                }`}>
                {m === "fast" ? t(locale, "result.refine.fast") : t(locale, "result.refine.full")}
              </button>
            ))}
          </div>
          {refineMode === "fast" && feedback.trim() && (
            <p className="text-[10px] text-text-muted leading-relaxed">
              {estimated.length > 0
                ? `预计重跑：${estimated.map((r) => REFINE_TARGET_NAMES[r]).join("、")}＋校验员（约 1/3 耗时）`
                : "无法判断涉及角色，将全量重跑（与深度重做一致）"}
            </p>
          )}
          <button onClick={() => { onRefine(feedback, refineMode); setFeedback(""); setShowRefine(false); }}
            disabled={!feedback.trim()}
            className="w-full py-2 rounded-xl text-[13px] font-medium
              bg-surface-3 text-text-1 hover:bg-border
              disabled:opacity-40 disabled:cursor-not-allowed
              transition-all duration-150 active:scale-[0.98]">
            <IconRefresh size={14} className="inline mr-1.5 -mt-0.5" />
            {refineMode === "fast" ? t(locale, "result.refine.fast") : t(locale, "result.refine.send")}
          </button>
        </div>
      )}
    </div>
  );
}

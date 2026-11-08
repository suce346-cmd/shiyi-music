import type { ExpertCard, Locale } from "../types";
import QAvatar from "./QAvatar";
import { t } from "../i18n";
import type { MessageKey } from "../i18n";

interface Props {
  experts: ExpertCard[];
  phase: "discussing" | "synthesizing" | "validating" | "done";
  validation: { passed: boolean; issues: string[] } | null;
  error: string | null;
  active: boolean;
  onOpenDetail: (expert: ExpertCard) => void;
  currentStage?: string | null;
  doneStages?: string[];
  usage?: { prompt_tokens: number; completion_tokens: number };
  locale?: Locale;
}

const PHASE_TEXT_KEY: Record<Props["phase"], MessageKey> = {
  discussing: "round.phase.discussing",
  synthesizing: "round.phase.synthesizing",
  validating: "round.phase.validating",
  done: "round.phase.done",
};

export default function RoundtablePanel({
  experts,
  phase,
  validation,
  error,
  active,
  onOpenDetail,
  currentStage,
  doneStages = [],
  usage,
  locale,
}: Props) {
  if (experts.length === 0) return null;

  const totalTokens = (usage?.prompt_tokens ?? 0) + (usage?.completion_tokens ?? 0);

  return (
    <div className="space-y-2.5">
      {/* Studio Telemetry Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="studio-section-tag px-1.5 py-0.5 rounded bg-surface-3 text-text-2 border border-border">
            02 · PIPELINE
          </span>
          <span className="text-[12px] font-semibold text-text-1 tracking-tight">
            {t(locale, PHASE_TEXT_KEY[phase])}
          </span>
        </div>
        <div className="flex items-center gap-1.5">
          {totalTokens > 0 && (
            <span className="studio-mono text-[10px] px-1.5 py-0.5 rounded bg-surface-2 border border-border text-text-muted">
              {(totalTokens / 1000).toFixed(1)}k tok
            </span>
          )}
          <span
            className={`w-2 h-2 rounded-full ${
              active
                ? "bg-brand-400 animate-pulse"
                : error
                  ? "bg-danger"
                  : doneStages.length > 0
                    ? "bg-success"
                    : "bg-border"
            }`}
          />
        </div>
      </div>

      {/* 信号链路进度条 */}
      <div className="flex items-center gap-1 flex-wrap">
        {experts.map((e, idx) => {
          const isCurrent = currentStage === e.id || e.status === "working";
          const isDone = doneStages.includes(e.id) || e.status === "done";
          return (
            <div
              key={e.id}
              onClick={() => onOpenDetail(e)}
              className={`px-2 py-0.5 rounded text-[10px] font-medium border transition-all cursor-pointer flex items-center gap-1
                ${isCurrent
                  ? "bg-brand-500/15 border-brand-500/50 text-brand-400"
                  : isDone
                    ? "bg-success/10 border-success/35 text-success"
                    : "bg-surface-0/60 border-border/60 text-text-muted"
                }`}
            >
              <span className="studio-mono opacity-60">0{idx + 1}</span>
              <span>{e.name}</span>
            </div>
          );
        })}
      </div>

      {/* 声学角色通道矩阵（Channel Strip Cards） */}
      <div className="grid grid-cols-2 gap-1.5">
        {experts.map((e) => {
          const isWorking = e.status === "working";
          const isDone = e.status === "done";
          const isError = e.status === "error";

          const bubbleText = isWorking
            ? e.note || t(locale, "round.bubble.thinking")
            : isError
              ? e.note || t(locale, "round.bubble.error")
              : isDone
                ? e.note || t(locale, "round.bubble.done")
                : e.knowledge.length > 0
                  ? e.knowledge.slice(0, 2).join(" · ")
                  : "";

          return (
            <div
              key={e.id}
              onClick={() => onOpenDetail(e)}
              className={`relative rounded-lg px-2.5 py-2 border transition-all duration-150 cursor-pointer flex items-start gap-2
                ${isWorking
                  ? "bg-surface-2 border-brand-500/55 shadow-sm"
                  : isDone
                    ? "bg-surface-1/90 border-border hover:border-border-hover"
                    : isError
                      ? "bg-danger/10 border-danger/40"
                      : "bg-surface-0/50 border-border/50 opacity-80 hover:opacity-100"
                }`}
            >
              <div className="relative shrink-0 mt-0.5">
                {isWorking && (
                  <span
                    className="absolute inset-0 rounded-full chibi-ripple-ring pointer-events-none"
                    style={{ border: `1.5px solid ${e.color}` }}
                  />
                )}
                <div className={`chibi-${e.status}`}>
                  <QAvatar expert={e} status={e.status} size={28} />
                </div>
              </div>

              <div className="min-w-0 flex-1">
                <div className="flex items-center justify-between gap-1">
                  <span
                    className="text-[11px] font-semibold truncate"
                    style={{ color: e.status === "idle" ? undefined : e.color }}
                  >
                    {e.name}
                  </span>
                  {isWorking && (
                    <span className="text-[9.5px] px-1.5 py-0.2 rounded bg-brand-500/15 text-brand-400 border border-brand-500/35 shrink-0">
                      {t(locale, "round.seat.working")}
                    </span>
                  )}
                  {isDone && (
                    <span className="text-[9.5px] px-1.5 py-0.2 rounded bg-success/12 text-success border border-success/30 shrink-0">
                      {t(locale, "round.seat.done")}
                    </span>
                  )}
                </div>
                <div className="text-[10px] text-text-muted line-clamp-2 mt-0.5 leading-snug">
                  {bubbleText}
                </div>
              </div>
            </div>
          );
        })}
      </div>

      {/* 底部提示 + 终稿校验结论 */}
      <div className="flex items-center justify-between text-[10px] text-text-muted pt-0.5">
        <span>{active ? t(locale, "round.tip.running") : t(locale, "round.tip.idle")}</span>
      </div>

      {validation && (
        <div
          className={`px-2.5 py-1.5 rounded-lg text-[10.5px] border ${
            validation.passed
              ? "bg-success/10 border-success/30 text-success"
              : "bg-warning/10 border-warning/30 text-warning"
          }`}
        >
          {validation.passed ? (
            <span>{t(locale, "round.valid.ok")}</span>
          ) : (
            <div>
              <div className="font-semibold">{t(locale, "round.valid.fail")}</div>
              <ul className="mt-1 space-y-0.5 text-[10px] opacity-90 list-disc list-inside">
                {validation.issues.slice(0, 4).map((iss, i) => (
                  <li key={i} className="truncate">
                    {iss}
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

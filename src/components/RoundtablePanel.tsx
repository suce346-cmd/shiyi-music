import { useMemo } from "react";
import QAvatar from "./QAvatar";
import type { ExpertCard, Locale } from "../types";
import { t } from "../i18n";

interface Props {
  experts: ExpertCard[];
  phase: "discussing" | "synthesizing" | "validating" | "done";
  validation: { passed: boolean; issues: string[] } | null;
  error: string | null;
  active: boolean;
  onOpenDetail: (expert: ExpertCard) => void;
  /** v2 流水线阶段 */
  currentStage?: string | null;
  doneStages?: string[];
  /** 本轮累计 token 用量（无用量时不显示） */
  usage?: { prompt_tokens: number; completion_tokens: number } | null;
  /** 界面语言（缺省中文） */
  locale?: Locale;
}

/**
 * 圆桌会议舞台（俯视圆桌，角色常态围坐）：
 * - 5 个 Q版角色永远坐在桌边（idle 也坐着），不是空椅子
 * - 每个角色发言/思考时：弹跳 + 光晕 + 头顶说话气泡（显示他说了什么）
 * - 完成时：开心弹跳 + 对勾；出错时：抖动 + 汗滴
 */

/** 围坐位置：以舞台容器中心为圆心（桌子居中），角色中心在圆桌外沿。
 *  半径 = 桌半宽/半高(110/75) + 角色半宽/半高(20/45, 对应 size=72 立绘) + 间距(6)，
 *  用 calc(50% ± Npx) 表达，窗口高度变化时自动保持围绕桌子。
 *  R6：人数自适应——4人(A/C)小圈，5人(B)中圈，6人(D)大圈，上下不挤出舞台。
 */
function seatPosition(index: number, total: number) {
  const angle = (index / total) * Math.PI * 2 - Math.PI / 2;
  const scale = total <= 4 ? 0.82 : total === 5 ? 0.92 : 1.0;
  const rx = 136 * scale; // 110 + 20 + 6
  const ry = 126 * scale; // 75 + 45 + 6
  const dx = Math.cos(angle) * rx;
  const dy = Math.sin(angle) * ry;
  return {
    left: `calc(50% + ${dx}px)`,
    top: `calc(50% + ${dy}px)`,
  };
}

/** 单个角色座位：Q版 + 名牌 + 说话气泡 + 状态动画 */
function Seat({
  expert,
  index,
  total,
  phase,
  onOpenDetail,
  locale,
}: {
  expert: ExpertCard;
  index: number;
  total: number;
  phase: Props["phase"];
  onOpenDetail: (e: ExpertCard) => void;
  locale?: Locale;
}) {
  const pos = seatPosition(index, total);
  const talking = expert.status === "working";
  const done = expert.status === "done";
  const error = expert.status === "error";

  // 动画 class：按状态驱动
  const animClass = error
    ? "avatar-shake"
    : done
      ? "avatar-celebrate"
      : talking
        ? "avatar-jump"
        : expert.status === "idle"
          ? "avatar-bob"
          : "";

  // 说话气泡内容
  const bubble = talking
    ? t(locale, "round.bubble.thinking")
    : done
      ? (expert.note || t(locale, "round.bubble.done"))
      : error
        ? t(locale, "round.bubble.error")
        : null;

  return (
    <div
      className="absolute flex flex-col items-center cursor-pointer"
      style={{ left: pos.left, top: pos.top, transform: "translate(-50%, -50%)", zIndex: 10 }}
      onClick={() => onOpenDetail(expert)}
      title={`查看 ${expert.name} 详情`}
    >      {/* 说话气泡（只在有内容时显示） */}
      {bubble && phase !== "done" && (
        <div
          className={`speech-bubble mb-0.5 max-w-[130px] text-center px-2 py-1 rounded-lg text-[9px] leading-snug border shadow-sm ${
            error
              ? "bg-danger/10 border-danger/30 text-danger"
              : done
                ? "bg-success/10 border-success/30 text-success"
                : "bg-white/90 border-amber-300/50 text-text-2"
          }`}
          style={{ position: "relative", zIndex: 20 }}
        >
          <span className="line-clamp-2">{bubble}</span>
          {/* 气泡尾巴 */}
          <span className="absolute left-1/2 -translate-x-1/2 -bottom-1 w-2 h-2 rotate-45 bg-inherit border-r border-b border-inherit" style={{ background: "inherit" }} />
        </div>
      )}

      {/* 角色（动态动画） */}
      <div className={`relative ${animClass}`} style={{ transformOrigin: "bottom center" }}>
        {/* 发言光晕（working 时双层发光环） */}
        {talking && (
          <>
            <span className="absolute inset-0 rounded-full glow-ring" style={{ boxShadow: `0 0 0 3px ${expert.color}55`, zIndex: -1 }} />
            <span className="absolute inset-0 rounded-full glow-ring" style={{ boxShadow: `0 0 0 3px ${expert.color}33`, animationDelay: "0.5s", zIndex: -1 }} />
          </>
        )}
        <QAvatar expert={expert} status={expert.status} size={72} />
      </div>

      {/* 名牌 */}
      <div
        className="mt-0.5 px-2 py-0.5 rounded-full bg-surface-2/90 border border-border/40 text-[9px] font-medium text-text-2 whitespace-nowrap shadow-sm"
        style={{ zIndex: 15 }}
      >
        {expert.emoji} {expert.name}
      </div>
    </div>
  );
}

/** 中央圆桌（俯视木质椭圆桌 + 中央话筒 + 文件） */
function Table() {
  return (
    <div
      className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2"
      style={{ width: 220, height: 150, zIndex: 5 }}
    >
      {/* 桌影 */}
      <div className="absolute inset-0 rounded-[50%] bg-black/10 blur-md translate-y-2" />
      {/* 桌面 */}
      <div
        className="absolute inset-0 rounded-[50%] bg-gradient-to-br from-amber-100 via-amber-200 to-amber-400
                     ring-1 ring-amber-500/30 shadow-[inset_0_4px_10px_rgba(255,255,255,0.6),inset_0_-6px_14px_rgba(120,80,20,0.25),0_10px_28px_rgba(120,80,20,0.2)]"
      >
        {/* 年轮纹理 */}
        <div className="absolute inset-[14%] rounded-[50%] border border-amber-500/20" />
        <div className="absolute inset-[28%] rounded-[50%] border border-amber-500/15" />
        {/* 中央话筒 */}
        <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 w-9 h-9 rounded-full bg-gradient-to-br from-amber-50 to-amber-200
                        ring-2 ring-amber-400/50 shadow-lg flex items-center justify-center text-[15px]">
          🎙️
        </div>
        {/* 散落的文件/音符 */}
        <span className="absolute left-[18%] top-[30%] text-[10px] rotate-[-15deg] opacity-60">📄</span>
        <span className="absolute right-[16%] top-[38%] text-[11px] rotate-[12deg] opacity-60">🎵</span>
        <span className="absolute left-[30%] bottom-[20%] text-[10px] rotate-[8deg] opacity-50">✏️</span>
      </div>
    </div>
  );
}

export default function RoundtablePanel({
  experts,
  phase,
  validation,
  error,
  active,
  onOpenDetail,
  currentStage = null,
  doneStages = [],
  usage = null,
  locale,
}: Props) {
  const total = experts.length || 5;

  const phaseLabel = useMemo(() => {
    switch (phase) {
      case "discussing": return t(locale, "round.phase.discussing");
      case "synthesizing": return t(locale, "round.phase.synthesizing");
      case "validating": return t(locale, "round.phase.validating");
      case "done": return t(locale, "round.phase.done");
    }
  }, [locale, phase]);

  const progressPct =
    phase === "discussing" ? 33 : phase === "synthesizing" ? 66 : phase === "validating" ? 88 : 100;

  // 阶段条按当前模式阵容过滤（不包含的角色不显示，固定主持/校验常显）
  const activeExpertIds = useMemo(() => new Set(experts.map((e) => e.id)), [experts]);
  const stages = useMemo(() => {
    const all: { key: string; label: string }[] = [
      { key: "emotion", label: "情感" },
      { key: "lyricist", label: "作词" },
      { key: "reviser", label: "改词" },
      { key: "producer", label: "制作" },
      { key: "style_analyst", label: "流行" },
      { key: "host", label: "主持" },
      { key: "auditor", label: "校验" },
    ];
    return all.filter((s) => s.key === "host" || s.key === "auditor" || activeExpertIds.has(s.key));
  }, [activeExpertIds]);

  return (
    <div className="glass-panel rounded-2xl border border-border/40 overflow-hidden flex flex-col">
      {/* 流水线阶段条（v2，按模式阵容过滤） */}
      <div className="flex items-center gap-1 px-4 py-2 border-b border-border/30 bg-surface-1/50 overflow-x-auto">
          {stages.map((st, i) => {
            const done = doneStages.includes(st.key) || (st.key === "auditor" && validation?.passed);
            const current = currentStage === st.key;
            return (
              <div key={st.key} className="flex items-center gap-1 shrink-0">
                <div
                  className={`px-2 py-0.5 rounded-full text-[9px] font-medium border transition-all duration-300 ${
                    current
                      ? "bg-brand-500/20 border-brand-500/50 text-brand-600 animate-pulse"
                      : done
                        ? "bg-success/15 border-success/40 text-success"
                        : "bg-surface-2/60 border-border/40 text-text-muted"
                  }`}
                >
                  {done ? "✓ " : ""}{st.label}
                </div>
                {i < stages.length - 1 && <span className="text-[9px] text-text-muted">→</span>}
              </div>
            );
          })}
      </div>

      {/* 顶部：标题 + 阶段 + 进度 */}
      <div className="flex items-center justify-between px-4 py-2.5 border-b border-border/40 shrink-0">
        <div className="flex items-center gap-2">
          <span className="text-[13px] font-semibold text-text-1">🪑 圆桌会议</span>
          <span className="text-[11px] text-text-muted">{phaseLabel}</span>
        </div>
        <div className="flex items-center gap-2">
          <div className="h-1.5 w-24 rounded-full bg-surface-3 overflow-hidden">
            <div
              className="h-full bg-gradient-to-r from-amber-400 to-amber-600 transition-all duration-700"
              style={{ width: `${progressPct}%` }}
            />
          </div>
          <span className="text-[10px] text-text-muted tabular-nums">
            {phase === "discussing"
              ? `${experts.filter((e) => e.status !== "idle").length}/${total}`
              : `${progressPct}%`}
          </span>
        </div>
      </div>

      {/* 圆桌舞台：角色永远围坐（idle 也坐着）；min-h 保证上下角色不超出 */}
      <div className="relative flex-1 min-h-[400px] overflow-hidden select-none">
        {/* 地面 */}
        <div className="absolute left-1/2 -translate-x-1/2 bottom-4 w-[85%] h-10 rounded-[50%] bg-black/5 blur-md" />

        {/* 中央圆桌 */}
        <Table />

        {/* 角色围坐（常态可见，idle 也坐着） */}
        {experts.map((e, i) => (
          <Seat key={e.id} expert={e} index={i} total={total} phase={phase} onOpenDetail={onOpenDetail} locale={locale} />
        ))}

        {/* 无专家时的引导 */}
        {experts.length === 0 && (
          <div className="absolute inset-0 flex items-center justify-center text-[11px] text-text-muted z-20">
            选择模式后专家将围坐讨论
          </div>
        )}
      </div>

      {/* 底部提示条 */}
      <div className="px-4 py-2 border-t border-border/40 shrink-0 flex items-center justify-between gap-2">
        <span className="text-[9px] text-text-muted">
          {active
            ? t(locale, "round.tip.running")
            : t(locale, "round.tip.idle")}
        </span>
        {/* 本轮累计 token 用量（有计数时显示，只计数不估算金额） */}
        {usage && (usage.prompt_tokens > 0 || usage.completion_tokens > 0) && (
          <span className="text-[9px] text-text-muted tabular-nums shrink-0">
            tokens {usage.prompt_tokens + usage.completion_tokens}
            <span className="opacity-70">（入 {usage.prompt_tokens} / 出 {usage.completion_tokens}）</span>
          </span>
        )}
      </div>

      {/* 校验 / 错误提示 */}
      {(validation || error) && (
        <div className="px-4 pb-3 shrink-0">
          {error && (
            <div className="mb-1.5 px-3 py-2 rounded-lg text-[11px] border border-danger/30 bg-danger/5 text-danger">
              ❌ {error}
            </div>
          )}
          {validation && (
            <div className={`px-3 py-2 rounded-lg text-[11px] border ${
              validation.passed
                ? "border-success/30 bg-success/5 text-success"
                : "border-warning/40 bg-warning/5 text-warning"
            }`}>
              {validation.passed
                ? t(locale, "round.valid.ok")
                : `${t(locale, "round.valid.fail")} ${validation.issues.length}`}
              {!validation.passed && validation.issues.length > 0 && (
                <ul className="mt-1 space-y-0.5 text-[10px]">
                  {validation.issues.map((i, idx) => <li key={idx}>· {i}</li>)}
                </ul>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

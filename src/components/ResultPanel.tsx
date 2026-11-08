import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  IconCopy,
  IconCheck,
  IconRefresh,
  IconChevronDown,
  IconSparkles,
  IconAlertTriangle,
  IconCompass,
  IconWaveSine,
  IconAward,
  IconCode,
} from "@tabler/icons-react";
import type {
  ChatTurn,
  LLMStatus,
  Mode,
  Locale,
  SongPlan,
  SunoMetaLimits,
  SunoMetaWire,
  TextEvalScorecard,
} from "../types";
import { t } from "../i18n";
import { estimateRefineTargets, REFINE_TARGET_NAMES } from "../utils/refineTargets";
import { parseSunoPackage, settingsShowsVarietyZero, type SunoPackageFields } from "../utils/sunoPackage";

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
  /** v3 SongPlan 结构化蓝图（由 FinalPlan 事件或历史条目回放提供） */
  songPlan?: SongPlan | null;
  /** P1-13：Judge 七维评分卡 */
  textEval?: TextEvalScorecard | null;
  /** C5/ADR-3：终稿硬校验是否通过 */
  validationPassed?: boolean;
  /** C5/ADR-3：运行期降级环节列表 */
  degraded?: string[];
}

interface EnergySection {
  label: string;
  energy: number | null;
}

const DEFAULT_SUNO_META: SunoMetaLimits = {
  style_max_chars: 1000,
  lyrics_max_chars: 5000,
  exclude_max_chars: 1000,
  style_tags_min: 8,
  style_tags_max: 15,
  variety_default: 0,
  energy_scale: 10,
  desc_line_max_chars: 80,
  section_cues_max: 3,
};

/** 归一化后端 `get_suno_meta` 返回的 `{ limits: [{ name, value, fact }] }`（兼顾扁平测试桩） */
export function normalizeSunoMeta(raw: unknown): SunoMetaLimits | null {
  if (!raw || typeof raw !== "object") return null;
  const next: SunoMetaLimits = { ...DEFAULT_SUNO_META };
  const maybeWire = raw as Partial<SunoMetaWire>;
  if (Array.isArray(maybeWire.limits)) {
    let matched = false;
    for (const item of maybeWire.limits) {
      if (!item || typeof item.name !== "string" || typeof item.value !== "number") continue;
      if (item.name === "style_max_chars") { next.style_max_chars = item.value; matched = true; }
      else if (item.name === "lyrics_max_chars") { next.lyrics_max_chars = item.value; matched = true; }
      else if (item.name === "exclude_max_chars") { next.exclude_max_chars = item.value; matched = true; }
      else if (item.name === "style_tags_min") { next.style_tags_min = item.value; matched = true; }
      else if (item.name === "style_tags_max") { next.style_tags_max = item.value; matched = true; }
      else if (item.name === "variety_default") { next.variety_default = item.value; matched = true; }
    }
    return matched ? next : null;
  }
  const flat = raw as Partial<SunoMetaLimits> & { style_tag_min?: number; style_tag_max?: number };
  if (typeof flat.style_max_chars === "number") {
    next.style_max_chars = flat.style_max_chars;
    if (typeof flat.lyrics_max_chars === "number") next.lyrics_max_chars = flat.lyrics_max_chars;
    if (typeof flat.exclude_max_chars === "number") next.exclude_max_chars = flat.exclude_max_chars;
    if (typeof flat.style_tags_min === "number") next.style_tags_min = flat.style_tags_min;
    else if (typeof flat.style_tag_min === "number") next.style_tags_min = flat.style_tag_min;
    if (typeof flat.style_tags_max === "number") next.style_tags_max = flat.style_tags_max;
    else if (typeof flat.style_tag_max === "number") next.style_tags_max = flat.style_tag_max;
    if (typeof flat.variety_default === "number") next.variety_default = flat.variety_default;
    return next;
  }
  return null;
}

const PLOT_TYPE_LABELS: Record<string, string> = {
  attitudinal: "态度宣言型 (Attitudinal)",
  situational: "情境切片型 (Situational)",
  narrative: "叙事推进型 (Narrative)",
  expository: "意象铺陈型 (Expository)",
};

/// 从方案文本提取每段说明行的"能量:X"标注，返回清洗后的展示文本 + 每段能量列表。
/// 说明行是 [乐器+行为, 空间, 人声, 能量:X] 的扩展格式——能量为内部校验锚点，
/// Suno 不识别数值标注，显示/复制时从说明行摘出，作为独立能量板块展示。
/// 结构标签行（[Verse 1]/[Chorus 2] 等，支持编号）作为段落名关联到后续说明行
const TAG_RE = /^\[(Final Chorus|Verse|Pre-Chorus|Chorus|Bridge|Outro|Hook|Drop|Intro|Interlude|Beat Switch)( \d+)?\]/;
/// render_suno 单行段头 `[Verse 1 | cue, energy N]`（标签为 SongPlan 自由文本，不限上表）
const HEAD_RE = /^\[([^\]|]+?)\s*\|/;
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
      // 全量复审 2026-10-20：render_suno 输出 `[Label | cue, energy N]` 单行头——标签与能量同行
      const head = line.match(HEAD_RE);
      if (head) currentLabel = head[1].trim().replace(/\s+(\d+)$/, "$1");
      const em = line.match(ENERGY_RE);
      if (em && line.trim().startsWith("[")) {
        // 单值取 X；区间（energy 3-4）取上限作为段落能量强度
        const energy = em[2] ? parseInt(em[2], 10) : parseInt(em[1], 10);
        sections.push({ label: currentLabel, energy });
        // 移除能量标注（含前导逗号/空格），若移除后竖线右侧为空则收拢为纯 [Label]，说明行保持干净（Suno 友好）
        return line.replace(ENERGY_RE, "").replace(/\s*\|\s*\]$/, "]");
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
    <div className="mt-2.5 pt-2 border-t border-border/40 space-y-1.5">
      {sections.map((s, i) => (
        <div key={i} className="flex items-center gap-2 text-[11px]">
          <span className="text-text-muted w-20 shrink-0 studio-mono truncate">{s.label}</span>
          <div className="flex gap-[3px] flex-1">
            {Array.from({ length: 10 }, (_, k) => (
              <div
                key={k}
                className={`h-2 flex-1 rounded-[2px] ${
                  s.energy !== null && k < s.energy ? "bg-brand-500/85" : "bg-surface-3"
                }`}
              />
            ))}
          </div>
          <span className="text-text-muted w-7 text-right shrink-0 studio-mono">{s.energy ?? "-"}</span>
        </div>
      ))}
    </div>
  );
}

/** 字符预算进度条（由 get_suno_meta 阈值驱动） */
function BudgetMeter({ current, max, unit = "chars" }: { current: number; max: number; unit?: string }) {
  const pct = Math.min(100, Math.round((current / Math.max(1, max)) * 100));
  const warn = pct >= 88;
  return (
    <div className="flex items-center gap-2">
      <div className="w-16 h-1.5 rounded-full bg-surface-3 overflow-hidden">
        <div
          className={`h-full rounded-full transition-all ${warn ? "bg-warning" : "bg-brand-400"}`}
          style={{ width: `${pct}%` }}
        />
      </div>
      <span className={`studio-mono text-[10px] ${warn ? "text-warning" : "text-text-muted"}`}>
        {current}/{max} {unit}
      </span>
    </div>
  );
}

export default function ResultPanel({
  conversation,
  streamText,
  status,
  onRefine,
  mode,
  locale,
  songPlan,
  textEval,
  validationPassed,
  degraded,
}: Props) {
  const [copied, setCopied] = useState(false);
  /** P1-11：分栏复制高亮（style|exclude|settings|lyrics|all） */
  const [copiedField, setCopiedField] = useState<string | null>(null);
  const [feedback, setFeedback] = useState("");
  const [showRefine, setShowRefine] = useState(false);
  /** 优化模式（fast=增量/full=全量），默认快速优化 */
  const [refineMode, setRefineMode] = useState<"fast" | "full">("fast");
  const [sunoMeta, setSunoMeta] = useState<SunoMetaLimits>(DEFAULT_SUNO_META);
  const [showRawPackage, setShowRawPackage] = useState(false);

  // 启动拉取 get_suno_meta（解析 Rust `SunoMeta { limits: Vec<Limit> }` 真实 wire 结构）
  useEffect(() => {
    let cancelled = false;
    invoke<unknown>("get_suno_meta")
      .then((raw) => {
        const parsed = normalizeSunoMeta(raw);
        if (!cancelled && parsed) {
          setSunoMeta(parsed);
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  /** 增量预估参跑角色（前端镜像，后端为准；无命中则后端回落全量） */
  const estimated =
    mode && refineMode === "fast" && feedback.trim()
      ? estimateRefineTargets(feedback, mode)
      : [];

  const lastAssistantContent =
    [...conversation].reverse().find((t) => t.role === "assistant")?.content ?? "";
  const pkg: SunoPackageFields = parseSunoPackage(parseEnergy(lastAssistantContent).display);

  const flashCopied = (field: string) => {
    setCopiedField(field);
    if (field === "all") setCopied(true);
    setTimeout(() => {
      setCopiedField(null);
      if (field === "all") setCopied(false);
    }, 2000);
  };

  const copyText = async (text: string, field: string) => {
    if (!text.trim()) return;
    try {
      await navigator.clipboard.writeText(text);
      flashCopied(field);
    } catch {
      /* */
    }
  };

  const handleCopy = async () => {
    // 复制 Suno 友好文本：能量标注已从说明行摘出（内部校验锚点，Suno 不识别）
    const allAssistant = conversation
      .filter((t) => t.role === "assistant")
      .map((t) => parseEnergy(t.content).display)
      .join("\n\n---\n\n");
    await copyText(allAssistant, "all");
  };

  const turns: ChatTurn[] = [...conversation];
  if (streamText && status !== "done") {
    turns.push({ role: "assistant", content: streamText, timestamp: Date.now() });
  }

  // #10/#11 操作条的**唯一判定处**
  const hasOutput = conversation.some((t) => t.role === "assistant") || streamText.trim() !== "";
  const busy = status === "loading" || status === "streaming";
  const lastAssistant = [...conversation].reverse().find((t) => t.role === "assistant");
  const lastIsPartial = lastAssistant?.partial === true;
  const showActions = hasOutput && !busy;

  if (!hasOutput && !busy) {
    if (status === "error") {
      return (
        <div className="h-full flex flex-col items-center justify-center text-text-muted px-8">
          <div className="w-14 h-14 rounded-2xl studio-card flex items-center justify-center mb-3">
            <IconAlertTriangle size={24} className="text-danger/70" />
          </div>
          <p className="text-[13px] text-text-2 font-medium">{t(locale, "result.failed")}</p>
          <p className="text-[11px] mt-1.5 text-center">{t(locale, "result.failed.hint")}</p>
        </div>
      );
    }
    if (conversation.length === 0) {
      return (
        <div className="h-full flex flex-col items-center justify-center text-text-muted px-8">
          <div className="w-14 h-14 rounded-2xl studio-card flex items-center justify-center mb-3 border border-brand-500/30">
            <IconSparkles size={24} className="text-brand-400" />
          </div>
          <p className="text-[13px] text-text-1 font-medium mb-4">{t(locale, "empty.hint")}</p>
          <div className="w-full max-w-[460px] studio-card rounded-xl p-4 space-y-2.5 text-[11.5px] leading-relaxed">
            <div className="flex items-center justify-between border-b border-border/60 pb-2">
              <span className="text-text-1 font-semibold">{t(locale, "empty.flow")}</span>
              <span className="studio-mono text-[10px] text-brand-400">SUNO v6 · PLAN IR v3</span>
            </div>
            <p className="flex items-start gap-2">
              <span className="studio-mono text-[10px] px-1.5 py-0.2 rounded bg-brand-500/15 text-brand-400 border border-brand-500/30 mt-0.5">
                01
              </span>
              <span>{t(locale, "empty.s1")}</span>
            </p>
            <p className="flex items-start gap-2">
              <span className="studio-mono text-[10px] px-1.5 py-0.2 rounded bg-brand-500/15 text-brand-400 border border-brand-500/30 mt-0.5">
                02
              </span>
              <span>{t(locale, "empty.s2")}</span>
            </p>
            <p className="flex items-start gap-2">
              <span className="studio-mono text-[10px] px-1.5 py-0.2 rounded bg-brand-500/15 text-brand-400 border border-brand-500/30 mt-0.5">
                03
              </span>
              <span>{t(locale, "empty.s3")}</span>
            </p>
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

  const styleTags = pkg.style
    ? pkg.style
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean)
    : [];
  const excludeTags = pkg.exclude
    ? pkg.exclude
        .split(",")
        .map((s) => s.trim())
        .filter(Boolean)
    : [];

  const hasBlueprint = Boolean(
    songPlan &&
      (songPlan.intent ||
        songPlan.asymmetry ||
        songPlan.global?.mood ||
        songPlan.global?.arrangement_hook ||
        songPlan.mode_meta?.douyin ||
        songPlan.pronunciation_hints ||
        songPlan.persona ||
        songPlan.notes),
  );

  return (
    <div className="animate-slide-up space-y-3.5">
      {/* =====================================================================
          03 · SUNO v6 STUDIO DELIVERY DECK（四栏工程交付台）
          ===================================================================== */}
      {showActions && pkg.isPackage && (
        <div className="studio-card rounded-xl p-3.5 space-y-3">
          {/* 顶部曲目与全局声学规格横幅 */}
          <div className="flex items-center justify-between gap-3 flex-wrap border-b border-border/60 pb-2.5">
            <div className="flex items-center gap-2 min-w-0">
              <span className="studio-section-tag px-1.5 py-0.5 rounded bg-brand-500/15 text-brand-400 border border-brand-500/35 shrink-0">
                03 · MASTER DECK
              </span>
              <h2 className="text-[14px] font-bold text-text-1 tracking-tight truncate">
                {pkg.title || songPlan?.title || "Suno v6 结构化母带交付包"}
              </h2>
            </div>
            <div className="flex items-center gap-1.5 flex-wrap">
              {typeof validationPassed === "boolean" && (
                <span
                  className={`studio-mono text-[10px] px-2 py-0.5 rounded border ${
                    validationPassed
                      ? "bg-success/12 border-success/35 text-success"
                      : "bg-warning/15 border-warning/40 text-warning"
                  }`}
                >
                  {validationPassed ? "✓ 硬校验通过" : "⚠ 硬校验未过"}
                </span>
              )}
              {degraded && degraded.length > 0 && (
                <span
                  title={degraded.join("\n")}
                  className="studio-mono text-[10px] px-2 py-0.5 rounded bg-warning/15 border border-warning/40 text-warning cursor-help"
                >
                  ⚠ 降级 {degraded.length}
                </span>
              )}
              {songPlan?.global?.bpm && (
                <span className="studio-mono text-[10.5px] px-2 py-0.5 rounded bg-surface-2 border border-border text-text-1">
                  {songPlan.global.bpm} BPM
                </span>
              )}
              {songPlan?.global?.key && (
                <span className="studio-mono text-[10.5px] px-2 py-0.5 rounded bg-surface-2 border border-border text-text-2">
                  {songPlan.global.key}
                </span>
              )}
              {songPlan?.global?.meter && (
                <span className="studio-mono text-[10.5px] px-2 py-0.5 rounded bg-surface-2 border border-border text-text-2">
                  {songPlan.global.meter}
                </span>
              )}
              <button
                type="button"
                onClick={() => setShowRawPackage((v) => !v)}
                className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10.5px] bg-surface-2 hover:bg-surface-3 border border-border text-text-2 cursor-pointer"
              >
                <IconCode size={11} />
                <span>{showRawPackage ? "收起原始文本" : "查看原始文本"}</span>
              </button>
            </div>
          </div>

          {/* 双栏声学控制卡：Style Prompt + Exclude */}
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-2.5">
            {/* Style Prompt 卡 */}
            <div className="studio-subcard rounded-lg p-2.5 space-y-2">
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-1.5">
                  <span className="studio-mono text-[10.5px] font-semibold text-brand-400">
                    STYLE PROMPT · 风格母带
                  </span>
                  <span className="studio-mono text-[10px] px-1.5 py-0.2 rounded bg-surface-1 border border-border text-text-muted">
                    {styleTags.length}/{sunoMeta.style_tags_min}–{sunoMeta.style_tags_max} tags
                  </span>
                </div>
                <BudgetMeter current={pkg.style.length} max={sunoMeta.style_max_chars} />
              </div>
              <div className="flex flex-wrap gap-1">
                {styleTags.map((tag, idx) => (
                  <span
                    key={idx}
                    className={`studio-mono text-[10.5px] px-2 py-0.5 rounded border ${
                      idx === 0
                        ? "bg-brand-500/12 border-brand-500/40 text-brand-400 font-medium"
                        : "bg-surface-1 border-border/70 text-text-1"
                    }`}
                  >
                    {tag}
                  </span>
                ))}
              </div>
            </div>

            {/* Exclude 负向声学门禁卡 */}
            <div className="studio-subcard rounded-lg p-2.5 space-y-2">
              <div className="flex items-center justify-between gap-2">
                <span className="studio-mono text-[10.5px] font-semibold text-danger/90">
                  EXCLUDE · 负向声学门禁
                </span>
                <BudgetMeter current={pkg.exclude.length} max={sunoMeta.exclude_max_chars} />
              </div>
              <div className="flex flex-wrap gap-1">
                {excludeTags.length > 0 ? (
                  excludeTags.map((tag, idx) => (
                    <span
                      key={idx}
                      className="studio-mono text-[10.5px] px-2 py-0.5 rounded bg-danger/8 border border-danger/25 text-text-2"
                    >
                      − {tag}
                    </span>
                  ))
                ) : (
                  <span className="text-[11px] text-text-muted">无排除项</span>
                )}
              </div>
            </div>
          </div>

          {/* P1-11：交付包 Settings 栏显式展示 Variety（默认 0） */}
          {pkg.settings && (
            <div className="studio-subcard rounded-lg px-3 py-2 text-[11px] text-text-2 space-y-1">
              <div className="flex items-center justify-between gap-2">
                <span className="font-semibold text-text-1">{t(locale, "result.field.settings")}</span>
                <div className="flex items-center gap-1.5">
                  <BudgetMeter current={pkg.lyrics.length} max={sunoMeta.lyrics_max_chars} unit="lyrics chars" />
                  {settingsShowsVarietyZero(pkg.settings) ? (
                    <span className="studio-mono px-1.5 py-0.5 rounded bg-brand-500/15 text-brand-400 border border-brand-500/35">
                      Variety = 0
                    </span>
                  ) : (
                    <span className="studio-mono text-text-muted">
                      {pkg.settings.match(/Variety\s*=\s*\d+/i)?.[0] ?? "Variety"}
                    </span>
                  )}
                </div>
              </div>
              <p className="studio-mono text-text-muted whitespace-pre-wrap leading-relaxed">
                {pkg.settings}
              </p>
            </div>
          )}
        </div>
      )}

      {/* =====================================================================
          04 · SONGPLAN v3 词曲与声学蓝图检视器（Blueprint Inspector）
          ===================================================================== */}
      {showActions && hasBlueprint && songPlan && (
        <div className="studio-card rounded-xl p-3.5 space-y-2.5">
          <div className="flex items-center justify-between border-b border-border/60 pb-2">
            <div className="flex items-center gap-2">
              <span className="studio-section-tag px-1.5 py-0.5 rounded bg-accent/15 text-accent border border-accent/35">
                04 · BLUEPRINT
              </span>
              <span className="text-[12.5px] font-semibold text-text-1 flex items-center gap-1.5">
                <IconCompass size={14} className="text-accent" />
                词曲立意五问 · 十三辙声律 · 去机器味蓝图
              </span>
            </div>
            {songPlan.intent?.plot_type && (
              <span className="studio-mono text-[10px] px-2 py-0.5 rounded bg-surface-2 border border-border text-accent">
                {PLOT_TYPE_LABELS[songPlan.intent.plot_type] ?? songPlan.intent.plot_type}
              </span>
            )}
          </div>

          <div className="grid grid-cols-1 lg:grid-cols-2 gap-2.5">
            {/* 立意五问与十三辙统筹 */}
            {songPlan.intent && (
              <div className="studio-subcard rounded-lg p-2.5 space-y-1.5 text-[11.5px]">
                <div className="text-[10.5px] font-semibold text-text-2 uppercase tracking-wider">
                  立意五问 & 全局十三辙
                </div>
                <div className="text-text-1">
                  <span className="text-text-muted">核心真话：</span>
                  <span className="font-medium">{songPlan.intent.one_thing}</span>
                </div>
                <div className="flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-text-2">
                  <span>
                    <b className="text-text-muted font-normal">具象锚点：</b>
                    {songPlan.intent.anchor_object}
                  </span>
                  <span>
                    <b className="text-text-muted font-normal">倾诉对象：</b>
                    {songPlan.intent.to_whom}
                  </span>
                  <span>
                    <b className="text-text-muted font-normal">时空切片：</b>
                    {songPlan.intent.at_what_moment}
                  </span>
                </div>
                {songPlan.intent.rhyme_plan && (
                  <div className="pt-1 border-t border-border/50 text-[11px] text-brand-400">
                    <span className="text-text-muted">声律统筹：</span>
                    {songPlan.intent.rhyme_plan}
                  </div>
                )}
              </div>
            )}

            {/* 去机器味不对称设计 + 声场器乐钩子 */}
            <div className="studio-subcard rounded-lg p-2.5 space-y-1.5 text-[11.5px]">
              <div className="text-[10.5px] font-semibold text-text-2 uppercase tracking-wider">
                去机器味不对称 & 器乐记忆钩子
              </div>
              {songPlan.asymmetry && (
                <div className="space-y-1 text-[11px]">
                  {songPlan.asymmetry.short_line && (
                    <div>
                      <span className="text-text-muted">破格短句：</span>
                      <span className="text-text-1 font-medium">「{songPlan.asymmetry.short_line}」</span>
                    </div>
                  )}
                  {songPlan.asymmetry.chorus_variation && (
                    <div>
                      <span className="text-text-muted">副歌微变：</span>
                      <span className="text-text-2">{songPlan.asymmetry.chorus_variation}</span>
                    </div>
                  )}
                  {songPlan.asymmetry.withholding && (
                    <div>
                      <span className="text-text-muted">终局留白：</span>
                      <span className="text-text-2">{songPlan.asymmetry.withholding}</span>
                    </div>
                  )}
                </div>
              )}
              {(songPlan.global?.mood || songPlan.global?.arrangement_hook) && (
                <div className="pt-1 border-t border-border/50 flex flex-wrap gap-2 text-[10.5px] studio-mono">
                  {songPlan.global.mood && (
                    <span className="px-1.5 py-0.5 rounded bg-surface-1 border border-border text-text-2">
                      Mood: {songPlan.global.mood}
                    </span>
                  )}
                  {songPlan.global.arrangement_hook && (
                    <span className="px-1.5 py-0.5 rounded bg-brand-500/10 border border-brand-500/30 text-brand-400">
                      Hook: {songPlan.global.arrangement_hook}
                    </span>
                  )}
                </div>
              )}
            </div>
          </div>

          {/* Mode D 抖音 15s 黄金切片与循环尾信息 */}
          {songPlan.mode_meta?.douyin && (
            <div className="studio-subcard rounded-lg px-3 py-2 flex items-center justify-between gap-2 flex-wrap text-[11px]">
              <div className="flex items-center gap-2 min-w-0">
                <span className="studio-mono text-[10px] px-1.5 py-0.5 rounded bg-brand-500/15 text-brand-400 border border-brand-500/35 shrink-0">
                  DOUYIN 15S CLIP
                </span>
                <span className="text-text-1 font-medium truncate">
                  黄金记忆点：「{songPlan.mode_meta.douyin.hook_line?.replace(/\n/g, " / ")}」
                </span>
              </div>
              <div className="flex items-center gap-2 studio-mono text-[10px] text-text-muted shrink-0">
                {songPlan.mode_meta.douyin.clip && (
                  <span>
                    {songPlan.mode_meta.douyin.clip.start_section} → {songPlan.mode_meta.douyin.clip.end_section} (
                    {songPlan.mode_meta.douyin.clip.seconds}s)
                  </span>
                )}
                <span className="px-1.5 py-0.2 rounded bg-surface-1 border border-border">
                  Loop Tail: {songPlan.mode_meta.douyin.loop_tail ? "ON" : "OFF"}
                </span>
              </div>
            </div>
          )}

          {/* 发音注音 / 声线锚点 / 统筹备注 */}
          {(songPlan.pronunciation_hints || songPlan.persona || songPlan.notes) && (
            <div className="flex flex-wrap gap-2 text-[11px] text-text-2">
              {songPlan.pronunciation_hints && (
                <span className="px-2 py-1 rounded bg-surface-2 border border-border">
                  <b className="text-text-muted font-normal">咬字注音：</b>
                  {songPlan.pronunciation_hints}
                </span>
              )}
              {songPlan.persona && (
                <span className="px-2 py-1 rounded bg-surface-2 border border-border">
                  <b className="text-text-muted font-normal">声线人设：</b>
                  {songPlan.persona}
                </span>
              )}
              {songPlan.notes && (
                <span className="px-2 py-1 rounded bg-surface-2 border border-border">
                  <b className="text-text-muted font-normal">制作手记：</b>
                  {songPlan.notes}
                </span>
              )}
            </div>
          )}
        </div>
      )}

      {/* =====================================================================
          05 · SECTION TRACKS（逐段声学与歌词分轨视图）
          ===================================================================== */}
      {showActions && songPlan?.sections && songPlan.sections.length > 0 && (
        <div className="studio-card rounded-xl p-3.5 space-y-2.5">
          <div className="flex items-center justify-between border-b border-border/60 pb-2">
            <div className="flex items-center gap-2">
              <span className="studio-section-tag px-1.5 py-0.5 rounded bg-surface-3 text-text-2 border border-border">
                05 · TRACKS
              </span>
              <span className="text-[12.5px] font-semibold text-text-1 flex items-center gap-1.5">
                <IconWaveSine size={14} className="text-brand-400" />
                逐段声学编排与歌词分轨 ({songPlan.sections.length} Sections)
              </span>
            </div>
            <span className="studio-mono text-[10px] text-text-muted">
              Energy Scale 0–{sunoMeta.energy_scale}
            </span>
          </div>

          <div className="space-y-2">
            {songPlan.sections.map((sec, idx) => {
              const cues = [...(sec.positive_local ?? []), ...(sec.instruments ?? [])].slice(0, 4);
              const energyVal = typeof sec.energy === "number" ? sec.energy : null;
              return (
                <div key={idx} className="studio-subcard rounded-lg p-2.5 space-y-1.5">
                  <div className="flex items-center justify-between gap-2 flex-wrap">
                    <div className="flex items-center gap-1.5 flex-wrap">
                      <span className="studio-mono text-[10px] px-1.5 py-0.2 rounded bg-surface-1 border border-border text-text-muted">
                        #{String(idx + 1).padStart(2, "0")}
                      </span>
                      <span className="studio-mono text-[11.5px] font-bold text-brand-400">
                        [{sec.label || sec.kind || `Section ${idx + 1}`}]
                      </span>
                      {sec.motion && (
                        <span
                          className={`studio-mono text-[9.5px] px-1.5 py-0.2 rounded border ${
                            sec.motion === "push"
                              ? "bg-brand-500/15 border-brand-500/40 text-brand-400"
                              : "bg-accent/15 border-accent/40 text-accent"
                          }`}
                        >
                          {sec.motion === "push" ? "PUSH · 推进" : "STOP · 骤停"}
                        </span>
                      )}
                      {sec.subtraction && (
                        <span className="studio-mono text-[9.5px] px-1.5 py-0.2 rounded bg-warning/12 border border-warning/35 text-warning">
                          减法: {sec.subtraction}
                        </span>
                      )}
                      {typeof sec.target_seconds === "number" && (
                        <span className="studio-mono text-[9.5px] text-text-muted">
                          ~{sec.target_seconds}s
                        </span>
                      )}
                    </div>
                    {energyVal !== null && (
                      <div className="flex items-center gap-1.5">
                        <div className="flex gap-[2px] w-20">
                          {Array.from({ length: 10 }, (_, k) => (
                            <div
                              key={k}
                              className={`h-1.5 flex-1 rounded-[1px] ${
                                k < energyVal ? "bg-brand-400" : "bg-surface-3"
                              }`}
                            />
                          ))}
                        </div>
                        <span className="studio-mono text-[10px] text-text-muted w-6 text-right">
                          E:{energyVal}
                        </span>
                      </div>
                    )}
                  </div>

                  {cues.length > 0 && (
                    <div className="flex flex-wrap gap-1">
                      {cues.map((c, ci) => (
                        <span
                          key={ci}
                          className="studio-mono text-[10px] px-1.5 py-0.2 rounded bg-surface-1/80 border border-border/60 text-text-2"
                        >
                          {c}
                        </span>
                      ))}
                    </div>
                  )}

                  {sec.lines && sec.lines.length > 0 && (
                    <div className="pt-1 text-[12.5px] text-text-1 leading-relaxed whitespace-pre-wrap pl-1 border-l-2 border-brand-500/40">
                      {sec.lines.join("\n")}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* =====================================================================
          06 · JUDGE 七维文本评估看板（TextEval Scorecard）
          ===================================================================== */}
      {textEval && (
        <div className="studio-card rounded-xl p-3.5 space-y-2.5">
          <div className="flex items-center justify-between border-b border-border/60 pb-2">
            <div className="flex items-center gap-2">
              <span className="studio-section-tag px-1.5 py-0.5 rounded bg-brand-500/15 text-brand-400 border border-brand-500/35">
                06 · JUDGE
              </span>
              <span className="text-[12.5px] font-semibold text-text-1 flex items-center gap-1.5">
                <IconAward size={14} className="text-brand-400" />
                七维文本质量评估看板
              </span>
            </div>
            <span className="studio-mono text-[11px] font-bold px-2 py-0.5 rounded bg-brand-500/15 text-brand-400 border border-brand-500/35">
              {textEval.total} / 14 {textEval.small_sample ? "· 小样本" : ""}
            </span>
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-1.5">
            {textEval.lines.map((line, idx) => {
              const [dimId = `D${idx + 1}`, scoreStr = "0", ...noteParts] = line.split(":");
              const score = Number(scoreStr) || 0;
              const note = noteParts.join(":");
              return (
                <div key={idx} className="studio-subcard rounded px-2.5 py-1.5 flex items-center justify-between gap-2 text-[11px]">
                  <div className="min-w-0 flex-1">
                    <div className="font-medium text-text-1 truncate">{dimId}</div>
                    {note && <div className="text-[10px] text-text-muted truncate">{note}</div>}
                  </div>
                  <span
                    className={`studio-mono text-[10.5px] font-bold px-1.5 py-0.2 rounded border shrink-0 ${
                      score >= 2
                        ? "bg-success/12 border-success/35 text-success"
                        : score === 1
                          ? "bg-brand-500/12 border-brand-500/35 text-brand-400"
                          : "bg-danger/12 border-danger/35 text-danger"
                    }`}
                  >
                    {score}/2
                  </span>
                </div>
              );
            })}
          </div>
          {textEval.note && <p className="text-[10px] text-text-muted">{textEval.note}</p>}
        </div>
      )}

      {/* =====================================================================
          07 · 协作审改日志 & Suno 标准四栏原文（保持全量 DOM 兼容与直接复制）
          ===================================================================== */}
      <div className="space-y-2.5">
        {turns.map((turn, i) => {
          const isUser = turn.role === "user";
          const isExpert = turn.role === "expert";
          const isStreaming =
            i === turns.length - 1 && !isUser && !isExpert && status === "streaming";

          if (isUser) {
            return (
              <div key={i} className="studio-subcard rounded-lg px-3 py-2 text-[12px] text-text-2 flex items-start gap-2">
                <span className="studio-section-tag px-1.5 py-0.2 rounded bg-surface-3 text-text-muted border border-border shrink-0 mt-0.5">
                  INPUT
                </span>
                <div className="whitespace-pre-wrap leading-relaxed flex-1 min-w-0">{turn.content}</div>
              </div>
            );
          }

          if (isExpert) {
            return (
              <div key={i} className="studio-subcard rounded-lg px-3 py-2 text-[11.5px] text-text-2 flex items-start gap-2">
                <span className="shrink-0 w-5 h-5 rounded bg-surface-1 border border-border flex items-center justify-center text-[11px] mt-0.5">
                  {turn.speaker?.emoji ?? "🎙️"}
                </span>
                <div className="flex-1 min-w-0 whitespace-pre-wrap leading-relaxed">
                  <span className="text-[10.5px] font-semibold text-text-1 mr-1.5">
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
                const { display, sections } = parseEnergy(turn.content);
                return (
                  <>
                    {turn.partial && (
                      <div className="flex items-center gap-1.5 mb-1.5 text-[10px] text-warning">
                        <span className="px-1.5 py-0.5 rounded-md bg-warning/10 border border-warning/30 font-medium">
                          {t(locale, "result.partial")}
                        </span>
                        <span className="text-text-muted">{t(locale, "result.partial.hint")}</span>
                      </div>
                    )}
                    <div
                      className={`rounded-xl px-4 py-3 text-[12.5px] leading-relaxed studio-card border-l-2 border-brand-500/60 text-text-1 whitespace-pre-wrap selection:bg-brand-500/20 studio-mono ${
                        songPlan?.sections?.length && !showRawPackage && !isStreaming && !turn.partial
                          ? "max-h-64 overflow-y-auto"
                          : ""
                      }`}
                    >
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
                  <div
                    className="h-full w-1/3 rounded-full bg-brand-400/60"
                    style={{ animation: "streamBar 1.2s ease-in-out infinite" }}
                  />
                </div>
              )}
            </div>
          );
        })}
      </div>

      {/* #11 常驻底部工作台操作条（sticky bottom-0） */}
      {showActions && (
        <div className="sticky bottom-0 z-10 pt-2 pb-0.5">
          <div className="glass-panel rounded-xl border border-brand-500/40 bg-surface-1/90 backdrop-blur-md px-2.5 py-1.5 flex items-center gap-2 flex-wrap shadow-lg">
            <button
              onClick={handleCopy}
              className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-[12px] text-text-1 font-medium bg-surface-2 hover:bg-surface-3 border border-border transition-all duration-150 active:scale-95 cursor-pointer"
            >
              {copied ? <IconCheck size={13} className="text-success" /> : <IconCopy size={13} />}
              {copied ? t(locale, "result.copied") : t(locale, "result.copy")}
            </button>
            {pkg.isPackage && (
              <>
                {(
                  [
                    ["style", "result.copy.style", pkg.style],
                    ["exclude", "result.copy.exclude", pkg.exclude],
                    ["settings", "result.copy.settings", pkg.settings],
                    ["lyrics", "result.copy.lyrics", pkg.lyrics],
                  ] as const
                ).map(([key, i18nKey, val]) => (
                  <button
                    key={key}
                    onClick={() => copyText(val, key)}
                    disabled={!val.trim()}
                    className="flex items-center gap-1 px-2 py-1.5 rounded-lg text-[11px] text-text-2 studio-mono bg-surface-2/60 hover:bg-surface-3 border border-border/60 transition-all duration-150 active:scale-95 disabled:opacity-35 disabled:cursor-not-allowed cursor-pointer"
                  >
                    {copiedField === key ? (
                      <IconCheck size={12} className="text-success" />
                    ) : (
                      <IconCopy size={12} />
                    )}
                    {t(locale, i18nKey)}
                  </button>
                ))}
              </>
            )}
            <button
              onClick={() => setShowRefine(!showRefine)}
              className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-[12px] font-semibold border transition-all duration-150 active:scale-95 cursor-pointer ${
                showRefine
                  ? "bg-brand-500/20 border-brand-500/50 text-brand-400"
                  : "bg-brand-500/12 hover:bg-brand-500/22 border-brand-500/35 text-brand-400"
              }`}
            >
              <IconRefresh size={13} />
              {lastIsPartial ? t(locale, "result.refine.partial") : t(locale, "result.refine")}
              <IconChevronDown
                size={12}
                className={`transition-transform duration-150 ${showRefine ? "rotate-180" : ""}`}
              />
            </button>
            <span className="text-[10px] text-text-muted ml-auto pr-1">
              {lastIsPartial ? t(locale, "result.partial.ready") : t(locale, "result.actions.hint")}
            </span>
          </div>
        </div>
      )}

      {/* 定向精修与深度重做抽屉 */}
      {showRefine && !busy && hasOutput && (
        <div className="studio-card rounded-xl border border-border p-3.5 space-y-2.5 animate-fade-in">
          <div className="flex items-center gap-1.5 flex-wrap text-[11px]">
            <span className="text-text-muted">{t(locale, "result.refine.quick")}</span>
            {(
              [
                "result.refine.q1",
                "result.refine.q2",
                "result.refine.q3",
                "result.refine.q4",
              ] as const
            ).map((qk) => {
              const label = t(locale, qk);
              return (
                <button
                  key={qk}
                  type="button"
                  onClick={() =>
                    setFeedback((prev) => (prev.trim() ? `${prev}；${label}` : label))
                  }
                  className="px-2 py-0.5 rounded-md bg-surface-2 hover:bg-surface-3 border border-border text-text-2 transition-all duration-150 cursor-pointer"
                >
                  + {label}
                </button>
              );
            })}
          </div>
          <textarea
            value={feedback}
            onChange={(e) => setFeedback(e.target.value)}
            className="w-full h-16 bg-surface-0 border border-border rounded-lg px-3 py-2 text-[13px] text-text-1 placeholder:text-text-muted/40 resize-y focus:outline-none focus:border-brand-500/50 transition-all duration-150"
            placeholder="比如：唢呐不够炸、人声太软、洗脑循环不明显..."
          />
          {lastIsPartial && (
            <p className="text-[10px] text-warning leading-relaxed">
              {t(locale, "result.partial.panel")}
            </p>
          )}
          <div className="flex gap-2">
            {(["fast", "full"] as const).map((m) => (
              <button
                key={m}
                onClick={() => setRefineMode(m)}
                className={`flex-1 py-1.5 rounded-lg text-[12px] font-medium border transition-all duration-150 cursor-pointer ${
                  refineMode === m
                    ? "bg-brand-500/15 border-brand-500/45 text-brand-400"
                    : "bg-surface-2 border-border text-text-muted hover:text-text-2"
                }`}
              >
                {m === "fast" ? t(locale, "result.refine.fast") : t(locale, "result.refine.full")}
              </button>
            ))}
          </div>
          {refineMode === "fast" && feedback.trim() && (
            <p className="text-[10px] text-text-muted leading-relaxed">
              {estimated.length > 0
                ? `预计重跑：${estimated.map((r) => REFINE_TARGET_NAMES[r]).join("、")}＋校验员（跳过阶段0 初稿，直接基于上一版 SongPlan IR 定向精修）`
                : "无法判断涉及角色，将全量重跑（与深度重做一致）"}
            </p>
          )}
          <button
            onClick={() => {
              onRefine(feedback, refineMode);
              setFeedback("");
              setShowRefine(false);
            }}
            disabled={!feedback.trim()}
            className="w-full py-2 rounded-lg text-[12.5px] font-semibold brand-gradient-btn text-white disabled:opacity-40 disabled:cursor-not-allowed transition-all duration-150 active:scale-[0.99] cursor-pointer"
          >
            <IconRefresh size={14} className="inline mr-1.5 -mt-0.5" />
            {refineMode === "fast" ? t(locale, "result.refine.fast") : t(locale, "result.refine.send")}
          </button>
        </div>
      )}
    </div>
  );
}

import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { readTextFile } from "@tauri-apps/plugin-fs";
import { IconSend, IconWand, IconAdjustments, IconChevronDown, IconChevronUp, IconDisc } from "@tabler/icons-react";
import type { AppSettings, Locale, Mode, VocalMixMetaItem } from "../types";
import { t } from "../i18n";
import type { MessageKey } from "../i18n";
import SceneSelector from "./SceneSelector";

/** 四项高频声学胶囊（直接绑定 18 项受控词表 ID，与 knowledge/vocal_mix.csv 单源联动） */
const QUICK_PRESETS: { itemId: string; category: "vocal" | "mix"; key: MessageKey }[] = [
  {
    itemId: "V-09",
    category: "vocal",
    key: "input.preset.v09",
  },
  {
    itemId: "V-05",
    category: "vocal",
    key: "input.preset.v05",
  },
  {
    itemId: "M-07",
    category: "mix",
    key: "input.preset.m07",
  },
  {
    itemId: "M-09",
    category: "mix",
    key: "input.preset.m09",
  },
];

/** 18 条受控人声/混音词表内置兜底（与 src-tauri/knowledge/vocal_mix.csv 逐字同源，由 wireProtocol.test.ts 跨语言锁死） */
export const FALLBACK_VOCAL_MIX_META: VocalMixMetaItem[] = [
  { id: "V-01", category: "vocal", label_cn: "清晰咬字", label_en: "clear diction, upfront lead vocal, articulate", applies_to: "全程" },
  { id: "V-02", category: "vocal", label_cn: "气声转音", label_en: "breathy vocal, smooth runs, subtle vibrato, intimate", applies_to: "主歌/低能段" },
  { id: "V-03", category: "vocal", label_cn: "情绪哭腔", label_en: "emotional delivery, slight cry in chorus, building intensity, restrained", applies_to: "副歌" },
  { id: "V-04", category: "vocal", label_cn: "和声伴唱", label_en: "layered harmonies, double-tracked backing vocals, lead-forward", applies_to: "副歌" },
  { id: "V-05", category: "vocal", label_cn: "脆弱微破音", label_en: "fragile close-mic vocal, slight voice crack on high notes, audible breath", applies_to: "主歌/桥段/情感转折" },
  { id: "V-06", category: "vocal", label_cn: "胸声撕裂爆发", label_en: "powerful chest-voice belt, raw gritty edge, wide dynamic range", applies_to: "副歌/高潮" },
  { id: "V-07", category: "vocal", label_cn: "慵懒半说半唱", label_en: "lazy behind-the-beat drawl, conversational half-spoken delivery, relaxed pocket", applies_to: "主歌/R&B/说唱/抖音" },
  { id: "V-08", category: "vocal", label_cn: "八度与三度叠唱", label_en: "octave double-tracked lead, lush 3rd harmony stack, call-and-response ad-libs", applies_to: "副歌/终局副歌" },
  { id: "V-09", category: "vocal", label_cn: "终局升调叠唱", label_en: "key modulation up a half-step, soaring final chorus belt, wide octave backing choir", applies_to: "终局副歌/高潮" },
  { id: "M-01", category: "mix", label_cn: "人声贴唱", label_en: "vocal-forward, centered, close-mic, moderate reverb", applies_to: "全程" },
  { id: "M-02", category: "mix", label_cn: "低频增强", label_en: "full low-end, deep sub-bass, punchy kick, tight lows", applies_to: "全程" },
  { id: "M-03", category: "mix", label_cn: "空间混响", label_en: "wide stereo field, lush hall reverb, spacious", applies_to: "全程" },
  { id: "M-04", category: "mix", label_cn: "分轨层次感", label_en: "clear separation, balanced EQ, transparent mix", applies_to: "全程" },
  { id: "M-05", category: "mix", label_cn: "母带质感", label_en: "mastered, wide dynamics, polished, hi-fi", applies_to: "全程" },
  { id: "M-06", category: "mix", label_cn: "手机扬声器友好", label_en: "phone-speaker friendly, vocal-forward mids, controlled low-end", applies_to: "抖音/全程" },
  { id: "M-07", category: "mix", label_cn: "干声贴耳磁带暖感", label_en: "dry close-mic bedroom intimacy, subtle analog tape saturation, organic room tone", applies_to: "民谣/独立/主歌" },
  { id: "M-08", category: "mix", label_cn: "侧链脉冲与瞬态雕刻", label_en: "sidechained sub-bass pocket, crisp transient punch, carved midrange for vocal", applies_to: "流行/电子/摇滚/抖音" },
  { id: "M-09", category: "mix", label_cn: "二创分轨预净化", label_en: "dry isolated lead vocal, natural pitch, stem-clean frequency separation, minimal reverb tail", applies_to: "全程/二创" },
];

interface Props {
  mode: Mode;
  disabled: boolean;
  settings: AppSettings;
  onGenerate: (userInput: string, extra?: { originalLyrics?: string; douyinScene?: string }) => void;
  inputRef?: React.RefObject<HTMLTextAreaElement | null>;
  locale?: Locale;
}

export default function InputPanel({ mode, disabled, settings, onGenerate, inputRef, locale }: Props) {
  const [input, setInput] = useState("");
  const [originalLyrics, setOriginalLyrics] = useState("");
  const [douyinScene, setDouyinScene] = useState<string>("daily");
  const [dropMsg, setDropMsg] = useState("");
  const [vocalMixMeta, setVocalMixMeta] = useState<VocalMixMetaItem[]>(FALLBACK_VOCAL_MIX_META);
  const [showMatrix, setShowMatrix] = useState(false);
  const [selectedVocalId, setSelectedVocalId] = useState<string | null>(null);
  const [selectedMixIds, setSelectedMixIds] = useState<string[]>([]);
  const [referenceAudio, setReferenceAudio] = useState(false);

  // 启动拉取后端 18 项受控人声/混音词表（失败回退内置表，不阻断）
  useEffect(() => {
    let cancelled = false;
    invoke<VocalMixMetaItem[]>("get_vocal_mix_meta")
      .then((rows) => {
        if (!cancelled && Array.isArray(rows) && rows.length > 0) {
          setVocalMixMeta(rows);
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  // 支持拖入 .txt / .lrc 歌词文件
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      try {
        const u = await getCurrentWebview().onDragDropEvent(async (event) => {
          if (event.payload.type !== "drop") return;
          const filePath = event.payload.paths?.[0];
          if (!filePath) return;
          if (!/\.(txt|lrc)$/i.test(filePath)) {
            setDropMsg(t(locale, "input.drop.only"));
            setTimeout(() => setDropMsg(""), 3000);
            return;
          }
          try {
            const content = await readTextFile(filePath);
            if (mode === "mode_c") {
              setOriginalLyrics(content);
            } else {
              setInput(content);
            }
          } catch {
            setDropMsg(t(locale, "input.drop.fail"));
            setTimeout(() => setDropMsg(""), 3000);
          }
        });
        if (cancelled) u();
        else unlisten = u;
      } catch {
        /* 非 Tauri 环境静默 */
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [mode, locale]);

  const trimmed = input.trim();
  const minChars = mode === "mode_a" ? 10 : 1;
  const canSubmit =
    !disabled &&
    Boolean(settings.apiKey) &&
    trimmed.length >= minChars &&
    (mode !== "mode_c" || originalLyrics.trim().length > 0);

  const toggleVocalId = (id: string) => {
    setSelectedVocalId((prev) => (prev === id ? null : id));
  };

  const toggleMixId = (id: string) => {
    setSelectedMixIds((prev) => {
      if (prev.includes(id)) return prev.filter((x) => x !== id);
      if (prev.length >= 2) return [prev[1], id];
      return [...prev, id];
    });
  };

  const handleSubmit = () => {
    if (!canSubmit) return;
    const directives: string[] = [];
    if (selectedVocalId) {
      const vItem = vocalMixMeta.find((x) => x.id === selectedVocalId);
      if (vItem) {
        directives.push(
          `【人声锚点 ${vItem.id}（${vItem.label_cn}）：global.vocal 锁定单选 ["${vItem.id}"] (${vItem.label_en})】`,
        );
      }
    }
    if (selectedMixIds.length > 0) {
      const effectiveMixIds =
        mode === "mode_d"
          ? Array.from(new Set(["M-01", "M-06", ...selectedMixIds]))
          : selectedMixIds;
      const mLabels = effectiveMixIds
        .map((id) => {
          const mItem = vocalMixMeta.find((x) => x.id === id);
          return mItem ? `${mItem.id} (${mItem.label_cn}: ${mItem.label_en})` : id;
        })
        .join(" + ");
      directives.push(
        mode === "mode_d"
          ? `【混音/分轨锚点（保留抖音必填 M-01+M-06 并叠加）：global.mix 选用 ${mLabels}】`
          : `【混音/分轨锚点：global.mix 选用 ${mLabels}】`,
      );
    }
    if (referenceAudio) {
      directives.push(
        "【参考音频二创模式：settings.reference_audio=true，settings.variety=0，settings.audio_influence=75（满足参考音频声学锁定门禁）】",
      );
    }
    const finalInput = directives.length > 0 ? `${trimmed}\n\n${directives.join("\n")}` : trimmed;

    if (mode === "mode_c") {
      onGenerate(finalInput, { originalLyrics: originalLyrics.trim() });
    } else if (mode === "mode_d") {
      onGenerate(finalInput, { douyinScene });
    } else {
      onGenerate(finalInput, undefined);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      handleSubmit();
    }
  };

  const mainLabel =
    mode === "mode_a"
      ? t(locale, "input.lyrics")
      : mode === "mode_c"
        ? t(locale, "input.theme")
        : mode === "mode_d"
          ? t(locale, "input.inspiration.d")
          : t(locale, "input.inspiration.b");

  const suffixHint = `${t(locale, "input.ph.suffix.submit")} · ${t(locale, "input.ph.suffix.drop")}`;
  const mainPlaceholder =
    mode === "mode_a"
      ? `${t(locale, "input.ph.lyrics")} ${suffixHint}`
      : mode === "mode_c"
        ? `${t(locale, "input.ph.theme")} ${suffixHint}`
        : mode === "mode_d"
          ? `${t(locale, "input.ph.inspiration.d")} ${suffixHint}`
          : `${t(locale, "input.ph.inspiration.b")} ${suffixHint}`;

  const vocalEntries = vocalMixMeta.filter((x) => x.category === "vocal");
  const mixEntries = vocalMixMeta.filter((x) => x.category === "mix");
  const activeAnchorCount =
    (selectedVocalId ? 1 : 0) + selectedMixIds.length + (referenceAudio ? 1 : 0);

  return (
    <div className="space-y-2.5">
      {/* Studio Rack Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="studio-section-tag px-1.5 py-0.5 rounded bg-brand-500/12 text-brand-400 border border-brand-500/30">
            01 · INPUT
          </span>
          <span className="text-[12px] font-semibold text-text-1 tracking-tight">
            {mainLabel}
          </span>
        </div>
        <span className="studio-mono text-[10px] text-text-muted">
          {trimmed.length} chars · ⌘↵
        </span>
      </div>

      {/* Mode D 抖音场景与 BPM 档位选择 */}
      {mode === "mode_d" && (
        <div className="studio-subcard rounded-lg px-2.5 py-2">
          <SceneSelector
            value={douyinScene}
            onChange={setDouyinScene}
            inspiration={input}
            disabled={disabled}
            locale={locale}
          />
        </div>
      )}

      {/* Mode C 原歌词对照输入槽 */}
      {mode === "mode_c" && (
        <div className="space-y-1">
          <div className="flex items-center justify-between">
            <label className="text-[11px] font-medium text-text-2">
              {t(locale, "input.original")}
            </label>
            <span className="studio-mono text-[10px] text-text-muted">
              {originalLyrics.trim()
                ? `${originalLyrics.trim().split(/\r?\n/).filter((l) => l.trim()).length} lines`
                : "0 lines"}
            </span>
          </div>
          <textarea
            value={originalLyrics}
            onChange={(e) => setOriginalLyrics(e.target.value)}
            onKeyDown={handleKeyDown}
            disabled={disabled}
            rows={3}
            placeholder={`${t(locale, "input.ph.original")} ${suffixHint}`}
            className="w-full bg-surface-0 border border-border rounded-lg px-3 py-2 text-[12.5px]
                       text-text-1 placeholder:text-text-muted/45 focus:outline-none
                       focus:border-brand-500/60 resize-none leading-relaxed transition-all duration-150 disabled:opacity-50"
          />
        </div>
      )}

      {/* 主创作输入槽 */}
      <textarea
        ref={inputRef}
        value={input}
        onChange={(e) => setInput(e.target.value)}
        onKeyDown={handleKeyDown}
        disabled={disabled}
        rows={mode === "mode_c" ? 3 : 4}
        placeholder={mainPlaceholder}
        className="w-full bg-surface-0 border border-border rounded-lg px-3 py-2.5 text-[13px]
                   text-text-1 placeholder:text-text-muted/45 focus:outline-none
                   focus:border-brand-500/60 resize-none leading-relaxed transition-all duration-150 disabled:opacity-50"
      />

      {dropMsg && <p className="text-[11px] text-warning">{dropMsg}</p>}

      {/* 声学质感快捷胶囊 + 18项受控词表矩阵抽屉（双向同步单源） */}
      <div className="space-y-1.5">
        <div className="flex items-center justify-between gap-1.5 flex-wrap">
          <div className="flex items-center gap-1 flex-wrap">
            <span className="inline-flex items-center gap-1 text-[10.5px] text-text-muted mr-0.5">
              <IconWand size={11} className="text-brand-400" />
              {t(locale, "input.presets.label")}
            </span>
            {QUICK_PRESETS.map((p) => {
              const active =
                p.category === "vocal"
                  ? selectedVocalId === p.itemId
                  : selectedMixIds.includes(p.itemId);
              return (
                <button
                  key={p.itemId}
                  type="button"
                  disabled={disabled}
                  onClick={() =>
                    p.category === "vocal" ? toggleVocalId(p.itemId) : toggleMixId(p.itemId)
                  }
                  className={`px-2 py-0.5 rounded-md text-[10.5px] font-medium border transition-all duration-150 cursor-pointer
                    ${active
                      ? "bg-brand-500/15 border-brand-500/50 text-brand-400"
                      : "bg-surface-0/70 border-border/70 text-text-muted hover:text-text-2 hover:border-border-hover"
                    } disabled:opacity-40`}
                >
                  {t(locale, p.key)}
                </button>
              );
            })}
          </div>
          <button
            type="button"
            disabled={disabled}
            onClick={() => setShowMatrix((v) => !v)}
            className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-md text-[10.5px] font-medium border transition-all cursor-pointer
              ${showMatrix || activeAnchorCount > 0
                ? "bg-accent/12 border-accent/40 text-accent"
                : "bg-surface-0/70 border-border/70 text-text-muted hover:text-text-2"
              } disabled:opacity-40`}
          >
            <IconAdjustments size={11} />
            <span>18项声学矩阵{activeAnchorCount > 0 ? ` (${activeAnchorCount})` : ""}</span>
            {showMatrix ? <IconChevronUp size={11} /> : <IconChevronDown size={11} />}
          </button>
        </div>

        {/* 18项受控人声/混音词表抽屉（与后端 knowledge/vocal_mix.csv 1:1 绑定） */}
        {showMatrix && (
          <div className="studio-subcard rounded-lg p-2.5 space-y-2.5 animate-fade-in">
            {/* 人声物理锚点 V-01..V-09（RULE-VOCAL-SINGLE：单选互斥） */}
            <div>
              <div className="flex items-center justify-between mb-1">
                <span className="text-[10.5px] font-semibold text-text-2">
                  人声物理锚点 · Vocal Anchor（单选互斥）
                </span>
                <span className="studio-mono text-[9.5px] text-text-muted">
                  {selectedVocalId ? `已锁 ${selectedVocalId}` : "未指定（自动匹配）"}
                </span>
              </div>
              <div className="grid grid-cols-3 gap-1">
                {vocalEntries.map((item) => {
                  const active = selectedVocalId === item.id;
                  return (
                    <button
                      key={item.id}
                      type="button"
                      onClick={() => toggleVocalId(item.id)}
                      title={`${item.label_en} · 适用：${item.applies_to}`}
                      className={`px-2 py-1 rounded text-left border transition-all cursor-pointer
                        ${active
                          ? "bg-brand-500/15 border-brand-500/50 text-brand-400"
                          : "bg-surface-1/80 border-border/60 text-text-2 hover:border-border-hover"
                        }`}
                    >
                      <div className="flex items-center justify-between gap-1">
                        <span className="text-[10.5px] font-medium truncate">{item.label_cn}</span>
                        <span className="studio-mono text-[9px] opacity-70 shrink-0">{item.id}</span>
                      </div>
                    </button>
                  );
                })}
              </div>
            </div>

            {/* 混音与分轨质感 M-01..M-09（最多选 2 项） */}
            <div>
              <div className="flex items-center justify-between mb-1">
                <span className="text-[10.5px] font-semibold text-text-2">
                  声场与混音母带 · Mix & Stem（最多 2 项）
                </span>
                <span className="studio-mono text-[9.5px] text-text-muted">
                  {selectedMixIds.length > 0 ? selectedMixIds.join(" + ") : "0 / 2"}
                </span>
              </div>
              <div className="grid grid-cols-3 gap-1">
                {mixEntries.map((item) => {
                  const active = selectedMixIds.includes(item.id);
                  return (
                    <button
                      key={item.id}
                      type="button"
                      onClick={() => toggleMixId(item.id)}
                      title={`${item.label_en} · 适用：${item.applies_to}`}
                      className={`px-2 py-1 rounded text-left border transition-all cursor-pointer
                        ${active
                          ? "bg-accent/15 border-accent/50 text-accent"
                          : "bg-surface-1/80 border-border/60 text-text-2 hover:border-border-hover"
                        }`}
                    >
                      <div className="flex items-center justify-between gap-1">
                        <span className="text-[10.5px] font-medium truncate">{item.label_cn}</span>
                        <span className="studio-mono text-[9px] opacity-70 shrink-0">{item.id}</span>
                      </div>
                    </button>
                  );
                })}
              </div>
            </div>

            {/* 参考音频二创开关（reference_audio=true -> variety=0 & audio_influence=75 硬门） */}
            <div className="pt-1 border-t border-border/50 flex items-center justify-between">
              <div className="flex items-center gap-1.5">
                <IconDisc size={12} className={referenceAudio ? "text-brand-400" : "text-text-muted"} />
                <span className="text-[10.5px] text-text-2">
                  参考音频二创锁定（Reference Audio · Variety=0 & Audio Influence=75）
                </span>
              </div>
              <button
                type="button"
                onClick={() => setReferenceAudio((v) => !v)}
                className={`px-2 py-0.5 rounded text-[10px] studio-mono border transition-all cursor-pointer
                  ${referenceAudio
                    ? "bg-brand-500/20 border-brand-500/50 text-brand-400"
                    : "bg-surface-1 border-border text-text-muted"
                  }`}
              >
                {referenceAudio ? "ON · LOCKED" : "OFF"}
              </button>
            </div>
          </div>
        )}
      </div>

      {/* 主操作栏 */}
      <div className="flex items-center justify-end pt-0.5">
        <button
          onClick={handleSubmit}
          disabled={!canSubmit}
          className="flex items-center gap-1.5 px-4 py-1.5 rounded-lg text-[12.5px] font-semibold
                     brand-gradient-btn text-white
                     disabled:opacity-35 disabled:cursor-not-allowed
                     transition-all duration-150 active:scale-[0.98] cursor-pointer"
        >
          <IconSend size={13} />
          {disabled ? t(locale, "input.generating") : t(locale, "input.generate")}
        </button>
      </div>
    </div>
  );
}

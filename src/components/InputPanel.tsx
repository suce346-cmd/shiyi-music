import { useState } from "react";
import { IconSend, IconSparkles, IconEdit } from "@tabler/icons-react";
import type { Mode, AppSettings } from "../types";

interface Props {
  mode: Mode;
  disabled: boolean;
  settings: AppSettings;
  /** F12：mode_c 时附带原歌词独立字段（不再拼字符串） */
  onGenerate: (userInput: string, extra?: { originalLyrics: string }) => void;
}

export default function InputPanel({ mode, disabled, settings, onGenerate }: Props) {
  const [lyrics, setLyrics] = useState("");
  const [inspiration, setInspiration] = useState("");
  const [originalLyrics, setOriginalLyrics] = useState("");
  const [newTheme, setNewTheme] = useState("");

  const handleSubmit = () => {
    if (!settings.apiKey) return;
    let userInput = "";
    // F12：mode_c 原歌词独立字段直传——新主题即 userInput，原歌词走 extra.originalLyrics
    let extra: { originalLyrics: string } | undefined;
    if (mode === "mode_a") {
      userInput = lyrics;
      if (lyrics.trim().length < 10) return;
    } else if (mode === "mode_c") {
      userInput = newTheme;
      if (!originalLyrics.trim() || !newTheme.trim()) return;
      extra = { originalLyrics };
    } else {
      userInput = inspiration;
      if (!inspiration.trim()) return;
    }
    onGenerate(userInput, extra);
  };

  const canSubmit = (() => {
    if (!settings.apiKey || disabled) return false;
    if (mode === "mode_a") return lyrics.trim().length >= 10;
    if (mode === "mode_c") return !!originalLyrics.trim() && !!newTheme.trim();
    return !!inspiration.trim();
  })();

  const inputClass = "w-full bg-surface-1/60 backdrop-blur-sm border border-border/40 rounded-xl px-3 py-2.5 text-[13px] text-text-1 placeholder:text-text-muted/30 resize-y focus:outline-none focus:border-brand-500/50 focus:ring-1 focus:ring-brand-500/20 transition-all duration-150";
  const labelClass = "flex items-center gap-1.5 text-[11px] text-text-muted mb-1.5";

  return (
    <div className="space-y-3">
      {mode === "mode_a" && (
        <div>
          <label className={labelClass}><IconSparkles size={13} /> 歌词</label>
          <textarea value={lyrics} onChange={e => setLyrics(e.target.value)}
            className={`${inputClass} h-32`} placeholder="粘贴歌词文本..." disabled={disabled} />
        </div>
      )}

      {mode === "mode_c" && (
        <>
          <div>
            <label className={labelClass}><IconEdit size={13} /> 原歌词</label>
            <textarea value={originalLyrics} onChange={e => setOriginalLyrics(e.target.value)}
              className={`${inputClass} h-24`} placeholder="粘贴原歌词..." disabled={disabled} />
          </div>
          <div>
            <label className={labelClass}><IconSparkles size={13} /> 新主题 / 故事</label>
            <textarea value={newTheme} onChange={e => setNewTheme(e.target.value)}
              className={`${inputClass} h-20`} placeholder="比如：北漂青年过年回家的故事..." disabled={disabled} />
          </div>
        </>
      )}

      {(mode === "mode_b" || mode === "mode_d") && (
        <div>
          <label className={labelClass}><IconSparkles size={13} /> {mode === "mode_b" ? "灵感 / 话题 / 情绪 / 故事" : "灵感 / 话题 / 梗"}</label>
          <textarea value={inspiration} onChange={e => setInspiration(e.target.value)}
            className={`${inputClass} h-24`} disabled={disabled}
            placeholder={mode === "mode_b" ? "比如：异乡过年，看烟花想起故乡..." : "比如：踩到香蕉皮摔倒的尴尬瞬间..."} />
        </div>
      )}

      <button onClick={handleSubmit} disabled={!canSubmit}
        className="w-full py-2.5 rounded-xl text-[13px] font-medium flex items-center justify-center gap-2
                   brand-gradient-btn text-white active:scale-[0.98]
                   disabled:opacity-40 disabled:cursor-not-allowed disabled:shadow-none
                   transition-all duration-150">
        <IconSend size={15} />
        {disabled ? "生成中..." : "生成"}
      </button>
    </div>
  );
}

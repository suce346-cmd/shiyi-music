import { useState } from "react";
import { IconSend, IconSparkles, IconEdit } from "@tabler/icons-react";
import type { Mode, AppSettings } from "../types";

interface Props {
  mode: Mode;
  disabled: boolean;
  settings: AppSettings;
  onGenerate: (userInput: string) => void;
}

export default function InputPanel({
  mode, disabled, settings, onGenerate,
}: Props) {
  const [lyrics, setLyrics] = useState("");
  const [inspiration, setInspiration] = useState("");
  const [originalLyrics, setOriginalLyrics] = useState("");
  const [newTheme, setNewTheme] = useState("");

  const handleSubmit = () => {
    if (!settings.apiKey) { alert("请先在设置中填写 API Key"); return; }
    let userInput = "";
    if (mode === "mode_a") {
      userInput = lyrics;
      if (lyrics.trim().length < 10) { alert("歌词至少 10 个字以上"); return; }
    } else if (mode === "mode_c") {
      userInput = `原歌词：\n${originalLyrics}\n\n新主题：\n${newTheme}`;
      if (!originalLyrics.trim()) { alert("请填写原歌词"); return; }
      if (!newTheme.trim()) { alert("请填写新主题/故事"); return; }
    } else {
      userInput = inspiration;
      if (!inspiration.trim()) { alert("请填写灵感/话题/情绪/故事"); return; }
    }
    onGenerate(userInput);
  };

  return (
    <div className="bg-surface-2 rounded-2xl border border-border shadow-sm p-5 space-y-4">
      {mode === "mode_a" && (
        <div>
          <label className="flex items-center gap-1.5 text-xs text-text-muted mb-2">
            <IconSparkles size={14} /> 粘贴歌词
          </label>
          <textarea value={lyrics} onChange={e => setLyrics(e.target.value)}
            className="w-full h-40 bg-surface-0 border border-border rounded-xl p-3.5 text-sm
                       text-text-1 placeholder:text-text-muted/40 resize-y
                       focus:outline-none focus:ring-2 focus:ring-brand-500/30 focus:border-brand-500
                       transition-all duration-150"
            placeholder="粘贴歌词文本..." disabled={disabled} />
        </div>
      )}

      {mode === "mode_c" && (
        <>
          <div>
            <label className="flex items-center gap-1.5 text-xs text-text-muted mb-2">
              <IconEdit size={14} /> 原歌词
            </label>
            <textarea value={originalLyrics} onChange={e => setOriginalLyrics(e.target.value)}
              className="w-full h-32 bg-surface-0 border border-border rounded-xl p-3.5 text-sm
                         text-text-1 placeholder:text-text-muted/40 resize-y
                         focus:outline-none focus:ring-2 focus:ring-brand-500/30 focus:border-brand-500
                         transition-all duration-150"
              placeholder="粘贴原歌词..." disabled={disabled} />
          </div>
          <div>
            <label className="flex items-center gap-1.5 text-xs text-text-muted mb-2">
              <IconSparkles size={14} /> 新主题 / 故事
            </label>
            <textarea value={newTheme} onChange={e => setNewTheme(e.target.value)}
              className="w-full h-24 bg-surface-0 border border-border rounded-xl p-3.5 text-sm
                         text-text-1 placeholder:text-text-muted/40 resize-y
                         focus:outline-none focus:ring-2 focus:ring-brand-500/30 focus:border-brand-500
                         transition-all duration-150"
              placeholder="比如：把这首歌改成讲述一个北漂青年过年回家的故事..." disabled={disabled} />
          </div>
        </>
      )}

      {(mode === "mode_b" || mode === "mode_d") && (
        <div>
          <label className="flex items-center gap-1.5 text-xs text-text-muted mb-2">
            <IconSparkles size={14} /> {mode === "mode_b" ? "灵感 / 话题 / 情绪 / 故事" : "灵感 / 话题 / 梗"}
          </label>
          <textarea value={inspiration} onChange={e => setInspiration(e.target.value)}
            className="w-full h-32 bg-surface-0 border border-border rounded-xl p-3.5 text-sm
                       text-text-1 placeholder:text-text-muted/40 resize-y
                       focus:outline-none focus:ring-2 focus:ring-brand-500/30 focus:border-brand-500
                       transition-all duration-150"
            placeholder={mode === "mode_b" ? "比如：一个人在异乡过年，看着窗外烟花想起小时候的故乡..." : "比如：一个人走路踩到香蕉皮摔倒的尴尬瞬间..."} disabled={disabled} />
        </div>
      )}

      <button onClick={handleSubmit} disabled={disabled || !settings.apiKey}
        className="w-full py-3 rounded-xl font-medium text-sm flex items-center justify-center gap-2
                   bg-gradient-to-r from-brand-500 to-brand-600 text-white
                   hover:from-brand-600 hover:to-brand-700 active:scale-[0.98]
                   disabled:from-gray-300 disabled:to-gray-300 disabled:cursor-not-allowed
                   shadow-lg shadow-brand-500/20 hover:shadow-brand-500/30
                   transition-all duration-150">
        <IconSend size={16} />
        {!settings.apiKey ? "请先在 ⚙ 设置中填写 API Key" : disabled ? "生成中..." : "生成"}
      </button>
    </div>
  );
}

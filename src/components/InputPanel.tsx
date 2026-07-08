import { useState } from "react";
import { IconSend, IconSparkles } from "@tabler/icons-react";
import { generatePrompt } from "../hooks/useLLM";
import type { Mode, AppSettings, LLMResponse, LLMStatus } from "../types";

interface Props {
  mode: Mode;
  disabled: boolean;
  settings: AppSettings;
  onStatusChange: (s: LLMStatus) => void;
  onResultChange: (r: LLMResponse | null) => void;
  onStreamUpdate: (t: string) => void;
  onUserInputChange: (i: string) => void;
  onResultSave: (input: string, result: LLMResponse) => void;
  onError?: (msg: string) => void;
}

export default function InputPanel({
  mode, disabled, settings, onStatusChange, onResultChange, onStreamUpdate,
  onUserInputChange, onResultSave, onError,
}: Props) {
  const [lyrics, setLyrics] = useState("");
  const [inspiration, setInspiration] = useState("");

  const handleSubmit = async () => {
    if (!settings.apiKey) { alert("请先在设置中填写 API Key"); return; }
    const isModeA = mode === "mode_a";
    const userInput = isModeA ? lyrics : inspiration;
    if (isModeA && lyrics.trim().length < 10) { alert("歌词至少 10 个字以上"); return; }
    if (!isModeA && !inspiration.trim()) { alert("请填写灵感/话题/梗"); return; }

    onStatusChange("loading"); onResultChange(null); onStreamUpdate(""); onUserInputChange(userInput);
    try {
      const result = await generatePrompt(mode, userInput, settings, (text) => {
        onStreamUpdate(text); onStatusChange("streaming");
      });
      onResultChange(result); onResultSave(userInput, result); onStatusChange("done");
    } catch (e) {
      const msg = String(e); onStatusChange("error"); onError?.(msg); alert(`生成失败：${msg}`);
    }
  };

  return (
    <div className="bg-surface-2 rounded-2xl border border-border shadow-sm p-5 space-y-4">
      {mode === "mode_a" ? (
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
      ) : (
        <div>
          <label className="flex items-center gap-1.5 text-xs text-text-muted mb-2">
            <IconSparkles size={14} /> 灵感 / 话题 / 梗
          </label>
          <textarea value={inspiration} onChange={e => setInspiration(e.target.value)}
            className="w-full h-32 bg-surface-0 border border-border rounded-xl p-3.5 text-sm
                       text-text-1 placeholder:text-text-muted/40 resize-y
                       focus:outline-none focus:ring-2 focus:ring-brand-500/30 focus:border-brand-500
                       transition-all duration-150"
            placeholder="比如：一个人走路踩到香蕉皮摔倒的尴尬瞬间..." disabled={disabled} />
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
        {!settings.apiKey ? "请先在 ⚙ 设置中填写 API Key" : disabled ? "生成中..." : "生成 Prompt"}
      </button>
    </div>
  );
}
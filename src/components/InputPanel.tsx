import { useState, useRef, useEffect } from "react";
import { IconSend, IconSparkles, IconEdit } from "@tabler/icons-react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { readTextFile } from "@tauri-apps/plugin-fs";
import type { Mode, AppSettings, Locale } from "../types";
import { t } from "../i18n";
import { isSubmitHotkey, isImportableFile } from "../utils/hotkeys";

interface Props {
  mode: Mode;
  disabled: boolean;
  settings: AppSettings;
  /** F8：界面语言（缺省中文） */
  locale?: Locale;
  /** F12：mode_c 时附带原歌词独立字段（不再拼字符串） */
  onGenerate: (userInput: string, extra?: { originalLyrics: string }) => void;
  /** F14：Cmd/Ctrl+K 聚焦时由 App 层调用 */
  inputRef?: React.RefObject<HTMLTextAreaElement | null>;
}

export default function InputPanel({ mode, disabled, settings, onGenerate, inputRef, locale }: Props) {
  const [lyrics, setLyrics] = useState("");
  const [inspiration, setInspiration] = useState("");
  const [originalLyrics, setOriginalLyrics] = useState("");
  const [newTheme, setNewTheme] = useState("");
  /** F14：拖拽提示（非文本文件/读取失败时显示） */
  const [dropMsg, setDropMsg] = useState("");
  const localRef = useRef<HTMLTextAreaElement | null>(null);
  /** 当前模式主输入框（F14 聚焦 + 拖拽填充目标） */
  const mainRef = inputRef ?? localRef;

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

  /** F14：Cmd/Ctrl+Enter 提交（复用 handleSubmit 的全部校验） */
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (isSubmitHotkey(e) && canSubmit) {
      e.preventDefault();
      handleSubmit();
    }
  };

  /** F14：拖拽 .txt/.lrc 到输入区 → 读取并填入当前模式主输入框 */
  const handleDrop = async (e: React.DragEvent) => {
    e.preventDefault();
    setDropMsg("");
    const files = Array.from(e.dataTransfer.files ?? []);
    if (files.length === 0) {
      // Tauri WebView 拖拽走系统路径而非 File 对象时，尝试读 dataTransfer 文本
      const text = e.dataTransfer.getData("text/plain");
      if (text) {
        fillMain(text);
        return;
      }
      return;
    }
    const f = files[0];
    // Web File 有 path（Tauri 环境）则直接读文件；否则读文本内容
    const path = (f as unknown as { path?: string }).path;
    try {
      if (path && isImportableFile(path)) {
        fillMain(await readTextFile(path));
      } else if (path) {
        setDropMsg(t(locale, "input.drop.only"));
      } else if (isImportableFile(f.name)) {
        fillMain(await f.text());
      } else {
        setDropMsg(t(locale, "input.drop.only"));
      }
    } catch {
      setDropMsg(t(locale, "input.drop.fail"));
    } finally {
      setTimeout(() => setDropMsg(""), 3000);
    }
  };

  /** 按当前模式填充主输入框 */
  const fillMain = (text: string) => {
    if (mode === "mode_a") setLyrics(text);
    else if (mode === "mode_c") setOriginalLyrics(text);
    else setInspiration(text);
    mainRef.current?.focus();
  };

  // Tauri 系统级拖拽（文件从系统拖入窗口）：监听路径列表
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type === "drop" && event.payload.paths.length > 0) {
          const p = event.payload.paths[0];
          if (isImportableFile(p)) {
            readTextFile(p)
              .then(fillMain)
              .catch(() => setDropMsg("文件读取失败"));
          } else {
            setDropMsg(t(locale, "input.drop.only"));
            setTimeout(() => setDropMsg(""), 3000);
          }
        }
      })
      .then((fn) => { unlisten = fn; })
      .catch(() => { /* 非 Tauri 环境（浏览器预览）无拖拽事件，静默跳过 */ });
    return () => { unlisten?.(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode]);

  const inputClass = "w-full bg-surface-1/60 backdrop-blur-sm border border-border/40 rounded-xl px-3 py-2.5 text-[13px] text-text-1 placeholder:text-text-muted/30 resize-y focus:outline-none focus:border-brand-500/50 focus:ring-1 focus:ring-brand-500/20 transition-all duration-150";
  const labelClass = "flex items-center gap-1.5 text-[11px] text-text-muted mb-1.5";

  return (
    <div className="space-y-3" onDrop={handleDrop} onDragOver={(e) => e.preventDefault()}>
      {mode === "mode_a" && (
        <div>
          <label className={labelClass}><IconSparkles size={13} /> {t(locale, "input.lyrics")}</label>
          <textarea ref={mainRef} value={lyrics} onChange={e => setLyrics(e.target.value)}
            onKeyDown={handleKeyDown}
            className={`${inputClass} h-32`} placeholder={`${t(locale, "input.ph.lyrics")}${t(locale, "input.ph.suffix.submit")}，${t(locale, "input.ph.suffix.drop")}`} disabled={disabled} />
        </div>
      )}

      {mode === "mode_c" && (
        <>
          <div>
            <label className={labelClass}><IconEdit size={13} /> {t(locale, "input.original")}</label>
            <textarea value={originalLyrics} onChange={e => setOriginalLyrics(e.target.value)}
              onKeyDown={handleKeyDown}
              className={`${inputClass} h-24`} placeholder={`${t(locale, "input.ph.original")}（${t(locale, "input.ph.suffix.drop")}`} disabled={disabled} />
          </div>
          <div>
            <label className={labelClass}><IconSparkles size={13} /> {t(locale, "input.theme")}</label>
            <textarea ref={mainRef} value={newTheme} onChange={e => setNewTheme(e.target.value)}
              onKeyDown={handleKeyDown}
              className={`${inputClass} h-20`} placeholder={`${t(locale, "input.ph.theme")}${t(locale, "input.ph.suffix.submit")}）`} disabled={disabled} />
          </div>
        </>
      )}

      {(mode === "mode_b" || mode === "mode_d") && (
        <div>
          <label className={labelClass}><IconSparkles size={13} /> {mode === "mode_b" ? t(locale, "input.inspiration.b") : t(locale, "input.inspiration.d")}</label>
          <textarea ref={mainRef} value={inspiration} onChange={e => setInspiration(e.target.value)}
            onKeyDown={handleKeyDown}
            className={`${inputClass} h-24`} disabled={disabled}
            placeholder={mode === "mode_b" ? `${t(locale, "input.ph.inspiration.b")}${t(locale, "input.ph.suffix.submit")}，${t(locale, "input.ph.suffix.drop")}` : `${t(locale, "input.ph.inspiration.d")}${t(locale, "input.ph.suffix.submit")}，${t(locale, "input.ph.suffix.drop")}`} />
        </div>
      )}

      {dropMsg && (
        <p className="text-[11px] text-warning">{dropMsg}</p>
      )}

      <button onClick={handleSubmit} disabled={!canSubmit}
        className="w-full py-2.5 rounded-xl text-[13px] font-medium flex items-center justify-center gap-2
                   brand-gradient-btn text-white active:scale-[0.98]
                   disabled:opacity-40 disabled:cursor-not-allowed disabled:shadow-none
                   transition-all duration-150">
        <IconSend size={15} />
        {disabled ? t(locale, "input.generating") : t(locale, "input.generate")}
      </button>
    </div>
  );
}

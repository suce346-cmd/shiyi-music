import { useState, useRef } from "react";
import { IconSettings, IconHistory, IconSparkles } from "@tabler/icons-react";
import ModeSelector from "./components/ModeSelector";
import InputPanel from "./components/InputPanel";
import ResultPanel from "./components/ResultPanel";
import StatusIndicator from "./components/StatusIndicator";
import HistoryPanel from "./components/HistoryPanel";
import { useSettings } from "./hooks/useSettings";
import { refinePrompt } from "./hooks/useLLM";
import type { Mode, ChatMessage, HistoryEntry, LLMResponse, LLMStatus } from "./types";

const HISTORY_KEY = "suno-prompt-history";

function loadHistory(): HistoryEntry[] {
  try { return JSON.parse(localStorage.getItem(HISTORY_KEY) || "[]"); } catch { return []; }
}
function saveHistory(entries: HistoryEntry[]) {
  localStorage.setItem(HISTORY_KEY, JSON.stringify(entries));
}

let idCounter = Date.now();
function newId() { return `${++idCounter}`; }

export default function App() {
  const [mode, setMode] = useState<Mode>("mode_d");
  const [status, setStatus] = useState<LLMStatus>("idle");
  const [result, setResult] = useState<LLMResponse | null>(null);
  const [streamText, setStreamText] = useState("");
  const [errorMessage, setErrorMessage] = useState("");
  const [history, setHistory] = useState<ChatMessage[]>([]);
  const [lastUserInput, setLastUserInput] = useState("");
  const [historyEntries, setHistoryEntries] = useState<HistoryEntry[]>(loadHistory);
  const historyRef = useRef(historyEntries);
  historyRef.current = historyEntries;
  const [showHistory, setShowHistory] = useState(false);
  const [historyView, setHistoryView] = useState<HistoryEntry | null>(null);
  const { settings, updateSettings, showSettings, setShowSettings } = useSettings();

  const handleRefine = async (feedback: string) => {
    if (!feedback.trim()) return;
    setStatus("loading");
    setStreamText("");
    const updatedHistory = [...history, { role: "user", content: lastUserInput }, { role: "assistant", content: result?.raw || "" }];
    setHistory(updatedHistory);
    try {
      const newResult = await refinePrompt(mode, updatedHistory, feedback, settings, (text) => {
        setStreamText(text); setStatus("streaming");
      });
      setResult(newResult); setStatus("done");
      saveToHistory(`优化: ${feedback}`, newResult.raw);
    } catch (e) {
      setStatus("error"); setErrorMessage(String(e));
    }
  };

  const saveToHistory = (input: string, output: string) => {
    const entry: HistoryEntry = { id: newId(), mode, input, output, timestamp: Date.now() };
    const updated = [entry, ...historyRef.current];
    setHistoryEntries(updated); saveHistory(updated);
  };
  const deleteHistory = (id: string) => { const u = historyEntries.filter(e => e.id !== id); setHistoryEntries(u); saveHistory(u); };
  const clearHistory = () => { setHistoryEntries([]); saveHistory([]); };
  const selectHistory = (entry: HistoryEntry) => { setHistoryView(entry); setShowHistory(false); };

  return (
    <div className="min-h-screen bg-gradient-to-b from-surface-0 via-surface-1 to-surface-0">
      {/* Decorative header gradient */}
      <div className="absolute top-0 left-0 right-0 h-48 bg-gradient-to-b from-brand-500/5 to-transparent pointer-events-none" />

      <div className="relative max-w-2xl mx-auto px-4 py-8">
        {/* Header */}
        <header className="flex items-center justify-between mb-8">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-gradient-to-br from-brand-500 to-brand-700 flex items-center justify-center shadow-lg shadow-brand-500/20">
              <IconSparkles size={20} className="text-white" />
            </div>
            <div>
              <h1 className="text-lg font-semibold text-text-1 tracking-tight">shiyi音乐</h1>
              <p className="text-xs text-text-muted">AI 音乐提示词生成器</p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <button onClick={() => { setShowHistory(true); setHistoryView(null); }}
              className="flex items-center gap-1.5 px-3 py-2 rounded-xl text-sm text-text-2
                         bg-surface-2 hover:bg-border border border-border/50
                         transition-all duration-150">
              <IconHistory size={16} /> 历史
            </button>
            <button onClick={() => setShowSettings(!showSettings)}
              className="flex items-center gap-1.5 px-3 py-2 rounded-xl text-sm text-text-2
                         bg-surface-2 hover:bg-border border border-border/50
                         transition-all duration-150">
              <IconSettings size={16} />
            </button>
          </div>
        </header>

        {/* Settings Drawer */}
        {showSettings && (
          <div className="mb-6 p-5 bg-surface-2 rounded-2xl border border-border shadow-lg
                          animate-[fade_200ms_ease] space-y-3">
            <h3 className="text-sm font-medium text-text-1">API 设置</h3>
            <div>
              <label className="text-xs text-text-muted block mb-1.5">API Key</label>
              <input type="password" value={settings.apiKey}
                onChange={e => updateSettings({ apiKey: e.target.value })}
                className="w-full bg-surface-0 border border-border rounded-xl px-3.5 py-2.5 text-sm
                           text-text-1 placeholder:text-text-muted/50 focus:outline-none
                           focus:ring-2 focus:ring-brand-500/30 focus:border-brand-500
                           transition-all duration-150" placeholder="sk-..." />
            </div>
            <div className="flex gap-3">
              <div className="flex-1">
                <label className="text-xs text-text-muted block mb-1.5">模型</label>
                <input value={settings.model} onChange={e => updateSettings({ model: e.target.value })}
                  className="w-full bg-surface-0 border border-border rounded-xl px-3.5 py-2.5 text-sm
                             text-text-1 focus:outline-none focus:ring-2 focus:ring-brand-500/30" />
              </div>
              <div className="flex-1">
                <label className="text-xs text-text-muted block mb-1.5">API 地址</label>
                <input value={settings.baseUrl} onChange={e => updateSettings({ baseUrl: e.target.value })}
                  className="w-full bg-surface-0 border border-border rounded-xl px-3.5 py-2.5 text-sm
                             text-text-1 focus:outline-none focus:ring-2 focus:ring-brand-500/30" />
              </div>
            </div>
          </div>
        )}

        {/* Mode Selection */}
        <ModeSelector mode={mode} onChange={(m) => { setMode(m); setResult(null); setHistory([]); setLastUserInput(""); setStreamText(""); }} />

        {/* History View */}
        {historyView && (
          <div className="mb-4 p-3.5 bg-brand-500/8 border border-brand-500/20 rounded-xl
                          flex items-center justify-between">
            <span className="text-xs text-brand-700">
              查看历史 · {historyView.mode === "mode_a" ? "Mode A" : "Mode D"} · {new Date(historyView.timestamp).toLocaleString("zh-CN")}
            </span>
            <button onClick={() => setHistoryView(null)}
              className="text-xs text-brand-600 hover:text-brand-700 font-medium">关闭</button>
          </div>
        )}

        {/* Input Panel */}
        <InputPanel mode={mode} disabled={status === "loading" || status === "streaming"}
          settings={settings} onStatusChange={setStatus} onUserInputChange={setLastUserInput}
          onResultChange={(r) => { if (r) setResult(r); setStreamText(""); }}
          onResultSave={(input, result) => saveToHistory(input, result.raw)}
          onError={(msg) => setErrorMessage(msg)}
          onStreamUpdate={(text) => setStreamText(text)} />

        {/* Status */}
        {status !== "idle" && <StatusIndicator status={status} errorMessage={errorMessage} />}

        {/* Result */}
        {historyView ? (
          <ResultPanel result={{ raw: historyView.output }} streamText="" status="done" onRefine={() => {}} />
        ) : (result || streamText) ? (
          <ResultPanel result={result} streamText={streamText} status={status} onRefine={handleRefine} />
        ) : null}

        {/* History Panel */}
        {showHistory && (
          <HistoryPanel entries={historyEntries} onDelete={deleteHistory} onClear={clearHistory}
            onSelect={selectHistory} onClose={() => setShowHistory(false)} />
        )}
      </div>
    </div>
  );
}
import { useState, useRef, useCallback } from "react";
import { IconSettings, IconHistory, IconSparkles } from "@tabler/icons-react";
import ModeSelector from "./components/ModeSelector";
import InputPanel from "./components/InputPanel";
import ResultPanel from "./components/ResultPanel";
import StatusIndicator from "./components/StatusIndicator";
import HistoryPanel from "./components/HistoryPanel";
import { useSettings } from "./hooks/useSettings";
import { generatePrompt, refinePrompt } from "./hooks/useLLM";
import type { Mode, ChatMessage, ChatTurn, HistoryEntry, LLMStatus } from "./types";
import { MODE_LABELS } from "./types";

const HISTORY_KEY = "suno-prompt-history";

function loadHistory(): HistoryEntry[] {
  try { return JSON.parse(localStorage.getItem(HISTORY_KEY) || "[]"); } catch { return []; }
}
function saveHistory(entries: HistoryEntry[]) {
  localStorage.setItem(HISTORY_KEY, JSON.stringify(entries));
}

function newId() { return crypto.randomUUID(); }

export default function App() {
  const [mode, setMode] = useState<Mode>("mode_d");
  const [status, setStatus] = useState<LLMStatus>("idle");
  const [streamText, setStreamText] = useState("");
  const [errorMessage, setErrorMessage] = useState("");
  const [lastUserInput, setLastUserInput] = useState("");
  const [conversation, setConversation] = useState<ChatTurn[]>([]);
  const [historyEntries, setHistoryEntries] = useState<HistoryEntry[]>(loadHistory);
  const historyRef = useRef(historyEntries);
  historyRef.current = historyEntries;
  const chatHistoryRef = useRef<ChatMessage[]>([]);
  const [showHistory, setShowHistory] = useState(false);
  const [historyView, setHistoryView] = useState<HistoryEntry | null>(null);
  const [historyFilter, setHistoryFilter] = useState<Mode | "all">("all");
  const [lastFeedback, setLastFeedback] = useState("");
  const [currentHistoryId, setCurrentHistoryId] = useState<string | null>(null);
  const { settings, updateSettings, showSettings, setShowSettings } = useSettings();

  const saveToHistory = useCallback((input: string, output: string, conv: ChatTurn[], currentMode: Mode) => {
    const entry: HistoryEntry = { id: newId(), mode: currentMode, input, output, conversation: conv, timestamp: Date.now() };
    const updated = [entry, ...historyRef.current];
    setHistoryEntries(updated); saveHistory(updated);
  }, []);

  const updateHistoryEntry = useCallback((id: string, output: string, conv: ChatTurn[]) => {
    const updated = historyRef.current.map(e =>
      e.id === id ? { ...e, output, conversation: conv, timestamp: Date.now() } : e
    );
    setHistoryEntries(updated); saveHistory(updated);
  }, []);

  const handleGenerate = useCallback(async (userInput: string) => {
    setStatus("loading"); setStreamText(""); setErrorMessage("");
    setLastUserInput(userInput);
    chatHistoryRef.current = [];

    const callResult = await generatePrompt(mode, userInput, settings, (text) => {
      setStreamText(text); setStatus("streaming");
    });

    if (callResult.error) {
      setStatus("error"); setErrorMessage(callResult.error);
      if (callResult.partialText) setStreamText(callResult.partialText);
      return;
    }

    if (callResult.result) {
      let raw = callResult.result.raw;
      if (callResult.result.finish_reason === "length") {
        raw += "\n\n---\n⚠️ 输出因 token 上限被截断，建议重试或精简输入。";
      }
      const newTurns: ChatTurn[] = [
        { role: "user", content: userInput, timestamp: Date.now() },
        { role: "assistant", content: raw, timestamp: Date.now() },
      ];
      setConversation(newTurns);
      setStatus("done"); setStreamText("");
      chatHistoryRef.current = [
        { role: "user", content: userInput },
        { role: "assistant", content: raw },
      ];
      const entry: HistoryEntry = { id: newId(), mode, input: userInput, output: raw, conversation: newTurns, timestamp: Date.now() };
      setCurrentHistoryId(entry.id);
      const updated = [entry, ...historyRef.current];
      setHistoryEntries(updated); saveHistory(updated);
    }
  }, [mode, settings, saveToHistory]);

  const handleRefine = useCallback(async (feedback: string) => {
    if (!feedback.trim()) return;
    setStatus("loading"); setStreamText(""); setErrorMessage("");
    setLastFeedback(feedback);

    const currentHistory = [...chatHistoryRef.current];

    const callResult = await refinePrompt(mode, currentHistory, feedback, settings, (text) => {
      setStreamText(text); setStatus("streaming");
    });

    if (callResult.error) {
      setStatus("error"); setErrorMessage(callResult.error);
      if (callResult.partialText) setStreamText(callResult.partialText);
      return;
    }

    if (callResult.result) {
      let raw = callResult.result.raw;
      if (callResult.result.finish_reason === "length") {
        raw += "\n\n---\n⚠️ 输出因 token 上限被截断，建议重试或精简输入。";
      }
      const newTurns: ChatTurn[] = [
        ...conversation,
        { role: "user", content: feedback, timestamp: Date.now() },
        { role: "assistant", content: raw, timestamp: Date.now() },
      ];
      setConversation(newTurns);
      setStatus("done"); setStreamText("");
      chatHistoryRef.current = [
        ...currentHistory,
        { role: "user", content: feedback },
        { role: "assistant", content: raw },
      ];
      if (currentHistoryId) {
        updateHistoryEntry(currentHistoryId, raw, newTurns);
      } else {
        saveToHistory(conversation[0]?.content || lastUserInput, raw, newTurns, mode);
      }
    }
  }, [mode, settings, conversation, lastUserInput, saveToHistory, currentHistoryId, updateHistoryEntry]);

  const handleRetry = useCallback(async () => {
    if (status === "error" && lastFeedback) {
      await handleRefine(lastFeedback);
    } else if (status === "error" && lastUserInput) {
      await handleGenerate(lastUserInput);
    }
  }, [status, lastFeedback, lastUserInput, handleRefine, handleGenerate]);

  const deleteHistory = (id: string) => { const u = historyEntries.filter(e => e.id !== id); setHistoryEntries(u); saveHistory(u); };
  const clearHistory = () => { setHistoryEntries([]); saveHistory([]); };
  const selectHistory = (entry: HistoryEntry) => { setHistoryView(entry); setShowHistory(false); };

  const filteredHistory = historyFilter === "all" ? historyEntries : historyEntries.filter(e => e.mode === historyFilter);

  return (
    <div className="min-h-screen bg-gradient-to-b from-surface-0 via-surface-1 to-surface-0">
      <div className="absolute top-0 left-0 right-0 h-48 bg-gradient-to-b from-brand-500/5 to-transparent pointer-events-none" />

      <div className="relative max-w-2xl mx-auto px-4 py-8">
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
                         bg-surface-2 hover:bg-border border border-border/50 transition-all duration-150">
              <IconHistory size={16} /> 历史
            </button>
            <button onClick={() => setShowSettings(!showSettings)}
              className="flex items-center gap-1.5 px-3 py-2 rounded-xl text-sm text-text-2
                         bg-surface-2 hover:bg-border border border-border/50 transition-all duration-150">
              <IconSettings size={16} />
            </button>
          </div>
        </header>

        {showSettings && (
          <div className="mb-6 p-5 bg-surface-2 rounded-2xl border border-border shadow-lg animate-[fade_200ms_ease] space-y-3">
            <h3 className="text-sm font-medium text-text-1">API 设置</h3>
            <div>
              <label className="text-xs text-text-muted block mb-1.5">API Key</label>
              <input type="password" value={settings.apiKey}
                onChange={e => updateSettings({ apiKey: e.target.value })}
                className="w-full bg-surface-0 border border-border rounded-xl px-3.5 py-2.5 text-sm
                           text-text-1 placeholder:text-text-muted/50 focus:outline-none
                           focus:ring-2 focus:ring-brand-500/30 focus:border-brand-500 transition-all duration-150" placeholder="sk-..." />
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

        <ModeSelector mode={mode} onChange={(m) => {
          setMode(m); setStatus("idle"); setStreamText(""); setErrorMessage("");
          setConversation([]); chatHistoryRef.current = [];
          setLastUserInput(""); setLastFeedback("");
          setHistoryView(null); setCurrentHistoryId(null);
        }} />

        {historyView && (
          <div className="mb-4 p-3.5 bg-brand-500/8 border border-brand-500/20 rounded-xl flex items-center justify-between">
            <span className="text-xs text-brand-700">
              查看历史 · {MODE_LABELS[historyView.mode]} · {new Date(historyView.timestamp).toLocaleString("zh-CN")}
            </span>
            <button onClick={() => setHistoryView(null)}
              className="text-xs text-brand-600 hover:text-brand-700 font-medium">关闭</button>
          </div>
        )}

        <InputPanel mode={mode} disabled={status === "loading" || status === "streaming"}
          settings={settings} onGenerate={handleGenerate} />

        {status !== "idle" && <StatusIndicator status={status} errorMessage={errorMessage} onRetry={handleRetry} />}

        {historyView ? (
          <ResultPanel conversation={historyView.conversation || [{ role: "user", content: historyView.input, timestamp: historyView.timestamp }, { role: "assistant", content: historyView.output, timestamp: historyView.timestamp }]} streamText="" status="done" onRefine={() => {}} readOnly />
        ) : (conversation.length > 0 || streamText) ? (
          <ResultPanel conversation={conversation} streamText={streamText} status={status} onRefine={handleRefine} />
        ) : null}

        {showHistory && (
          <HistoryPanel entries={filteredHistory} allEntriesCount={historyEntries.length}
            filter={historyFilter} onFilterChange={setHistoryFilter}
            onDelete={deleteHistory} onClear={clearHistory}
            onSelect={selectHistory} onClose={() => setShowHistory(false)} />
        )}
      </div>
    </div>
  );
}

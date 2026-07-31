import { useState, useRef, useCallback } from "react";
import { IconSettings, IconHistory, IconSparkles, IconEye, IconEyeOff, IconPlug, IconLoader } from "@tabler/icons-react";
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
  const [showApiKey, setShowApiKey] = useState(false);
  const [testingApi, setTestingApi] = useState(false);
  const [testResult, setTestResult] = useState<"ok" | "fail" | null>(null);
  const { settings, updateSettings, showSettings, setShowSettings } = useSettings();

  const handleTestApi = useCallback(async () => {
    setTestingApi(true); setTestResult(null);
    try {
      const url = `${settings.baseUrl.replace(/\/$/, "")}/chat/completions`;
      const res = await fetch(url, {
        method: "POST",
        headers: { "Content-Type": "application/json", "Authorization": `Bearer ${settings.apiKey}` },
        body: JSON.stringify({ model: settings.model, messages: [{ role: "user", content: "hi" }], max_tokens: 1 }),
        signal: AbortSignal.timeout(10000),
      });
      setTestResult(res.ok ? "ok" : "fail");
    } catch {
      setTestResult("fail");
    } finally {
      setTestingApi(false);
    }
  }, [settings]);

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
    <div className="h-screen flex flex-col overflow-hidden">
      {/* Top bar */}
      <header className="flex items-center justify-between px-4 py-2.5 border-b border-border/40 shrink-0 glass-panel">
        <div className="flex items-center gap-2.5">
          <div className="w-8 h-8 rounded-xl brand-gradient-btn flex items-center justify-center">
            <IconSparkles size={16} className="text-white" />
          </div>
          <span className="text-[14px] font-semibold brand-gradient-text">shiyi音乐</span>
        </div>
        <div className="flex items-center gap-1.5">
          <button onClick={() => { setShowHistory(true); setHistoryView(null); }}
            className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-[12px] text-text-2
                       bg-surface-2/60 hover:bg-surface-3/80 border border-border/40 transition-all duration-150 active:scale-95">
            <IconHistory size={14} /> 历史
          </button>
          <button onClick={() => { setShowSettings(!showSettings); setTestResult(null); }}
            className="flex items-center justify-center w-8 h-8 rounded-lg text-text-2
                       bg-surface-2/60 hover:bg-surface-3/80 border border-border/40 transition-all duration-150 active:scale-95">
            <IconSettings size={15} />
          </button>
        </div>
      </header>

      {/* Settings dropdown */}
      {showSettings && (
        <div className="absolute right-4 top-14 z-40 w-80 p-3.5 glass-panel rounded-2xl border border-border/40
                        animate-[fade_200ms_ease] space-y-3">
          <h3 className="text-[13px] font-medium text-text-1">API 设置</h3>
          <div>
            <div className="flex items-center justify-between mb-1">
              <label className="text-[11px] text-text-muted">API Key</label>
              <button onClick={() => setShowApiKey(!showApiKey)}
                className="text-text-muted hover:text-text-2 transition-colors">
                {showApiKey ? <IconEyeOff size={13} /> : <IconEye size={13} />}
              </button>
            </div>
            <input type={showApiKey ? "text" : "password"} value={settings.apiKey}
              onChange={e => { updateSettings({ apiKey: e.target.value }); setTestResult(null); }}
              className="w-full bg-surface-0 border border-border/60 rounded-lg px-3 py-2 text-[13px]
                         text-text-1 placeholder:text-text-muted/30 focus:outline-none
                         focus:border-brand-500/40 focus:ring-1 focus:ring-brand-500/20
                         transition-all duration-150" placeholder="sk-..." />
          </div>
          <div className="flex gap-2">
            <div className="flex-1">
              <label className="text-[11px] text-text-muted block mb-1">模型</label>
              <input value={settings.model} onChange={e => { updateSettings({ model: e.target.value }); setTestResult(null); }}
                className="w-full bg-surface-0 border border-border/60 rounded-lg px-3 py-2 text-[13px]
                           text-text-1 focus:outline-none focus:border-brand-500/40 focus:ring-1 focus:ring-brand-500/20
                           transition-all duration-150" />
            </div>
            <div className="flex-1">
              <label className="text-[11px] text-text-muted block mb-1">API 地址</label>
              <input value={settings.baseUrl} onChange={e => { updateSettings({ baseUrl: e.target.value }); setTestResult(null); }}
                className="w-full bg-surface-0 border border-border/60 rounded-lg px-3 py-2 text-[13px]
                           text-text-1 focus:outline-none focus:border-brand-500/40 focus:ring-1 focus:ring-brand-500/20
                           transition-all duration-150" />
            </div>
          </div>
          <button onClick={handleTestApi} disabled={testingApi || !settings.apiKey}
            className="w-full flex items-center justify-center gap-1.5 py-2 rounded-lg text-[12px] font-medium
                       bg-surface-2 hover:bg-surface-3 border border-border/50 text-text-2
                       disabled:opacity-50 transition-all duration-150 active:scale-[0.98]">
            {testingApi ? <IconLoader size={13} className="animate-spin" /> : <IconPlug size={13} />}
            {testResult === "ok" && "连接成功"}
            {testResult === "fail" && "连接失败，请检查"}
            {testResult === null && (testingApi ? "测试中..." : "测试连接")}
          </button>
        </div>
      )}

      {/* Main content */}
      <div className="flex-1 flex overflow-hidden">
        {/* Left panel */}
        <div className="w-[300px] shrink-0 flex flex-col border-r border-border/40 overflow-y-auto">
          <div className="p-3 border-b border-border/50">
            <ModeSelector mode={mode} onChange={(m) => {
              setMode(m); setStatus("idle"); setStreamText(""); setErrorMessage("");
              setConversation([]); chatHistoryRef.current = [];
              setLastUserInput(""); setLastFeedback("");
              setHistoryView(null); setCurrentHistoryId(null);
            }} />
          </div>

          <div className="p-3 flex-1">
            {historyView && (
              <div className="mb-3 p-2.5 bg-brand-500/8 border border-brand-500/20 rounded-lg flex items-center justify-between">
                <span className="text-[11px] text-brand-400 truncate">
                  {MODE_LABELS[historyView.mode]} · {new Date(historyView.timestamp).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}
                </span>
                <button onClick={() => setHistoryView(null)}
                  className="text-[11px] text-brand-400 hover:text-brand-300 font-medium shrink-0 ml-2">关闭</button>
              </div>
            )}

            <InputPanel mode={mode} disabled={status === "loading" || status === "streaming"}
              settings={settings} onGenerate={handleGenerate} />
          </div>
        </div>

        {/* Right panel */}
        <div className="flex-1 flex flex-col overflow-hidden">
          {status !== "idle" && (
            <div className="shrink-0">
              <StatusIndicator status={status} errorMessage={errorMessage} onRetry={handleRetry} />
            </div>
          )}

          <div className="flex-1 overflow-y-auto p-4">
            {historyView ? (
              <ResultPanel conversation={historyView.conversation || [
                { role: "user", content: historyView.input, timestamp: historyView.timestamp },
                { role: "assistant", content: historyView.output, timestamp: historyView.timestamp }
              ]} streamText="" status="done" onRefine={() => {}} readOnly />
            ) : (conversation.length > 0 || streamText) ? (
              <ResultPanel conversation={conversation} streamText={streamText} status={status} onRefine={handleRefine} />
            ) : (
              <div className="flex flex-col items-center justify-center h-full text-text-muted">
                <div className="w-16 h-16 rounded-2xl glass-panel flex items-center justify-center mb-3">
                  <IconSparkles size={28} className="text-brand-400/50" />
                </div>
                <p className="text-[13px]">在左侧输入内容开始生成</p>
              </div>
            )}
          </div>
        </div>
      </div>

      {showHistory && (
        <HistoryPanel entries={filteredHistory} allEntriesCount={historyEntries.length}
          filter={historyFilter} onFilterChange={setHistoryFilter}
          onDelete={deleteHistory} onClear={clearHistory}
          onSelect={selectHistory} onClose={() => setShowHistory(false)} />
      )}
    </div>
  );
}

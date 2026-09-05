import { useState, useRef, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { IconSettings, IconHistory, IconSparkles, IconEye, IconEyeOff, IconPlug, IconLoader } from "@tabler/icons-react";
import ModeSelector from "./components/ModeSelector";
import InputPanel from "./components/InputPanel";
import ResultPanel from "./components/ResultPanel";
import StatusIndicator from "./components/StatusIndicator";
import HistoryPanel from "./components/HistoryPanel";
import RoundtablePanel from "./components/RoundtablePanel";
import { useSettingsWithSecrets } from "./hooks/useSettings";
import { usePipeline, ROLE_NAMES } from "./hooks/usePipeline";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import type { Mode, ChatMessage, ChatTurn, HistoryEntry, LLMStatus, ExpertCard, PipelineRoleKey } from "./types";
import { MODE_LABELS, errText } from "./types";

/** 角色 emoji（设置面板展示用，与后端 PipelineRole::emoji 对齐） */
const ROLE_EMOJIS: Record<PipelineRoleKey, string> = {
  host: "👑",
  auditor: "🔍",
  emotion: "🎭",
  lyricist: "📝",
  reviser: "✍️",
  producer: "🎤",
  style_analyst: "🔥",
};

const HISTORY_KEY = "suno-prompt-history";

function loadHistory(): HistoryEntry[] {
  try {
    const parsed = JSON.parse(localStorage.getItem(HISTORY_KEY) || "[]");
    // 损坏数据防御：非数组（旧版写入 {}/null）直接丢弃，防渲染白屏
    return Array.isArray(parsed) ? parsed : [];
  } catch { return []; }
}
function saveHistory(entries: HistoryEntry[]) {
  // M9：配额超限时降级（丢弃 conversation 只存输入/输出），失败不阻断生成流程
  try {
    localStorage.setItem(HISTORY_KEY, JSON.stringify(entries));
  } catch {
    try {
      const slim = entries.map(({ conversation: _c, ...rest }) => rest);
      localStorage.setItem(HISTORY_KEY, JSON.stringify(slim));
    } catch {
      // 持久化失败：静默放弃（生成流程不受影响）
    }
  }
}
function newId() {
  // 低版本 WebKitGTK 可能缺失 randomUUID（低危修复）
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  return `id-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
}

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
  const [showRoleApi, setShowRoleApi] = useState(true); // 默认展开角色级 API（用户反馈看不到）
  const [expandedRole, setExpandedRole] = useState<PipelineRoleKey | null>(null);
  const { settings, updateSettings, showSettings, setShowSettings, secretsReady } = useSettingsWithSecrets();
  const pipeline = usePipeline();
  const [detailExpert, setDetailExpert] = useState<ExpertCard | null>(null);
  /** llm-chunk 单例监听（H4：任何时刻最多一个，注册前先清旧的） */
  const llmUnlistenRef = useRef<UnlistenFn | null>(null);
  /** run 递增 token：切模式/新 run 后，过期 run 的结果一律丢弃（H4） */
  const runTokenRef = useRef(0);
  /** 本轮生成/优化的专家发言记录（生成结束时合并进对话流，避免被覆盖） */
  const speechLogRef = useRef<ChatTurn[]>([]);

  /** 注册 llm-chunk 单例监听（失败抛错由调用方 catch，H5 修复：不卡死） */
  const ensureLlmListener = useCallback(async () => {
    if (llmUnlistenRef.current) {
      await llmUnlistenRef.current();
      llmUnlistenRef.current = null;
    }
    const token = runTokenRef.current; // 捕获当前 run（切模式/新 run 后过期 chunk 丢弃）
    const un = await listen<{ content: string }>("llm-chunk", (event) => {
      if (token !== runTokenRef.current) return; // 过期 run 的流式 chunk 丢弃
      setStreamText((prev) => prev + event.payload.content);
      setStatus("streaming");
    });
    llmUnlistenRef.current = un;
  }, []);

  // 卸载时清理单例监听（H5）
  useEffect(() => {
    return () => {
      runTokenRef.current++; // 作废在途 run
      if (llmUnlistenRef.current) {
        llmUnlistenRef.current();
        llmUnlistenRef.current = null;
      }
    };
  }, []);

  /** 更新某角色的 API 覆盖（空值/全空格 = 清空该字段继承全局；全空 = 移除该角色条目） */
  const updateRoleOverride = useCallback((role: PipelineRoleKey, field: "model" | "api_key" | "base_url", value: string) => {
    const trimmed = value.trim();
    const current = settings.roleOverrides?.[role] ?? {};
    const next = { ...current, [field]: trimmed || undefined };
    const cleaned = next.model || next.api_key || next.base_url ? next : undefined;
    const roleOverrides = { ...settings.roleOverrides };
    if (cleaned) {
      roleOverrides[role] = cleaned;
    } else {
      delete roleOverrides[role];
    }
    updateSettings({ roleOverrides });
    setTestResult(null);
  }, [settings, updateSettings]);

  // 模式切换时重置圆桌状态（围坐角色跟随当前模式阵容）
  useEffect(() => {
    pipeline.reset(mode);
    setDetailExpert(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode]);

  /** 全局测试连接：走 Rust 命令（绕过 WebView CORS，前端 fetch 跨域会被拦） */
  const handleTestApi = useCallback(async () => {
    setTestingApi(true); setTestResult(null);
    try {
      await invoke<string>("test_api", {
        baseUrl: settings.baseUrl,
        apiKey: settings.apiKey,
        model: settings.model,
      });
      setTestResult("ok");
    } catch {
      setTestResult("fail");
    } finally {
      setTestingApi(false);
    }
  }, [settings]);

  /** 角色级测试状态（idle/testing/ok/fail） */
  const [roleTestStates, setRoleTestStates] = useState<Record<string, "idle" | "testing" | "ok" | "fail">>({});

  // 配置变更后旧测试徽标失效（✓/✗ 与当前配置不符）——settings 任何字段变化都重置
  useEffect(() => {
    setRoleTestStates({});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings]);
  /** 当前配置快照：测试请求 resolve 时对比，配置已变则结果过期丢弃（防旧结果写入新配置旁） */
  const settingsRef = useRef(settings);
  settingsRef.current = settings;

  /** 测试某角色的生效配置：覆盖字段缺省 fallback 全局 */
  const handleTestRoleApi = useCallback(async (role: PipelineRoleKey) => {
    const ov = settings.roleOverrides?.[role] ?? {};
    const baseUrl = ov.base_url || settings.baseUrl;
    const apiKey = ov.api_key || settings.apiKey;
    const model = ov.model || settings.model;
    if (!apiKey) {
      setRoleTestStates((p) => ({ ...p, [role]: "fail" }));
      return;
    }
    const snap = { baseUrl, apiKey, model };
    setRoleTestStates((p) => ({ ...p, [role]: "testing" }));
    try {
      await invoke<string>("test_api", { baseUrl, apiKey, model });
      // 测试期间配置被改（effect 已清徽标）→ 结果过期，丢弃
      const cur = settingsRef.current;
      if (cur.apiKey !== snap.apiKey || cur.model !== snap.model || cur.baseUrl !== snap.baseUrl) return;
      setRoleTestStates((p) => ({ ...p, [role]: "ok" }));
    } catch {
      const cur = settingsRef.current;
      if (cur.apiKey !== snap.apiKey || cur.model !== snap.model || cur.baseUrl !== snap.baseUrl) return;
      setRoleTestStates((p) => ({ ...p, [role]: "fail" }));
    }
  }, [settings]);

  const saveToHistory = useCallback((input: string, output: string, conv: ChatTurn[], currentMode: Mode, usage?: { prompt_tokens: number; completion_tokens: number }) => {
    const entry: HistoryEntry = { id: newId(), mode: currentMode, input, output, conversation: conv, usage, timestamp: Date.now() };
    const updated = [entry, ...historyRef.current];
    setHistoryEntries(updated); saveHistory(updated);
  }, []);

  const updateHistoryEntry = useCallback((id: string, output: string, conv: ChatTurn[], usage?: { prompt_tokens: number; completion_tokens: number }) => {
    const updated = historyRef.current.map(e =>
      e.id === id ? { ...e, output, conversation: conv, usage: usage ?? e.usage, timestamp: Date.now() } : e
    );
    setHistoryEntries(updated); saveHistory(updated);
  }, []);

  const handleGenerate = useCallback(async (userInput: string, extra?: { originalLyrics: string }) => {
    const token = ++runTokenRef.current; // 作废旧 run（H4）
    setStatus("loading"); setStreamText(""); setErrorMessage("");
    // F12：mode_c 的 userInput 即新主题（原歌词走独立字段）；历史展示用拼接文本保留上下文
    const displayInput = mode === "mode_c" && extra
      ? `原歌词：\n${extra.originalLyrics}\n\n新主题：\n${userInput}`
      : userInput;
    setLastUserInput(displayInput);
    chatHistoryRef.current = [];
    speechLogRef.current = [];
    setConversation([]);
    setDetailExpert(null);
    setHistoryView(null); // 生成时关闭历史面板：新结果不能被旧历史遮住

    // 四模式统一走流水线（A/B/C/D）
    try {
      await ensureLlmListener(); // H5：失败抛错进 catch，不再卡死
      // F12：原歌词独立字段直传（旧字符串拼接+正则拆分+P6 补丁整条退役）
      const originalLyrics = mode === "mode_c" ? extra?.originalLyrics : undefined;
      const raw = await pipeline.run({
        mode,
        userInput,
        settings,
        extra: undefined,
        originalLyrics,
        // 专家发言实时追加到对话流（记录进 speechLog，结束时合并，不进入 chatHistoryRef）
        onSpeech: (speech) => {
          // P5：阶段0流式结束后清空中间态流式文本（首个专家发言时），避免统领全文重复显示
          if (speechLogRef.current.length === 0) setStreamText("");
          speechLogRef.current.push(speech);
          setConversation((prev) => [...prev, speech]);
        },
      });
      if (token !== runTokenRef.current) return; // 过期 run 的结果丢弃（H4）
      const allTurns: ChatTurn[] = [
        { role: "user", content: displayInput, timestamp: Date.now() },
        ...speechLogRef.current,
        { role: "assistant", content: raw, timestamp: Date.now() },
      ];
      setConversation(allTurns);
      setStatus("done"); setStreamText("");
      chatHistoryRef.current = [
        { role: "user", content: displayInput },
        { role: "assistant", content: raw },
      ];
      const entry: HistoryEntry = { id: newId(), mode, input: displayInput, output: raw, conversation: allTurns, usage: { ...pipeline.usage }, timestamp: Date.now() };
      setCurrentHistoryId(entry.id);
      const updated = [entry, ...historyRef.current];
      setHistoryEntries(updated); saveHistory(updated);
    } catch (e) {
      if (token !== runTokenRef.current) return; // 过期 run 的错误丢弃（H4）
      setStatus("error"); setErrorMessage(errText(e));
    }
  }, [mode, settings, ensureLlmListener, pipeline]);

  const handleRefine = useCallback(async (feedback: string) => {
    if (!feedback.trim()) return;
    const token = ++runTokenRef.current; // 作废旧 run（H4）
    setStatus("loading"); setStreamText(""); setErrorMessage("");
    setLastFeedback(feedback);
    speechLogRef.current = [];
    setHistoryView(null); // 优化时同样关闭历史面板（与 handleGenerate 一致）

    const currentHistory = [...chatHistoryRef.current];

    let raw: string;
    try {
      await ensureLlmListener(); // H5：失败抛错进 catch，不再卡死
      // F12：lastUserInput 是展示用拼接文本（原歌词+新主题），从中拆出两部分直传。
      // 注意：这是展示文本的解析（用户可见格式，稳定），不是旧协议——新生成已不再依赖它。
      let originalLyrics: string | undefined;
      let input = lastUserInput;
      if (mode === "mode_c") {
        const m = lastUserInput.match(/原歌词：\n([\s\S]*)\n\n新主题：\n([\s\S]*)$/);
        if (m) {
          originalLyrics = m[1];
          input = m[2];
        }
      }
      // P1：取最后一条 assistant（重复 refine 时注入的是上一版而非初始版）
      const lastOutput = [...currentHistory].reverse().find((m) => m.role === "assistant")?.content || "";
      raw = await pipeline.refine({
        mode,
        userInput: input,
        lastOutput,
        feedback,
        settings,
        extra: undefined,
        originalLyrics,
        onSpeech: (speech) => {
          // P5：阶段0流式结束后清空中间态流式文本（首个专家发言时）
          if (speechLogRef.current.length === 0) setStreamText("");
          speechLogRef.current.push(speech);
          setConversation((prev) => [...prev, speech]);
        },
      });
    } catch (e) {
      if (token !== runTokenRef.current) return; // 过期 run 的错误丢弃（H4）
      setStatus("error"); setErrorMessage(errText(e));
      return;
    }
    if (token !== runTokenRef.current) return; // 过期 run 的结果丢弃（H4）

    const allTurns: ChatTurn[] = [
      ...conversation,
      ...speechLogRef.current,
      { role: "user", content: feedback, timestamp: Date.now() },
      { role: "assistant", content: raw, timestamp: Date.now() },
    ];
    setConversation(allTurns);
    setStatus("done"); setStreamText("");
    chatHistoryRef.current = [
      ...currentHistory,
      { role: "user", content: feedback },
      { role: "assistant", content: raw },
    ];
    if (currentHistoryId) {
      updateHistoryEntry(currentHistoryId, raw, allTurns, { ...pipeline.usage });
    } else {
      saveToHistory(conversation[0]?.content || lastUserInput, raw, allTurns, mode, { ...pipeline.usage });
    }
  }, [mode, settings, conversation, lastUserInput, saveToHistory, currentHistoryId, updateHistoryEntry, pipeline, ensureLlmListener]);

  const handleRetry = useCallback(async () => {
    if (status === "error" && lastFeedback) {
      await handleRefine(lastFeedback);
    } else if (status === "error" && lastUserInput) {
      // F12：重试走展示文本解析路径（handleGenerate 内部处理直传，此处传原始展示文本由其二次解析）
      // 注意：mode_c 重试时 lastUserInput 为展示拼接文本，handleGenerate 会误判为新主题——
      // 因此 mode_c 重试改走 refine 路径（带上次反馈），避免原歌词丢失
      if (mode === "mode_c" && lastFeedback) {
        await handleRefine(lastFeedback);
      } else if (mode !== "mode_c") {
        await handleGenerate(lastUserInput);
      }
    }
  }, [status, lastFeedback, lastUserInput, handleRefine, handleGenerate, mode]);

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
            aria-label="API 设置"
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
                aria-label={showApiKey ? "隐藏 API Key" : "显示 API Key"}
                className="text-text-muted hover:text-text-2 transition-colors">
                {showApiKey ? <IconEyeOff size={13} /> : <IconEye size={13} />}
              </button>
            </div>
            <input type={showApiKey ? "text" : "password"} value={settings.apiKey}
              onChange={e => { updateSettings({ apiKey: e.target.value }); setTestResult(null); }}
              disabled={!secretsReady}
              className="w-full bg-surface-0 border border-border/60 rounded-lg px-3 py-2 text-[13px]
                         text-text-1 placeholder:text-text-muted/30 focus:outline-none
                         focus:border-brand-500/40 focus:ring-1 focus:ring-brand-500/20
                         transition-all duration-150 disabled:opacity-50" placeholder={secretsReady ? "sk-..." : "密钥加载中…"} />
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
          {/* 思考模式：后端按模型能力路由表自动注入厂商思考参数（DeepSeek/讯飞 → thinking；o 系/gpt-5 → reasoning_effort；未登记模型自动忽略） */}
          <div className="flex items-center justify-between gap-2 rounded-lg border border-border/40 bg-surface-0/40 px-3 py-2">
            <label htmlFor="thinking-mode" className="text-[11px] text-text-2 cursor-pointer select-none">
              思考模式
              <span className="block text-[10px] text-text-muted font-normal">先推理再回答，质量更稳但更慢；自动适配模型能力</span>
            </label>
            <input id="thinking-mode" type="checkbox" checked={settings.thinking}
              onChange={e => { updateSettings({ thinking: e.target.checked }); setTestResult(null); }}
              className="w-4 h-4 accent-brand-500 cursor-pointer shrink-0" />
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

          {/* 角色级 API 覆盖（可选）：不配置 = 所有角色共用全局；配置了生效单独 */}
          <div className="border-t border-border/40 pt-3">
            <button onClick={() => setShowRoleApi(!showRoleApi)}
              className="w-full flex items-center justify-between text-[11px] text-text-2 hover:text-text-1 transition-colors">
              <span>角色级 API（可选）</span>
              <span className="text-[10px] text-text-muted">{showRoleApi ? "收起 ▲" : "展开 ▼"}</span>
            </button>
            {showRoleApi && (
              <div className="mt-2 space-y-1.5 max-h-64 overflow-y-auto pr-0.5">
                {(Object.keys(ROLE_NAMES) as PipelineRoleKey[]).map((role) => (
                  <div key={role} className="rounded-lg border border-border/40 bg-surface-0/40 p-2">
                    <div className="flex items-center justify-between">
                      <span className="text-[11px] text-text-1">{ROLE_EMOJIS[role]} {ROLE_NAMES[role]}</span>
                      <div className="flex items-center gap-1.5">
                        {/* 角色级测试连接（测该角色的生效配置：覆盖缺省 fallback 全局） */}
                        <button
                          onClick={() => handleTestRoleApi(role)}
                          disabled={roleTestStates[role] === "testing"}
                          className={`flex items-center gap-0.5 px-1.5 py-0.5 rounded text-[10px] border transition-colors ${
                            roleTestStates[role] === "ok"
                              ? "text-success border-success/40 bg-success/10"
                              : roleTestStates[role] === "fail"
                                ? "text-danger border-danger/40 bg-danger/10"
                                : "text-text-muted hover:text-text-2 border-border/40 hover:border-border/70"
                          }`}
                          title="测试该角色的 API 配置（未配置字段继承全局）"
                        >
                          {roleTestStates[role] === "testing"
                            ? <IconLoader size={9} className="animate-spin" />
                            : roleTestStates[role] === "ok" ? "✓" : roleTestStates[role] === "fail" ? "✗" : "测试"}
                        </button>
                        <button onClick={() => setExpandedRole(expandedRole === role ? null : role)}
                          className="text-[10px] text-text-muted hover:text-text-2 transition-colors">
                          {expandedRole === role ? "收起" : "配置"}
                        </button>
                      </div>
                    </div>
                    {expandedRole === role && (
                      <div className="mt-1.5 space-y-1.5">
                        <input value={settings.roleOverrides?.[role]?.model ?? ""}
                          onChange={e => updateRoleOverride(role, "model", e.target.value)}
                          placeholder="模型（留空继承全局）"
                          className="w-full bg-surface-0 border border-border/60 rounded-lg px-2.5 py-1.5 text-[11px] text-text-1 placeholder:text-text-muted/30 focus:outline-none focus:border-brand-500/40 transition-all duration-150" />
                        <input value={settings.roleOverrides?.[role]?.api_key ?? ""}
                          onChange={e => updateRoleOverride(role, "api_key", e.target.value)}
                          placeholder="API Key（留空继承全局）"
                          className="w-full bg-surface-0 border border-border/60 rounded-lg px-2.5 py-1.5 text-[11px] text-text-1 placeholder:text-text-muted/30 focus:outline-none focus:border-brand-500/40 transition-all duration-150" />
                        <input value={settings.roleOverrides?.[role]?.base_url ?? ""}
                          onChange={e => updateRoleOverride(role, "base_url", e.target.value)}
                          placeholder="API 地址（留空继承全局）"
                          className="w-full bg-surface-0 border border-border/60 rounded-lg px-2.5 py-1.5 text-[11px] text-text-1 placeholder:text-text-muted/30 focus:outline-none focus:border-brand-500/40 transition-all duration-150" />
                      </div>
                    )}
                  </div>
                ))}
              </div>
            )}
            <p className="text-[9px] text-text-muted mt-1.5 leading-relaxed">
              不配置 = 所有角色共用全局 API；给某角色单独配置后，该角色用自己的，留空的字段仍继承全局
            </p>
          </div>
        </div>
      )}

      {/* Main content */}
      <div className="flex-1 flex flex-col overflow-hidden">
        {/* 模式选择（横向 tab 置顶） */}
        <div className="shrink-0 px-4 pt-3 pb-2 border-b border-border/40">
          <div className="flex items-center gap-3">
            <div className="flex-1 max-w-[720px]">
              <ModeSelector mode={mode} onChange={(m) => {
                runTokenRef.current++; // 作废在途 run（H4）
                if (llmUnlistenRef.current) { llmUnlistenRef.current(); llmUnlistenRef.current = null; }
                setMode(m); setStatus("idle"); setStreamText(""); setErrorMessage("");
                setConversation([]); chatHistoryRef.current = [];
                setLastUserInput(""); setLastFeedback("");
                setHistoryView(null); setCurrentHistoryId(null);
              }} />
            </div>
          </div>
        </div>

        {/* 工作区：圆桌舞台 + 结果 */}
        <div className="flex-1 flex overflow-hidden min-h-0">
          {/* 左侧：圆桌舞台（四模式统一渲染，M13：Mode C 不再隐藏进度） */}
          <div className="w-[380px] shrink-0 flex flex-col border-r border-border/40 overflow-y-auto">
            <div className="p-3">
              <RoundtablePanel
                experts={pipeline.experts}
                phase={pipeline.phase}
                validation={pipeline.validation}
                error={pipeline.error}
                active={pipeline.active}
                onOpenDetail={(e) => setDetailExpert(e)}
                currentStage={pipeline.currentStage}
                doneStages={pipeline.doneStages}
                usage={pipeline.usage}
              />
            </div>

            {detailExpert && (
              <div className="mx-3 mb-3 p-3 glass-panel rounded-xl border border-border/40 animate-[fade_200ms_ease]">
                <div className="flex items-center justify-between mb-1.5">
                  <span className="text-[12px] font-semibold text-text-1">
                    {detailExpert.emoji} {detailExpert.name}
                  </span>
                  <button onClick={() => setDetailExpert(null)} className="text-text-muted hover:text-text-2 text-[11px]">关闭</button>
                </div>
                <p className="text-[11px] text-text-2 leading-relaxed">
                  知识库：📚 {detailExpert.knowledge.length > 0 ? detailExpert.knowledge.join(", ") + ".csv" : "（无，凭专业判断）"}
                </p>
                <p className="text-[10px] text-text-muted mt-1">
                  {detailExpert.status === "working" && "当前正在分析…"}
                  {detailExpert.status === "done" && `已产出：${detailExpert.note || "（摘要见讨论区）"}`}
                  {detailExpert.status === "error" && `出错：${detailExpert.note || "（详见信息）"}`}
                  {detailExpert.status === "idle" && "等待分配任务…"}
                </p>
              </div>
            )}
          </div>

          {/* 右侧：输入 + 结果 */}
          <div className="flex-1 flex flex-col overflow-hidden">
            {status !== "idle" && (
              <div className="shrink-0">
                <StatusIndicator status={status} errorMessage={errorMessage} onRetry={handleRetry} onCancel={pipeline.cancel} />
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
                <div className="h-full flex flex-col items-center justify-center text-text-muted px-8">
                  <div className="w-16 h-16 rounded-2xl glass-panel flex items-center justify-center mb-3">
                    <IconSparkles size={28} className="text-brand-400/50" />
                  </div>
                  <p className="text-[13px] mb-4">输入内容开始生成，专家接力协作后出方案</p>
                  <div className="w-full max-w-[420px] glass-panel rounded-xl border border-border/40 p-4 space-y-2 text-[11px] leading-relaxed">
                    <p className="text-text-2 font-medium">🪑 流水线流程</p>
                    <p className="flex items-center gap-1.5"><span className="w-1.5 h-1.5 rounded-full bg-brand-400" /> 主持人全局统领产出方案（各模式完整指令）</p>
                    <p className="flex items-center gap-1.5"><span className="w-1.5 h-1.5 rounded-full bg-brand-400" /> 专业角色审改 → 校验员把关 → 主持人汇总分发，讨论收敛</p>
                    <p className="flex items-center gap-1.5"><span className="w-1.5 h-1.5 rounded-full bg-brand-400" /> 校验员按标准格式输出最终提示词包</p>
                  </div>
                </div>
              )}
            </div>

            {/* 输入区（底部固定） */}
            <div className="shrink-0 p-4 border-t border-border/40">
              {historyView && (
                <div className="mb-2 p-2 bg-brand-500/8 border border-brand-500/20 rounded-lg flex items-center justify-between">
                  <span className="text-[11px] text-brand-400 truncate">
                    {MODE_LABELS[historyView.mode]} · {new Date(historyView.timestamp).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}
                  </span>
                  <button onClick={() => setHistoryView(null)}
                    className="text-[11px] text-brand-400 hover:text-brand-300 font-medium shrink-0 ml-2">关闭</button>
                </div>
              )}
              <InputPanel key={mode} mode={mode} disabled={status === "loading" || status === "streaming"}
                settings={settings} onGenerate={handleGenerate} />
            </div>
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

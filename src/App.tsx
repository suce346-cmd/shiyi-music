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
import { usePipeline, ROLE_NAMES, ROLE_EMOJIS } from "./hooks/usePipeline";
import { useQueue, queueLabel, dequeueNext } from "./hooks/useQueue";
import QueuePanel from "./components/QueuePanel";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import type { Mode, ChatMessage, ChatTurn, HistoryEntry, LLMStatus, ExpertCard, PipelineRoleKey } from "./types";
import { MODE_LABELS, errText, isCancelledError } from "./types";
import { t } from "./i18n";

const HISTORY_KEY = "suno-prompt-history";
/** 历史迁移标记（localStorage → 文件一次性迁移） */
const HISTORY_MIGRATED_KEY = "suno-prompt-history-migrated";

/** 文件持久化写回（防抖由调用方控制；失败透出由调用方展示，不阻断生成） */
async function saveHistoryFile(entries: HistoryEntry[]): Promise<void> {
  await invoke("history_save", { entries });
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
  // 历史改走文件存储——初始空，启动 useEffect 从 history_load 回填 + 迁移旧 localStorage
  const [historyEntries, setHistoryEntries] = useState<HistoryEntry[]>([]);
  const historyRef = useRef(historyEntries);
  historyRef.current = historyEntries;
  /** 写入防抖 timer（500ms 合并连续写入） */
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** 计划持久化（防抖写文件；失败静默，生成流程不受影响） */
  const scheduleSave = useCallback((entries: HistoryEntry[]) => {
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    saveTimerRef.current = setTimeout(() => {
      saveHistoryFile(entries).catch(() => { /* 持久化失败静默（与旧配额降级语义一致） */ });
    }, 500);
  }, []);
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
  /** 生成队列（提交分流 + 顺序执行；后端零改动，纯前端调度） */
  const queue = useQueue();
  /** 当前运行队列项 id（高亮 + 取消归属；直接生成时为 null） */
  const [runningQueueId, setRunningQueueId] = useState<string | null>(null);
  const [detailExpert, setDetailExpert] = useState<ExpertCard | null>(null);
  /** Cmd/Ctrl+K 聚焦目标输入框 */
  const inputRef = useRef<HTMLTextAreaElement | null>(null);
  /** llm-chunk 单例监听（任何时刻最多一个，注册前先清旧的） */
  const llmUnlistenRef = useRef<UnlistenFn | null>(null);
  /** run 递增 token：切模式/新 run 后，过期 run 的结果一律丢弃 */
  const runTokenRef = useRef(0);
  /** 本轮生成/优化的专家发言记录（生成结束时合并进对话流，避免被覆盖） */
  const speechLogRef = useRef<ChatTurn[]>([]);

  /** 注册 llm-chunk 单例监听（失败抛错由调用方 catch，历史修复：不卡死） */
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

  // 卸载时清理单例监听
  useEffect(() => {
    return () => {
      runTokenRef.current++; // 作废在途 run
      if (llmUnlistenRef.current) {
        llmUnlistenRef.current();
        llmUnlistenRef.current = null;
      }
    };
  }, []);

  // 全局快捷键（输入框内 Enter 由 InputPanel 处理；这里处理 K/,(设置面板开关)）
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      if (!mod) return;
      // 聚焦输入框（输入框内已聚焦时不重复处理）
      if ((e.key === "k" || e.key === "K") && document.activeElement?.tagName !== "TEXTAREA") {
        e.preventDefault();
        inputRef.current?.focus();
      } else if (e.key === ",") {
        e.preventDefault();
        setShowSettings((v) => !v);
        setTestResult(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setShowSettings]);

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

  /** 设置导入导出结果提示 */
  const [settingsMsg, setSettingsMsg] = useState("");
  const flashSettingsMsg = useCallback((msg: string) => {
    setSettingsMsg(msg);
    setTimeout(() => setSettingsMsg(""), 4000);
  }, []);

  /** 导出配置（排除密钥，经 dialog 选路径写文件） */
  const handleExportSettings = useCallback(async () => {
    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const { writeTextFile } = await import("@tauri-apps/plugin-fs");
      const { scrubSettingsForExport } = await import("./utils/hotkeys");
      const filePath = await save({
        defaultPath: "shiyi-settings.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!filePath) return; // 用户取消
      await writeTextFile(filePath, JSON.stringify(scrubSettingsForExport(settings), null, 2));
      flashSettingsMsg(`已导出：${filePath}`);
    } catch (e) {
      flashSettingsMsg(`导出失败：${errText(e)}`);
    }
  }, [settings, flashSettingsMsg]);

  /** 导入配置（dialog 选文件 → sanitizeStored 清洗 → 全量替换；密钥需重新输入） */
  const handleImportSettings = useCallback(async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const { readTextFile } = await import("@tauri-apps/plugin-fs");
      const { sanitizeStored } = await import("./hooks/useSettings");
      const filePath = await open({
        multiple: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!filePath) return; // 用户取消
      const text = await readTextFile(filePath as string);
      const cleaned = sanitizeStored(JSON.parse(text));
      await updateSettings({ ...cleaned });
      setTestResult(null);
      flashSettingsMsg("已导入（密钥需重新输入）");
    } catch (e) {
      flashSettingsMsg(`导入失败：${errText(e)}`);
    }
  }, [updateSettings, flashSettingsMsg]);

  // 主题应用（system 跟随媒体查询；light/dark 强制）
  useEffect(() => {
    const apply = () => {
      const theme = settings.theme ?? "system";
      const dark = theme === "dark"
        || (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
      document.documentElement.classList.toggle("dark", dark);
    };
    apply();
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => { if ((settings.theme ?? "system") === "system") apply(); };
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, [settings.theme]);

  // 模式切换时重置圆桌状态（围坐角色跟随当前模式阵容）
  useEffect(() => {
    pipeline.reset(mode);
    setDetailExpert(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode]);

  // 启动回填 + 一次性迁移（localStorage → 文件）
  useEffect(() => {
    let cancelled = false;
    (async () => {
      let migrated: HistoryEntry[] | null = null;
      // 1. 迁移：旧 localStorage 有非空历史且未迁移过 → 先写文件
      try {
        if (!localStorage.getItem(HISTORY_MIGRATED_KEY)) {
          const raw = localStorage.getItem(HISTORY_KEY);
          if (raw) {
            const parsed = JSON.parse(raw);
            if (Array.isArray(parsed) && parsed.length > 0) {
              await invoke("history_save", { entries: parsed });
            }
          }
          try { localStorage.removeItem(HISTORY_KEY); } catch { /* 忽略 */ }
          try { localStorage.setItem(HISTORY_MIGRATED_KEY, "1"); } catch { /* 忽略 */ }
        }
      } catch { /* 迁移失败不阻断（下次启动 localStorage 已清则跳过） */ }
      // 2. 回填：文件 → 内存（损坏数据防御保留：非数组直接丢弃）
      try {
        const entries = await invoke<HistoryEntry[]>("history_load", {});
        if (!cancelled && Array.isArray(entries)) {
          migrated = entries;
        }
      } catch { /* 读取失败保持空 */ }
      if (!cancelled && migrated) {
        setHistoryEntries(migrated);
      }
    })();
    return () => { cancelled = true; };
  }, []);

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

  /** 日志目录按钮文案（成功显示路径 4s，失败显示错误） */
  const [logDirMsg, setLogDirMsg] = useState("");
  const handleOpenLogDir = useCallback(async () => {
    try {
      const dir = await invoke<string>("open_log_dir", {});
      setLogDirMsg(`已打开：${dir}`);
    } catch (e) {
      setLogDirMsg(`打开失败：${errText(e)}`);
    } finally {
      setTimeout(() => setLogDirMsg(""), 4000);
    }
  }, []);

  /** 配置目录按钮文案（同日志按钮模式） */
  const [configDirMsg, setConfigDirMsg] = useState("");
  const handleOpenConfigDir = useCallback(async () => {
    try {
      const dir = await invoke<string>("open_config_dir", {});
      setConfigDirMsg(`已打开：${dir}`);
    } catch (e) {
      setConfigDirMsg(`打开失败：${errText(e)}`);
    } finally {
      setTimeout(() => setConfigDirMsg(""), 4000);
    }
  }, []);

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
    setHistoryEntries(updated); scheduleSave(updated);
  }, []);

  const updateHistoryEntry = useCallback((id: string, output: string, conv: ChatTurn[], usage?: { prompt_tokens: number; completion_tokens: number }) => {
    const updated = historyRef.current.map(e =>
      e.id === id ? { ...e, output, conversation: conv, usage: usage ?? e.usage, timestamp: Date.now() } : e
    );
    setHistoryEntries(updated); scheduleSave(updated);
  }, []);

  /** 实际执行一次生成（直接提交与队列调度共用；queueId 非空时归属队列项） */
  const runGenerateNow = useCallback(async (
    runId: string,
    runMode: Mode,
    userInput: string,
    extra?: { originalLyrics: string },
  ) => {
    const token = ++runTokenRef.current; // 作废旧 run
    setStatus("loading"); setStreamText(""); setErrorMessage("");
    // mode_c 的 userInput 即新主题（原歌词走独立字段）；历史展示用拼接文本保留上下文
    const displayInput = runMode === "mode_c" && extra
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
      await ensureLlmListener(); // 失败抛错进 catch，不再卡死
      // 原歌词独立字段直传（旧字符串拼接+正则拆分整条退役）
      const originalLyrics = runMode === "mode_c" ? extra?.originalLyrics : undefined;
      const raw = await pipeline.run({
        mode: runMode,
        userInput,
        settings,
        extra: undefined,
        originalLyrics,
        // 专家发言实时追加到对话流（记录进 speechLog，结束时合并，不进入 chatHistoryRef）
        onSpeech: (speech) => {
          // 阶段0流式结束后清空中间态流式文本（首个专家发言时），避免统领全文重复显示
          if (speechLogRef.current.length === 0) setStreamText("");
          speechLogRef.current.push(speech);
          setConversation((prev) => [...prev, speech]);
        },
      });
      if (token !== runTokenRef.current) return; // 过期 run 的结果丢弃
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
      const entry: HistoryEntry = { id: newId(), mode: runMode, input: displayInput, output: raw, conversation: allTurns, usage: { ...pipeline.usage }, timestamp: Date.now() };
      setCurrentHistoryId(entry.id);
      const updated = [entry, ...historyRef.current];
      setHistoryEntries(updated); scheduleSave(updated);
      // 队列项完成归档（queueId 命中时标记 done + 关联 history id）
      if (queue.peek().some((q) => q.id === runId)) {
        queue.mark(runId, "done");
      }
    } catch (e) {
      if (token !== runTokenRef.current) return; // 过期 run 的错误丢弃
      // Q4：取消走空闲通道（不标红、不写历史）；失败才走 error
      if (isCancelledError(e)) {
        setStatus("idle"); setErrorMessage("已取消");
      } else {
        setStatus("error"); setErrorMessage(errText(e));
      }
      // 队列项失败标记（用户可从队列点击查看错误态，点击删除清理）
      if (queue.peek().some((q) => q.id === runId)) {
        queue.mark(runId, "error");
      }
    } finally {
      // 完成链——无论成败，取队首继续（取消走 cancel 流程同样经此处继续）
      setRunningQueueId(null);
      const next = dequeueNext(queue.peek());
      if (next) {
        setRunningQueueId(next.id);
        queue.mark(next.id, "running");
        // 注意：不 await，后台顺序执行
        void runGenerateNow(next.id, next.mode, next.userInput, next.extra);
      }
    }
  }, [settings, ensureLlmListener, pipeline]);

  const handleGenerate = useCallback(async (userInput: string, extra?: { originalLyrics: string }) => {
    // 忙时入队（当前有运行项）——排队顺序执行，不作废在途任务
    if (pipeline.active) {
      queue.push({
        id: newId(),
        label: queueLabel(userInput),
        mode,
        userInput,
        extra,
      });
      return;
    }
    await runGenerateNow(newId(), mode, userInput, extra);
  }, [mode, settings, ensureLlmListener, pipeline, queue]);

  const handleRefine = useCallback(async (feedback: string, refineMode: "fast" | "full" = "fast") => {
    if (!feedback.trim()) return;
    const token = ++runTokenRef.current; // 作废旧 run
    setStatus("loading"); setStreamText(""); setErrorMessage("");
    setLastFeedback(feedback);
    speechLogRef.current = [];
    setHistoryView(null); // 优化时同样关闭历史面板（与 handleGenerate 一致）

    const currentHistory = [...chatHistoryRef.current];

    let raw: string;
    try {
      await ensureLlmListener(); // 失败抛错进 catch，不再卡死
      // lastUserInput 是展示用拼接文本（原歌词+新主题），从中拆出两部分直传。
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
      // 取最后一条 assistant（重复 refine 时注入的是上一版而非初始版）
      const lastOutput = [...currentHistory].reverse().find((m) => m.role === "assistant")?.content || "";
      // fast 增量时前端预估 targets 透传（无命中传空→后端自动路由；full 不传=全量）
      const { estimateRefineTargets } = await import("./utils/refineTargets");
      const estimated = refineMode === "fast" ? estimateRefineTargets(feedback, mode) : [];
      raw = await pipeline.refine({
        mode,
        userInput: input,
        lastOutput,
        feedback,
        settings,
        extra: undefined,
        originalLyrics,
        refineMode,
        refineTargets: refineMode === "fast" && estimated.length > 0 ? estimated : undefined,
        onSpeech: (speech) => {
          // 阶段0流式结束后清空中间态流式文本（首个专家发言时）
          if (speechLogRef.current.length === 0) setStreamText("");
          speechLogRef.current.push(speech);
          setConversation((prev) => [...prev, speech]);
        },
      });
    } catch (e) {
      if (token !== runTokenRef.current) return; // 过期 run 的错误丢弃
      // Q4：取消走空闲通道
      if (isCancelledError(e)) {
        setStatus("idle"); setErrorMessage("已取消");
      } else {
        setStatus("error"); setErrorMessage(errText(e));
      }
      return;
    }
    if (token !== runTokenRef.current) return; // 过期 run 的结果丢弃

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

  /** U-3：重试可执行判定——mode_c 无 lastFeedback 时重试必然无路可走（原歌词会丢），按钮禁用而非死按 */
  const canRetry =
    status === "error" &&
    (Boolean(lastFeedback) || (mode !== "mode_c" && Boolean(lastUserInput)));
  const handleRetry = useCallback(async () => {
    if (status === "error" && lastFeedback) {
      await handleRefine(lastFeedback, "fast");
    } else if (status === "error" && lastUserInput) {
      // 重试走展示文本解析路径（handleGenerate 内部处理直传，此处传原始展示文本由其二次解析）
      // 注意：mode_c 重试时 lastUserInput 为展示拼接文本，handleGenerate 会误判为新主题——
      // 因此 mode_c 重试改走 refine 路径（带上次反馈），避免原歌词丢失
      if (mode === "mode_c" && lastFeedback) {
        await handleRefine(lastFeedback, "fast");
      } else if (mode !== "mode_c") {
        await handleGenerate(lastUserInput);
      }
    }
  }, [status, lastFeedback, lastUserInput, handleRefine, handleGenerate, mode]);

  /** R3：从上次继续（检查点续跑；无检查点时后端明确报错展示，不静默从头来） */
  const handleResume = useCallback(async () => {
    if (status !== "error") return;
    const token = ++runTokenRef.current; // 作废旧 run
    setStatus("loading"); setStreamText(""); setErrorMessage("");
    speechLogRef.current = [];
    try {
      await ensureLlmListener();
      const originalLyrics = mode === "mode_c"
        ? lastUserInput.match(/原歌词：\n([\s\S]*)\n\n新主题：\n([\s\S]*)$/)?.[1]
        : undefined;
      const input = mode === "mode_c"
        ? (lastUserInput.match(/原歌词：\n([\s\S]*)\n\n新主题：\n([\s\S]*)$/)?.[2] ?? lastUserInput)
        : lastUserInput;
      const raw = await pipeline.resume({
        mode,
        userInput: input,
        settings,
        extra: undefined,
        originalLyrics,
        onSpeech: (speech) => {
          if (speechLogRef.current.length === 0) setStreamText("");
          speechLogRef.current.push(speech);
          setConversation((prev) => [...prev, speech]);
        },
      });
      if (token !== runTokenRef.current) return; // 过期 run 的结果丢弃
      const allTurns: ChatTurn[] = [
        ...conversation,
        ...speechLogRef.current,
        { role: "assistant", content: raw, timestamp: Date.now() },
      ];
      setConversation(allTurns);
      setStatus("done"); setStreamText("");
      chatHistoryRef.current = [
        ...chatHistoryRef.current,
        { role: "assistant", content: raw },
      ];
      if (currentHistoryId) {
        updateHistoryEntry(currentHistoryId, raw, allTurns, { ...pipeline.usage });
      } else {
        saveToHistory(conversation[0]?.content || lastUserInput, raw, allTurns, mode, { ...pipeline.usage });
      }
    } catch (e) {
      if (token !== runTokenRef.current) return; // 过期 run 的错误丢弃
      // Q4：取消走空闲通道
      if (isCancelledError(e)) {
        setStatus("idle"); setErrorMessage("已取消");
      } else {
        setStatus("error"); setErrorMessage(errText(e));
      }
    }
  }, [status, mode, lastUserInput, conversation, saveToHistory, currentHistoryId, updateHistoryEntry, pipeline, ensureLlmListener]);

  const deleteHistory = (id: string) => { const u = historyEntries.filter(e => e.id !== id); setHistoryEntries(u); scheduleSave(u); };
  const clearHistory = () => { setHistoryEntries([]); scheduleSave([]); };
  const selectHistory = (entry: HistoryEntry) => { setHistoryView(entry); setShowHistory(false); };

  /** 队列查看——完成/失败项点击查看对应历史（按 input 匹配最新一条） */
  const selectQueueItem = useCallback((id: string) => {
    const item = queue.peek().find((q) => q.id === id);
    if (!item) return;
    const entry = historyRef.current.find((e) => e.input.includes(item.userInput.slice(0, 20)));
    if (entry) {
      setHistoryView(entry);
      setShowHistory(false);
    }
  }, [queue]);

  /** 队列取消当前——只杀当前运行项，队列继续 */
  const handleCancelCurrent = useCallback(async () => {
    if (runningQueueId) {
      queue.mark(runningQueueId, "cancelled");
    }
    await pipeline.cancel();
    // 完成链由 runGenerateNow.finally 驱动（取消同样走 finally 继续队首）
  }, [pipeline, queue, runningQueueId]);

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
            <IconHistory size={14} /> {t(settings.language, "app.history")}
          </button>
          <button onClick={() => { setShowSettings(!showSettings); setTestResult(null); }}
            aria-label={t(settings.language, "app.settings")}
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
          <h3 className="text-[13px] font-medium text-text-1">{t(settings.language, "settings.title")}</h3>
          <div>
            <div className="flex items-center justify-between mb-1">
              <label className="text-[11px] text-text-muted">{t(settings.language, "settings.apikey")}</label>
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
              <label className="text-[11px] text-text-muted block mb-1">{t(settings.language, "settings.model")}</label>
              <input value={settings.model} onChange={e => { updateSettings({ model: e.target.value }); setTestResult(null); }}
                className="w-full bg-surface-0 border border-border/60 rounded-lg px-3 py-2 text-[13px]
                           text-text-1 focus:outline-none focus:border-brand-500/40 focus:ring-1 focus:ring-brand-500/20
                           transition-all duration-150" />
            </div>
            <div className="flex-1">
              <label className="text-[11px] text-text-muted block mb-1">{t(settings.language, "settings.baseurl")}</label>
              <input value={settings.baseUrl} onChange={e => { updateSettings({ baseUrl: e.target.value }); setTestResult(null); }}
                className="w-full bg-surface-0 border border-border/60 rounded-lg px-3 py-2 text-[13px]
                           text-text-1 focus:outline-none focus:border-brand-500/40 focus:ring-1 focus:ring-brand-500/20
                           transition-all duration-150" />
            </div>
          </div>
          {/* 思考模式：后端按模型能力路由表自动注入厂商思考参数（DeepSeek/讯飞 → thinking；o 系/gpt-5 → reasoning_effort；未登记模型自动忽略） */}
          <div className="flex items-center justify-between gap-2 rounded-lg border border-border/40 bg-surface-0/40 px-3 py-2">
            <label htmlFor="thinking-mode" className="text-[11px] text-text-2 cursor-pointer select-none">
              {t(settings.language, "settings.thinking")}
              <span className="block text-[10px] text-text-muted font-normal">{t(settings.language, "settings.thinking.desc")}</span>
            </label>
            <input id="thinking-mode" type="checkbox" checked={settings.thinking}
              onChange={e => { updateSettings({ thinking: e.target.checked }); setTestResult(null); }}
              className="w-4 h-4 accent-brand-500 cursor-pointer shrink-0" />
          </div>
          {/* 高级参数（缺省走后端默认；temperature 0~2，max_tokens 1000~30000——F-1 与后端 MAX_TOKENS_CAP 对齐，32000 会被后端拒绝） */}
          <div className="rounded-lg border border-border/40 bg-surface-0/40 px-3 py-2 space-y-2">
            <div className="flex items-center justify-between gap-2">
              <label htmlFor="gen-temperature" className="text-[11px] text-text-2 cursor-pointer select-none">
                {t(settings.language, "settings.advanced.temp")}
                <span className="block text-[10px] text-text-muted font-normal">{t(settings.language, "settings.advanced.temp.desc")}</span>
              </label>
              <input id="gen-temperature" type="number" min={0} max={2} step={0.1}
                value={settings.generation?.temperature ?? ""}
                onChange={e => {
                  const v = e.target.value === "" ? undefined : Number(e.target.value);
                  updateSettings({ generation: { ...settings.generation, temperature: v } });
                  setTestResult(null);
                }}
                placeholder="默认"
                className="w-20 bg-surface-0 border border-border/60 rounded-lg px-2 py-1 text-[12px] text-text-1 focus:outline-none focus:border-brand-500/40 transition-all duration-150" />
            </div>
            <div className="flex items-center justify-between gap-2">
              <label htmlFor="gen-max-tokens" className="text-[11px] text-text-2 cursor-pointer select-none">
                {t(settings.language, "settings.advanced.maxtokens")}
                <span className="block text-[10px] text-text-muted font-normal">{t(settings.language, "settings.advanced.maxtokens.desc")}</span>
              </label>
              <input id="gen-max-tokens" type="number" min={1000} max={30000} step={1000}
                value={settings.generation?.max_tokens ?? ""}
                onChange={e => {
                  const v = e.target.value === "" ? undefined : Math.round(Number(e.target.value));
                  updateSettings({ generation: { ...settings.generation, max_tokens: v } });
                  setTestResult(null);
                }}
                placeholder="默认"
                className="w-20 bg-surface-0 border border-border/60 rounded-lg px-2 py-1 text-[12px] text-text-1 focus:outline-none focus:border-brand-500/40 transition-all duration-150" />
            </div>
          </div>
          <button onClick={handleTestApi} disabled={testingApi || !settings.apiKey || !secretsReady}
            className="w-full flex items-center justify-center gap-1.5 py-2 rounded-lg text-[12px] font-medium
                       bg-surface-2 hover:bg-surface-3 border border-border/50 text-text-2
                       disabled:opacity-50 transition-all duration-150 active:scale-[0.98]">
            {testingApi ? <IconLoader size={13} className="animate-spin" /> : <IconPlug size={13} />}
            {testResult === "ok" && t(settings.language, "settings.ok")}
            {testResult === "fail" && t(settings.language, "settings.fail")}
            {testResult === null && (testingApi ? t(settings.language, "settings.testing") : t(settings.language, "settings.test"))}
          </button>
          {/* 打开日志目录（诊断用，失败提示路径） */}
          <button onClick={handleOpenLogDir}
            className="w-full py-1.5 rounded-lg text-[11px] text-text-muted hover:text-text-2
                       bg-surface-2/60 hover:bg-surface-3/80 border border-border/40
                       transition-all duration-150 active:scale-[0.98]">
            {logDirMsg || t(settings.language, "settings.logdir")}
          </button>
          {/* 打开配置目录（prompt/知识库覆盖文件投放处） */}
          <button onClick={handleOpenConfigDir}
            className="w-full py-1.5 rounded-lg text-[11px] text-text-muted hover:text-text-2
                       bg-surface-2/60 hover:bg-surface-3/80 border border-border/40
                       transition-all duration-150 active:scale-[0.98]">
            {configDirMsg || t(settings.language, "settings.configdir")}
          </button>

          {/* 角色级 API 覆盖（可选）：不配置 = 所有角色共用全局；配置了生效单独 */}
          <div className="border-t border-border/40 pt-3">
            <button onClick={() => setShowRoleApi(!showRoleApi)}
              className="w-full flex items-center justify-between text-[11px] text-text-2 hover:text-text-1 transition-colors">
              <span>{t(settings.language, "settings.roles")}</span>
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

          {/* 配置导入导出（密钥不落地：导出排除 apiKey/api_key，需重新输入） */}
          <div className="border-t border-border/40 pt-3">
            <div className="flex gap-2">
              <button onClick={handleExportSettings}
                className="flex-1 py-1.5 rounded-lg text-[11px] text-text-2 bg-surface-2 hover:bg-surface-3 border border-border/50 transition-all duration-150 active:scale-[0.98]">
                {t(settings.language, "settings.export")}
              </button>
              <button onClick={handleImportSettings}
                className="flex-1 py-1.5 rounded-lg text-[11px] text-text-2 bg-surface-2 hover:bg-surface-3 border border-border/50 transition-all duration-150 active:scale-[0.98]">
                {t(settings.language, "settings.import")}
              </button>
            </div>
            {settingsMsg && (
              <p className="text-[10px] text-text-muted mt-1.5 leading-relaxed">{settingsMsg}</p>
            )}
            <p className="text-[9px] text-text-muted mt-1 leading-relaxed">
              导出不含密钥（需重新输入）；导入经格式清洗后生效
            </p>
          </div>

          {/* 主题 + 语言 */}
          <div className="border-t border-border/40 pt-3 space-y-2">
            <div className="flex items-center justify-between gap-2">
              <span className="text-[11px] text-text-2">{t(settings.language, "settings.theme")}</span>
              <div className="flex gap-1">
                {(["system", "light", "dark"] as const).map((v) => (
                  <button key={v} onClick={() => updateSettings({ theme: v })}
                    className={`px-2 py-1 rounded-lg text-[10px] font-medium border transition-all duration-150 ${
                      (settings.theme ?? "system") === v
                        ? "bg-brand-500/15 border-brand-500/40 text-brand-400"
                        : "text-text-muted hover:text-text-2 border-border/40"
                    }`}>
                    {t(settings.language, `settings.theme.${v}`)}
                  </button>
                ))}
              </div>
            </div>
            <div className="flex items-center justify-between gap-2">
              <span className="text-[11px] text-text-2">{t(settings.language, "settings.language")}</span>
              <div className="flex gap-1">
                {(["zh", "en"] as const).map((v) => (
                  <button key={v} onClick={() => updateSettings({ language: v })}
                    className={`px-2 py-1 rounded-lg text-[10px] font-medium border transition-all duration-150 ${
                      (settings.language ?? "zh") === v
                        ? "bg-brand-500/15 border-brand-500/40 text-brand-400"
                        : "text-text-muted hover:text-text-2 border-border/40"
                    }`}>
                    {v === "zh" ? "中文" : "EN"}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Main content */}
      <div className="flex-1 flex flex-col overflow-hidden">
        {/* 模式选择（横向 tab 置顶） */}
        <div className="shrink-0 px-4 pt-3 pb-2 border-b border-border/40">
          <div className="flex items-center gap-3">
            <div className="flex-1 max-w-[720px]">
              <ModeSelector mode={mode} locale={settings.language} onChange={(m) => {
                runTokenRef.current++; // 作废在途 run
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
          {/* 左侧：圆桌舞台（四模式统一渲染） */}
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
                locale={settings.language}
              />
            </div>

            {detailExpert && (
              <div className="mx-3 mb-3 p-3 glass-panel rounded-xl border border-border/40 animate-[fade_200ms_ease]">
                <div className="flex items-center justify-between mb-1.5">
                  <span className="text-[12px] font-semibold text-text-1">
                    {detailExpert.emoji} {detailExpert.name}
                  </span>
                  <button onClick={() => setDetailExpert(null)} className="text-text-muted hover:text-text-2 text-[11px]">{t(settings.language, "detail.close")}</button>
                </div>
                <p className="text-[11px] text-text-2 leading-relaxed">
                  {t(settings.language, "detail.knowledge")}📚 {detailExpert.knowledge.length > 0 ? detailExpert.knowledge.join(", ") + ".csv" : t(settings.language, "detail.none")}
                </p>
                <p className="text-[10px] text-text-muted mt-1">
                  {detailExpert.status === "working" && t(settings.language, "detail.working")}
                  {detailExpert.status === "done" && `${t(settings.language, "detail.done")}${detailExpert.note || t(settings.language, "detail.done.empty")}`}
                  {detailExpert.status === "error" && `${t(settings.language, "detail.error")}${detailExpert.note || errText(detailExpert.note)}`}
                  {detailExpert.status === "idle" && t(settings.language, "detail.idle")}
                </p>
              </div>
            )}
          </div>

          {/* 右侧：输入 + 结果 */}
          <div className="flex-1 flex flex-col overflow-hidden">
            {status !== "idle" && (
              <div className="shrink-0">
                <StatusIndicator status={status} errorMessage={errorMessage} onRetry={handleRetry} canRetry={canRetry} onResume={handleResume} onCancel={handleCancelCurrent} getRunId={pipeline.getRunId} locale={settings.language} />
                {/* 生成队列面板（等待项列表；完成项点击查看） */}
                <QueuePanel
                  queue={queue.queue}
                  locale={settings.language}
                  runningId={runningQueueId}
                  onRemove={(id) => queue.remove(id)}
                  onClear={() => queue.clearWaiting()}
                  onSelect={selectQueueItem}
                />
              </div>
            )}

            <div className="flex-1 overflow-y-auto p-4">
              {historyView ? (
                <ResultPanel conversation={historyView.conversation || [
                  { role: "user", content: historyView.input, timestamp: historyView.timestamp },
                  { role: "assistant", content: historyView.output, timestamp: historyView.timestamp }
                ]} streamText="" status="done" onRefine={() => {}} readOnly />
              ) : (conversation.length > 0 || streamText) ? (
                <ResultPanel conversation={conversation} streamText={streamText} status={status} onRefine={handleRefine} mode={mode} locale={settings.language} />
              ) : (
                <div className="h-full flex flex-col items-center justify-center text-text-muted px-8">
                  <div className="w-16 h-16 rounded-2xl glass-panel flex items-center justify-center mb-3">
                    <IconSparkles size={28} className="text-brand-400/50" />
                  </div>
                  <p className="text-[13px] mb-4">{t(settings.language, "empty.hint")}</p>
                  <div className="w-full max-w-[420px] glass-panel rounded-xl border border-border/40 p-4 space-y-2 text-[11px] leading-relaxed">
                    <p className="text-text-2 font-medium">{t(settings.language, "empty.flow")}</p>
                    <p className="flex items-center gap-1.5"><span className="w-1.5 h-1.5 rounded-full bg-brand-400" /> {t(settings.language, "empty.s1")}</p>
                    <p className="flex items-center gap-1.5"><span className="w-1.5 h-1.5 rounded-full bg-brand-400" /> {t(settings.language, "empty.s2")}</p>
                    <p className="flex items-center gap-1.5"><span className="w-1.5 h-1.5 rounded-full bg-brand-400" /> {t(settings.language, "empty.s3")}</p>
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
              {/* U-4：钥匙串回填完成前（secretsReady=false）哨兵还不是真 key，输入面板整体禁用防带哨兵直发 */}
              <InputPanel key={mode} mode={mode} disabled={status === "loading" || status === "streaming" || !secretsReady}
                settings={settings} onGenerate={handleGenerate} inputRef={inputRef}  locale={settings.language} />
            </div>
          </div>
        </div>
      </div>

      {showHistory && (
        <HistoryPanel entries={filteredHistory} allEntriesCount={historyEntries.length}
          filter={historyFilter} onFilterChange={setHistoryFilter}
          onDelete={deleteHistory} onClear={clearHistory}
          onSelect={selectHistory} onClose={() => setShowHistory(false)} locale={settings.language} />
      )}
    </div>
  );
}

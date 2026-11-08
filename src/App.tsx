import { useState, useRef, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { IconSettings, IconHistory, IconSparkles, IconEye, IconEyeOff, IconPlug, IconLoader } from "@tabler/icons-react";
import ModeSelector from "./components/ModeSelector";
import InputPanel from "./components/InputPanel";
import ResultPanel from "./components/ResultPanel";
import StatusIndicator from "./components/StatusIndicator";
import RoundGateBar from "./components/RoundGateBar";
import HistoryPanel from "./components/HistoryPanel";
import RoundtablePanel from "./components/RoundtablePanel";
import { useSettingsWithSecrets, sanitizeStored } from "./hooks/useSettings";
import { SecretSyncError } from "./components/SecretSyncError";
import { usePipeline, ROLE_NAMES, ROLE_EMOJIS, MODE_EXPERTS } from "./hooks/usePipeline";
import { orderRoundSpeech, assembleFinalTurns } from "./utils/speechOrder";
import { freezePartial, toChatMessages } from "./utils/partialDraft";
import { useQueue, queueLabel, dequeueNext, terminalQueueStatus, queueResultEntry } from "./hooks/useQueue";
import QueuePanel from "./components/QueuePanel";
import SearchableSelect from "./components/SearchableSelect";
import ProviderTemplates from "./components/ProviderTemplates";
import { modelsForBaseUrl } from "./data/providers";
import { formatUrlIssue } from "./utils/urlGuard";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";
import { scrubSettingsForExport } from "./utils/hotkeys";
import { estimateRefineTargets } from "./utils/refineTargets";
import type { AppError, Mode, ChatMessage, ChatTurn, HistoryEntry, LLMStatus, ExpertCard, PipelineRoleKey, StreamChunk, StructuredMode, SongPlan } from "./types";
import { acceptStreamChunk } from "./utils/streamChunk";
import { historyPlanFields } from "./utils/sunoPackage";
import { MODE_LABELS, errKind, errText, isCancelledError, capKey, STRUCTURED_MODES } from "./types";
import { probeStructuredCaps, withProbedCap, effectiveMode } from "./utils/structuredCaps";
import { t } from "./i18n";

/** P1-10：历史上限（与后端 HISTORY_MAX_ENTRIES 同源约定） */
const HISTORY_MAX_ENTRIES = 200;

/** 文件持久化写回（串行化由调用方队列保证） */
async function saveHistoryFile(entries: HistoryEntry[]): Promise<void> {
  await invoke("history_save", { entries: entries.slice(0, HISTORY_MAX_ENTRIES) });
}

/** 关闭前 flush：走 history_flush，绕过防抖立即落盘 */
async function flushHistoryFile(entries: HistoryEntry[]): Promise<void> {
  await invoke("history_flush", { entries: entries.slice(0, HISTORY_MAX_ENTRIES) });
}
function newId() {
  // 低版本 WebKitGTK 可能缺失 randomUUID（低危修复）
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  return `id-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
}

/** 修复包 C（#9）：历史条目优化装载上下文（模块级纯函数，handleRefineFromHistory 与单测共用）。
 * 旧记录可能无 conversation 字段（类型注释声明）——以 input/output 构造两轮对话兜底。 */
export function buildHistoryRefineContext(entry: HistoryEntry) {
  const conversation: ChatTurn[] = entry.conversation?.length
    ? entry.conversation
    : [
        { role: "user", content: entry.input, timestamp: entry.timestamp },
        { role: "assistant", content: entry.output, timestamp: entry.timestamp },
      ];
  const lastOutput = [...conversation].reverse().find((m) => m.role === "assistant")?.content || entry.output;
  return { mode: entry.mode, conversation, lastUserInput: entry.input, lastOutput };
}

export default function App() {
  const [mode, setMode] = useState<Mode>("mode_d");
  /** D-1（#13）：当前模式阵容序（speech 稳定排序用）。ref 而非 state——onSpeech 回调
   * 不应因阵容重渲染重建；mode/meta 变化时同步。 */
  const rosterRef = useRef<string[]>(MODE_EXPERTS["mode_d"].map((e) => e.id));
  const [status, setStatus] = useState<LLMStatus>("idle");
  const [streamText, setStreamText] = useState("");
  /** #10 流式正文的**唯一写出口**：ref（catch 固化半成品读）与 state（渲染）同点更新。
   *  分散写必然漏——漏一处就会把上一轮残留正文当本轮半成品固化（可见状态与固化内容背离）。 */
  const streamTextRef = useRef("");
  const writeStreamText = useCallback((v: string) => {
    streamTextRef.current = v;
    setStreamText(v);
  }, []);
  const [errorMessage, setErrorMessage] = useState("");
  /** #27-c：终态错误类别（与 errorMessage 同源写入——同一错误的两个投影）。
   *  限流终态据此给可操作指引；null = 非结构化错误或无错误。 */
  const [errorKind, setErrorKind] = useState<AppError["kind"] | null>(null);
  /** #27-c 单源：run 错误落点——文案与类别同点写入（此前只写文案，429 的"限流"身份
   *  在 UI 层丢失，只能与网络错误同形提示）。三处 run catch 共用，杜绝各写各的。 */
  const applyRunError = useCallback((e: unknown) => {
    if (isCancelledError(e)) {
      // 取消走空闲通道：不标红、不带类别（否则"限流"指引会挂在取消态上）
      setStatus("idle"); setErrorMessage("已取消"); setErrorKind(null);
    } else {
      setStatus("error"); setErrorMessage(errText(e)); setErrorKind(errKind(e));
    }
  }, []);
  /** #27-c 单源：错误落点清除——任何"重新开始/复位"都经此，
   *  避免上一轮 kind 残留让下一次普通错误也带上"限流"指引。 */
  const clearRunError = useCallback(() => {
    setErrorMessage(""); setErrorKind(null);
  }, []);
  const [lastUserInput, setLastUserInput] = useState("");
  /** Mode D 最近一次生效场景（供优化/续跑/重试沿用，不再硬编码退回 daily） */
  const lastDouyinSceneRef = useRef<string>("daily");
  /** 最近一次定稿的 SongPlan JSON（供快速增量优化注入，使后端 plan_from_checkpoint 成功并跳过阶段 0） */
  const lastPlanRef = useRef<unknown>(null);
  const [conversation, setConversation] = useState<ChatTurn[]>([]);
  // 历史改走文件存储——初始空，启动 useEffect 从 history_load 回填
  const [historyEntries, setHistoryEntries] = useState<HistoryEntry[]>([]);
  const historyRef = useRef(historyEntries);
  historyRef.current = historyEntries;
  /** 写入防抖 timer（500ms 合并连续写入） */
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** C-09：串行写队列（旧 in-flight 不得覆盖新数组） */
  const saveChainRef = useRef(Promise.resolve());
  /** 防抖窗口内最新待写快照 */
  const pendingSaveRef = useRef<HistoryEntry[] | null>(null);
  /** 立即 flush（关闭/卸载）：取消防抖并以最新快照落盘 */
  const flushHistory = useCallback(() => {
    if (saveTimerRef.current) {
      clearTimeout(saveTimerRef.current);
      saveTimerRef.current = null;
    }
    const snapshot = (pendingSaveRef.current ?? historyRef.current).slice(0, HISTORY_MAX_ENTRIES);
    pendingSaveRef.current = null;
    saveChainRef.current = saveChainRef.current
      .then(() => flushHistoryFile(snapshot))
      .catch(() => { /* flush 失败静默 */ });
    return saveChainRef.current;
  }, []);
  /** 计划持久化（防抖 + 串行；失败静默，生成流程不受影响） */
  const scheduleSave = useCallback((entries: HistoryEntry[]) => {
    pendingSaveRef.current = entries.slice(0, HISTORY_MAX_ENTRIES);
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    saveTimerRef.current = setTimeout(() => {
      const snapshot = (pendingSaveRef.current ?? []).slice(0, HISTORY_MAX_ENTRIES);
      pendingSaveRef.current = null;
      saveChainRef.current = saveChainRef.current
        .then(() => saveHistoryFile(snapshot))
        .catch(() => { /* 持久化失败静默（与旧配额降级语义一致） */ });
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
  const [showRoleApi, setShowRoleApi] = useState(false);
  const [expandedRole, setExpandedRole] = useState<PipelineRoleKey | null>(null);
  const { settings, updateSettings, clearGlobalApiKey, showSettings, setShowSettings, secretsReady, secretErrors } = useSettingsWithSecrets();
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
    const un = await listen<StreamChunk>("llm-chunk", (event) => {
      if (token !== runTokenRef.current) return; // 过期 run 的流式 chunk 丢弃
      // RULE-RUN-ID（C-06）：再按归属 run_id 过滤——token 只能区分"监听代次"，区分不了在途旧流
      if (!acceptStreamChunk(event.payload, pipeline.getRunId())) return;
      // #10：ref 与 state 同点更新——catch 里固化半成品读 ref（state 闭包停留在生成前的空串）
      streamTextRef.current += event.payload.content;
      setStreamText((prev) => prev + event.payload.content);
      setStatus("streaming");
    });
    llmUnlistenRef.current = un;
  }, [pipeline.getRunId]);

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
    const cleaned = next.model || next.api_key || next.base_url || next.structured_mode ? next : undefined;
    const roleOverrides = { ...settings.roleOverrides };
    if (cleaned) {
      roleOverrides[role] = cleaned;
    } else {
      delete roleOverrides[role];
    }
    updateSettings({ roleOverrides });
    setTestResult(null);
  }, [settings, updateSettings]);

  /** P1-2：设置/清除某角色的结构化档位（清除后条目全空则移除） */
  const updateRoleStructuredMode = useCallback((role: PipelineRoleKey, mode: StructuredMode | undefined) => {
    const next = { ...(settings.roleOverrides?.[role] ?? {}), structured_mode: mode };
    const roleOverrides = { ...settings.roleOverrides };
    if (next.model || next.api_key || next.base_url || next.structured_mode) roleOverrides[role] = next;
    else delete roleOverrides[role];
    updateSettings({ roleOverrides });
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
      const filePath = await save({
        defaultPath: "shiyi-settings.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!filePath) return;
      await writeTextFile(filePath, JSON.stringify(scrubSettingsForExport(settings), null, 2));
      flashSettingsMsg(`已导出：${filePath}`);
    } catch (e) {
      flashSettingsMsg(`导出失败：${errText(e)}`);
    }
  }, [settings, flashSettingsMsg]);

  /** 导入配置（dialog 选文件 → sanitizeStored 清洗 → 全量替换；密钥需重新输入） */
  const handleImportSettings = useCallback(async () => {
    try {
      const filePath = await open({
        multiple: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!filePath) return;
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
      document.documentElement.setAttribute("data-theme", dark ? "dark" : "light");
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

  // P1-10：启动回填（RULE-HISTORY-V2；不再迁移 localStorage——B-03 覆盖风险已退役）
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const entries = await invoke<HistoryEntry[]>("history_load", {});
        if (!cancelled && Array.isArray(entries)) {
          setHistoryEntries(entries.slice(0, HISTORY_MAX_ENTRIES));
        }
      } catch { /* 读取失败保持空 */ }
    })();
    return () => { cancelled = true; };
  }, []);

  // P1-10 / C-09：关闭前 / 卸载 flush，避免防抖窗口丢最后一条
  useEffect(() => {
    const onUnload = () => { void flushHistory(); };
    window.addEventListener("beforeunload", onUnload);
    let unlistenClose: (() => void) | undefined;
    void (async () => {
      try {
        unlistenClose = await getCurrentWindow().onCloseRequested(async () => {
          await flushHistory();
        });
      } catch { /* 非 Tauri / 测试环境忽略 */ }
    })();
    return () => {
      window.removeEventListener("beforeunload", onUnload);
      unlistenClose?.();
      void flushHistory();
    };
  }, [flushHistory]);

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
      // P1-2：连接成功后探测结构化档位并缓存（探测失败不记档）
      const mode = await probeStructuredCaps(settings.baseUrl, settings.apiKey, settings.model);
      if (mode) updateSettings({ structuredCaps: withProbedCap(settingsRef.current.structuredCaps, settings.baseUrl, settings.model, mode) });
    } catch {
      setTestResult("fail");
    } finally {
      setTestingApi(false);
    }
  }, [settings, updateSettings]);

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
  // P1-2：探测缓存写入（structuredCaps）不算配置变更——否则刚测出的 ✓ 会被自己清掉
  const settingsSansCaps = JSON.stringify({ ...settings, structuredCaps: undefined });
  useEffect(() => {
    setRoleTestStates({});
  }, [settingsSansCaps]);
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
      const cur = settingsRef.current;
      if (cur.apiKey !== snap.apiKey || cur.model !== snap.model || cur.baseUrl !== snap.baseUrl) return;
      setRoleTestStates((p) => ({ ...p, [role]: "ok" }));
      const mode = await probeStructuredCaps(baseUrl, apiKey, model);
      if (mode) updateSettings({ structuredCaps: withProbedCap(settingsRef.current.structuredCaps, baseUrl, model, mode) });
    } catch {
      const cur = settingsRef.current;
      if (cur.apiKey !== snap.apiKey || cur.model !== snap.model || cur.baseUrl !== snap.baseUrl) return;
      setRoleTestStates((p) => ({ ...p, [role]: "fail" }));
    }
  }, [settings, updateSettings]);

  const saveToHistory = useCallback((input: string, output: string, conv: ChatTurn[], currentMode: Mode, usage?: { prompt_tokens: number; completion_tokens: number }) => {
    const entry: HistoryEntry = { id: newId(), mode: currentMode, input, output, conversation: conv, usage, timestamp: Date.now(), ...historyPlanFields(output, pipeline.finalPlanRef.current) };
    const updated = [entry, ...historyRef.current].slice(0, HISTORY_MAX_ENTRIES);
    setHistoryEntries(updated); scheduleSave(updated);
  }, [scheduleSave, pipeline.finalPlanRef]);

  const updateHistoryEntry = useCallback((id: string, output: string, conv: ChatTurn[], usage?: { prompt_tokens: number; completion_tokens: number }, planFields?: Partial<Pick<HistoryEntry, "song_plan" | "rendered">>) => {
    const updated = historyRef.current.map(e =>
      e.id === id ? { ...e, ...(planFields ?? {}), output, conversation: conv, usage: usage ?? e.usage, timestamp: Date.now() } : e
    );
    setHistoryEntries(updated); scheduleSave(updated);
  }, [scheduleSave]);

  /** 实际执行一次生成（直接提交与队列调度共用；queueId 非空时归属队列项） */
  const runGenerateNow = useCallback(async (
    runId: string,
    runMode: Mode,
    userInput: string,
    extra?: { originalLyrics?: string; douyinScene?: string },
  ) => {
    const token = ++runTokenRef.current; // 作废旧 run
    setStatus("loading"); writeStreamText(""); clearRunError();
    const displayInput = runMode === "mode_c" && extra
      ? `原歌词：\n${extra.originalLyrics}\n\n新主题：\n${userInput}`
      : userInput;
    setLastUserInput(displayInput);
    if (runMode === "mode_d") {
      lastDouyinSceneRef.current = extra?.douyinScene ?? "daily";
    }
    chatHistoryRef.current = [];
    speechLogRef.current = [];
    lastPlanRef.current = null;
    setConversation([]);
    setDetailExpert(null);
    setHistoryView(null);

    try {
      await ensureLlmListener();
      const originalLyrics = runMode === "mode_c" ? extra?.originalLyrics : undefined;
      const raw = await pipeline.run({
        mode: runMode,
        userInput,
        settings,
        extra: undefined,
        originalLyrics,
        douyinScene: runMode === "mode_d" ? lastDouyinSceneRef.current : undefined,
        onSpeech: (speech) => {
          if (speechLogRef.current.length === 0) writeStreamText("");
          speechLogRef.current.push(speech);
          setConversation((prev) => orderRoundSpeech([...prev, speech], rosterRef.current));
        },
      });
      if (token !== runTokenRef.current) return;
      lastPlanRef.current = pipeline.finalPlanRef.current;
      const allTurns: ChatTurn[] = assembleFinalTurns(
        [{ role: "user", content: displayInput, timestamp: Date.now() }],
        speechLogRef.current,
        [{ role: "assistant", content: raw, timestamp: Date.now() }],
        rosterRef.current,
      );
      setConversation(allTurns);
      setStatus("done"); writeStreamText("");
      chatHistoryRef.current = [
        { role: "user", content: displayInput },
        { role: "assistant", content: raw },
      ];
      const degradedFlags = pipeline.degradedRef.current;
      const entry: HistoryEntry = {
        id: newId(), mode: runMode, input: displayInput, output: raw,
        conversation: allTurns, usage: { ...pipeline.usageRef.current }, timestamp: Date.now(),
        ...(degradedFlags.length > 0 ? { degraded: [...degradedFlags] } : {}),
        ...(pipeline.validationRef.current
          ? { validation_passed: pipeline.validationRef.current.passed }
          : {}),
        ...historyPlanFields(raw, pipeline.finalPlanRef.current),
      };
      setCurrentHistoryId(entry.id);
      const updated = [entry, ...historyRef.current];
      setHistoryEntries(updated); scheduleSave(updated);
      if (queue.peek().some((q) => q.id === runId)) {
        queue.mark(runId, "done");
        queue.linkHistory(runId, entry.id);
      }
    } catch (e) {
      if (token !== runTokenRef.current) return;
      const cancelled = isCancelledError(e);
      applyRunError(e);
      const partialText = streamTextRef.current;
      const frozen = freezePartial(partialText, displayInput);
      if (frozen) {
        setConversation((prev) => [...prev, frozen.turn]);
        chatHistoryRef.current = frozen.history;
        writeStreamText("");
      }
      if (queue.peek().some((q) => q.id === runId)) {
        queue.mark(runId, terminalQueueStatus(cancelled));
        if (frozen) queue.setResult(runId, { output: partialText, partial: true });
      }
    } finally {
      setRunningQueueId(null);
      const next = dequeueNext(queue.peek());
      if (next) {
        setRunningQueueId(next.id);
        queue.mark(next.id, "running");
        void runGenerateNow(next.id, next.mode, next.userInput, next.extra);
      }
    }
  }, [settings, ensureLlmListener, pipeline, applyRunError, clearRunError, queue, scheduleSave, writeStreamText]);

  const handleGenerate = useCallback(async (userInput: string, extra?: { originalLyrics?: string; douyinScene?: string }) => {
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
  }, [mode, pipeline.active, queue, runGenerateNow]);

  const handleRefine = useCallback(async (feedback: string, refineMode: "fast" | "full" = "fast") => {
    if (!feedback.trim()) return;
    const token = ++runTokenRef.current;
    setStatus("loading"); writeStreamText(""); clearRunError();
    setLastFeedback(feedback);
    speechLogRef.current = [];
    setHistoryView(null);

    const currentHistory = [...chatHistoryRef.current];

    let raw: string;
    try {
      await ensureLlmListener();
      let originalLyrics: string | undefined;
      let input = lastUserInput;
      if (mode === "mode_c") {
        const m = lastUserInput.match(/原歌词：\n([\s\S]*)\n\n新主题：\n([\s\S]*)$/);
        if (m) {
          originalLyrics = m[1];
          input = m[2];
        }
      }
      const lastMsg = [...currentHistory].reverse().find((m) => m.role === "assistant");
      const lastOutputPartial = lastMsg?.partial === true;
      // 关键协议对齐：fast 增量模式且存在有效 SongPlan IR 时，传入 SongPlan JSON 字符串作为【上一版方案】，
      // 使后端 plan_from_checkpoint 校验通过并真正跳过阶段 0 初稿，直接跑目标角色增量审改；
      // full 深度重做或半成品则传渲染文本，由阶段 0 重算初稿。
      const activePlan = lastPlanRef.current ?? pipeline.finalPlanRef.current;
      const lastOutput =
        refineMode === "fast" && !lastOutputPartial && activePlan && typeof activePlan === "object"
          ? JSON.stringify(activePlan)
          : (lastMsg?.content || "");
      const estimated = refineMode === "fast" ? estimateRefineTargets(feedback, mode) : [];
      raw = await pipeline.refine({
        mode,
        userInput: input,
        lastOutput,
        feedback,
        settings,
        extra: undefined,
        originalLyrics,
        douyinScene: mode === "mode_d" ? lastDouyinSceneRef.current : undefined,
        refineMode,
        lastOutputPartial,
        refineTargets: refineMode === "fast" && estimated.length > 0 ? estimated : undefined,
        onSpeech: (speech) => {
          if (speechLogRef.current.length === 0) writeStreamText("");
          speechLogRef.current.push(speech);
          setConversation((prev) => orderRoundSpeech([...prev, speech], rosterRef.current));
        },
      });
    } catch (e) {
      if (token !== runTokenRef.current) return;
      applyRunError(e);
      return;
    }
    if (token !== runTokenRef.current) return;

    lastPlanRef.current = pipeline.finalPlanRef.current;
    const allTurns: ChatTurn[] = assembleFinalTurns(
      conversation,
      speechLogRef.current,
      [
        { role: "user", content: feedback, timestamp: Date.now() },
        { role: "assistant", content: raw, timestamp: Date.now() },
      ],
      rosterRef.current,
    );
    setConversation(allTurns);
    setStatus("done"); writeStreamText("");
    chatHistoryRef.current = [
      ...currentHistory,
      { role: "user", content: feedback },
      { role: "assistant", content: raw },
    ];
    if (currentHistoryId) {
      updateHistoryEntry(currentHistoryId, raw, allTurns, { ...pipeline.usageRef.current }, historyPlanFields(raw, pipeline.finalPlanRef.current));
    } else {
      saveToHistory(conversation[0]?.content || lastUserInput, raw, allTurns, mode, { ...pipeline.usageRef.current });
    }
  }, [mode, settings, conversation, lastUserInput, saveToHistory, currentHistoryId, updateHistoryEntry, pipeline, ensureLlmListener, applyRunError, clearRunError, writeStreamText]);

  const canRetry =
    status === "error" &&
    (Boolean(lastFeedback) || (mode !== "mode_c" && Boolean(lastUserInput)));
  const handleRetry = useCallback(async () => {
    if (status === "error" && lastFeedback) {
      await handleRefine(lastFeedback, "fast");
    } else if (status === "error" && lastUserInput) {
      if (mode === "mode_c" && lastFeedback) {
        await handleRefine(lastFeedback, "fast");
      } else if (mode !== "mode_c") {
        await handleGenerate(lastUserInput, mode === "mode_d" ? { douyinScene: lastDouyinSceneRef.current } : undefined);
      }
    }
  }, [status, lastFeedback, lastUserInput, handleRefine, handleGenerate, mode]);

  const handleResume = useCallback(async () => {
    if (status !== "error") return;
    const token = ++runTokenRef.current;
    setStatus("loading"); writeStreamText(""); clearRunError();
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
        douyinScene: mode === "mode_d" ? lastDouyinSceneRef.current : undefined,
        onSpeech: (speech) => {
          if (speechLogRef.current.length === 0) writeStreamText("");
          speechLogRef.current.push(speech);
          setConversation((prev) => orderRoundSpeech([...prev, speech], rosterRef.current));
        },
      });
      if (token !== runTokenRef.current) return;
      lastPlanRef.current = pipeline.finalPlanRef.current;
      const allTurns: ChatTurn[] = assembleFinalTurns(
        conversation,
        speechLogRef.current,
        [{ role: "assistant", content: raw, timestamp: Date.now() }],
        rosterRef.current,
      );
      setConversation(allTurns);
      setStatus("done"); writeStreamText("");
      chatHistoryRef.current = [
        ...chatHistoryRef.current,
        { role: "assistant", content: raw },
      ];
      if (currentHistoryId) {
        updateHistoryEntry(currentHistoryId, raw, allTurns, { ...pipeline.usageRef.current }, historyPlanFields(raw, pipeline.finalPlanRef.current));
      } else {
        saveToHistory(conversation[0]?.content || lastUserInput, raw, allTurns, mode, { ...pipeline.usageRef.current });
      }
    } catch (e) {
      if (token !== runTokenRef.current) return;
      applyRunError(e);
    }
  }, [status, mode, lastUserInput, conversation, saveToHistory, currentHistoryId, updateHistoryEntry, pipeline, ensureLlmListener, applyRunError, clearRunError, writeStreamText, settings]);

  const deleteHistory = (id: string) => { const u = historyEntries.filter(e => e.id !== id); setHistoryEntries(u); scheduleSave(u); };
  const clearHistory = () => { setHistoryEntries([]); scheduleSave([]); };
  const selectHistory = (entry: HistoryEntry) => { setHistoryView(entry); setShowHistory(false); };

  const handleRefineFromHistory = useCallback(async (entry: HistoryEntry, feedback: string, refineMode: "fast" | "full" = "fast") => {
    if (!feedback.trim()) return;
    const ctx = buildHistoryRefineContext(entry);
    chatHistoryRef.current = toChatMessages(ctx.conversation);
    lastPlanRef.current = entry.song_plan ?? null;
    setConversation(ctx.conversation);
    setMode(ctx.mode);
    if (ctx.mode === "mode_d") {
      const savedScene = (entry.song_plan as { mode_meta?: { douyin?: { scene?: string } } } | undefined)?.mode_meta?.douyin?.scene;
      if (savedScene) lastDouyinSceneRef.current = savedScene;
    }
    rosterRef.current = MODE_EXPERTS[ctx.mode].map((e) => e.id);
    setLastUserInput(ctx.lastUserInput);
    setCurrentHistoryId(null);
    setHistoryView(null);
    await handleRefine(feedback, refineMode);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [handleRefine]);

  const selectQueueItem = useCallback((id: string) => {
    const item = queue.peek().find((q) => q.id === id);
    if (!item) return;
    const entry = item.historyId ? historyRef.current.find((e) => e.id === item.historyId) : undefined;
    if (entry) {
      setHistoryView(entry);
      setShowHistory(false);
      return;
    }
    const fromResult = queueResultEntry(item);
    if (fromResult) {
      setHistoryView(fromResult);
      setShowHistory(false);
    }
  }, [queue]);

  const handleCancelCurrent = useCallback(async () => {
    if (runningQueueId) {
      queue.mark(runningQueueId, "cancelled");
    }
    await pipeline.cancel();
  }, [pipeline, queue, runningQueueId]);

  const handleGateDecide = useCallback(async (decision: "continue" | "finalize") => {
    try {
      await pipeline.decideGate(decision);
    } catch {
      /* decideGate 已写入 pipeline.error */
    }
  }, [pipeline]);

  const handleGateAutoOff = useCallback(async () => {
    await handleGateDecide("continue");
    updateSettings({ roundGate: false });
  }, [handleGateDecide, updateSettings]);

  const filteredHistory = historyFilter === "all" ? historyEntries : historyEntries.filter(e => e.mode === historyFilter);
  const baseUrlIssue = formatUrlIssue(settings.baseUrl);

  return (
    <div className="h-screen flex flex-col overflow-hidden ambient-bg">
      {/* =====================================================================
          TOP STUDIO COMMAND BAR（工作台顶栏 + 四模式矩阵）
          ===================================================================== */}
      <header className="flex items-center justify-between gap-4 px-4 py-2 border-b border-border/70 shrink-0 glass-panel">
        <div className="flex items-center gap-2.5 shrink-0">
          <div className="w-8 h-8 rounded-lg brand-gradient-btn flex items-center justify-center">
            <IconSparkles size={16} className="text-white" />
          </div>
          <div>
            <div className="flex items-center gap-1.5">
              <span className="text-[13.5px] font-bold tracking-tight text-text-1">shiyi音乐</span>
              <span className="studio-mono text-[9.5px] px-1.5 py-0.2 rounded bg-brand-500/15 text-brand-400 border border-brand-500/35">
                STUDIO v6.2
              </span>
            </div>
            <div className="text-[10px] text-text-muted">Suno v6 声学提示词编译工作台</div>
          </div>
        </div>

        {/* 四模式切换矩阵 */}
        <div className="flex-1 max-w-[760px]">
          <ModeSelector mode={mode} locale={settings.language} onChange={(m) => {
            runTokenRef.current++;
            if (llmUnlistenRef.current) { llmUnlistenRef.current(); llmUnlistenRef.current = null; }
            setMode(m);
            rosterRef.current = MODE_EXPERTS[m].map((e) => e.id);
            lastPlanRef.current = null;
            setStatus("idle"); writeStreamText(""); clearRunError();
            setConversation([]); chatHistoryRef.current = [];
            setLastUserInput(""); setLastFeedback("");
            setHistoryView(null); setCurrentHistoryId(null);
          }} />
        </div>

        <div className="flex items-center gap-1.5 shrink-0">
          <button onClick={() => { setShowHistory(true); setHistoryView(null); }}
            className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-[12px] text-text-2
                       bg-surface-2 hover:bg-surface-3 border border-border transition-all duration-150 active:scale-95 cursor-pointer">
            <IconHistory size={14} /> {t(settings.language, "app.history")}
          </button>
          <button onClick={() => { setShowSettings(!showSettings); setTestResult(null); }}
            aria-label={t(settings.language, "app.settings")}
            className="flex items-center justify-center w-8 h-8 rounded-lg text-text-2
                       bg-surface-2 hover:bg-surface-3 border border-border transition-all duration-150 active:scale-95 cursor-pointer">
            <IconSettings size={15} />
          </button>
        </div>
      </header>

      {/* Settings dropdown */}
      {showSettings && (
        <div className="absolute right-4 top-14 z-40 w-[600px] p-4 glass-panel rounded-2xl border border-border
                        animate-fade-in space-y-3 max-h-[calc(100vh-4.5rem)] overflow-y-auto shadow-2xl">
          <h3 className="text-[13px] font-medium text-text-1">{t(settings.language, "settings.title")}</h3>
          <div className="grid grid-cols-2 gap-x-4 gap-y-3 items-start">
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
                         transition-all duration-150 disabled:opacity-50" placeholder={secretsReady ? "sk-...（清空后留空 = 沿用已保存的 Key）" : "密钥加载中…"} />
            <SecretSyncError message={secretErrors.global} locale={settings.language} />
            {/* P1-12 / D-04：显式清除全局 Key（二次确认） */}
            <button type="button"
              disabled={!secretsReady || !settings.apiKey}
              onClick={async () => {
                if (!window.confirm(t(settings.language, "settings.apikey.clear.confirm"))) return;
                try {
                  await clearGlobalApiKey();
                  setTestResult(null);
                } catch { /* secretErrors 已就地显示 */ }
              }}
              className="mt-1.5 text-[11px] text-danger/80 hover:text-danger disabled:opacity-40 disabled:cursor-not-allowed transition-colors">
              {t(settings.language, "settings.apikey.clear")}
            </button>
          </div>
          {/* #27-a 厂商模板卡片：点击 = 写入该厂商 baseUrl + 默认模型（不动 API Key） */}
          <ProviderTemplates baseUrl={settings.baseUrl} locale={settings.language}
            onApply={(p) => { updateSettings({ baseUrl: p.baseUrl, model: p.defaultModel }); setTestResult(null); }} />
          <div className="flex gap-2 col-span-2">
            <div className="flex-1">
              <label className="text-[11px] text-text-muted block mb-1">{t(settings.language, "settings.model")}</label>
              {/* #27-b 可搜索模型下拉：候选来自当前 baseUrl 命中的厂商模板，自由输入实时透传（不锁死） */}
              <SearchableSelect value={settings.model} options={modelsForBaseUrl(settings.baseUrl)}
                locale={settings.language} placeholder={t(settings.language, "settings.model.ph")}
                ariaLabel={t(settings.language, "settings.model")}
                onChange={(v) => { updateSettings({ model: v }); setTestResult(null); }} />
            </div>
            <div className="flex-1">
              <label className="text-[11px] text-text-muted block mb-1">{t(settings.language, "settings.baseurl")}</label>
              <input value={settings.baseUrl} onChange={e => { updateSettings({ baseUrl: e.target.value }); setTestResult(null); }}
                className="w-full bg-surface-0 border border-border/60 rounded-lg px-3 py-2 text-[13px]
                           text-text-1 focus:outline-none focus:border-brand-500/40 focus:ring-1 focus:ring-brand-500/20
                           transition-all duration-150" />
              {baseUrlIssue && (
                <p className="text-[9px] text-danger mt-1 leading-relaxed">
                  {t(settings.language, `settings.url.err.${baseUrlIssue}`)}
                </p>
              )}
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
          {/* #12 轮间确认门开关 */}
          <div className="flex items-center justify-between gap-2 rounded-lg border border-border/40 bg-surface-0/40 px-3 py-2">
            <label htmlFor="round-gate" className="text-[11px] text-text-2 cursor-pointer select-none">
              {t(settings.language, "settings.roundGate")}
              <span className="block text-[10px] text-text-muted font-normal">{t(settings.language, "settings.roundGate.desc")}</span>
            </label>
            <input id="round-gate" type="checkbox" checked={settings.roundGate ?? false}
              onChange={e => { updateSettings({ roundGate: e.target.checked }); setTestResult(null); }}
              className="w-4 h-4 accent-brand-500 cursor-pointer shrink-0" />
          </div>
          {/* P1-13：文本评估 Judge 旁路 */}
          <div className="flex items-center justify-between gap-2 rounded-lg border border-border/40 bg-surface-0/40 px-3 py-2">
            <label htmlFor="text-eval-judge" className="text-[11px] text-text-2 cursor-pointer select-none">
              {t(settings.language, "settings.textEvalJudge")}
              <span className="block text-[10px] text-text-muted font-normal">{t(settings.language, "settings.textEvalJudge.desc")}</span>
            </label>
            <input id="text-eval-judge" type="checkbox" checked={settings.textEvalJudge ?? false}
              onChange={e => { updateSettings({ textEvalJudge: e.target.checked }); setTestResult(null); }}
              className="w-4 h-4 accent-brand-500 cursor-pointer shrink-0" />
          </div>
          {/* 产出语言开关 */}
          <div className="flex items-center justify-between gap-2 rounded-lg border border-border/40 bg-surface-0/40 px-3 py-2">
            <label htmlFor="output-lang" className="text-[11px] text-text-2 cursor-pointer select-none">
              {t(settings.language, "settings.outputLang")}
              <span className="block text-[10px] text-text-muted font-normal">{t(settings.language, "settings.outputLang.desc")}</span>
            </label>
            <select id="output-lang" value={settings.outputLang ?? "zh"}
              onChange={e => { updateSettings({ outputLang: e.target.value as "zh" | "en" }); setTestResult(null); }}
              className="text-[11px] bg-surface-1 border border-border/40 rounded-md px-1.5 py-1 cursor-pointer">
              <option value="zh">{t(settings.language, "settings.outputLang.zh")}</option>
              <option value="en">{t(settings.language, "settings.outputLang.en")}</option>
            </select>
          </div>
          {/* 高级参数 */}
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
                onBlur={e => {
                  const n = Number(e.target.value);
                  if (e.target.value !== "" && Number.isFinite(n) && (n < 0 || n > 2)) {
                    updateSettings({ generation: { ...settings.generation, temperature: Math.min(2, Math.max(0, n)) } });
                  }
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
                onBlur={e => {
                  const n = Math.round(Number(e.target.value));
                  if (e.target.value !== "" && Number.isFinite(n) && (n < 1000 || n > 30000)) {
                    updateSettings({ generation: { ...settings.generation, max_tokens: Math.min(30000, Math.max(1000, n)) } });
                  }
                }}
                placeholder="默认"
                className="w-20 bg-surface-0 border border-border/60 rounded-lg px-2 py-1 text-[12px] text-text-1 focus:outline-none focus:border-brand-500/40 transition-all duration-150" />
            </div>
          </div>
          <button onClick={handleTestApi} disabled={testingApi || !settings.apiKey || !secretsReady || baseUrlIssue !== null}
            className="w-full flex items-center justify-center gap-1.5 py-2 rounded-lg text-[12px] font-medium
                       bg-surface-2 hover:bg-surface-3 border border-border/50 text-text-2
                       disabled:opacity-50 transition-all duration-150 active:scale-[0.98]">
            {testingApi ? <IconLoader size={13} className="animate-spin" /> : <IconPlug size={13} />}
            {testResult === "ok" && t(settings.language, "settings.ok")}
            {testResult === "fail" && t(settings.language, "settings.fail")}
            {testResult === null && (testingApi ? t(settings.language, "settings.testing") : t(settings.language, "settings.test"))}
          </button>
          {(() => {
            const m = settings.structuredCaps?.[capKey(settings.baseUrl, settings.model)];
            return m ? (
              <p role={m === "none" ? "alert" : undefined}
                className={`text-[10px] leading-relaxed ${m === "none" ? "text-danger" : "text-text-muted"}`}>
                {t(settings.language, `settings.structured.${m}`)}
              </p>
            ) : null;
          })()}
          <div className="space-y-1.5">
          <button onClick={handleOpenLogDir}
            className="w-full py-1.5 rounded-lg text-[11px] text-text-muted hover:text-text-2
                       bg-surface-2/60 hover:bg-surface-3/80 border border-border/40
                       transition-all duration-150 active:scale-[0.98]">
            {logDirMsg || t(settings.language, "settings.logdir")}
          </button>
          <button onClick={handleOpenConfigDir}
            className="w-full py-1.5 rounded-lg text-[11px] text-text-muted hover:text-text-2
                       bg-surface-2/60 hover:bg-surface-3/80 border border-border/40
                       transition-all duration-150 active:scale-[0.98]">
            {configDirMsg || t(settings.language, "settings.configdir")}
          </button>
          </div>
          </div>

          {/* 角色级 API 覆盖（可选） */}
          <div className="border-t border-border/40 pt-3">
            <button onClick={() => setShowRoleApi(!showRoleApi)}
              className="w-full flex items-center justify-between text-[11px] text-text-2 hover:text-text-1 transition-colors">
              <span>{t(settings.language, "settings.roles")}</span>
              <span className="text-[10px] text-text-muted">{showRoleApi ? "收起 ▲" : "展开 ▼"}</span>
            </button>
            {showRoleApi && (
              <div className="mt-2 space-y-1.5 max-h-64 overflow-y-auto pr-0.5">
                {(Object.keys(ROLE_NAMES) as PipelineRoleKey[]).map((role) => {
                  const roleOverrideUrl = settings.roleOverrides?.[role]?.base_url?.trim() ?? "";
                  const roleBaseUrl = roleOverrideUrl || settings.baseUrl;
                  const roleUrlIssue = roleOverrideUrl ? formatUrlIssue(roleOverrideUrl) : null;
                  return (
                  <div key={role} className="rounded-lg border border-border/40 bg-surface-0/40 p-2">
                    <div className="flex items-center justify-between">
                      <span className="text-[11px] text-text-1">{ROLE_EMOJIS[role]} {ROLE_NAMES[role]}</span>
                      <div className="flex items-center gap-1.5">
                        <button
                          onClick={() => handleTestRoleApi(role)}
                          disabled={roleTestStates[role] === "testing" || roleUrlIssue !== null}
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
                        <SearchableSelect value={settings.roleOverrides?.[role]?.model ?? ""}
                          options={modelsForBaseUrl(roleBaseUrl)} locale={settings.language}
                          placeholder="模型（留空继承全局）"
                          ariaLabel={`${ROLE_NAMES[role]} 模型`}
                          onChange={(v) => updateRoleOverride(role, "model", v)}
                          inputClassName="w-full bg-surface-0 border border-border/60 rounded-lg px-2.5 py-1.5 text-[11px] text-text-1 placeholder:text-text-muted/30 focus:outline-none focus:border-brand-500/40 transition-all duration-150" />
                        <input value={settings.roleOverrides?.[role]?.api_key ?? ""}
                          onChange={e => updateRoleOverride(role, "api_key", e.target.value)}
                          placeholder="API Key（留空继承全局）"
                          className="w-full bg-surface-0 border border-border/60 rounded-lg px-2.5 py-1.5 text-[11px] text-text-1 placeholder:text-text-muted/30 focus:outline-none focus:border-brand-500/40 transition-all duration-150" />
                        <SecretSyncError message={secretErrors[`role:${role}`]} locale={settings.language} />
                        <input value={settings.roleOverrides?.[role]?.base_url ?? ""}
                          onChange={e => updateRoleOverride(role, "base_url", e.target.value)}
                          placeholder="API 地址（留空继承全局）"
                          className="w-full bg-surface-0 border border-border/60 rounded-lg px-2.5 py-1.5 text-[11px] text-text-1 placeholder:text-text-muted/30 focus:outline-none focus:border-brand-500/40 transition-all duration-150" />
                        <select value={settings.roleOverrides?.[role]?.structured_mode ?? ""}
                          aria-label={`${ROLE_NAMES[role]} 结构化输出档位`}
                          onChange={e => updateRoleStructuredMode(role, (e.target.value || undefined) as StructuredMode | undefined)}
                          className="w-full bg-surface-0 border border-border/60 rounded-lg px-2 py-1 text-[11px] text-text-1 focus:outline-none focus:border-brand-500/40">
                          <option value="">{t(settings.language, "settings.structured.auto")}</option>
                          {STRUCTURED_MODES.map((m) => <option key={m} value={m}>{m}</option>)}
                        </select>
                        {effectiveMode(settings.roleOverrides?.[role]?.structured_mode, settings.structuredCaps,
                          roleBaseUrl, settings.roleOverrides?.[role]?.model || settings.model) === "none" && (
                          <p role="alert" className="text-[9px] text-danger leading-relaxed">
                            {t(settings.language, "settings.structured.none")}
                          </p>
                        )}
                        {roleUrlIssue && (
                          <p className="text-[9px] text-danger leading-relaxed">
                            {t(settings.language, `settings.url.err.${roleUrlIssue}`)}
                          </p>
                        )}
                      </div>
                    )}
                  </div>
                  );
                })}
              </div>
            )}
            <p className="text-[9px] text-text-muted mt-1.5 leading-relaxed">
              不配置 = 所有角色共用全局 API；给某角色单独配置后，该角色用自己的，留空的字段仍继承全局
            </p>
          </div>

          {/* 配置导入导出 */}
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

      {/* =====================================================================
          MAIN STUDIO WORKBENCH（左：输入与编排机架 · 右：四栏母带与蓝图舞台）
          ===================================================================== */}
      <div className="flex-1 flex overflow-hidden min-h-0">
        {/* 左侧机架（410px）：01 创作与声学预设台 + 02 编排管线与角色通道 */}
        <aside className="w-[410px] shrink-0 flex flex-col border-r border-border/70 bg-surface-1/35 overflow-y-auto">
          {/* 01 · 创作立意与 18 项受控声学矩阵输入台 */}
          <div className="p-3.5 border-b border-border/70">
            <InputPanel
              key={mode}
              mode={mode}
              disabled={status === "loading" || status === "streaming" || !secretsReady}
              settings={settings}
              onGenerate={handleGenerate}
              inputRef={inputRef}
              locale={settings.language}
            />
          </div>

          {/* 02 · 编排管线与专家声学通道 */}
          <div className="p-3.5 flex-1">
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
            {pipeline.cancelNotice && (
              <p role="status" className="mt-2 text-[11px] text-text-muted">{pipeline.cancelNotice}</p>
            )}

            {detailExpert && (
              <div className="mt-3 p-3 studio-card rounded-xl border border-border animate-fade-in">
                <div className="flex items-center justify-between mb-1.5">
                  <span className="text-[12px] font-semibold text-text-1">
                    {detailExpert.emoji} {detailExpert.name}
                  </span>
                  <button onClick={() => setDetailExpert(null)} className="text-text-muted hover:text-text-1 text-[11px] cursor-pointer">
                    {t(settings.language, "detail.close")}
                  </button>
                </div>
                <p className="text-[11px] text-text-2 leading-relaxed">
                  {t(settings.language, "detail.knowledge")}📚 {detailExpert.knowledge.length > 0 ? detailExpert.knowledge.join(", ") + ".csv" : t(settings.language, "detail.none")}
                </p>
                <p className="text-[10.5px] text-text-muted mt-1.5 whitespace-pre-wrap leading-relaxed">
                  {detailExpert.status === "working" && t(settings.language, "detail.working")}
                  {detailExpert.status === "done" && `${t(settings.language, "detail.done")}${detailExpert.note || t(settings.language, "detail.done.empty")}`}
                  {detailExpert.status === "error" && `${t(settings.language, "detail.error")}${detailExpert.note || errText(detailExpert.note)}`}
                  {detailExpert.status === "idle" && t(settings.language, "detail.idle")}
                </p>
              </div>
            )}
          </div>
        </aside>

        {/* 右侧主舞台：遥测状态条 + 轮间门 + 四栏工程交付与蓝图检视台 */}
        <main className="flex-1 flex flex-col overflow-hidden min-w-0">
          {status !== "idle" && (
            <div className="shrink-0">
              <StatusIndicator
                status={status}
                errorMessage={errorMessage}
                onRetry={handleRetry}
                canRetry={canRetry}
                onResume={handleResume}
                onCancel={handleCancelCurrent}
                getRunId={pipeline.getRunId}
                locale={settings.language}
                backoff={pipeline.backoff}
                errorKind={errorKind}
                stageNote={pipeline.stageNote}
              />
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

          {pipeline.gate && (
            <div className="shrink-0">
              <RoundGateBar
                gate={pipeline.gate}
                onDecide={handleGateDecide}
                onAutoOff={handleGateAutoOff}
                getRunId={pipeline.getRunId}
                locale={settings.language}
              />
            </div>
          )}

          {historyView && (
            <div className="mx-4 mt-2.5 px-3 py-2 bg-brand-500/10 border border-brand-500/30 rounded-lg flex items-center justify-between shrink-0">
              <span className="text-[11.5px] text-brand-400 font-medium truncate">
                历史快照回放 · {MODE_LABELS[historyView.mode]} · {new Date(historyView.timestamp).toLocaleString("zh-CN", { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" })}
              </span>
              <button onClick={() => setHistoryView(null)}
                className="text-[11px] text-brand-400 hover:text-text-1 font-semibold shrink-0 ml-2 cursor-pointer">
                返回当前工作台
              </button>
            </div>
          )}

          <div className="flex-1 overflow-y-auto p-4">
            {historyView ? (() => {
              const ctx = buildHistoryRefineContext(historyView);
              return (
                <ResultPanel
                  conversation={ctx.conversation}
                  streamText=""
                  status="done"
                  onRefine={(feedback, refineMode) => { handleRefineFromHistory(historyView, feedback, refineMode); }}
                  mode={historyView.mode}
                  locale={settings.language}
                  songPlan={(historyView.song_plan as SongPlan | null) ?? null}
                  validationPassed={historyView.validation_passed}
                  degraded={historyView.degraded}
                />
              );
            })() : (
              <ResultPanel
                conversation={conversation}
                streamText={streamText}
                status={status}
                onRefine={handleRefine}
                mode={mode}
                locale={settings.language}
                songPlan={pipeline.finalPlan}
                textEval={pipeline.textEval}
                validationPassed={pipeline.validation?.passed}
                degraded={pipeline.degradedRef.current}
              />
            )}
          </div>
        </main>
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

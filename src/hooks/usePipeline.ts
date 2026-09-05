import { useState, useCallback, useRef, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppSettings,
  ChatTurn,
  Mode,
  PipelineEvent,
  PipelineRequest,
  ExpertCard,
} from "../types";
import { errText } from "../types";

/** 各模式的流水线角色阵容（与后端 orchestrator.rs steps_for_mode 对齐） */
export const MODE_EXPERTS: Record<Mode, Omit<ExpertCard, "status" | "note">[]> = {
  // 想法模式(B)：情感 → 作词 → 制作 + 固定（主持/校验）
  mode_b: [
    { id: "emotion", name: "情感分析师", emoji: "🎭", color: "#f5a3b7", knowledge: ["emotions"] },
    { id: "lyricist", name: "作词人", emoji: "📝", color: "#a78bfa", knowledge: ["cliches", "hooks"] },
    { id: "producer", name: "制作人", emoji: "🎤", color: "#f5b35c", knowledge: ["style_genre", "instruments", "suno_rules"] },
    { id: "host", name: "主持人", emoji: "👑", color: "#f59e0b", knowledge: [] },
    { id: "auditor", name: "校验员", emoji: "🔍", color: "#a3a3a3", knowledge: ["suno_rules"] },
  ],
  // 歌词模式(A)：情感 → 制作 + 固定（无作词）
  mode_a: [
    { id: "emotion", name: "情感分析师", emoji: "🎭", color: "#f5a3b7", knowledge: ["emotions"] },
    { id: "producer", name: "制作人", emoji: "🎤", color: "#f5b35c", knowledge: ["style_genre", "instruments", "suno_rules"] },
    { id: "host", name: "主持人", emoji: "👑", color: "#f59e0b", knowledge: [] },
    { id: "auditor", name: "校验员", emoji: "🔍", color: "#a3a3a3", knowledge: ["suno_rules"] },
  ],
  // 改写模式(C)：改词 → 制作 + 固定
  mode_c: [
    { id: "reviser", name: "改词人", emoji: "✍️", color: "#7ec8a0", knowledge: ["cliches"] },
    { id: "producer", name: "制作人", emoji: "🎤", color: "#f5b35c", knowledge: ["style_genre", "instruments", "suno_rules"] },
    { id: "host", name: "主持人", emoji: "👑", color: "#f59e0b", knowledge: [] },
    { id: "auditor", name: "校验员", emoji: "🔍", color: "#a3a3a3", knowledge: ["suno_rules"] },
  ],
  // 抖音模式(D)：情感 → 作词 → 流行 → 制作 + 固定
  mode_d: [
    { id: "emotion", name: "情感分析师", emoji: "🎭", color: "#f5a3b7", knowledge: ["emotions"] },
    { id: "lyricist", name: "作词人", emoji: "📝", color: "#a78bfa", knowledge: ["cliches", "hooks"] },
    { id: "style_analyst", name: "流行风格分析师", emoji: "🔥", color: "#f472b6", knowledge: ["hooks"] },
    { id: "producer", name: "制作人", emoji: "🎤", color: "#f5b35c", knowledge: ["style_genre", "instruments", "suno_rules"] },
    { id: "host", name: "主持人", emoji: "👑", color: "#f59e0b", knowledge: [] },
    { id: "auditor", name: "校验员", emoji: "🔍", color: "#a3a3a3", knowledge: ["suno_rules"] },
  ],
};

/** 角色名映射（后端 PipelineRole 对齐） */
export const ROLE_NAMES: Record<string, string> = {
  host: "主持人",
  auditor: "校验员",
  emotion: "情感分析师",
  lyricist: "作词人",
  reviser: "改词人",
  producer: "制作人",
  style_analyst: "流行风格分析师",
};

/** 角色 emoji（对话流发言展示用，与后端/App 对齐） */
export const ROLE_EMOJIS: Record<string, string> = {
  host: "👑",
  auditor: "🔍",
  emotion: "🎭",
  lyricist: "📝",
  reviser: "✍️",
  producer: "🎤",
  style_analyst: "🔥",
};



export interface PipelineRunOptions {
  mode: Mode;
  userInput: string;
  settings: AppSettings;
  extra?: string;
  /** 角色发言回调：每个专家产出/主持收口/校验结果时调用（追加到对话流） */
  onSpeech?: (speech: ChatTurn) => void;
}

export interface PipelineRefineOptions {
  mode: Mode;
  userInput: string;
  lastOutput: string;
  feedback: string;
  settings: AppSettings;
  extra?: string;
  /** 角色发言回调（同上） */
  onSpeech?: (speech: ChatTurn) => void;
}

interface PipelineState {
  active: boolean;
  experts: ExpertCard[];
  phase: "discussing" | "synthesizing" | "validating" | "done";
  validation: { passed: boolean; issues: string[] } | null;
  error: string | null;
  currentStage: string | null;
  doneStages: string[];
}

/** 组装角色级 API 覆盖：过滤全空/全空格条目 + 非法角色 key（无覆盖的角色的不传给后端） */
const buildRoleOverrides = (settings: AppSettings): PipelineRequest["role_overrides"] => {
  const overrides = settings.roleOverrides ?? {};
  const entries = Object.entries(overrides).filter(([k, v]) =>
    ROLE_NAMES[k] && v && (v.model?.trim() || v.api_key?.trim() || v.base_url?.trim())
  );
  return entries.length > 0 ? Object.fromEntries(entries) : undefined;
};

/** 初始阵容（按模式，idle 围坐展示） */
const makeInitialExperts = (mode: Mode): ExpertCard[] => {
  return (MODE_EXPERTS[mode] ?? []).map((e) => ({ ...e, status: "idle" as const, note: "" }));
};

/**
 * 流水线讨论 hook（v2）：
 * - 监听后端 "pipeline" 事件（流水线阶段/主持/校验）
 * - 调用 pipeline_generate / pipeline_refine
 * - 最终文本的流式输出由调用方通过 llm-chunk 通道处理
 */
export function usePipeline() {
  const [state, setState] = useState<PipelineState>({
    active: false,
    // 初始摆 Mode D 座位（与 App 默认模式一致，避免首帧阵容闪烁）
    experts: makeInitialExperts("mode_d"),
    phase: "discussing",
    validation: null,
    error: null,
    currentStage: null,
    doneStages: [],
  });

  const unlistenRef = useRef<UnlistenFn | null>(null);
  /** run 递增 token：新 run/切模式后，过期 run 的 pipeline 事件一律丢弃（H4 修复） */
  const runTokenRef = useRef(0);
  /** 当前 run 的发言回调（step_done/host_done/audit_result 时调用，追加对话流） */
  const speechCbRef = useRef<((s: ChatTurn) => void) | null>(null);

  /** 构造一条专家发言（对话流用） */
  const makeSpeech = useCallback((id: string, content: string): ChatTurn => ({
    role: "expert",
    content,
    timestamp: Date.now(),
    speaker: { id, emoji: ROLE_EMOJIS[id] ?? "🎙️", name: ROLE_NAMES[id] ?? id },
  }), []);

  const cleanup = useCallback(async () => {
    if (unlistenRef.current) {
      await unlistenRef.current();
      unlistenRef.current = null;
    }
  }, []);

  useEffect(() => {
    return () => {
      cleanup();
    };
  }, [cleanup]);

  /** 按专家 id 更新（name 匹配在名称漂移时会静默失效） */
  const updateExpert = useCallback((expertId: string, patch: Partial<ExpertCard>) => {
    setState((prev) => ({
      ...prev,
      experts: prev.experts.map((e) => (e.id === expertId ? { ...e, ...patch } : e)),
    }));
  }, []);

  /** 订阅进度事件（每次 run/refine 前先清理旧订阅；token 校验丢弃过期 run 的事件） */
  const startListening = useCallback(async (token: number) => {
    await cleanup();
    unlistenRef.current = await listen<PipelineEvent>("pipeline", (event) => {
      if (token !== runTokenRef.current) return; // 过期 run 的事件丢弃（H4）
      const e = event.payload;
      switch (e.type) {
        case "step_start":
          setState((prev) => ({ ...prev, currentStage: e.role }));
          updateExpert(e.role, { status: "working", note: "思考中…" });
          break;
        case "step_done":
          updateExpert(e.role, { status: "done", note: e.summary });
          setState((prev) => ({
            ...prev,
            doneStages: prev.doneStages.includes(e.role as string) ? prev.doneStages : [...prev.doneStages, e.role as string],
          }));
          // 角色发言进入对话流（可读摘要，非原始 JSON）
          speechCbRef.current?.(makeSpeech(e.role, e.summary));
          break;
        case "host_start":
          setState((prev) => ({ ...prev, phase: "synthesizing", currentStage: "host" }));
          updateExpert("host", {
            status: "working",
            note: e.stage === "summarize" ? "汇总讨论并分发任务中…" : "全局统领中…",
          });
          break;
        case "host_done":
          updateExpert("host", {
            status: "done",
            note: e.stage === "summarize" ? "完成汇总并分发任务" : "完成全局统领",
          });
          setState((prev) => ({
            ...prev,
            // 与 step_done 一致去重（HostDone 每次 run 发两次：initial + summarize）
            doneStages: prev.doneStages.includes("host") ? prev.doneStages : [...prev.doneStages, "host"],
          }));
          // 按阶段区分发言文案（阶段0=统领初稿；阶段1=汇总修订与校验员观点）
          speechCbRef.current?.(makeSpeech(
            "host",
            e.stage === "summarize"
              ? "汇总本轮修订与校验员观点，产出新版方案"
              : "完成全局统领，产出方案初稿",
          ));
          break;
        case "audit_start":
          setState((prev) => ({
            ...prev,
            phase: "validating",
            currentStage: "auditor",
            // 重置 auditor 的完成态：讨论轮审查已完成 ≠ 最终格式输出已完成（阶段条 chip 与卡片状态一致）
            doneStages: prev.doneStages.filter((s) => s !== "auditor"),
            validation: null,
          }));
          updateExpert("auditor", { status: "working", note: "格式输出中…" });
          break;
        case "audit_result":
          updateExpert("auditor", {
            status: "done",
            note: e.pass ? "格式输出完成" : `格式问题 ${e.findings.length} 条`,
          });
          setState((prev) => ({
            ...prev,
            phase: "validating",
            currentStage: null,
            validation: { passed: e.pass, issues: e.findings },
            // 审查通过时清掉打回错误（H6 修复）
            error: e.pass ? null : prev.error,
          }));
          speechCbRef.current?.(makeSpeech(
            "auditor",
            e.pass
              ? "标准格式输出完成 ✅"
              : `格式输出发现 ${e.findings.length} 个问题：${e.findings.slice(0, 3).join("；")}${e.findings.length > 3 ? "…" : ""}`,
          ));
          break;
        case "retry":
          // P4：打回过程可见——auditor 卡片状态 + 对话流发言 + 错误提示
          setState((prev) => ({ ...prev, error: `格式打回重做：${e.reason}` }));
          updateExpert("auditor", { status: "working", note: "格式打回重做中…" });
          speechCbRef.current?.(makeSpeech(
            "auditor",
            `格式打回：${e.reason}，重新格式化中…`,
          ));
          break;
        case "discussion_round": {
          // 一轮讨论结束：提出修订的角色（含校验员如有异议）已由主持人汇总产出新版方案
          const names = (e.roles ?? []).map((r) => ROLE_NAMES[r] ?? r).join("、");
          speechCbRef.current?.(makeSpeech(
            "host",
            `第 ${e.round} 轮讨论完成：${names} 提出修订，主持人已汇总产出新版方案`,
          ));
          break;
        }
        case "cancelled":
          // B3：用户取消——回到空闲，不标红为错误
          setState((prev) => ({ ...prev, error: null, phase: "done", active: false, currentStage: null }));
          break;
        case "failed":
          // active:false + 清 currentStage：防后端只发 Failed 不返回 Err 时 UI 永久卡"进行中"
          setState((prev) => ({ ...prev, error: e.error, phase: "done", active: false, currentStage: null }));
          break;
      }
    });
  }, [cleanup, updateExpert]);

  const finishRun = useCallback(() => {
    setState((prev) => ({ ...prev, active: false, phase: "done", error: null }));
    cleanup();
  }, [cleanup]);

  /** 流水线运行核心：状态初始化 + 事件监听 + request 组装 + invoke + 清理（run/refine 共用，低危#2 重构） */
  const startRun = useCallback(
    async (
      opts: PipelineRunOptions & { lastOutput?: string },
      token: number,
      command: "pipeline_generate" | "pipeline_refine",
      feedback?: string,
    ): Promise<string> => {
      speechCbRef.current = opts.onSpeech ?? null;
      setState({
        active: true,
        experts: makeInitialExperts(opts.mode),
        phase: "discussing",
        validation: null,
        error: null,
        currentStage: null,
        doneStages: [],
      });

      try {
        await startListening(token);
      } catch (e) {
        // 监听失败：状态复位并上抛（App 层 catch 展示错误，不卡死在"进行中"）
        setState((prev) => ({ ...prev, active: false, phase: "done", error: errText(e) }));
        throw e;
      }

      const isRefine = command === "pipeline_refine";
      // P1：refine 时上一版方案注入 user_input（主持人阶段0可见上一版+反馈，优化有对照）
      const request: PipelineRequest = {
        mode: opts.mode,
        user_input: isRefine && opts.lastOutput
          ? `${opts.userInput}\n\n【上一版方案】\n${opts.lastOutput}`
          : opts.userInput,
        model: opts.settings.model,
        api_key: opts.settings.apiKey,
        base_url: opts.settings.baseUrl,
        extra: opts.extra,
        // 旧 localStorage 可能缺字段（合并默认值后恒为 boolean，兜底 || false）
        thinking: opts.settings.thinking ?? false,
        role_overrides: buildRoleOverrides(opts.settings),
      };

      try {
        const args = isRefine && feedback !== undefined ? { request, feedback } : { request };
        const text = await invoke<string>(command, args);
        finishRun();
        return text;
      } catch (e) {
        setState((prev) => ({ ...prev, active: false, phase: "done", error: errText(e) }));
        throw e;
      } finally {
        cleanup();
      }
    },
    [startListening, finishRun, cleanup]
  );

  /** 启动流水线生成；返回最终文本（或抛错） */
  const run = useCallback(
    async (opts: PipelineRunOptions): Promise<string> => {
      const token = ++runTokenRef.current; // 作废旧 run（H4）
      return startRun(opts, token, "pipeline_generate");
    },
    [startRun]
  );

  /** 优化：按反馈路由重跑相关专家 */
  const refine = useCallback(
    async (opts: PipelineRefineOptions): Promise<string> => {
      const token = ++runTokenRef.current; // 作废旧 run（H4）
      return startRun(opts, token, "pipeline_refine", opts.feedback);
    },
    [startRun]
  );

  /** B3：请求取消当前生成（后端检查点中断 + Cancelled 事件回传） */
  const cancel = useCallback(async () => {
    runTokenRef.current++; // 作废在途 run 的事件（H4 双保险）
    try {
      await invoke("cancel_pipeline");
    } catch {
      // 取消命令本身失败不展示（已无在途任务可取消时属正常）
    }
    setState((prev) => ({ ...prev, active: false, phase: "done", error: null, currentStage: null }));
  }, []);

  /** 重置 */
  const reset = useCallback(
    (mode: Mode = "mode_a") => {
      runTokenRef.current++; // 作废在途 run（H4）
      speechCbRef.current = null;
      cleanup();
      // B3：重置同时请求后端取消在途任务（补旧遗漏：切模式只作废事件会导致后台继续烧钱）
      invoke("cancel_pipeline").catch(() => {});
      setState({
        active: false,
        experts: makeInitialExperts(mode),
        phase: "discussing",
        validation: null,
        error: null,
        currentStage: null,
        doneStages: [],
      });
    },
    [cleanup]
  );

  // 保留 run/refine/reset 命名（App 调用不变）+ B3 的 cancel
  return { ...state, run, refine, reset, cancel };
}

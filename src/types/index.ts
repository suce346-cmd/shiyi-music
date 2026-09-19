export type Mode = "mode_a" | "mode_b" | "mode_c" | "mode_d";

export const MODE_LABELS: Record<Mode, string> = {
  mode_a: "Mode A",
  mode_b: "Mode B",
  mode_c: "Mode C",
  mode_d: "Mode D",
};

export type LLMStatus = "idle" | "loading" | "streaming" | "done" | "error";

export interface AppSettings {
  apiKey: string;
  model: string;
  baseUrl: string;
  /** 思考模式：开启后后端按模型能力路由表自动注入厂商思考参数（先推理再回答，更稳但更慢） */
  thinking: boolean;
  /** 角色级 API 覆盖（可选）：某角色配了就用配的，空字段继承全局；不配 = 全用全局 */
  roleOverrides?: Partial<Record<PipelineRoleKey, RoleApiOverride>>;
  /** 生成参数覆盖（缺省走后端内置默认；旧数据无此字段） */
  generation?: GenerationConfig;
  /** 主题（缺省跟随系统；旧数据无此字段） */
  theme?: ThemeMode;
  /** 界面语言（缺省中文；旧数据无此字段） */
  language?: Locale;
  /** #12 轮间人工确认门：每轮结束暂停等用户决断。
   *  设置默认开启（旧数据无此字段 → 视为开启）；请求缺省 false = 后端不暂停。 */
  roundGate?: boolean;
  /** 产出语言三态（"zh" 全中文 / "mix" 中文歌词+英文 Style / "en" 全英文）：
   *  界面语言与专家讨论始终为中文。缺省 "zh"（旧数据无此字段 → 现状行为零漂移）。 */
  outputLang?: "zh" | "mix" | "en";
}

/** 主题模式 */
export type ThemeMode = "system" | "light" | "dark";
/** 界面语言 */
export type Locale = "zh" | "en";

/** 生成参数（全字段可选，与后端 GenerationConfig 对齐） */
export interface GenerationConfig {
  temperature?: number;
  max_tokens?: number;
}

export interface ChatMessage {
  role: string;
  content: string;
  /** #10 该条 assistant 是生成中断的半成品（handleRefine 据此决定是否附带告知） */
  partial?: boolean;
}

/** 单轮对话（用户输入 / AI 回复 / 专家发言） */
export interface ChatTurn {
  role: "user" | "assistant" | "expert";
  content: string;
  timestamp: number;
  /** 专家发言时：角色信息（气泡/对话流展示用） */
  speaker?: { id: string; emoji: string; name: string };
  /** #10 半成品标记：生成中断（出错/取消）时固化的产出——
   *  可能被截断，故对外显式标注，并在以其为基础优化时明确告知模型。旧记录无此字段 */
  partial?: boolean;
}

export interface HistoryEntry {
  id: string;
  mode: Mode;
  input: string;
  output: string;
  /** 完整对话流（含原始输入 + 每轮优化）。旧记录可能没有此字段 */
  conversation?: ChatTurn[];
  /** 该次生成的累计 token 用量。旧记录没有此字段 */
  usage?: { prompt_tokens: number; completion_tokens: number };
  timestamp: number;
  /** C5/ADR-3：降级标记（flag：明细 列表）。旧记录没有此字段 */
  degraded?: string[];
  /** C5/ADR-3：终稿硬校验结论。旧记录没有此字段（与后端 serde 字段名同形 snake_case） */
  validation_passed?: boolean;
}

// ---------------------------------------------------------------------------
// 流水线（v2 新架构）类型
// ---------------------------------------------------------------------------

/** 专家在 UI 上的展示状态 */
export type ExpertUiState = "idle" | "working" | "done" | "error";

/** 专家 UI 卡片数据 */
export interface ExpertCard {
  id: string;
  name: string;
  emoji: string;
  color: string;
  /** 绑定知识库表名（来自后端 profile） */
  knowledge: string[];
  status: ExpertUiState;
  /** 完成/摘要或错误信息 */
  note: string;
}

/** 流水线角色（与 Rust PipelineRole 对齐） */
export type PipelineRoleKey =
  | "host" | "auditor" | "emotion" | "lyricist" | "reviser" | "producer" | "style_analyst";

/** 角色级 API 覆盖（可选，全字段可空；空字段 fallback 全局配置） */
export interface RoleApiOverride {
  model?: string;
  api_key?: string;
  base_url?: string;
}

/** 主持人阶段（与 Rust HostStage 对齐）：initial=阶段0统领初稿 / summarize=阶段1汇总分发 */
export type HostStage = "initial" | "summarize";

/**
 * #12 轮间确认门决断（与 Rust `gate::GateDecision::as_str` 同源）。
 * 第二十一批更正注记：原注称"wire format 由 `models::tests::round_gate_events_wire_format` 锁定"
 * ——**当时为假**，该测试只自证 Rust 侧字面量，对本联合零约束（实测改后端标签两侧全绿）。
 * 真正的**跨语言锁**是 `src/test/wireProtocol.test.ts`（`?raw` 读 `gate.rs` 逐位比对）。
 * 前端只可提交 `continue` / `finalize`；`timeout` 由后端在等待超时后自产。
 */
export type GateDecision = "continue" | "finalize" | "timeout";

/** 流水线进度事件（`type` 名与各事件字段名的**跨语言锁** = `src/test/wireProtocol.test.ts`，
 *  双向比对 Rust `models::PipelineEvent` 枚举；Rust 侧 `models::tests::*_wire_format` 只自证本侧）。 */
export type PipelineEvent =
  | { type: "step_start"; role: PipelineRoleKey }
  | { type: "step_done"; role: PipelineRoleKey; summary: string }
  | { type: "host_start"; stage: HostStage }
  | { type: "host_done"; stage: HostStage }
  | { type: "audit_start" }
  | { type: "audit_result"; pass: boolean; findings: string[] }
  | { type: "retry"; role: PipelineRoleKey; reason: string }
  | { type: "discussion_round"; round: number; roles: PipelineRoleKey[]; reason: string }
  | { type: "failed"; error: string }
  | { type: "cancelled" }
  | { type: "step_usage"; role: PipelineRoleKey; prompt_tokens: number; completion_tokens: number }
  | { type: "degraded"; flag: string; detail: string }
  /** #12 轮间确认门开启：流水线已暂停，等用户决断（timeout_secs 后自动继续） */
  | { type: "round_gate_pending"; round: number; next_round: number; timeout_secs: number }
  /** #12 门已解除（decision ∈ continue / finalize / timeout） */
  | { type: "round_gate_resolved"; round: number; decision: GateDecision }
  /** #27-c 进入退避等待（原因 + 实际等待秒数，前端据此显示倒计时） */
  | { type: "backoff"; attempt: number; wait_secs: number; reason: BackoffReason }
  /** #27-c 退避结束、即将重试（前端据此撤下倒计时） */
  | { type: "backoff_end"; attempt: number };

/**
 * #27-c 退避原因（与 Rust `llm::BackoffReason::as_str` 同源）。
 * 第二十一批更正注记：原注称"wire format 由 `llm::tests::backoff_reason_wire_format` +
 * `orchestrator::tests::backoff_notice_maps_to_pipeline_event` 锁定"——**当时为假**，
 * 这两个测试只自证 Rust 侧（后者锁"通知→事件"映射，同样不碰前端），对本联合零约束。
 * 真正的**跨语言锁**是 `src/test/wireProtocol.test.ts`（`?raw` 读 `llm.rs` 逐位比对）。
 * 取值即 i18n 文案键后缀（`status.backoff.<reason>`）——新增原因必须同时补中英文案。
 */
export type BackoffReason = "rate_limit" | "server_error" | "network";

/** #27-c 退避等待态（usePipeline 维护；StatusIndicator 据此渲染倒计时） */
export interface BackoffInfo {
  /** 第几次尝试即将重试（1 起，与后端 attempt 同义） */
  attempt: number;
  /** 后端给出的实际等待秒数（已含抖动） */
  waitSecs: number;
  reason: BackoffReason;
  /** 本端收到事件的时刻（倒计时基准；用本端时钟避免与后端时钟漂移） */
  startedAt: number;
}

/** 事件信封（后端统一包 envelope 传输；run_id 归属，旧裸事件不再出现） */
export interface PipelineEnvelope {
  run_id: string;
  event: PipelineEvent;
}

/** 后端结构化错误 {kind, message}。command 失败时 Tauri 返回该对象（非字符串）。 */
export interface AppError {
  kind: "network" | "auth" | "rate_limit" | "timeout" | "cancelled" | "parse" | "validation" | "internal";
  message: string;
}

/** 错误取文案——对象取 message，字符串原样（双形态兼容过渡期）。 */
export function errText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && typeof (e as AppError).message === "string") {
    return (e as AppError).message;
  }
  return String(e);
}

/** 结构化错误取 kind——非结构化错误返回 null（与 errText 同源：同一个 e 的两个投影） */
export function errKind(e: unknown): AppError["kind"] | null {
  if (e && typeof e === "object" && typeof (e as AppError).kind === "string") {
    return (e as AppError).kind;
  }
  return null;
}

/** 是否取消错误（Q4：取消走空闲通道，不标红；结构化 kind 与文案双认） */
export function isCancelledError(e: unknown): boolean {
  if (errKind(e) === "cancelled") return true;
  const msg = errText(e);
  return msg.includes("取消") || msg.includes("cancel");
}

/** 流水线请求 */
export interface PipelineRequest {
  mode: Mode;
  user_input: string;
  model: string;
  api_key: string;
  base_url: string;
  extra?: string;
  /** Mode C 原歌词独立字段（替代 extra 字符串拼接协议；旧后端无此字段时回退 extra） */
  original_lyrics?: string;
  /** 思考模式：后端按模型能力路由表注入厂商思考参数（与 AppSettings.thinking 对齐） */
  thinking: boolean;
  /** 角色级 API 覆盖（可选）：某角色配了就用配的，空字段继承全局 */
  role_overrides?: Partial<Record<PipelineRoleKey, RoleApiOverride>>;
  /** 增量优化目标角色（缺省=后端按反馈自动路由；旧后端忽略） */
  refine_targets?: PipelineRoleKey[];
  /** 任务归属 id（后端 envelope/取消/插话定向；旧后端忽略未知字段） */
  run_id?: string;
  /** 生成参数覆盖（缺省走后端默认；旧后端忽略） */
  generation?: GenerationConfig;
  /** #12 轮间人工确认门：true = 每轮结束暂停等用户决断；缺省/旧后端 = 全自动推进 */
  round_gate?: boolean;
  /** 产出语言（"zh" 全中文 / "mix" 中文歌词+英文 Style / "en" 全英文）：
   *  en 时流水线产出物（方案/歌词/最终提示词包）以英文输出；mix 仅 STYLE 层英文、歌词保持原语言。
   *  缺省/旧后端 = "zh"（现状行为，零漂移）；未知值后端保守按中文。 */
  output_lang?: string;
}

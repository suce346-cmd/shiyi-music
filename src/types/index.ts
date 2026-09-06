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
}

/** 单轮对话（用户输入 / AI 回复 / 专家发言） */
export interface ChatTurn {
  role: "user" | "assistant" | "expert";
  content: string;
  timestamp: number;
  /** 专家发言时：角色信息（气泡/对话流展示用） */
  speaker?: { id: string; emoji: string; name: string };
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

/** 流水线进度事件 */
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
  | { type: "step_usage"; role: PipelineRoleKey; prompt_tokens: number; completion_tokens: number };

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

/** 是否取消错误（Q4：取消走空闲通道，不标红；结构化 kind 与文案双认） */
export function isCancelledError(e: unknown): boolean {
  if (e && typeof e === "object" && (e as AppError).kind === "cancelled") return true;
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
}

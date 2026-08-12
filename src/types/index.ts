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
  | { type: "failed"; error: string };

/** 流水线请求 */
export interface PipelineRequest {
  mode: Mode;
  user_input: string;
  model: string;
  api_key: string;
  base_url: string;
  extra?: string;
  /** 思考模式：后端按模型能力路由表注入厂商思考参数（与 AppSettings.thinking 对齐） */
  thinking: boolean;
  /** 角色级 API 覆盖（可选）：某角色配了就用配的，空字段继承全局 */
  role_overrides?: Partial<Record<PipelineRoleKey, RoleApiOverride>>;
}

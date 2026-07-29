export type Mode = "mode_a" | "mode_b" | "mode_c" | "mode_d";

export const MODE_LABELS: Record<Mode, string> = {
  mode_a: "Mode A",
  mode_b: "Mode B",
  mode_c: "Mode C",
  mode_d: "Mode D",
};

export interface LLMRequest {
  mode: Mode;
  user_input: string;
  model: string;
  api_key: string;
  base_url: string;
}

export interface LLMResponse {
  raw: string;
  finish_reason?: string;
}

export interface StreamChunk {
  content: string;
}

export type LLMStatus = "idle" | "loading" | "streaming" | "done" | "error";

export interface AppSettings {
  apiKey: string;
  model: string;
  baseUrl: string;
}

export interface ChatMessage {
  role: string;
  content: string;
}

/** 单轮对话（用户输入或 AI 回复） */
export interface ChatTurn {
  role: "user" | "assistant";
  content: string;
  timestamp: number;
}

export interface RefineRequest {
  model: string;
  api_key: string;
  base_url: string;
  mode: Mode;
  history: ChatMessage[];
  feedback: string;
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

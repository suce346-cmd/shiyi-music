export type Mode = "mode_a" | "mode_c" | "mode_d";

export interface LLMRequest {
  mode: Mode;
  user_input: string;
  model: string;
  api_key: string;
  base_url: string;
}

export interface LLMResponse {
  raw: string;
}

export interface StreamChunk {
  content: string;
  finished: boolean;
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
  timestamp: number;
}
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ChatMessage, LLMRequest, LLMResponse, RefineRequest, AppSettings, Mode } from "../types";

/** LLM 调用结果：成功时 result 有值，失败时 error + partialText 有值 */
export interface LLMCallResult {
  result?: LLMResponse;
  error?: string;
  partialText?: string;
}

/** 将后端错误信息转为用户友好的中文提示 */
function friendlyError(error: string): string {
  if (error.includes("429")) return "API 限流 (429)，请稍后重试";
  if (error.includes("401") || error.includes("403")) return "API Key 无效或无权限，请检查设置";
  if (error.includes("500") || error.includes("502") || error.includes("503")) return "API 服务器异常，请稍后重试";
  if (error.includes("timeout") || error.includes("Timeout")) return "请求超时，请检查网络或稍后重试";
  if (error.includes("connect") || error.includes("Connect")) return "无法连接 API，请检查网络或 API 地址";
  return error;
}

export async function generatePrompt(
  mode: Mode,
  userInput: string,
  settings: AppSettings,
  onChunk: (text: string) => void
): Promise<LLMCallResult> {
  const request: LLMRequest = {
    mode,
    user_input: userInput,
    model: settings.model,
    api_key: settings.apiKey,
    base_url: settings.baseUrl,
  };

  let fullText = "";

  const unlisten = await listen<{ content: string }>("llm-chunk", (event) => {
    fullText += event.payload.content;
    onChunk(fullText);
  });

  try {
    const result = await invoke<LLMResponse>("generate_prompt", { request });
    return { result };
  } catch (e) {
    return { error: friendlyError(String(e)), partialText: fullText || undefined };
  } finally {
    unlisten();
  }
}

export async function refinePrompt(
  mode: Mode,
  history: ChatMessage[],
  feedback: string,
  settings: AppSettings,
  onChunk: (text: string) => void
): Promise<LLMCallResult> {
  const request: RefineRequest = {
    mode,
    history,
    feedback,
    model: settings.model,
    api_key: settings.apiKey,
    base_url: settings.baseUrl,
  };

  let fullText = "";

  const unlisten = await listen<{ content: string }>("llm-chunk", (event) => {
    fullText += event.payload.content;
    onChunk(fullText);
  });

  try {
    const result = await invoke<LLMResponse>("refine_prompt", { request });
    return { result };
  } catch (e) {
    return { error: friendlyError(String(e)), partialText: fullText || undefined };
  } finally {
    unlisten();
  }
}

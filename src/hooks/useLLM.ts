import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ChatMessage, LLMRequest, LLMResponse, RefineRequest, AppSettings, Mode } from "../types";

export async function generatePrompt(
  mode: Mode,
  userInput: string,
  settings: AppSettings,
  onChunk: (text: string) => void
): Promise<LLMResponse> {
  const request: LLMRequest = {
    mode,
    user_input: userInput,
    model: settings.model,
    api_key: settings.apiKey,
    base_url: settings.baseUrl,
  };

  let fullText = "";

  const unlisten = await listen<{ content: string; finished: boolean }>("llm-chunk", (event) => {
    if (event.payload.finished) return;
    fullText += event.payload.content;
    onChunk(fullText);
  });

  try {
    const result = await invoke<LLMResponse>("generate_prompt", { request });
    return result;
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
): Promise<LLMResponse> {
  const request: RefineRequest = {
    mode,
    history,
    feedback,
    model: settings.model,
    api_key: settings.apiKey,
    base_url: settings.baseUrl,
  };

  let fullText = "";

  const unlisten = await listen<{ content: string; finished: boolean }>("llm-chunk", (event) => {
    if (event.payload.finished) return;
    fullText += event.payload.content;
    onChunk(fullText);
  });

  try {
    const result = await invoke<LLMResponse>("refine_prompt", { request });
    return result;
  } finally {
    unlisten();
  }
}
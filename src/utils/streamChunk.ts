import type { StreamChunk } from "../types";

/** RULE-RUN-ID（BLUEPRINT_v2 §3.6 / P0-3，C-06）：llm-chunk 只接收当前 run 的流式片段。
 *  旧规则仅凭"监听注册 token"过滤：取消瞬间旧流仍在发、新 run 已注册新监听 → 旧 chunk 串进新正文。
 *  当前无 run（空 id）或载荷缺 run_id → 一律丢弃（宁丢不串）。 */
export function acceptStreamChunk(payload: Partial<StreamChunk> | null | undefined, currentRunId: string): boolean {
  if (!payload || typeof payload.content !== "string") return false;
  if (!currentRunId) return false;
  return typeof payload.run_id === "string" && payload.run_id === currentRunId;
}

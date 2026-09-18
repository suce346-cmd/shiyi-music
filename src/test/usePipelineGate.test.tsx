// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";
import { usePipeline } from "../hooks/usePipeline";
import type { AppSettings, PipelineEnvelope } from "../types";

/** #12 轮间确认门的前端接线（事件 → 状态 → 决断命令）：
 * 1. round_gate_pending 必须把门的三个数字落到 state（App 据此渲染门条）；
 * 2. round_gate_resolved 必须撤下门条（后端超时/决断都会发）；
 * 3. decideGate 走 round_gate_decide 且带上当前 run_id（无 run_id 后端无从投递决断）。
 * 事件驱动路径无法只靠源码形状锁覆盖——这里用真实事件派发钉死行为。
 */
const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

/** 捕获 listen 注册的回调，直接派发后端事件 */
let listener: ((e: { payload: PipelineEnvelope }) => void) | null = null;
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (_name: string, cb: (e: { payload: PipelineEnvelope }) => void) => {
    listener = cb;
    return () => { listener = null; };
  },
}));

const settings: AppSettings = {
  apiKey: "k", model: "m", baseUrl: "u", thinking: false, roundGate: true,
};

/** 待决的 pipeline_generate：保持运行中，事件才不会被"过期 run"过滤 */
let resolveRun: ((v: string) => void) | null = null;

const runIdOf = (): string =>
  (invokeMock.mock.calls.find((c) => c[0] === "pipeline_generate")?.[1] as { request: { run_id: string } })
    .request.run_id;

const emit = (event: PipelineEnvelope["event"]) =>
  act(() => {
    listener?.({ payload: { run_id: runIdOf(), event } });
  });

beforeEach(() => {
  listener = null;
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "pipeline_generate") {
      return new Promise<string>((res) => { resolveRun = res; });
    }
    return Promise.resolve(null);
  });
});
afterEach(() => { resolveRun = null; });

/** 起一个在途 run（不 await——保持 active 才能收事件） */
async function startInFlight(h: { result: { current: ReturnType<typeof usePipeline> } }) {
  act(() => {
    void h.result.current.run({ mode: "mode_b", userInput: "x", settings }).catch(() => {});
  });
  await waitFor(() => expect(invokeMock.mock.calls.some((c) => c[0] === "pipeline_generate")).toBe(true));
  await waitFor(() => expect(listener).not.toBeNull());
}

describe("usePipeline × #12 轮间确认门", () => {
  it("round_gate_pending 落门状态，round_gate_resolved 撤门", async () => {
    const h = renderHook(() => usePipeline());
    await startInFlight(h);
    expect(h.result.current.gate).toBeNull();

    emit({ type: "round_gate_pending", round: 1, next_round: 2, timeout_secs: 300 });
    expect(h.result.current.gate).toEqual({ round: 1, nextRound: 2, timeoutSecs: 300 });

    emit({ type: "round_gate_resolved", round: 1, decision: "continue" });
    expect(h.result.current.gate).toBeNull();

    act(() => { resolveRun?.("done"); });
  });

  it("decideGate 带当前 run_id 调 round_gate_decide，并立即撤下门条", async () => {
    const h = renderHook(() => usePipeline());
    await startInFlight(h);
    emit({ type: "round_gate_pending", round: 2, next_round: 3, timeout_secs: 300 });

    await act(async () => { await h.result.current.decideGate("finalize"); });
    expect(invokeMock).toHaveBeenCalledWith("round_gate_decide", { runId: runIdOf(), decision: "finalize" });
    expect(h.result.current.gate).toBeNull();

    act(() => { resolveRun?.("done"); });
  });

  it("无在途 run 时 decideGate 直接抛错（不静默发送空 run_id）", async () => {
    const h = renderHook(() => usePipeline());
    await expect(h.result.current.decideGate("continue")).rejects.toThrow(/run_id/);
  });

  it("生成请求透传设置里的 roundGate；缺省视为关闭", async () => {
    const h = renderHook(() => usePipeline());
    await startInFlight(h);
    const req = (invokeMock.mock.calls.find((c) => c[0] === "pipeline_generate")?.[1] as {
      request: { round_gate: boolean };
    }).request;
    expect(req.round_gate).toBe(true);
    act(() => { resolveRun?.("done"); });
  });
});
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import RoundGateBar from "../components/RoundGateBar";

/** #12 轮间确认门条（流程真的停下等决断）——三件事必须钉死：
 * 1. 数字来自事件（round/next/timeout），不是硬编码；
 * 2. 三个出口各自触发对应回调，且"以后自动推进"不会被误当成"结束讨论"；
 * 3. 暂停期间插话走 interject_feedback（按 run_id 定向），失败不静默。
 */
const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(null);
});
afterEach(cleanup);

const gate = { round: 1, nextRound: 2, timeoutSecs: 300 };
const base = { getRunId: () => "run-abc", onDecide: () => {}, onAutoOff: () => {} };

describe("RoundGateBar（#12）", () => {
  it("标题与超时提示按事件数字渲染（非硬编码）", () => {
    render(<RoundGateBar {...base} gate={gate} />);
    expect(screen.getByText("第 1 轮讨论已完成 — 是否继续第 2 轮？")).toBeTruthy();
    expect(screen.getByText("若 300 秒内未确认，将自动继续下一轮")).toBeTruthy();
  });

  it("locale=en 时整句切英文（旧硬编码中文必红）", () => {
    render(<RoundGateBar {...base} gate={gate} locale="en" />);
    expect(screen.getByText("Round 1 done — continue to round 2?")).toBeTruthy();
    expect(screen.queryByText(/第 1 轮/)).toBeNull();
  });

  it("三个出口各自触发对应回调", () => {
    const onDecide = vi.fn();
    const onAutoOff = vi.fn();
    render(<RoundGateBar {...base} gate={gate} onDecide={onDecide} onAutoOff={onAutoOff} />);
    fireEvent.click(screen.getByText("继续下一轮"));
    expect(onDecide).toHaveBeenCalledWith("continue");
    fireEvent.click(screen.getByText("结束讨论，直接出终稿"));
    expect(onDecide).toHaveBeenCalledWith("finalize");
    expect(onAutoOff).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("以后自动推进（关闭轮间确认）"));
    expect(onAutoOff).toHaveBeenCalledTimes(1);
  });

  it("插话按 run_id 定向送 interject_feedback，成功后提示已送达", async () => {
    render(<RoundGateBar {...base} gate={gate} />);
    const input = screen.getByPlaceholderText(/副歌再炸一点/);
    fireEvent.change(input, { target: { value: "副歌再炸一点" } });
    fireEvent.click(screen.getByText("发送"));
    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("interject_feedback", { runId: "run-abc", note: "副歌再炸一点" });
    });
    expect(await screen.findByText("已送达，将在下一轮讨论中纳入")).toBeTruthy();
    expect((input as HTMLInputElement).value).toBe("");
  });

  it("插话失败显式提示（不静默）", async () => {
    invokeMock.mockRejectedValueOnce(new Error("槽已满"));
    render(<RoundGateBar {...base} gate={gate} />);
    fireEvent.change(screen.getByPlaceholderText(/副歌再炸一点/), { target: { value: "x" } });
    fireEvent.click(screen.getByText("发送"));
    expect(await screen.findByText(/发送失败：/)).toBeTruthy();
  });
});
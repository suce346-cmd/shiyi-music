// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
import StatusIndicator from "../components/StatusIndicator";

/** #27-c 退避等待 UI（G1 修复落点）——必须钉死三件事：
 * 1. 原因来自事件的 reason（不是硬编码"网络错误"）——三种原因各渲染各的文案；
 * 2. 倒计时真的在走（每秒钟减 1），否则与旧"静止 UI"无异，仍是假 affordance；
 * 3. 限流终态给可操作指引（G2：429 终态此前被报成 network，用户分不清"该等"还是"该换 Key"）。
 */
const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(null);
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
  cleanup();
});

/** 退避态构造（startedAt 用本端时钟——与组件同一时钟基准） */
const backoff = (over: Partial<{ attempt: number; waitSecs: number; reason: "rate_limit" | "server_error" | "network" }> = {}) => ({
  attempt: 1,
  waitSecs: 30,
  reason: "rate_limit" as const,
  startedAt: Date.now(),
  ...over,
});

describe("StatusIndicator 退避提示（#27-c）", () => {
  it("限流退避：原因 + 倒计时 + 第几次尝试三件齐备", () => {
    render(<StatusIndicator status="loading" locale="zh" backoff={backoff({ attempt: 2, waitSecs: 30 })} />);
    expect(screen.getByText("接口限流，等待配额恢复")).toBeTruthy();
    expect(screen.getByText(/30 秒后自动重试/)).toBeTruthy();
    expect(screen.getByText(/第 2 次尝试/)).toBeTruthy();
  });

  it("三种原因各自渲染（旧规则一律报网络错误必红）", () => {
    render(<StatusIndicator status="loading" locale="zh" backoff={backoff({ reason: "server_error" })} />);
    expect(screen.getByText("服务端异常，等待恢复")).toBeTruthy();
    cleanup();
    render(<StatusIndicator status="loading" locale="zh" backoff={backoff({ reason: "network" })} />);
    expect(screen.getByText("网络中断，等待重连")).toBeTruthy();
  });

  it("倒计时真的在走：3 秒后剩余秒数减 3", () => {
    render(<StatusIndicator status="loading" locale="zh" backoff={backoff({ waitSecs: 30 })} />);
    expect(screen.getByText(/30 秒后自动重试/)).toBeTruthy();
    act(() => { vi.advanceTimersByTime(3000); });
    expect(screen.getByText(/27 秒后自动重试/)).toBeTruthy();
  });

  it("倒计时不会走成负数（等待时长被截断时下限为 0）", () => {
    render(<StatusIndicator status="loading" locale="zh" backoff={backoff({ waitSecs: 2 })} />);
    act(() => { vi.advanceTimersByTime(10_000); });
    expect(screen.getByText(/0 秒后自动重试/)).toBeTruthy();
  });

  it("无退避态时不渲染退避行（不做常驻假提示）", () => {
    render(<StatusIndicator status="loading" locale="zh" backoff={null} />);
    expect(screen.queryByText(/自动重试/)).toBeNull();
  });

  it("locale=en 整句切英文", () => {
    render(<StatusIndicator status="loading" locale="en" backoff={backoff({ attempt: 2, waitSecs: 30 })} />);
    expect(screen.getByText("Rate limited, waiting for quota")).toBeTruthy();
    expect(screen.getByText(/Auto-retry in 30s/)).toBeTruthy();
    expect(screen.getByText(/attempt 2/)).toBeTruthy();
  });

  it("限流终态渲染可操作指引（该等还是该换 Key）", () => {
    render(<StatusIndicator status="error" errorMessage="API 限流：重试 3 次后仍被拒绝" errorKind="rate_limit" locale="zh" />);
    expect(screen.getByText(/该 Key 已触发接口限流/)).toBeTruthy();
  });

  it("非限流终态不渲染限流指引（指引不能张冠李戴）", () => {
    render(<StatusIndicator status="error" errorMessage="网络错误" errorKind="network" locale="zh" />);
    expect(screen.queryByText(/该 Key 已触发接口限流/)).toBeNull();
  });
});

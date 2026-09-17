// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useSettingsWithSecrets } from "../hooks/useSettings";

/** keychain 同步语义（v0.5.5 数据丢失回路修复）：
 * 1. 全局 Key 留空保存 = 沿用钥匙串旧值（AINA 参考 SettingsPage.tsx:383 undefined 语义），不得 keychain_delete
 * 2. 密钥同步防抖：连续输入只落最终值，消除按键中间态残缺写入
 * 3. 角色级 api_key 留空 = 清除覆盖继承全局（UI 明示语义），保留 keychain_delete */

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const calls = (cmd: string) => invokeMock.mock.calls.filter((c) => c[0] === cmd);

beforeEach(() => {
  localStorage.clear();
  invokeMock.mockReset();
  invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "keychain_get") {
      if (args?.account === "global") return "ak-old-saved-key";
      return null;
    }
    return null;
  });
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

async function renderReady() {
  const h = renderHook(() => useSettingsWithSecrets());
  // 启动回填完成（keychain_get → "ak-old-saved-key"）；fake timers 下 waitFor 不可用，手动轮询
  for (let i = 0; i < 20 && !h.result.current.secretsReady; i++) {
    await act(async () => {
      await vi.runAllTimersAsync();
    });
  }
  expect(h.result.current.secretsReady).toBe(true);
  return h;
}

describe("useSettingsWithSecrets 密钥同步语义", () => {
  it("全局 Key 留空保存：不删钥匙串，内存读回旧值（留空=沿用）", async () => {
    const h = await renderReady();
    expect(h.result.current.settings.apiKey).toBe("ak-old-saved-key");

    await act(async () => {
      await h.result.current.updateSettings({ apiKey: "" });
    });

    expect(calls("keychain_delete")).toHaveLength(0);
    expect(h.result.current.settings.apiKey).toBe("ak-old-saved-key");
  });

  it("连续输入防抖：只落最终值，不写按键中间态", async () => {
    const h = await renderReady();
    await act(async () => {
      h.result.current.updateSettings({ apiKey: "ak-a" });
      h.result.current.updateSettings({ apiKey: "ak-ab" });
      h.result.current.updateSettings({ apiKey: "ak-abc" });
      await vi.advanceTimersByTimeAsync(799);
    });
    expect(calls("keychain_set")).toHaveLength(0);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    const sets = calls("keychain_set");
    expect(sets).toHaveLength(1);
    expect(sets[0][1]).toMatchObject({ account: "global", secret: "ak-abc" });
  });

  it("退出竞态：输入后 800ms 内卸载组件 → flush 以最终值写入（最终态不丢）", async () => {
    const h = await renderReady();
    await act(async () => {
      h.result.current.updateSettings({ apiKey: "ak-abc" });
      // 仅推进 100ms：处于防抖窗口内，尚未落钥匙串
      await vi.advanceTimersByTimeAsync(100);
    });
    expect(calls("keychain_set")).toHaveLength(0);

    // 卸载（等价用户 Cmd+Q 前组件树销毁）→ cleanup 应 flush pending 同步
    act(() => {
      h.unmount();
    });
    const sets = calls("keychain_set");
    expect(sets).toHaveLength(1);
    expect(sets[0][1]).toMatchObject({ account: "global", secret: "ak-abc" });
  });
});

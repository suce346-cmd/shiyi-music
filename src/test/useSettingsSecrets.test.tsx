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

/** 与 `useSettings.ts` 的同名常量逐字一致（沿用 sanitizeStored.test / persistNonSecrets.test 的既有约定） */
const STORAGE_KEY = "suno-prompt-settings";
const MIGRATED_KEY = "suno-prompt-keychain-migrated";

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

/** #7（U-2 迁移标记行为锁）：明文迁移标记只在**全部写入成功**后置位。
 * 旧规则失败也置位——标记一置，明文迁移路径再也不会重试，明文静默留在 localStorage。
 * 本组锁死两条：失败不置位且明文保留（下次启动可重试）/ 成功才置位。 */
describe("U-2 迁移标记行为锁", () => {
  it("迁移写入失败：不置标记，明文保留待下次启动重试", async () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ apiKey: "ak-plain-legacy" }));
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "keychain_set") throw new Error("keychain locked");
      return null; // keychain_get 一律 null：回填不改内存 → 不触发 persistNonSecrets 清洗
    });

    await renderReady();

    expect(localStorage.getItem(MIGRATED_KEY)).toBeNull();
    expect(localStorage.getItem(STORAGE_KEY)).toContain("ak-plain-legacy");
  });

  it("迁移写入成功：置标记（一次性迁移，不重复执行）", async () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ apiKey: "ak-plain-legacy" }));

    await renderReady();

    expect(localStorage.getItem(MIGRATED_KEY)).toBe("1");
    const sets = calls("keychain_set");
    expect(
      sets.some((c) => (c[1] as { secret?: string } | undefined)?.secret === "ak-plain-legacy"),
    ).toBe(true);
  });
});

/** C-10（P0-6）：迁移后 localStorage 不得残留明文——changed=false（回填无变化）也必须写出脱敏副本 */
describe("C-10 迁移后无条件脱敏落盘", () => {
  const noBackfill = async (cmd: string) => {
    if (cmd === "keychain_get") return null; // 回填不改内存：changed=false
    return null;
  };

  it("迁移成功：明文被哨兵化，钥匙串持有原值", async () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ apiKey: "ak-plain-legacy", roleOverrides: { emotion: { model: "m", api_key: "ak-role-plain" } } }),
    );
    // 真实语义：keychain_set 后 keychain_get 可读回（有状态 mock）
    const kc = new Map<string, string>();
    invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "keychain_set") kc.set(String(args?.account), String(args?.secret));
      if (cmd === "keychain_get") return kc.get(String(args?.account)) ?? null;
      return null;
    });
    await renderReady();
    const raw = localStorage.getItem(STORAGE_KEY) ?? "";
    expect(raw).not.toContain("ak-plain-legacy");
    expect(raw).not.toContain("ak-role-plain");
    expect(JSON.parse(raw).apiKey).toBe("__keychain__");
    expect(localStorage.getItem(MIGRATED_KEY)).toBe("1");
    expect(kc.get("global")).toBe("ak-plain-legacy");
    expect(kc.get("role:emotion")).toBe("ak-role-plain");
  });

  it("此前已迁移但仍有残留明文（旧版本遗留）：启动即清", async () => {
    localStorage.setItem(MIGRATED_KEY, "1");
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ apiKey: "ak-plain-legacy" }));
    invokeMock.mockImplementation(noBackfill);
    await renderReady();
    expect(localStorage.getItem(STORAGE_KEY)).not.toContain("ak-plain-legacy");
    expect(calls("keychain_set")).toHaveLength(0); // 已迁移不重复写钥匙串
  });

  it("迁移失败 + 角色回填 changed=true：仍不写出（防明文被哨兵化而钥匙串无副本）", async () => {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ apiKey: "ak-plain-legacy", roleOverrides: { emotion: { model: "m", api_key: "__keychain__" } } }),
    );
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "keychain_set") throw new Error("keychain locked");
      return null; // role 哨兵读回 null → changed=true
    });
    await renderReady();
    expect(localStorage.getItem(MIGRATED_KEY)).toBeNull();
    expect(localStorage.getItem(STORAGE_KEY)).toContain("ak-plain-legacy");
  });
});

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

  it("迁移写入成功（读回校验通过）：置标记", async () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ apiKey: "ak-plain-legacy" }));
    const kc = new Map<string, string>();
    invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "keychain_set") kc.set(String(args?.account), String(args?.secret));
      if (cmd === "keychain_get") return kc.get(String(args?.account)) ?? null;
      return null;
    });

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

  it("F1：已迁移标记 + 钥匙串为空 + 明文：写入无法读回 → 明文保留，界面显示原因", async () => {
    localStorage.setItem(MIGRATED_KEY, "1");
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ apiKey: "ak-plain-legacy" }));
    invokeMock.mockImplementation(noBackfill); // keychain_set 不落地、keychain_get 恒 null
    const h = await renderReady();
    expect(localStorage.getItem(STORAGE_KEY)).toContain("ak-plain-legacy");
    expect(h.result.current.settings.apiKey).toBe("ak-plain-legacy"); // 内存仍可用
    expect(h.result.current.secretErrors.global).toContain("读回不一致");
  });

  it("F1：钥匙串持有不同值 → 写入并读回校验后才清明文", async () => {
    localStorage.setItem(MIGRATED_KEY, "1");
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ apiKey: "ak-plain-legacy" }));
    const kc = new Map<string, string>([["global", "ak-other"]]);
    invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "keychain_set") kc.set(String(args?.account), String(args?.secret));
      if (cmd === "keychain_get") return kc.get(String(args?.account)) ?? null;
      return null;
    });
    await renderReady();
    expect(kc.get("global")).toBe("ak-plain-legacy");
    expect(localStorage.getItem(STORAGE_KEY)).not.toContain("ak-plain-legacy");
  });

  it("F1：钥匙串读回已等于明文 → 直接清明文，不重复写", async () => {
    localStorage.setItem(MIGRATED_KEY, "1");
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ apiKey: "ak-plain-legacy" }));
    invokeMock.mockImplementation(async (cmd: string) => (cmd === "keychain_get" ? "ak-plain-legacy" : null));
    await renderReady();
    expect(localStorage.getItem(STORAGE_KEY)).not.toContain("ak-plain-legacy");
    expect(calls("keychain_set")).toHaveLength(0);
  });

  it("F1：迁移验证完成前的设置写出也不得哨兵化明文", async () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ apiKey: "ak-plain-legacy" }));
    invokeMock.mockImplementation(async () => { throw new Error("keychain locked"); });
    const h = await renderReady();
    await act(async () => { await h.result.current.updateSettings({ model: "x" }); });
    expect(localStorage.getItem(STORAGE_KEY)).toContain("ak-plain-legacy");
    expect(h.result.current.secretErrors.global).toContain("keychain locked");
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

/** P0-10 残留：Key 被后端拒绝（格式门）→ 设置界面就地显示错误；随后保存成功 → 错误清除 */
describe("钥匙串同步失败对用户可见", () => {
  it("keychain_set 拒绝 → secretErrors 记录后端 message；再次成功 → 清除", async () => {
    let reject = true;
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "keychain_set" && reject) {
        throw { kind: "validation", message: "API Key 含空白、引号、反斜杠或非 ASCII 字符，请检查是否粘贴错误" };
      }
      if (cmd === "keychain_get") return null;
      return null;
    });
    const h = await renderReady();
    await act(async () => {
      await h.result.current.updateSettings({ apiKey: "sk bad key" });
      await vi.advanceTimersByTimeAsync(900);
    });
    expect(h.result.current.secretErrors.global).toContain("API Key 含空白");
    reject = false;
    await act(async () => {
      await h.result.current.updateSettings({ apiKey: "sk-good-key" });
      await vi.advanceTimersByTimeAsync(900);
    });
    expect(h.result.current.secretErrors.global).toBeUndefined();
  });

  it("角色级 Key 失败按 role:{id} 记录", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "keychain_set") throw { kind: "validation", message: "bad" };
      return null;
    });
    const h = await renderReady();
    await act(async () => {
      await h.result.current.updateSettings({ roleOverrides: { emotion: { api_key: "x y" } } });
      await vi.advanceTimersByTimeAsync(900);
    });
    expect(h.result.current.secretErrors["role:emotion"]).toBe("bad");
  });
});

/** W3：迁移/同步失败后用户成功再保存 → releaseHold，明文从 localStorage 消失 */
describe("W3 releaseHold after successful re-entry", () => {
  it("迁移失败保留明文 → 之后 keychain_set 成功 → 明文哨兵化且 secretErrors 清除", async () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ apiKey: "ak-plain-legacy" }));
    let allowSet = false;
    const kc = new Map<string, string>();
    invokeMock.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
      if (cmd === "keychain_set") {
        if (!allowSet) throw new Error("keychain locked");
        kc.set(String(args?.account), String(args?.secret));
        return null;
      }
      if (cmd === "keychain_get") return kc.get(String(args?.account)) ?? null;
      return null;
    });
    const h = await renderReady();
    expect(localStorage.getItem(STORAGE_KEY)).toContain("ak-plain-legacy");
    expect(h.result.current.secretErrors.global).toBeTruthy();
    allowSet = true;
    await act(async () => {
      await h.result.current.updateSettings({ apiKey: "ak-plain-legacy" });
      await vi.advanceTimersByTimeAsync(900);
    });
    expect(h.result.current.secretErrors.global).toBeUndefined();
    expect(kc.get("global")).toBe("ak-plain-legacy");
    // releaseHold 后下一次写出应哨兵化（hold 已空）
    await act(async () => { await h.result.current.updateSettings({ model: "m2" }); });
    expect(localStorage.getItem(STORAGE_KEY)).not.toContain("ak-plain-legacy");
    expect(JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}").apiKey).toBe("__keychain__");
  });
});

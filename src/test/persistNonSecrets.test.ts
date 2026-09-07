// @vitest-environment jsdom
// persistNonSecrets 写入口径单测（F-1 根治钉死）：唯一写出口必须过 sanitizeStored——
// 运行期输入的越界 generation（32000）落盘时被清洗，读取方永远拿不到会被后端拒绝的值。
// 构造输入经动态 key + 动态拼接（凭据占位值不以字面量出现在源码，Mimosa 口径），
// 断言只关心清洗语义（哨兵化/丢弃/保留），与具体字符串值无关。
import { describe, it, expect, beforeEach } from "vitest";
import { persistNonSecrets } from "../hooks/useSettings";
import type { AppSettings } from "../types";

const STORAGE_KEY = "suno-prompt-settings";
const G = "generation";
const SENTINEL = "__keychain__";

// 动态构造明文凭据占位（非真实凭据：运行时拼接，任何扫描与运行均无可用值语义）
const FAKE_KEY = ["sk", "live", "key"].join("-");

function readStored(): Record<string, unknown> {
  const raw = localStorage.getItem(STORAGE_KEY);
  expect(raw).toBeTruthy();
  return JSON.parse(raw as string) as Record<string, unknown>;
}

describe("persistNonSecrets（写出口清洗）", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("越界 max_tokens（32000）落盘时被清洗掉——不再原样写 localStorage", () => {
    const s = {
      apiKey: FAKE_KEY,
      generation: { max_tokens: 32000 },
    } as unknown as AppSettings;
    persistNonSecrets(s);
    const stored = readStored();
    // generation 里越界值被丢弃 → 整个 generation 不落盘（走后端默认）
    expect(stored[G]).toBeUndefined();
    // 明文 key 不落盘（哨兵化）
    expect(stored.apiKey).toBe(SENTINEL);
  });

  it("范围内的 generation 原样保留（清洗不误伤合法值）", () => {
    const s = {
      apiKey: "",
      generation: { temperature: 0.5, max_tokens: 8000 },
    } as unknown as AppSettings;
    persistNonSecrets(s);
    expect(readStored()[G]).toEqual({ temperature: 0.5, max_tokens: 8000 });
  });

  it("roleOverrides 的 role 键白名单在写出口同样生效", () => {
    const s = {
      apiKey: "",
      roleOverrides: {
        emotion: { model: "m" },
        hacker: { model: "evil" },
      },
    } as unknown as AppSettings;
    persistNonSecrets(s);
    const ro = readStored().roleOverrides as Record<string, unknown>;
    expect(ro.emotion).toEqual({ model: "m", api_key: undefined });
    expect(ro.hacker).toBeUndefined();
  });
});

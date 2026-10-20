import { describe, it, expect, vi, beforeEach } from "vitest";
import CAPS_SRC from "../../src-tauri/src/llm_caps.rs?raw";
import MODELS_SRC from "../../src-tauri/src/models/mod.rs?raw";
import LIB_SRC from "../../src-tauri/src/lib.rs?raw";
import TYPES_SRC from "../types/index.ts?raw";
import appSource from "../App.tsx?raw";
import { capKey, STRUCTURED_MODES } from "../types";
import { sanitizeStored } from "../hooks/useSettings";
import { buildRoleOverrides, phaseProgressNote } from "../hooks/usePipeline";
import { effectiveMode, probeStructuredCaps, withProbedCap } from "../utils/structuredCaps";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invokeMock(...a) }));

/** P1-2 结构化能力：跨语言值域锁 + 缓存/解析语义 + 接线形状锁 */
describe("P1-2 StructuredMode 跨语言同源", () => {
  it("TS 值域 = Rust as_str 值域（逐位）", () => {
    const rust = [...CAPS_SRC.matchAll(/Self::\w+ => "(\w+)"/g)].map((m) => m[1]);
    expect(rust).toEqual([...STRUCTURED_MODES]);
    expect(TYPES_SRC).toContain('export type StructuredMode = "json_schema" | "json_object" | "none";');
  });

  it("PhaseKind 变体 = TS 联合", () => {
    const body = MODELS_SRC.slice(MODELS_SRC.indexOf("pub enum PhaseKind {"));
    const vs = body.slice(0, body.indexOf("}")).match(/^\s+(\w+),/gm)!.map((v) =>
      v.trim().replace(",", "").replace(/[A-Z]/g, (c, i) => (i ? "_" : "") + c.toLowerCase()));
    expect(vs).toEqual(["structuring", "parsed"]);
    expect(TYPES_SRC).toContain('export type PhaseKind = "structuring" | "parsed";');
  });

  it("capKey 与 Rust cap_key 同形（同一组向量）", () => {
    expect(capKey(" https://x/v1// ", " m ")).toBe("https://x/v1|m");
    expect(CAPS_SRC).toContain('assert_eq!(cap_key(" https://x/v1// ", " m "), "https://x/v1|m");');
  });

  it("探测命令已注册", () => {
    expect(LIB_SRC).toContain("commands::llm::probe_structured_caps,");
  });
});

describe("P1-2 缓存与解析语义", () => {
  beforeEach(() => invokeMock.mockReset());

  it("sanitizeStored 只保留合法档位", () => {
    const s = sanitizeStored(JSON.parse('{"structuredCaps":{"a|m":"json_schema","b|m":"bogus","c|m":3},"roleOverrides":{"host":{"structured_mode":"none"},"auditor":{"structured_mode":"x"}}}'));
    expect(s.structuredCaps).toEqual({ "a|m": "json_schema" });
    expect(s.roleOverrides?.host?.structured_mode).toBe("none");
    expect(s.roleOverrides?.auditor?.structured_mode).toBeUndefined();
  });

  it("withProbedCap 值不变时返回原对象（不制造 settings 变更）", () => {
    const c = withProbedCap(undefined, "https://x/v1/", "m", "json_object");
    expect(c).toEqual({ "https://x/v1|m": "json_object" });
    expect(withProbedCap(c, "https://x/v1", "m", "json_object")).toBe(c);
    expect(withProbedCap(c, "https://x/v1", "m", "none")).toEqual({ "https://x/v1|m": "none" });
  });

  it("effectiveMode：显式 > 缓存 > 未知", () => {
    const caps = { "u|m": "json_schema" as const };
    expect(effectiveMode("none", caps, "u", "m")).toBe("none");
    expect(effectiveMode(undefined, caps, "u/", "m")).toBe("json_schema");
    expect(effectiveMode(undefined, caps, "u", "z")).toBeUndefined();
  });

  it("probeStructuredCaps：失败返回 null（不把失败记成档位）", async () => {
    invokeMock.mockResolvedValueOnce("json_object");
    expect(await probeStructuredCaps("u", "k", "m")).toBe("json_object");
    expect(invokeMock).toHaveBeenCalledWith("probe_structured_caps", { baseUrl: "u", apiKey: "k", model: "m" });
    invokeMock.mockRejectedValueOnce({ kind: "auth", message: "401" });
    expect(await probeStructuredCaps("u", "k", "m")).toBeNull();
    invokeMock.mockResolvedValueOnce("weird");
    expect(await probeStructuredCaps("u", "k", "m")).toBeNull();
  });

  it("只配了档位的角色覆盖也会进请求", () => {
    const ro = buildRoleOverrides({ apiKey: "", model: "m", baseUrl: "u", thinking: false, roleOverrides: { host: { structured_mode: "json_schema" } } });
    expect(ro).toEqual({ host: { structured_mode: "json_schema" } });
  });

  it("phaseProgressNote", () => {
    expect(phaseProgressNote("structuring", null)).toContain("正在输出");
    expect(phaseProgressNote("parsed", " 主歌×2 ")).toBe("主歌×2");
    expect(phaseProgressNote("parsed", null)).toBe("方案已通过校验");
  });
});

describe("P1-2 设置面板接线形状锁", () => {
  it("全局/角色测试成功后都探测并写缓存", () => {
    expect(appSource.match(/await probeStructuredCaps\(/g)?.length).toBe(2);
    expect(appSource.match(/structuredCaps: withProbedCap\(settingsRef\.current\.structuredCaps/g)?.length).toBe(2);
  });

  it("none 档对用户可见（role=alert + 文案键），且角色可显式选择档位", () => {
    expect(appSource).toContain('t(settings.language, "settings.structured.none")');
    expect(appSource).toContain('role={m === "none" ? "alert" : undefined}');
    expect(appSource).toContain("updateRoleStructuredMode(role,");
  });

  it("探测写缓存不清空测试徽标（重置依赖排除 structuredCaps）", () => {
    expect(appSource).toContain("JSON.stringify({ ...settings, structuredCaps: undefined })");
    expect(appSource).toContain("}, [settingsSansCaps]);");
  });
});

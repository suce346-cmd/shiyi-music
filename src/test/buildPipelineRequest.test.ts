import { describe, it, expect } from "vitest";
import { buildPipelineRequest, type RequestKind } from "../hooks/usePipeline";
import { composeRefineInput } from "../utils/partialDraft";
import type { AppSettings, OutputLang } from "../types";

// C-04（BLUEPRINT_v2 P0-2）：run/refine/resume 请求组装单源。
// 旧规则 resume 手写字面量漏传 output_lang → 后端缺省 "zh"，用户 en/mix 设置在续跑被静默忽略。
const makeSettings = (patch: Partial<AppSettings> = {}): AppSettings => ({
  apiKey: "",
  model: "gpt-4o",
  baseUrl: "https://api.openai.com/v1",
  thinking: false,
  roleOverrides: {},
  ...patch,
});
const base = { mode: "mode_b" as const, userInput: "雨天", extra: undefined, originalLyrics: undefined };
const KINDS: RequestKind[] = ["generate", "refine", "resume"];
const LANGS: OutputLang[] = ["zh", "mix", "en"];

describe("buildPipelineRequest（C-04 单源）", () => {
  it("三条路径都透传 output_lang（回归：resume 曾漏传）", () => {
    for (const kind of KINDS) {
      for (const lang of LANGS) {
        const req = buildPipelineRequest(kind, { ...base, settings: makeSettings({ outputLang: lang }) }, "r1");
        expect(req.output_lang, `${kind}/${lang}`).toBe(lang);
      }
    }
  });

  it("设置缺 outputLang（旧数据）→ 三路径一致缺省 zh", () => {
    for (const kind of KINDS) {
      expect(buildPipelineRequest(kind, { ...base, settings: makeSettings() }, "r1").output_lang).toBe("zh");
    }
  });

  it("resume：round_gate 恒 false、不带 refine_targets、复用传入 runId", () => {
    const req = buildPipelineRequest(
      "resume",
      { ...base, settings: makeSettings({ roundGate: true }), refineMode: "fast", refineTargets: ["producer"] },
      "keep-me",
    );
    expect(req.round_gate).toBe(false);
    expect(req.refine_targets).toBeUndefined();
    expect(req.run_id).toBe("keep-me");
    expect(req.user_input).toBe("雨天");
  });

  it("generate/refine：round_gate 随设置；缺省 false", () => {
    for (const kind of ["generate", "refine"] as const) {
      expect(buildPipelineRequest(kind, { ...base, settings: makeSettings({ roundGate: true }) }, "r").round_gate).toBe(true);
      expect(buildPipelineRequest(kind, { ...base, settings: makeSettings() }, "r").round_gate).toBe(false);
    }
  });

  it("refine：上一版注入 user_input（composeRefineInput 单源）；fast 透传 targets，full 不传", () => {
    const s = makeSettings();
    const fast = buildPipelineRequest(
      "refine",
      { ...base, settings: s, lastOutput: "上一版", lastOutputPartial: true, refineMode: "fast", refineTargets: ["producer"] },
      "r",
    );
    expect(fast.user_input).toBe(composeRefineInput("雨天", "上一版", true));
    expect(fast.refine_targets).toEqual(["producer"]);
    const full = buildPipelineRequest(
      "refine",
      { ...base, settings: s, lastOutput: "上一版", refineMode: "full", refineTargets: ["producer"] },
      "r",
    );
    expect(full.refine_targets).toBeUndefined();
  });

  it("generate：不注入上一版、不带 targets（即使传了 refine 字段）", () => {
    const req = buildPipelineRequest(
      "generate",
      { ...base, settings: makeSettings(), lastOutput: "x", refineMode: "fast", refineTargets: ["producer"] },
      "r",
    );
    expect(req.user_input).toBe("雨天");
    expect(req.refine_targets).toBeUndefined();
  });

  it("W6：structured_caps 三路径透传（sanitize 后的探测缓存）", () => {
    const caps = { "https://x/v1|m": "json_schema" as const };
    for (const kind of KINDS) {
      const req = buildPipelineRequest(kind, { ...base, settings: makeSettings({ structuredCaps: caps }) }, "r1");
      expect(req.structured_caps, kind).toEqual(caps);
    }
    // 缺省 / 空 → undefined（不传脏对象）
    for (const kind of KINDS) {
      expect(buildPipelineRequest(kind, { ...base, settings: makeSettings() }, "r1").structured_caps).toBeUndefined();
    }
  });

  it("三路径共享字段完全一致（除 kind 显式差异外无漂移）", () => {
    const s = makeSettings({ thinking: true, outputLang: "en", roundGate: false });
    const [g, r, m] = KINDS.map((k) => buildPipelineRequest(k, { ...base, settings: s }, "same"));
    for (const other of [r, m]) {
      expect(other).toEqual(g);
    }
  });
});

  it("P1-7：douyin_scene 透传（mode_d）", () => {
    const req = buildPipelineRequest(
      "generate",
      { ...base, mode: "mode_d", settings: makeSettings(), douyinScene: "dance" },
      "r",
    );
    expect(req.douyin_scene).toBe("dance");
    expect(buildPipelineRequest("generate", { ...base, settings: makeSettings() }, "r").douyin_scene).toBeUndefined();
  });

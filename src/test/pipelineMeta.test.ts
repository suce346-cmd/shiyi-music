import { describe, it, expect } from "vitest";
import { buildExpertsFromMeta, ROLE_COLORS, type PipelineMeta } from "../hooks/usePipeline";

const meta: PipelineMeta = {
  // R6 口径：modes 为座位（含主持/校验落座），与后端 seats_for_mode 对齐
  modes: {
    mode_a: ["emotion", "producer", "host", "auditor"],
    mode_d: ["emotion", "lyricist", "style_analyst", "producer", "host", "auditor"],
  },
  roles: {
    emotion: { name: "情感分析师", emoji: "🎭", knowledge: ["emotions"] },
    producer: { name: "制作人", emoji: "🎤", knowledge: ["style_genre"] },
    lyricist: { name: "作词人", emoji: "📝", knowledge: ["cliches"] },
    style_analyst: { name: "流行风格分析师", emoji: "🔥", knowledge: ["hooks"] },
    host: { name: "主持人", emoji: "👑", knowledge: [] },
    auditor: { name: "校验员", emoji: "🔍", knowledge: ["suno_rules"] },
  },
};

describe("buildExpertsFromMeta", () => {
  it("按后端阵容建卡（name/emoji/knowledge取后端，color留前端）", () => {
    const list = buildExpertsFromMeta(meta, "mode_a");
    expect(list.map((e) => e.id)).toEqual(["emotion", "producer", "host", "auditor"]);
    expect(list[0]).toMatchObject({ name: "情感分析师", emoji: "🎭", knowledge: ["emotions"] });
    expect(list[0].color).toBe(ROLE_COLORS.emotion);
  });

  it("R6：主持校验落座（座位含 host/auditor，事件点亮有人可点）", () => {
    const list = buildExpertsFromMeta(meta, "mode_d");
    expect(list.map((e) => e.id)).toEqual(
      ["emotion", "lyricist", "style_analyst", "producer", "host", "auditor"]
    );
  });

  it("未知角色回退（name=id，emoji默认，color默认）", () => {
    const m: PipelineMeta = {
      modes: { mode_b: ["new_role"] },
      roles: {},
    };
    const list = buildExpertsFromMeta(m, "mode_b");
    expect(list[0].name).toBe("new_role");
    expect(list[0].emoji).toBe("🎙️");
  });

  it("未知模式返回空数组", () => {
    expect(buildExpertsFromMeta(meta, "mode_b")).toEqual([]);
  });
});

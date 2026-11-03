import { describe, it, expect } from "vitest";
import { historyPlanFields } from "../utils/sunoPackage";

describe("historyPlanFields（历史条目 song_plan/rendered 落盘）", () => {
  it("Suno 包 + SongPlan → 两字段都写，且自动剥离段头内部 energy 锚点", () => {
    const raw = "Title: 夜\nStyle: indie folk, 80 bpm\nExclude: rap\nSettings: weirdness 30\nLyrics:\n[Verse 1 | close room, energy 3]\n[Bridge | energy 2]\n夜很长";
    const f = historyPlanFields(raw, { sections: [] });
    expect(f.song_plan).toEqual({ sections: [] });
    expect(f.rendered?.style).toContain("indie folk");
    expect(f.rendered?.lyrics).toContain("[Verse 1 | close room]");
    expect(f.rendered?.lyrics).toContain("[Bridge]");
    expect(f.rendered?.lyrics).not.toMatch(/energy/);
    expect(f.rendered?.lyrics).toContain("夜很长");
    expect(f.rendered?.title).toBe("夜");
  });
  it("非包文本且无方案 → 空对象（不写空字段）", () => {
    expect(historyPlanFields("随便聊聊", null)).toEqual({});
  });
});

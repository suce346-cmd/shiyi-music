import { describe, it, expect } from "vitest";
import { parseEnergy } from "../components/ResultPanel";

describe("parseEnergy", () => {
  it("单值中文标注摘出并建能量条", () => {
    const text = "[Verse 1]\n[钢琴独奏, 空旷, 能量:2]\n雨敲窗";
    const { display, sections } = parseEnergy(text);
    expect(display).not.toContain("能量:2");
    expect(display).toContain("[钢琴独奏, 空旷]");
    expect(sections).toEqual([{ label: "Verse1", energy: 2 }]);
  });

  it("区间取上限（energy 3-4 → 4）", () => {
    const text = "[Chorus]\n[piano, hall, energy 3-4]\n啦啦啦";
    const { sections } = parseEnergy(text);
    expect(sections).toEqual([{ label: "Chorus", energy: 4 }]);
  });

  it("无能量标注时 sections 为空、文本不变", () => {
    const text = "[Verse 1]\n[钢琴, 雨声]\n雨敲窗";
    const { display, sections } = parseEnergy(text);
    expect(display).toBe(text);
    expect(sections).toEqual([]);
  });

  it("英文 energy:8 冒号形态兼容", () => {
    const text = "[Hook]\n[drums, energy:8]\n嘿";
    const { sections } = parseEnergy(text);
    expect(sections).toEqual([{ label: "Hook", energy: 8 }]);
  });

  it("多段各自关联段名", () => {
    const text = "[Verse 1]\n[a, 能量:2]\nx\n[Chorus]\n[b, 能量:8]\ny";
    const { sections } = parseEnergy(text);
    expect(sections).toEqual([
      { label: "Verse1", energy: 2 },
      { label: "Chorus", energy: 8 },
    ]);
  });
});

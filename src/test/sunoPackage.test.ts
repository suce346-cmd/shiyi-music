import { describe, it, expect } from "vitest";
import { parseSunoPackage, settingsShowsVarietyZero } from "../utils/sunoPackage";

const SAMPLE = `Title: 雨巷

Style: chinese folk ballad, 72 BPM

Exclude: edm drops

Settings: Weirdness=30 | Style Influence=70 | Variety=0 | Vocal Gender=Female

Lyrics:
[Intro | rain]
[Chorus]
雨落长街
`;

describe("parseSunoPackage (P1-11)", () => {
  it("splits four fields and preserves Variety=0 in Settings", () => {
    const p = parseSunoPackage(SAMPLE);
    expect(p.isPackage).toBe(true);
    expect(p.title).toBe("雨巷");
    expect(p.style).toContain("chinese folk ballad");
    expect(p.exclude).toBe("edm drops");
    expect(p.settings).toContain("Variety=0");
    expect(settingsShowsVarietyZero(p.settings)).toBe(true);
    expect(p.lyrics).toContain("雨落长街");
    expect(p.lyrics).toContain("[Chorus]");
  });

  it("legacy Style Prompt blob is not a package", () => {
    const p = parseSunoPackage("Style Prompt: 夏日城市流行\n[Verse 1]\n歌词");
    expect(p.isPackage).toBe(false);
  });
});

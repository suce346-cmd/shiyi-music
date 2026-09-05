import { describe, it, expect } from "vitest";
import { outputSummary } from "../components/HistoryPanel";
import type { HistoryEntry } from "../types";

const makeEntry = (output: string): HistoryEntry => ({
  id: "id-1",
  mode: "mode_d",
  input: "灵感",
  output,
  timestamp: 1,
});

describe("outputSummary", () => {
  it("短输出原样返回", () => {
    expect(outputSummary(makeEntry("短方案"))).toBe("短方案");
  });

  it("超 60 字截断加省略号", () => {
    const long = "啊".repeat(100);
    const s = outputSummary(makeEntry(long));
    expect(s.length).toBe(63);
    expect(s.endsWith("...")).toBe(true);
  });

  it("空输出返回空串", () => {
    expect(outputSummary(makeEntry(""))).toBe("");
  });
});

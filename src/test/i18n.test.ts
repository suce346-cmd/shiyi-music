import { describe, it, expect } from "vitest";
import { t, tf, dictKeys } from "../i18n";

describe("i18n", () => {
  it("中英字典 key 集合一致（漏 key 即失败）", () => {
    expect(dictKeys("en")).toEqual(dictKeys("zh"));
    expect(dictKeys("zh").length).toBeGreaterThan(50);
  });

  it("取文案：英文/中文/缺省/未知 key 回退", () => {
    expect(t("en", "settings.test")).toBe("Test connection");
    expect(t("zh", "settings.test")).toBe("测试连接");
    expect(t(undefined, "settings.test")).toBe("测试连接");
    expect(t("en", "no.such.key")).toBe("no.such.key");
  });

  it("占位符取文案：中英语序各自成立，缺参占位符原样保留", () => {
    // 含数字的整句在中英语序不同——拆前后缀拼接会破坏语序，故整句 + 占位符单源
    expect(tf("zh", "gate.title", { round: 1, next: 2 })).toBe("第 1 轮讨论已完成 — 是否继续第 2 轮？");
    expect(tf("en", "gate.title", { round: 1, next: 2 })).toBe("Round 1 done — continue to round 2?");
    expect(tf("zh", "gate.title", { round: 1 })).toBe("第 1 轮讨论已完成 — 是否继续第 {next} 轮？");
    expect(tf(undefined, "no.such.key", {})).toBe("no.such.key");
  });
});

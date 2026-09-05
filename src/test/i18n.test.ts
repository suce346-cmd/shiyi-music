import { describe, it, expect } from "vitest";
import { t, dictKeys } from "../i18n";

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
});

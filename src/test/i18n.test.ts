import { describe, it, expect } from "vitest";
import { t, tf, dictKeys } from "../i18n";
import type { MessageKey } from "../i18n";

/** 运行期兜底探针：**故意**越过 `MessageKey` 编译期锁，验证"历史键/外部覆盖文件"不白屏。
 *  这不再是"未知 key 只有运行期能发现"的证据——键集合一致性已由编译期穷尽映射锁定
 *  （见 `i18n.ts` 文件头与 `i18nKeys.test.ts`）。 */
const LEGACY_KEY = "no.such.key" as MessageKey;

describe("i18n", () => {
  it("中英字典 key 集合一致（运行期双保险；编译期由 Record<MessageKey, string> 穷尽锁定）", () => {
    expect(dictKeys("en")).toEqual(dictKeys("zh"));
    expect(dictKeys("zh").length).toBeGreaterThan(50);
  });

  it("取文案：英文/中文/缺省/历史 key 回退（回退不白屏）", () => {
    expect(t("en", "settings.test")).toBe("Test connection");
    expect(t("zh", "settings.test")).toBe("测试连接");
    expect(t(undefined, "settings.test")).toBe("测试连接");
    expect(t("en", LEGACY_KEY)).toBe(LEGACY_KEY);
  });

  it("占位符取文案：中英语序各自成立，缺参占位符原样保留", () => {
    // 含数字的整句在中英语序不同——拆前后缀拼接会破坏语序，故整句 + 占位符单源
    expect(tf("zh", "gate.title", { round: 1, next: 2 })).toBe("第 1 轮讨论已完成 — 是否继续第 2 轮？");
    expect(tf("en", "gate.title", { round: 1, next: 2 })).toBe("Round 1 done — continue to round 2?");
    expect(tf("zh", "gate.title", { round: 1 })).toBe("第 1 轮讨论已完成 — 是否继续第 {next} 轮？");
    expect(tf(undefined, LEGACY_KEY, {})).toBe(LEGACY_KEY);
  });
});

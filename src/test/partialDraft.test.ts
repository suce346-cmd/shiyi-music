import { describe, it, expect } from "vitest";
import {
  PREVIOUS_PLAN_MARKER,
  PARTIAL_DRAFT_NOTICE,
  composeRefineInput,
  freezePartial,
} from "../utils/partialDraft";

/** #10 半成品告知的**协议单源锁**：
 * 1. 标记必须与后端 orchestrator::extract_previous_plan 的标记逐字相同（改了即失联，
 *    后端找不到标记会把半成品当"无上一版"处理——用户以为在改，其实从头再来）；
 * 2. 半成品告知必须落在【上一版方案】**段内**（紧贴被描述对象），不能拼进 feedback；
 * 3. 完整产出**不得**附带半成品告知（误告知会让模型去"补齐"完整方案）。 */
describe("partialDraft（#10 半成品告知单源）", () => {
  it("标记与后端 extract_previous_plan 同源（逐字锁）", () => {
    expect(PREVIOUS_PLAN_MARKER).toBe("【上一版方案】");
  });

  it("半成品：告知落在【上一版方案】段内且正文在告知之后", () => {
    const out = composeRefineInput("主题", "Style Prompt: 快歌", true);
    const markerAt = out.indexOf(PREVIOUS_PLAN_MARKER);
    const noticeAt = out.indexOf(PARTIAL_DRAFT_NOTICE);
    const bodyAt = out.indexOf("Style Prompt: 快歌");
    expect(markerAt).toBeGreaterThanOrEqual(0);
    expect(noticeAt).toBeGreaterThan(markerAt);
    expect(bodyAt).toBeGreaterThan(noticeAt);
    // 取标记之后全文（后端算法）必须同时含告知与正文
    const plan = out.slice(markerAt + PREVIOUS_PLAN_MARKER.length).trim();
    expect(plan).toContain(PARTIAL_DRAFT_NOTICE.trim());
    expect(plan).toContain("Style Prompt: 快歌");
  });

  it("完整产出：不含任何半成品告知（不误告知）", () => {
    const out = composeRefineInput("主题", "完整方案正文", false);
    expect(out).toContain(PREVIOUS_PLAN_MARKER);
    expect(out).not.toContain("半成品");
    expect(out.endsWith("完整方案正文")).toBe(true);
  });

  it("告知明确要求补齐而非当完整方案处理（语义不可退化为空话）", () => {
    expect(PARTIAL_DRAFT_NOTICE).toContain("半成品");
    expect(PARTIAL_DRAFT_NOTICE).toContain("截断");
    expect(PARTIAL_DRAFT_NOTICE).toContain("补齐");
  });
});

describe("freezePartial（#10 失败固化的唯一判定处）", () => {
  it("有正文：可见轮与会话历史**同形**（都带 partial，否则看得见改不了 / 改了看不见）", () => {
    const f = freezePartial("Style Prompt: 夏日城市流", "写一首夏天海边的城市流行歌");
    expect(f).not.toBeNull();
    expect(f!.turn.role).toBe("assistant");
    expect(f!.turn.partial).toBe(true);
    expect(f!.turn.content).toBe("Style Prompt: 夏日城市流");
    expect(f!.history).toEqual([
      { role: "user", content: "写一首夏天海边的城市流行歌" },
      { role: "assistant", content: "Style Prompt: 夏日城市流", partial: true },
    ]);
  });

  it("无正文 / 纯空白：不固化（不产生空气泡）", () => {
    expect(freezePartial("", "主题")).toBeNull();
    expect(freezePartial("   \n\t ", "主题")).toBeNull();
  });
});
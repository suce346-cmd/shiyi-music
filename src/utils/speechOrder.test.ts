import { describe, it, expect } from "vitest";
import { orderSpeechTurns, rosterOrder } from "./speechOrder";
import type { ChatTurn } from "../types";

/** 修复包 D-1（#13 界面层）红灯：并发审改时专家发言按事件到达序插入对话流，
 * 与后端汇总顺序（阵容序）不一致 → "发言打架"观感。
 * orderSpeechTurns 按模式阵容序稳定排序 expert 发言（同位次保持到达序）。 */

const sp = (id: string, seq: number): ChatTurn => ({
  role: "expert",
  content: `${id}-${seq}`,
  timestamp: seq,
  speaker: { id, emoji: "🎙️", name: id },
});
const user = (seq: number): ChatTurn => ({ role: "user", content: `u${seq}`, timestamp: seq });
const assistant = (seq: number): ChatTurn => ({ role: "assistant", content: `a${seq}`, timestamp: seq });

const MODE_B_ROSTER = ["emotion", "lyricist", "producer", "host", "auditor"];

describe("rosterOrder", () => {
  it("阵容内角色返回其阵容下标", () => {
    expect(rosterOrder("lyricist", MODE_B_ROSTER)).toBe(1);
    expect(rosterOrder("auditor", MODE_B_ROSTER)).toBe(4);
  });
  it("阵容外角色（插话/未知）排在末尾，互保到达序", () => {
    expect(rosterOrder("host", MODE_B_ROSTER)).toBeLessThan(rosterOrder("unknown-x", MODE_B_ROSTER));
  });
});

describe("orderSpeechTurns（红灯：函数尚不存在）", () => {
  it("乱序到达的专家发言重排为阵容序", () => {
    const turns = [sp("producer", 3), sp("emotion", 1), sp("auditor", 5), sp("lyricist", 2)];
    const out = orderSpeechTurns(turns, MODE_B_ROSTER);
    expect(out.map((t) => t.speaker?.id)).toEqual(["emotion", "lyricist", "producer", "auditor"]);
  });
  it("稳定排序：同角色多条保持到达序（不互调）", () => {
    const turns = [sp("emotion", 9), sp("emotion", 2)];
    const out = orderSpeechTurns(turns, MODE_B_ROSTER);
    expect(out.map((t) => t.content)).toEqual(["emotion-9", "emotion-2"]);
  });
  it("user/assistant 原样保留不参与排序，相对位置不变", () => {
    const turns = [user(0), sp("auditor", 1), assistant(2), sp("emotion", 3)];
    const out = orderSpeechTurns(turns, MODE_B_ROSTER);
    expect(out[0].role).toBe("user");
    expect(out[2].role).toBe("assistant");
    expect(out.filter((t) => t.role === "expert").map((t) => t.speaker?.id)).toEqual(["emotion", "auditor"]);
  });
  it("空数组与全非 expert 输入原样返回", () => {
    expect(orderSpeechTurns([], MODE_B_ROSTER)).toEqual([]);
    const turns = [user(1), assistant(2)];
    expect(orderSpeechTurns(turns, MODE_B_ROSTER)).toEqual(turns);
  });
});

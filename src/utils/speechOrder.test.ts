import { describe, it, expect } from "vitest";
import { orderRoundSpeech, rosterOrder, assembleFinalTurns } from "./speechOrder";
import type { ChatTurn } from "../types";

/** 修复包 D-1（#13 界面层）：专家发言按**阵容序**归位（与后端汇总顺序一致）→
 * 消除到达序 ≠ 阵容序造成的"发言打架"观感。
 * #14：后端已改阵容序串行（到达序 == 阵容序），本模块成幂等兜底，断言不变。
 * 规则边界：排序作用域是**本轮**（末尾连续 expert 段），跨轮永不重排。 */

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

describe("orderRoundSpeech（本轮归位）", () => {
  it("本轮乱序到达的专家发言重排为阵容序", () => {
    const turns = [sp("producer", 3), sp("emotion", 1), sp("auditor", 5), sp("lyricist", 2)];
    const out = orderRoundSpeech(turns, MODE_B_ROSTER);
    expect(out.map((t) => t.speaker?.id)).toEqual(["emotion", "lyricist", "producer", "auditor"]);
  });

  it("稳定排序：同角色多条保持到达序（不互调）", () => {
    const turns = [sp("emotion", 9), sp("emotion", 2)];
    const out = orderRoundSpeech(turns, MODE_B_ROSTER);
    expect(out.map((t) => t.content)).toEqual(["emotion-9", "emotion-2"]);
  });

  it("本轮段内不足 2 条时原样返回（同一引用，不做无谓拷贝）", () => {
    const onlyUser = [user(1), assistant(2)];
    expect(orderRoundSpeech(onlyUser, MODE_B_ROSTER)).toBe(onlyUser);
    const oneExpert = [user(1), sp("emotion", 2)];
    expect(orderRoundSpeech(oneExpert, MODE_B_ROSTER)).toBe(oneExpert);
    expect(orderRoundSpeech([], MODE_B_ROSTER)).toEqual([]);
  });

  // ── 判别性断言（旧全局实现必红）──────────────────────────────────
  // 旧实现 orderSpeechTurns 对整数组全局归位：优化/续跑时 conversation 不重置，
  // 本轮新到的发言会被排进上一轮中间。以下断言钉死"跨轮不重排"。
  it("跨轮不重排：上一轮专家段原位不动，本轮只在末尾段内归位", () => {
    const round1 = [sp("emotion", 1), sp("producer", 2), sp("auditor", 3)]; // 上一轮（已归位）
    const turns = [user(0), ...round1, assistant(10), sp("auditor", 11), sp("emotion", 12)];
    const out = orderRoundSpeech(turns, MODE_B_ROSTER);
    // 上一轮 5 个元素（user + 3 expert + assistant）一个都不许动
    expect(out.slice(0, 5)).toEqual(turns.slice(0, 5));
    // 本轮两条按阵容序归位（emotion 在 auditor 前）
    expect(out.slice(5).map((t) => t.speaker?.id)).toEqual(["emotion", "auditor"]);
  });

  it("跨轮不重排：本轮发言不得前插到上一轮 expert 段之内", () => {
    const turns = [
      sp("emotion", 1), sp("producer", 2), // 上一轮（已归位）
      assistant(10),
      sp("producer", 12), sp("emotion", 11), // 本轮到达序 = 逆阵容序
    ];
    const out = orderRoundSpeech(turns, MODE_B_ROSTER);
    // 上一轮两条必须仍在 assistant 之前原位；本轮两条只在 a10 之后归位
    expect(out.map((t) => t.content)).toEqual([
      "emotion-1",
      "producer-2",
      "a10",
      "emotion-11",
      "producer-12",
    ]);
  });

  it("末尾不是 expert 时（本轮未开始/已收尾）原样返回", () => {
    const turns = [sp("producer", 1), sp("emotion", 2), assistant(3)];
    expect(orderRoundSpeech(turns, MODE_B_ROSTER)).toBe(turns);
  });
});

describe("assembleFinalTurns（结束态组装）", () => {
  // ── 判别性断言（旧实现在结束态不归位，必红）──────────────────────
  // 旧实现：allTurns = [user, ...speechLog(到达序), assistant] 直接铺开，
  // 流式期已归位的顺序在运行结束时被覆盖回乱序 —— 而这份数组正是 ResultPanel
  // 渲染的列表，也是写进历史条目的那一份。
  it("生成路径：乱序到达的发言在结束态仍为阵容序，user 在首 assistant 在尾", () => {
    const speeches = [sp("auditor", 5), sp("producer", 3), sp("emotion", 1), sp("lyricist", 2)];
    const out = assembleFinalTurns(
      [{ role: "user", content: "写一首夏天海边的城市流行歌", timestamp: 0 }],
      speeches,
      [{ role: "assistant", content: "final", timestamp: 99 }],
      MODE_B_ROSTER,
    );
    expect(out.map((t) => t.content)).toEqual([
      "写一首夏天海边的城市流行歌",
      "emotion-1",
      "lyricist-2",
      "producer-3",
      "auditor-5",
      "final",
    ]);
  });

  it("优化路径：上一轮一个不动，本轮归位，尾部（反馈+产出）仍在最后", () => {
    const round1: ChatTurn[] = [
      user(0),
      sp("emotion", 1),
      sp("producer", 2),
      sp("auditor", 3),
      assistant(10),
    ];
    const speeches = [sp("producer", 13), sp("emotion", 12)]; // 本轮到达序
    const out = assembleFinalTurns(round1, speeches, [user(20), assistant(30)], MODE_B_ROSTER);
    expect(out.slice(0, 5)).toEqual(round1); // 上一轮一字不动
    expect(out.slice(5).map((t) => t.content)).toEqual(["emotion-12", "producer-13", "u20", "a30"]);
  });
});
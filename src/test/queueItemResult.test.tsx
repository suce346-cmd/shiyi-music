// @vitest-environment jsdom
import { describe, it, expect, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import appSource from "../App.tsx?raw";
import QueuePanel from "../components/QueuePanel";
import {
  canViewQueueItem,
  markQueueStatus,
  setQueueResult,
  linkQueueHistory,
  terminalQueueStatus,
  queueResultEntry,
  type QueueItem,
} from "../hooks/useQueue";

/** 第十一批（#3）：队列项的产出落点与「点击 ≠ 静默」规则。
 *  根因：完成链立即取队首续跑，新 run 开头清空会话——失败项的产出（半成品）若只活在会话
 *  state 里，会在被看见之前被抹掉；且点击查看按「输入前 20 字」模糊匹配历史，失败项永远查不到
 *  却仍被画成可点击（虚假 affordance + 静默无响应）。
 *  锁三件事：① 可点击 ⟺ 有可查看内容（单源）；② 产出快照与历史关联在各状态迁移中不丢；
 *  ③ 面板据此渲染（有内容才可点）。 */
afterEach(cleanup);

const base = (over: Partial<QueueItem> = {}): QueueItem => ({
  id: "q1",
  label: "写一首夏天海边的城市流行歌",
  mode: "mode_d",
  userInput: "写一首夏天海边的城市流行歌",
  status: "error",
  enqueuedAt: 1,
  ...over,
});

describe("队列项产出落点（#3）", () => {
  it("canViewQueueItem：只有 historyId 或 result 才算可查看（可点击性的唯一判定源）", () => {
    expect(canViewQueueItem(base())).toBe(false);
    expect(canViewQueueItem(base({ result: { output: "Style Prompt: 夏日", partial: true } }))).toBe(true);
    expect(canViewQueueItem(base({ historyId: "h1" }))).toBe(true);
  });

  it("状态迁移不吞产出快照（此前失败/取消项在 mark 之后仍是空的）", () => {
    const q = setQueueResult([base()], "q1", { output: "半成品正文", partial: true });
    expect(q[0].result).toEqual({ output: "半成品正文", partial: true });
    // 完成链里的 mark（error / cancelled）必须保留快照——否则点击立刻失去内容
    expect(markQueueStatus(q, "q1", "cancelled")[0].result).toEqual({ output: "半成品正文", partial: true });
    expect(markQueueStatus(q, "q1", "error")[0].result).toEqual({ output: "半成品正文", partial: true });
  });

  it("linkQueueHistory 只改目标项（不误伤同队列其他项的产出）", () => {
    const other = base({ id: "q2", status: "error", result: { output: "另一项半成品", partial: true } });
    const q = linkQueueHistory([base({ id: "q1", status: "done" }), other], "q1", "h1");
    expect(q[0].historyId).toBe("h1");
    expect(q[1].historyId).toBeUndefined();
    expect(q[1].result).toEqual({ output: "另一项半成品", partial: true });
  });

  it("面板：无可查看内容的项不给可点击样式（不再有死点击）", () => {
    const { container } = render(
      <QueuePanel
        queue={[base({ id: "dead", status: "error" }), base({ id: "alive", status: "error", historyId: "h1" })]}
        runningId={null}
        onRemove={() => {}}
        onClear={() => {}}
        onSelect={() => {}}
        locale="zh"
      />,
    );
    const rows = container.querySelectorAll("div.space-y-1 > div");
    expect(rows[0].className).not.toContain("cursor-pointer");
    expect(rows[1].className).toContain("cursor-pointer");
  });

  it("面板：有产出快照的取消项可点击（此前取消项被排除在点击之外）", () => {
    let picked = "";
    render(
      <QueuePanel
        queue={[base({ id: "c1", status: "cancelled", result: { output: "半成品", partial: true } })]}
        runningId={null}
        onRemove={() => {}}
        onClear={() => {}}
        onSelect={(id) => { picked = id; }}
        locale="zh"
      />,
    );
    fireEvent.click(screen.getByText("写一首夏天海边的城市流行歌"));
    expect(picked).toBe("c1");
  });

  it("终态单源：取消写 cancelled（不得被失败覆盖成红灯「失败」）", () => {
    expect(terminalQueueStatus(true)).toBe("cancelled");
    expect(terminalQueueStatus(false)).toBe("error");
  });

  it("产出快照 → 视图条目：partial 落到 assistant 轮上（否则从这里再优化不再告知「可能被截断」）", () => {
    const entry = queueResultEntry(base({ result: { output: "Style Prompt: 夏日", partial: true } }));
    expect(entry).not.toBeNull();
    expect(entry!.output).toBe("Style Prompt: 夏日");
    expect(entry!.mode).toBe("mode_d");
    expect(entry!.input).toBe("写一首夏天海边的城市流行歌");
    const last = [...entry!.conversation!].reverse().find((t) => t.role === "assistant");
    expect(last?.partial).toBe(true);
    // 无快照 → null（该项不可查看，面板也不会给可点击样式）
    expect(queueResultEntry(base())).toBeNull();
  });
});

/** 源码形状锁（前端侧同 Rust 的"生产段形状"思路）：纯函数测得到的规则，若接线处不调用
 *  就等于零——这三条锁把"接线"本身固定住：失败路径必须写快照、成功路径必须关联历史、
 *  结果区不得再复制一份"有无产出"的判定。 */
describe("App 接线形状锁（#2/#3）", () => {
  const src = appSource;

  it("失败路径：catch 的 fabric（freezePartial → finally）段内必须写产出快照 + 用终态单源", () => {
    const start = src.indexOf("const frozen = freezePartial");
    const end = src.indexOf("} finally {", start);
    expect(start).toBeGreaterThan(-1);
    expect(end).toBeGreaterThan(start);
    const segment = src.slice(start, end);
    expect(segment).toContain("queue.setResult(");
    expect(segment).toContain("terminalQueueStatus(");
  });

  it("成功路径：归档后必须精确关联队列项与历史条目 id", () => {
    expect(src).toContain("queue.linkHistory(runId, entry.id)");
  });

  it("结果区内容判定单源：App 不得再复制「有无产出」的分支（错误态曾被它丢回新手引导）", () => {
    expect(src).not.toContain("conversation.length > 0 || streamText");
  });
});
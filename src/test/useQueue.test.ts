import { describe, it, expect } from "vitest";
import {
  queueLabel,
  enqueue,
  dequeueNext,
  markQueueStatus,
  removeQueueItem,
  clearQueued,
  type QueueItem,
} from "../hooks/useQueue";

const mk = (id: string, status: QueueItem["status"] = "queued"): QueueItem => ({
  id,
  label: id,
  mode: "mode_d",
  userInput: id,
  status,
  enqueuedAt: 1,
});

describe("queueLabel", () => {
  it("短输入原样", () => {
    expect(queueLabel("灵感")).toBe("灵感");
  });
  it("超 30 字截断", () => {
    expect(queueLabel("啊".repeat(40)).length).toBe(31);
  });
  it("换行压空格", () => {
    expect(queueLabel("a\nb\nc")).toBe("a b c");
  });
});

describe("queue ops", () => {
  it("入队追加且状态 queued", () => {
    const q = enqueue([], { id: "1", label: "1", mode: "mode_b", userInput: "hi" });
    expect(q).toHaveLength(1);
    expect(q[0].status).toBe("queued");
  });

  it("取队首待跑项（跳过 running/done）", () => {
    const q = [mk("a", "running"), mk("b"), mk("c")];
    expect(dequeueNext(q)?.id).toBe("b");
    expect(dequeueNext([mk("a", "done")])).toBeUndefined();
  });

  it("标记状态只改目标项", () => {
    const q = markQueueStatus([mk("a"), mk("b")], "a", "running");
    expect(q[0].status).toBe("running");
    expect(q[1].status).toBe("queued");
  });

  it("移除与清空等待项（running 保留）", () => {
    expect(removeQueueItem([mk("a"), mk("b")], "a").map((q) => q.id)).toEqual(["b"]);
    const q = clearQueued([mk("a", "running"), mk("b"), mk("c", "done")]);
    expect(q.map((x) => x.id)).toEqual(["a", "c"]);
  });
});

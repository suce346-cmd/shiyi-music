import { describe, it, expect } from "vitest";
import { acceptStreamChunk } from "../utils/streamChunk";

// RULE-RUN-ID（C-06）：llm-chunk 按归属 run_id 过滤
describe("acceptStreamChunk", () => {
  it("当前 run 的 chunk 接收", () => {
    expect(acceptStreamChunk({ run_id: "r1", content: "雨" }, "r1")).toBe(true);
  });
  it("旧 run 在途 chunk 丢弃（取消/队列交错回归）", () => {
    expect(acceptStreamChunk({ run_id: "r-old", content: "串" }, "r-new")).toBe(false);
  });
  it("当前无 run（空 id）一律丢弃", () => {
    expect(acceptStreamChunk({ run_id: "", content: "x" }, "")).toBe(false);
    expect(acceptStreamChunk({ run_id: "r1", content: "x" }, "")).toBe(false);
  });
  it("缺 run_id 的旧形状载荷丢弃（宁丢不串）", () => {
    expect(acceptStreamChunk({ content: "x" }, "r1")).toBe(false);
    expect(acceptStreamChunk(null, "r1")).toBe(false);
    expect(acceptStreamChunk({ run_id: "r1" }, "r1")).toBe(false);
  });
});

// 集成级（无 GUI）：模拟 llm-chunk 事件总线 + run 切换，取消 A 后立即开 B，A 的在途 chunk 与 B 交错到达
describe("取消后立即新 run：chunk 过滤序列", () => {
  it("只显示当前 run 的正文，旧 run 迟到 chunk 全部丢弃", () => {
    let currentRunId = "run-a";
    let shown = "";
    const listener = (raw: string) => {
      const payload = JSON.parse(raw); // 与后端 serde 线格式一致：{"run_id","content"}
      if (!acceptStreamChunk(payload, currentRunId)) return;
      shown += payload.content;
    };
    const wire = (run_id: string, content: string) => JSON.stringify({ run_id, content });
    listener(wire("run-a", "A1"));
    expect(shown).toBe("A1");
    // 停止：前端清空 runId 与正文
    currentRunId = "";
    shown = "";
    listener(wire("run-a", "A2-在途"));
    // 新 run 开始
    currentRunId = "run-b";
    listener(wire("run-a", "A3-迟到"));
    listener(wire("run-b", "B1"));
    listener(wire("run-a", "A4-迟到"));
    listener(wire("run-b", "B2"));
    expect(shown).toBe("B1B2");
  });
});

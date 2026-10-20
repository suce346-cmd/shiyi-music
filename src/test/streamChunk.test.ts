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

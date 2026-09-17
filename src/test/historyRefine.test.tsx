// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { HistoryEntry } from "../types";

/** 修复包 C 红灯：历史结果反馈（#9）。
 * 验收目标：从历史条目触发优化时，pipeline.refine 收到正确的
 * mode / userInput（原输入）/ lastOutput（该条目的产出）/ feedback，
 * 且旧格式记录（无 conversation 字段）同样可达。 */

const refineMock = vi.fn();
const runMock = vi.fn();

vi.mock("./usePipeline", () => ({
  // App 从 "./hooks/usePipeline" 导入；本测试文件位于 src/test/，mock 路径按 vitest 解析以被测模块为准
  usePipeline: () => ({
    state: { phase: "idle", experts: [], usage: { prompt_tokens: 0, completion_tokens: 0 } },
    run: runMock,
    refine: refineMock,
    reset: vi.fn(),
    cancel: vi.fn(),
    getRunId: () => "test-run",
    resume: vi.fn(),
    usageRef: { current: { prompt_tokens: 0, completion_tokens: 0 } },
    degradedRef: { current: [] },
    validationRef: { current: null },
  }),
  ROLE_NAMES: {},
  ROLE_EMOJIS: {},
}));

// App 组件太重（Tauri invoke 全家桶），这里只对"历史装载逻辑"做契约测试——
// 装载函数从 App.tsx 导出（施工时 export），保证接线不依赖整组件渲染。
describe("历史条目反馈装载契约（红灯）", () => {
  beforeEach(() => {
    refineMock.mockReset();
    refineMock.mockResolvedValue("新方案文本");
    runMock.mockReset();
  });

  it("有 conversation 的条目：装载 mode/input/output 供 refine 使用", async () => {
    const { buildHistoryRefineContext } = await import("../App");
    const entry: HistoryEntry = {
      id: "h1",
      mode: "mode_b",
      input: "夏夜烧烤摊",
      output: "Style Prompt: old",
      conversation: [
        { role: "user", content: "夏夜烧烤摊", timestamp: 1 },
        { role: "assistant", content: "Style Prompt: old", timestamp: 2 },
      ],
      timestamp: 3,
    };
    const ctx = buildHistoryRefineContext(entry);
    expect(ctx.mode).toBe("mode_b");
    expect(ctx.lastUserInput).toBe("夏夜烧烤摊");
    expect(ctx.lastOutput).toBe("Style Prompt: old");
    expect(ctx.conversation).toHaveLength(2);
  });

  it("旧格式记录（无 conversation）：以 input/output 构造对话流（兜底可达）", async () => {
    const { buildHistoryRefineContext } = await import("../App");
    const entry: HistoryEntry = {
      id: "h2",
      mode: "mode_a",
      input: "深夜加班",
      output: "Style Prompt: v1",
      timestamp: 9,
    };
    const ctx = buildHistoryRefineContext(entry);
    expect(ctx.mode).toBe("mode_a");
    expect(ctx.lastUserInput).toBe("深夜加班");
    expect(ctx.lastOutput).toBe("Style Prompt: v1");
    expect(ctx.conversation).toEqual([
      { role: "user", content: "深夜加班", timestamp: 9 },
      { role: "assistant", content: "Style Prompt: v1", timestamp: 9 },
    ]);
  });
});

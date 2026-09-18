import { describe, it, expect } from "vitest";
import pipelineSource from "../hooks/usePipeline.ts?raw";
import appSource from "../App.tsx?raw";
import statusSource from "../components/StatusIndicator.tsx?raw";
import typesSource from "../types/index.ts?raw";

/** 第十六批（#27-c）源码形状锁。
 *  组件测到不等于接线了——"写了组件没人用"是纯函数测试照不出的绿灯空转（第十一批起用 `?raw` 锁）。
 *  同时锁住**旧规则的零残留**：错误落点若还留着裸 setErrorMessage，就是两个写入入口（双源），
 *  errorKind 会在某些路径上停留旧值，限流指引张冠李戴。 */
describe("退避 UI 接线形状锁（#27-c）", () => {
  it("usePipeline 消费 backoff / backoff_end 两个事件，且带本端时钟基准", () => {
    expect(pipelineSource).toContain('case "backoff":');
    expect(pipelineSource).toContain('case "backoff_end":');
    expect(pipelineSource).toContain("startedAt: Date.now()");
  });

  it("usePipeline 的退避态在每个 run 边界复位（startRun/reset/resume/finishRun）", () => {
    // 4 处显式复位 + 初始 state 1 处 = 5 处 backoff 归位（漏一处就会把倒计时留在屏上）
    expect((pipelineSource.match(/backoff: null/g) ?? []).length).toBeGreaterThanOrEqual(5);
  });

  it("App 把退避态与错误类别透传给状态条", () => {
    expect(appSource).toContain("backoff={pipeline.backoff}");
    expect(appSource).toContain("errorKind={errorKind}");
  });

  it("App 错误落点单源：三处 catch 全部走 applyRunError，复位全部走 clearRunError", () => {
    expect((appSource.match(/applyRunError\(e\)/g) ?? []).length).toBe(3);
    expect((appSource.match(/clearRunError\(\)/g) ?? []).length).toBe(4);
    // 旧规则零残留：裸写入字面量全仓各只允许出现一次（即在 helper 内）
    expect((appSource.match(/setErrorMessage\(errText\(e\)\)/g) ?? []).length).toBe(1);
    expect((appSource.match(/setErrorMessage\("已取消"\)/g) ?? []).length).toBe(1);
    expect((appSource.match(/setErrorMessage\(""\)/g) ?? []).length).toBe(1);
  });

  it("App 的 kind 与文案同源（errKind 取自同一个 e）", () => {
    expect(appSource).toContain("setErrorKind(errKind(e))");
  });

  it("StatusIndicator 消费 errorKind === rate_limit 给指引，且倒计时在 hooks 提前 return 之前", () => {
    expect(statusSource).toContain('errorKind === "rate_limit"');
    expect(statusSource).toContain("BACKOFF_TEXT_KEY");
    // hooks 顺序：useEffect 必须在 `if (status === "idle") return null;` 之前
    expect(statusSource.indexOf("useEffect(")).toBeLessThan(statusSource.indexOf('if (status === "idle") return null;'));
  });

  it("types 单源：BackoffReason 三值 + errKind 投影", () => {
    expect(typesSource).toContain('export type BackoffReason = "rate_limit" | "server_error" | "network"');
    expect(typesSource).toContain("export function errKind(");
  });
});

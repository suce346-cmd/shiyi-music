import { describe, it, expect } from "vitest";
import appSource from "../App.tsx?raw";

/** 第十五批（#27-a/#27-b）源码形状锁。
 *  组件与纯函数各自测到，不等于 App 真的接线了——"写了组件没人用"是纯函数测试照不出的绿灯空转
 *  （第十一批起用 `?raw` 锁接线，此处继续）。同时锁住**旧裸 input 的清除**：
 *  模型字段若同时留着旧 input，就是两个写入入口（双源），用户看到的与落盘的会分叉。 */
describe("App 设置面板接线形状锁（#27-a/#27-b）", () => {
  const src = appSource;
  /** 设置面板里"厂商模板 → 模型/地址"这一段：用两处注释锚点切窗，
   *  避免全文件 contains 被别处同名片段误判为已接线 */
  const start = src.indexOf("#27-a 厂商模板卡片");
  const end = src.indexOf("{/* 思考模式：", start);
  const panel = src.slice(start, end);

  it("窗口切得出来（锚点被改动时本测试即红，而不是静默变成空串断言）", () => {
    expect(start).toBeGreaterThan(-1);
    expect(end).toBeGreaterThan(start);
  });

  it("厂商模板卡片已接线，且点击一次性写入 baseUrl 与该厂商默认模型", () => {
    expect(panel).toContain("<ProviderTemplates");
    expect(panel).toContain("updateSettings({ baseUrl: p.baseUrl, model: p.defaultModel })");
  });

  it("模型字段已换成 SearchableSelect，旧裸 input 双入口清除干净", () => {
    expect(panel).toContain("<SearchableSelect");
    expect(panel).toContain("modelsForBaseUrl(settings.baseUrl)");
    // 旧裸 input 的 onChange 字面量必须消失——留着就是第二个写入入口（双源）
    expect(src).not.toContain("updateSettings({ model: e.target.value })");
  });

  it("baseUrl 格式层校验已接线，并在面板内展示原因", () => {
    expect(src).toContain("formatUrlIssue(settings.baseUrl)");
    expect(panel).toContain("settings.url.err.");
  });

  it("角色级模型同样走可搜索下拉（候选取该角色生效地址）", () => {
    expect(src.match(/<SearchableSelect/g)?.length ?? 0).toBeGreaterThanOrEqual(2);
    expect(src).toContain("modelsForBaseUrl(roleBaseUrl)");
  });

  it("测试连接按钮在地址格式不过时置灰（不做必然失败的假 affordance）", () => {
    expect(src).toContain("disabled={testingApi || !settings.apiKey || !secretsReady || baseUrlIssue !== null}");
    expect(src).toContain('disabled={roleTestStates[role] === "testing" || roleUrlIssue !== null}');
  });
});

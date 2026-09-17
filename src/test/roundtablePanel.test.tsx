// @vitest-environment jsdom
import { describe, it, expect, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import RoundtablePanel from "../components/RoundtablePanel";
import type { ExpertCard } from "../types";

afterEach(cleanup);

/** 圆桌发言状态契约（#13 / 包一 D-1 回归守卫）。
 *
 * 守住两件事：
 * 1. 座位角标：working → "审改中"、done → "已发言"（不与抢话叙事混淆）；
 * 2. 并发横幅的显示条件必须是"≥2 个座位同时 working"（真实并发窗口），
 *    不得回退为绑定 phase —— 旧实现绑 phase==="discussing" 覆盖的是主持人统领
 *    流式窗口，与专家真并发（synthesizing 阶段）不重合，正是角标/横幅时序错位的根因。
 */

const mk = (id: string, status: ExpertCard["status"]): ExpertCard => ({
  id,
  name: id,
  emoji: "🎭",
  color: "#f59e0b",
  knowledge: [],
  status,
  note: "",
});

const base = {
  validation: null,
  error: null,
  active: false,
  onOpenDetail: () => {},
};

describe("圆桌发言状态契约（#13 回归守卫）", () => {
  it("done 座位渲染 已发言", () => {
    render(
      <RoundtablePanel
        {...base}
        experts={[mk("emotion", "done")]}
        phase="done"
      />
    );
    expect(screen.getByText("已发言")).toBeTruthy();
  });

  it("working 座位渲染 审改中", () => {
    render(
      <RoundtablePanel
        {...base}
        experts={[mk("emotion", "working")]}
        phase="discussing"
      />
    );
    expect(screen.getByText("审改中")).toBeTruthy();
  });

  it("≥2 座位 working 时渲染并发横幅（即便 phase 不是 discussing）", () => {
    render(
      <RoundtablePanel
        {...base}
        experts={[mk("emotion", "working"), mk("lyricist", "working")]}
        phase="synthesizing"
      />
    );
    expect(screen.getByText(/专家团正并发审改不同维度/)).toBeTruthy();
  });

  it("仅 1 座位 working 时不渲染并发横幅（旧 phase===\"discussing\" 绑定会误显）", () => {
    render(
      <RoundtablePanel
        {...base}
        experts={[mk("host", "working")]}
        phase="discussing"
      />
    );
    expect(screen.queryByText(/专家团正并发审改不同维度/)).toBeNull();
  });

  // ── 判别性断言（locale 切换）────────────────────────────────────
  // 前 4 条中"角标渲染中文"一类，旧硬编码版本同样通过——那只是防回归，不是生效证据。
  // 下面两条以 locale="en" 渲染：旧硬编码中文实现在此必然变红，故可判别"i18n 化"规则已生效。
  it("locale=en 时角标切英文（旧硬编码中文必红）", () => {
    render(
      <RoundtablePanel
        {...base}
        experts={[mk("emotion", "done"), mk("lyricist", "working")]}
        phase="discussing"
        locale="en"
      />
    );
    expect(screen.getByText("Reviewed")).toBeTruthy();
    expect(screen.getByText("Reviewing")).toBeTruthy();
    expect(screen.queryByText("已发言")).toBeNull();
    expect(screen.queryByText("审改中")).toBeNull();
  });

  it("locale=en 时并发横幅切英文（旧硬编码中文必红）", () => {
    render(
      <RoundtablePanel
        {...base}
        experts={[mk("emotion", "working"), mk("lyricist", "working")]}
        phase="synthesizing"
        locale="en"
      />
    );
    expect(screen.getByText(/reviewing different dimensions in parallel/)).toBeTruthy();
  });
});
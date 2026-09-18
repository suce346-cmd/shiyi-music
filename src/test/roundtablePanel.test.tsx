// @vitest-environment jsdom
import { describe, it, expect, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import RoundtablePanel from "../components/RoundtablePanel";
import type { ExpertCard } from "../types";

afterEach(cleanup);

/** 圆桌发言状态契约（#13 / 包一 D-1 回归守卫）。
 *
 * 守座位角标：working → "审改中"、done → "已发言"（不与抢话叙事混淆）。
 *
 * #14（讨论轮改阵容序串行）已退役"多专家并发"叙事：同一时刻至多一个座位 working，
 * 原先的"≥2 座位 working 显示并发横幅"（绑真实并发窗口，非 phase）连同横幅与
 * i18n 文案一并删除——相关用例不再保留（死代码 + 假叙事）。
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

  // ── 判别性断言（locale 切换）────────────────────────────────────
  // 座位角标类断言旧硬编码版本同样通过——那只是防回归，不是生效证据。
  // 下面这条以 locale="en" 渲染：旧硬编码中文实现在此必然变红，故可判别"i18n 化"规则已生效。
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
});
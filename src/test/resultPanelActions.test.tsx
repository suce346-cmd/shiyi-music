// @vitest-environment jsdom
import { describe, it, expect, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import ResultPanel from "../components/ResultPanel";
import type { ChatTurn } from "../types";

/** #10/#11 结果操作条的状态化规则（此前写死 status === "done"，失败/取消时整条消失）。
 *  锁三件事：① 有产出就可用（含失败固化的半成品）；② 在途不给操作；
 *  ③ 无产出明确说明而不是留白；④ 半成品有显式标注与对应文案。 */
afterEach(cleanup);

const user: ChatTurn = { role: "user", content: "写一首夏天海边的城市流行歌", timestamp: 1 };
const full: ChatTurn = { role: "assistant", content: "Style Prompt: 夏日城市流行\n[Verse 1]\n歌词", timestamp: 2 };
const partial: ChatTurn = { role: "assistant", content: "Style Prompt: 夏日城市流", timestamp: 2, partial: true };
const expert: ChatTurn = {
  role: "expert", content: "副歌需要更抓耳", timestamp: 3,
  speaker: { id: "producer", emoji: "🎤", name: "制作人" },
};

const setup = (conversation: ChatTurn[], status: Parameters<typeof ResultPanel>[0]["status"], streamText = "") =>
  render(<ResultPanel conversation={conversation} streamText={streamText} status={status} onRefine={() => {}} />);

describe("ResultPanel 操作条（#10/#11）", () => {
  it("完成态：常驻操作条可见（sticky 常驻 + 复制/优化双入口）", () => {
    setup([user, full], "done");
    expect(screen.getByText("复制")).toBeTruthy();
    expect(screen.getByText("优化")).toBeTruthy();
    // #11 常驻：操作条容器必须是 sticky（否则又回到"藏在最底部"）
    const bar = screen.getByText("复制").closest("div.sticky");
    expect(bar).not.toBeNull();
    expect(bar!.className).toContain("bottom-0");
  });

  it("出错但有半成品：复制与优化都可用，且带半成品标注与专用文案", () => {
    setup([user, expert, partial], "error");
    expect(screen.getByText("复制")).toBeTruthy();
    expect(screen.getByText("基于半成品继续优化")).toBeTruthy();
    expect(screen.getByText("半成品（生成中断）")).toBeTruthy();
    expect(screen.getByText("生成中断时保留，可能不完整")).toBeTruthy();
  });

  it("取消（idle）同样保留操作条（旧实现 status!=='done' 即消失）", () => {
    setup([user, partial], "idle");
    expect(screen.getByText("复制")).toBeTruthy();
  });

  it("在途（loading/streaming）不提供操作，避免对旧结果误操作", () => {
    const { unmount } = setup([user, full], "streaming");
    expect(screen.queryByText("复制")).toBeNull();
    unmount();
    setup([user, full], "loading");
    expect(screen.queryByText("复制")).toBeNull();
  });

  it("失败且无产出：显示失败态并指向重试（不再冒充新手引导）", () => {
    setup([user, expert], "error");
    expect(screen.getByText("本次生成失败，没有产出内容")).toBeTruthy();
    expect(screen.getByText("可点上方「重试」或「从上次继续」")).toBeTruthy();
    expect(screen.queryByText("复制")).toBeNull();
    // 关键：#2 的锁——错误态绝不能落回新手引导（旧实现在"零产出"时把用户丢回引导）
    expect(screen.queryByText(/输入内容开始生成/)).toBeNull();
  });

  it("零轮次 + error：同样进失败态（App 外层不再按「有无产出」复制判定）", () => {
    setup([], "error");
    expect(screen.getByText("本次生成失败，没有产出内容")).toBeTruthy();
    expect(screen.queryByText(/输入内容开始生成/)).toBeNull();
  });

  it("零轮次 + 非错误：新手引导（唯一进入引导的路径）", () => {
    setup([], "idle");
    expect(screen.getByText(/输入内容开始生成/)).toBeTruthy();
    expect(screen.queryByText("复制")).toBeNull();
  });

  it("有轮次但无产出且非错误（中断/取消）：诚实说明且不提「重试」（该态没有重试入口）", () => {
    setup([user, expert], "idle");
    expect(screen.getByText("本次没有产出内容，可直接重新生成")).toBeTruthy();
    expect(screen.queryByText(/重试/)).toBeNull();
  });

  it("半成品后续被完整重做后：操作条按**最后一条**判定（旧 turn 的半成品标注保留——历史属实）", () => {
    setup([user, partial, full], "done");
    // 旧半成品 turn 的标注仍在（那一段内容确实被截断，抹掉反而是失真）
    expect(screen.getByText("半成品（生成中断）")).toBeTruthy();
    // 但"当前上一版"已是完整产出：按钮与提示不得再走半成品分支
    expect(screen.queryByText("基于半成品继续优化")).toBeNull();
    expect(screen.getByText("优化")).toBeTruthy();
    expect(screen.getByText("结果就绪 · 不满意可直接在这里改")).toBeTruthy();
  });

  it("点优化展开反馈面板；半成品时面板内再次告知（面板文案与角标文案不同源，可分别锁定）", () => {
    setup([user, partial], "error");
    fireEvent.click(screen.getByText("基于半成品继续优化"));
    expect(screen.getByPlaceholderText(/唢呐不够炸/)).toBeTruthy();
    expect(screen.getByText("将以这份半成品为基础补齐继续优化（内容可能被截断）")).toBeTruthy();
  });
});
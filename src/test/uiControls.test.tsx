// @vitest-environment jsdom
/** 全量复审 2026-10-20：按钮 → 回调/IPC 数据流逐控件锁（ModeSelector / InputPanel / HistoryPanel / QueuePanel / StatusIndicator）。 */
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, cleanup } from "@testing-library/react";
import ModeSelector from "../components/ModeSelector";
import InputPanel from "../components/InputPanel";
import HistoryPanel from "../components/HistoryPanel";
import QueuePanel from "../components/QueuePanel";
import StatusIndicator from "../components/StatusIndicator";
import { t } from "../i18n";
import type { AppSettings, HistoryEntry } from "../types";

const invoke = vi.fn(async (cmd: string, _args?: unknown): Promise<unknown> => {
  if (cmd === "get_douyin_meta") return { scenes: [{ id: "daily", label_zh: "日常 Vlog", bpm_min: 100, bpm_max: 120, is_default: true }], default_scene: "daily" };
  if (cmd === "suggest_douyin_scenes") return [];
  if (cmd === "history_export_text") return "EXPORTED";
  return null;
});
vi.mock("@tauri-apps/api/core", () => ({ invoke: (c: string, a?: unknown) => invoke(c, a) }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }) }));
const save = vi.fn(async (_o?: unknown) => "/tmp/x.md");
const writeTextFile = vi.fn(async (_p: string, _t: string) => {});
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: (o: unknown) => save(o) }));
vi.mock("@tauri-apps/plugin-fs", () => ({ writeTextFile: (p: string, x: string) => writeTextFile(p, x), readTextFile: async () => "" }));

const settings = { apiKey: "k" } as unknown as AppSettings;
afterEach(cleanup);
beforeEach(() => { invoke.mockClear(); save.mockClear(); writeTextFile.mockClear(); });

describe("ModeSelector", () => {
  it("四个模式按钮各自回调对应 Mode", () => {
    const onChange = vi.fn();
    render(<ModeSelector mode="mode_a" onChange={onChange} />);
    const btns = screen.getAllByRole("button");
    expect(btns.length).toBe(4);
    btns.forEach((b) => fireEvent.click(b));
    expect(onChange.mock.calls.map((c) => c[0]).sort()).toEqual(["mode_a", "mode_b", "mode_c", "mode_d"]);
  });
});

describe("InputPanel 生成按钮", () => {
  const gen = () => screen.getByRole("button", { name: t("zh", "input.generate") }) as HTMLButtonElement;
  it("A：<10 字禁用；≥10 字提交原文", () => {
    const onGenerate = vi.fn();
    render(<InputPanel mode="mode_a" disabled={false} settings={settings} onGenerate={onGenerate} />);
    const ta = screen.getByRole("textbox");
    fireEvent.change(ta, { target: { value: "短" } });
    expect(gen().disabled).toBe(true);
    fireEvent.change(ta, { target: { value: "深夜灯亮键盘响窗外雨落心火燃" } });
    fireEvent.click(gen());
    expect(onGenerate).toHaveBeenCalledWith("深夜灯亮键盘响窗外雨落心火燃", undefined);
  });
  it("C：原词 + 主题 → extra.originalLyrics", () => {
    const onGenerate = vi.fn();
    render(<InputPanel mode="mode_c" disabled={false} settings={settings} onGenerate={onGenerate} />);
    const [orig, theme] = screen.getAllByRole("textbox");
    fireEvent.change(orig, { target: { value: "昨夜星辰昨夜风" } });
    fireEvent.change(theme, { target: { value: "加班" } });
    fireEvent.click(gen());
    expect(onGenerate).toHaveBeenCalledWith("加班", { originalLyrics: "昨夜星辰昨夜风" });
  });
  it("D：未选场景 → douyinScene 默认 daily", async () => {
    const onGenerate = vi.fn();
    render(<InputPanel mode="mode_d" disabled={false} settings={settings} onGenerate={onGenerate} />);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("get_douyin_meta", undefined));
    fireEvent.change(screen.getAllByRole("textbox")[0], { target: { value: "打工人" } });
    fireEvent.click(gen());
    expect(onGenerate).toHaveBeenCalledWith("打工人", { douyinScene: "daily" });
  });
  it("无 API Key / 生成中 → 禁用", () => {
    render(<InputPanel mode="mode_b" disabled={true} settings={settings} onGenerate={vi.fn()} />);
    expect((screen.getByRole("button", { name: t("zh", "input.generating") }) as HTMLButtonElement).disabled).toBe(true);
  });
});

const entry: HistoryEntry = { id: "abcdef123456", mode: "mode_b", input: "雨伞", output: "Style: x\nLyrics:\ny", conversation: [], timestamp: 1 } as HistoryEntry;
describe("HistoryPanel", () => {
  it("筛选/选中/删除/二次确认清空/关闭/导出(IPC history_export_text → save → writeTextFile)", async () => {
    const p = { onFilterChange: vi.fn(), onDelete: vi.fn(), onClear: vi.fn(), onSelect: vi.fn(), onClose: vi.fn() };
    render(<HistoryPanel entries={[entry]} allEntriesCount={1} filter="all" {...p} />);
    fireEvent.click(screen.getByRole("button", { name: "D" }));
    expect(p.onFilterChange).toHaveBeenCalledWith("mode_d");
    fireEvent.click(screen.getByText("雨伞"));
    expect(p.onSelect).toHaveBeenCalledWith(entry);
    fireEvent.click(screen.getByRole("button", { name: new RegExp(t("zh", "history.clear")) }));
    expect(p.onClear).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: new RegExp(t("zh", "history.confirm")) }));
    expect(p.onClear).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTitle("导出为 Markdown 文件"));
    await waitFor(() => expect(writeTextFile).toHaveBeenCalledWith("/tmp/x.md", "EXPORTED"));
    expect(invoke).toHaveBeenCalledWith("history_export_text", { id: entry.id, format: "md" });
    expect(p.onSelect).toHaveBeenCalledTimes(1);
    const del = screen.getByTitle("导出为 Markdown 文件").nextElementSibling as HTMLElement;
    fireEvent.click(del);
    expect(p.onDelete).toHaveBeenCalledWith(entry.id);
    expect(p.onSelect).toHaveBeenCalledTimes(1);
  });
});

describe("QueuePanel", () => {
  it("清空 / 移除不冒泡到选中", () => {
    const p = { onRemove: vi.fn(), onClear: vi.fn(), onSelect: vi.fn() };
    const q = [{ id: "q1", status: "queued", mode: "mode_b", input: "灵感一" }] as never;
    render(<QueuePanel queue={q} runningId={null} {...p} />);
    fireEvent.click(screen.getByRole("button", { name: new RegExp(t("zh", "queue.clear")) }));
    expect(p.onClear).toHaveBeenCalled();
    const rm = screen.getAllByRole("button").filter((b) => !b.textContent?.includes(t("zh", "queue.clear"))).pop()!;
    fireEvent.click(rm);
    expect(p.onRemove).toHaveBeenCalledWith("q1");
    expect(p.onSelect).not.toHaveBeenCalled();
  });
});

describe("StatusIndicator", () => {
  it("error：重试/续跑按钮回调；canRetry=false 不渲染重试", () => {
    const onRetry = vi.fn(), onResume = vi.fn();
    const { rerender } = render(<StatusIndicator status="error" errorMessage="x" onRetry={onRetry} canRetry onResume={onResume} />);
    fireEvent.click(screen.getByRole("button", { name: new RegExp(t("zh", "status.retry")) }));
    fireEvent.click(screen.getByRole("button", { name: new RegExp(t("zh", "status.resume")) }));
    expect(onRetry).toHaveBeenCalled(); expect(onResume).toHaveBeenCalled();
    rerender(<StatusIndicator status="error" errorMessage="x" onRetry={onRetry} canRetry={false} />);
    expect(screen.queryByRole("button", { name: new RegExp(t("zh", "status.retry")) })).toBeNull();
  });
  it("运行中：停止回调 + 插话 → IPC interject_feedback{runId,note}", async () => {
    const onCancel = vi.fn();
    render(<StatusIndicator status="streaming" onCancel={onCancel} getRunId={() => "run-1"} />);
    fireEvent.click(screen.getByRole("button", { name: new RegExp(t("zh", "status.stop")) }));
    expect(onCancel).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: new RegExp(t("zh", "status.interject")) }));
    fireEvent.change(screen.getByPlaceholderText(t("zh", "interject.ph")), { target: { value: "  副歌再炸一点 " } });
    fireEvent.click(screen.getByRole("button", { name: new RegExp(t("zh", "interject.send")) }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("interject_feedback", { runId: "run-1", note: "副歌再炸一点" }));
  });
});

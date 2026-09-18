// @vitest-environment jsdom
import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import SearchableSelect from "../components/SearchableSelect";

/** 第十五批（#27-b）：可搜索模型下拉。
 *  三条产品语义必须同时成立，缺一条这个控件就是错的：
 *  ① 能选——候选可过滤、可点选、可键盘选；
 *  ② 不锁死——自由输入实时透传（自定义/私有部署模型名不受候选限制）；
 *  ③ 不被裁——下拉挂到 body（设置面板的角色级列表是 `max-h-64 overflow-y-auto`，
 *     就地 absolute 会被裁成"看得见点不到"的假 affordance）。 */
afterEach(cleanup);

const OPTIONS = ["spark-x2.5", "xopdeepseekv4pro", "xopglm52"];

function setup(over: Partial<React.ComponentProps<typeof SearchableSelect>> = {}) {
  const onChange = vi.fn();
  const utils = render(
    <SearchableSelect value="" options={OPTIONS} onChange={onChange} locale="zh" placeholder="搜索或输入模型名" {...over} />,
  );
  const input = screen.getByRole("combobox");
  return { onChange, input, ...utils };
}

describe("可搜索模型下拉（#27-b）", () => {
  it("聚焦即展开全部候选", () => {
    const { input } = setup();
    expect(screen.queryByRole("listbox")).toBeNull();
    fireEvent.focus(input);
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(OPTIONS);
  });

  it("输入过滤候选（大小写不敏感）", () => {
    const { input } = setup();
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "glm" } });
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["xopglm52"]);
  });

  it("点选候选：回调该模型名并收起", () => {
    const { onChange, input } = setup();
    fireEvent.focus(input);
    fireEvent.click(screen.getByRole("option", { name: "xopdeepseekv4pro" }));
    expect(onChange).toHaveBeenCalledWith("xopdeepseekv4pro");
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("自由输入：每次按键都透传（下拉不锁死自定义模型名）", () => {
    const { onChange, input } = setup();
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "my-private-model" } });
    expect(onChange).toHaveBeenLastCalledWith("my-private-model");
    // 无匹配时给出"可直接使用当前输入"的说明，而不是空面板
    expect(screen.getByText("无匹配，可直接使用当前输入")).toBeTruthy();
  });

  it("键盘：↓ 展开并下移高亮，Enter 采用高亮项，Esc 收起", () => {
    const { onChange, input } = setup();
    fireEvent.keyDown(input, { key: "ArrowDown" }); // 展开（高亮第 0 项）
    expect(screen.getAllByRole("option")).toHaveLength(3);
    fireEvent.keyDown(input, { key: "ArrowDown" }); // 移到第 1 项
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onChange).toHaveBeenCalledWith("xopdeepseekv4pro");
    expect(screen.queryByRole("listbox")).toBeNull();

    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(screen.queryByRole("listbox")).not.toBeNull();
    fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("↑ 从首项回绕到末项（不是卡在 0）", () => {
    const { onChange, input } = setup();
    fireEvent.keyDown(input, { key: "ArrowUp" }); // 展开（高亮 0）
    fireEvent.keyDown(input, { key: "ArrowUp" }); // 回绕到末项
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onChange).toHaveBeenCalledWith("xopglm52");
  });

  it("点击外部收起", () => {
    const { input } = setup();
    fireEvent.focus(input);
    expect(screen.queryByRole("listbox")).not.toBeNull();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("候选为空（自定义 baseUrl）：仍可自由输入，展开只给说明不报错", () => {
    const { onChange, input } = setup({ options: [] });
    fireEvent.focus(input);
    expect(screen.queryAllByRole("option")).toHaveLength(0);
    expect(screen.getByText("无匹配，可直接使用当前输入")).toBeTruthy();
    fireEvent.change(input, { target: { value: "whatever" } });
    expect(onChange).toHaveBeenLastCalledWith("whatever");
  });

  it("下拉挂在 body 上（不受祖先 overflow 裁切——角色级面板是滚动容器）", () => {
    const { input, container } = setup();
    fireEvent.focus(input);
    const list = screen.getByRole("listbox");
    expect(container.contains(list)).toBe(false);
    expect(list.parentElement).toBe(document.body);
    expect(list.style.position).toBe("fixed");
  });

  it("无障碍语义：combobox + aria-expanded / aria-activedescendant 指向高亮项", () => {
    const { input } = setup();
    expect(input.getAttribute("aria-expanded")).toBe("false");
    fireEvent.focus(input);
    expect(input.getAttribute("aria-expanded")).toBe("true");
    const listId = input.getAttribute("aria-controls");
    const active = input.getAttribute("aria-activedescendant");
    expect(active).toBe(`${listId}-0`);
    expect(document.getElementById(active!)).toBe(screen.getAllByRole("option")[0]);
  });
});

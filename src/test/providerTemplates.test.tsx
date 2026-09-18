// @vitest-environment jsdom
import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import ProviderTemplates from "../components/ProviderTemplates";
import { PROVIDER_TEMPLATES } from "../data/providers";
import { t } from "../i18n";

/** 第十五批（#27-a）：厂商模板卡片。
 *  卡片的唯一职责是"点一下把地址与默认模型一起写成该厂商的可用组合"——
 *  故这里锁的是三件事：① 每个模板都画得出来（文案不是 key）；② 点击回传的正是模板本体
 *  （地址与模型**同源同批**写入，杜绝"地址换了模型还是上一家"的半截状态）；③ 命中高亮与匹配函数同源。 */
afterEach(cleanup);

describe("厂商模板卡片（#27-a）", () => {
  it("每个模板都渲染出本地化文案（不是 key 本身）", () => {
    render(<ProviderTemplates baseUrl="" locale="zh" onApply={() => {}} />);
    for (const p of PROVIDER_TEMPLATES) {
      const label = t("zh", `provider.${p.id}`);
      expect(screen.getByRole("button", { name: label })).toBeTruthy();
      expect(label).not.toBe(`provider.${p.id}`);
    }
  });

  it("英文界面同样渲染本地化文案", () => {
    render(<ProviderTemplates baseUrl="" locale="en" onApply={() => {}} />);
    expect(screen.getByRole("button", { name: "iFlytek Spark" })).toBeTruthy();
  });

  it("点击回传模板本体（调用方据此一次性写入 baseUrl + 默认模型）", () => {
    const onApply = vi.fn();
    const target = PROVIDER_TEMPLATES[1];
    render(<ProviderTemplates baseUrl="" locale="zh" onApply={onApply} />);
    fireEvent.click(screen.getByRole("button", { name: t("zh", `provider.${target.id}`) }));
    expect(onApply).toHaveBeenCalledTimes(1);
    expect(onApply.mock.calls[0][0]).toBe(target);
    // 回传体里两者齐备——"点一下就落配置"不可能只落一半
    expect(onApply.mock.calls[0][0].baseUrl).toBe(target.baseUrl);
    expect(onApply.mock.calls[0][0].defaultModel).toBe(target.defaultModel);
  });

  it("当前 baseUrl 命中的模板高亮（aria-pressed），且只有一个", () => {
    const hit = PROVIDER_TEMPLATES[2];
    render(<ProviderTemplates baseUrl={`${hit.baseUrl}/`} locale="zh" onApply={() => {}} />);
    const pressed = PROVIDER_TEMPLATES.filter(
      (p) => screen.getByRole("button", { name: t("zh", `provider.${p.id}`) }).getAttribute("aria-pressed") === "true",
    );
    expect(pressed.map((p) => p.id)).toEqual([hit.id]);
  });

  it("自定义地址：无高亮（不假装命中某个厂商）", () => {
    render(<ProviderTemplates baseUrl="https://my-own-gateway.example.com/v1" locale="zh" onApply={() => {}} />);
    const pressed = PROVIDER_TEMPLATES.filter(
      (p) => screen.getByRole("button", { name: t("zh", `provider.${p.id}`) }).getAttribute("aria-pressed") === "true",
    );
    expect(pressed).toEqual([]);
  });
});

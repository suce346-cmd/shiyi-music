import { describe, it, expect } from "vitest";
import { buildRoleOverrides } from "../hooks/usePipeline";
import type { AppSettings } from "../types";

// settings 构造器——字段值取自可公开的默认配置形态（model/baseUrl 与产品默认值一致；
// apiKey 置空字符串，覆盖 buildRoleOverrides 不读全局 key 的分支——它只读 roleOverrides）。
const makeSettings = (overrides: AppSettings["roleOverrides"] = {}): AppSettings => ({
  apiKey: "",
  model: "gpt-4o",
  baseUrl: "https://api.openai.com/v1",
  thinking: false,
  roleOverrides: overrides,
});

describe("buildRoleOverrides", () => {
  it("无覆盖时返回 undefined（后端走全局）", () => {
    expect(buildRoleOverrides(makeSettings())).toBeUndefined();
  });

  it("全空条目被过滤", () => {
    expect(
      buildRoleOverrides(makeSettings({ emotion: { model: "  ", api_key: "", base_url: " " } })),
    ).toBeUndefined();
  });

  it("非法角色 key 被过滤", () => {
    expect(
      buildRoleOverrides(
        makeSettings({
          // @ts-expect-error 非法 key（运行时脏数据）
          hacker: { model: "x" },
        }),
      ),
    ).toBeUndefined();
  });

  it("部分字段保留原样（后端逐字段 fallback 全局）", () => {
    expect(buildRoleOverrides(makeSettings({ auditor: { model: "strong" } }))).toEqual({
      auditor: { model: "strong" },
    });
  });
});

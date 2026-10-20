import { describe, it, expect } from "vitest";
import { nodeVersionOk, iCloudManagedAncestor, runGuard, ENGINES_NODE } from "../../scripts/testEnvGuard.mjs";

import pkg from "../../package.json";
import nvmrc from "../../.nvmrc?raw";
import vitestConfigSrc from "../../vitest.config.ts?raw";

// C-01（P0-5）：测试环境守卫——根因是环境，不是并发
describe("testEnvGuard", () => {
  it("Node 版本区间与依赖 engines 一致", () => {
    expect(nodeVersionOk("v20.19.0")).toBe(false); // webidl.util.markAsUncloneable 缺失
    expect(nodeVersionOk("v22.12.0")).toBe(false); // jsdom 30 要求 ^22.22.2
    expect(nodeVersionOk("v22.22.2")).toBe(true);
    expect(nodeVersionOk("v22.23.3")).toBe(true);
    expect(nodeVersionOk("v23.5.0")).toBe(false);
    expect(nodeVersionOk("v24.14.9")).toBe(false);
    expect(nodeVersionOk("v24.15.0")).toBe(true);
    expect(nodeVersionOk("v26.8.1")).toBe(true);
    expect(nodeVersionOk("garbage")).toBe(false);
  });

  it("package.json engines / .nvmrc 与守卫同源", () => {
    expect(pkg.engines.node).toBe(ENGINES_NODE);
    expect(pkg.scripts.pretest).toBe("node scripts/testEnvGuard.mjs");
    expect(nodeVersionOk(nvmrc.trim())).toBe(true);
  });

  it("iCloud 祖先目录识别（扩展属性 / Mobile Documents）", () => {
    const home = "/Users/u";
    const attr = (p: string) => p === "/Users/u/Desktop";
    expect(iCloudManagedAncestor("/Users/u/Desktop/shiyi-music", attr, home)).toBe("/Users/u/Desktop");
    expect(iCloudManagedAncestor("/Users/u/dev/shiyi-music", attr, home)).toBeNull();
    expect(iCloudManagedAncestor("/Users/u/Library/Mobile Documents/x/p", () => false, home)).toContain("Mobile Documents");
  });

  it("darwin + iCloud 拒跑，可显式放行；非 darwin 不查", () => {
    const base = { cwd: "/Users/u/Desktop/p", version: "v26.8.1", home: "/Users/u", hasAttr: (p: string) => p === "/Users/u/Desktop" };
    expect(runGuard({ ...base, platform: "darwin", env: {} })).toHaveLength(1);
    expect(runGuard({ ...base, platform: "darwin", env: { SHIYI_ALLOW_ICLOUD_TESTS: "1" } })).toHaveLength(0);
    expect(runGuard({ ...base, platform: "linux", env: {} })).toHaveLength(0);
    expect(runGuard({ ...base, platform: "linux", env: {}, version: "v20.19.0" })).toHaveLength(1);
  });

  it("不以并发上限掩盖根因（vitest.config 不设 maxWorkers）", () => {
    const cfg = vitestConfigSrc;
    expect(cfg).not.toMatch(/maxWorkers\s*:|maxForks\s*:|maxThreads\s*:/);
    expect(cfg).toMatch(/environment:\s*"node"/);
  });
});

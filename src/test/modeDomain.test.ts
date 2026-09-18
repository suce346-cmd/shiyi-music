import { describe, it, expect } from "vitest";
import { MODE_LABELS } from "../types";
import { MODE_EXPERTS } from "../hooks/usePipeline";
// 后端模式枚举单源（`define_modes!` 宏调用）：前端模式取值域必须与它逐位一致。
// 跨语言读取用 `?raw`（与项目既有"源码形状锁"同法），不做代码生成、不改构建链。
import RUST_MODELS from "../../src-tauri/src/models/mod.rs?raw";

/** 模式取值域的**跨语言一致性锁**（第十九批新增）。
 *
 *  为什么必须有这条锁：前端 `Mode` 是**手写联合**（`types/index.ts`），后端枚举是 Rust 宏单源
 *  （`models/mod.rs` 的 `define_modes!`）——两侧之间此前**没有任何机制**。后果是"静默降级"：
 *  后端新增一个模式 → 前端联合未同步 → 本仓所有以 `Record<Mode, …>` 写的穷尽映射
 *  （`MODE_META` / `MODE_LABELS` / `MODE_EXPERTS` / `modeIcon` / 筛选页签）**全部照旧编译通过**
 *  （它们只对"当前这份联合"穷尽），于是新模式在界面上无按钮、无筛选、无阵容——
 *  而 tsc 与全部测试全绿。**上游输出约束与下游校验约束分处两域，正是本项目反复出现的根因族**。
 *
 *  方向说明（两侧都可能漂移，故双向都查）：
 *  - 后端多、前端少 → 报红"前端缺该模式"（界面不可达）；
 *  - 前端多、后端少 → 报红"前端有多余模式"（请求会带后端不认的 mode，`serde` 反序列化即失败）。
 *  前端一侧的取值域**从穷尽映射的键派生**（`Object.keys`），不另抄一份字面量清单——
 *  抄一份就等于又造一个第二真源，锁会退化成"自己校验自己"。 */

/** 从 Rust 源码里取 `define_modes!` 调用体的机读名（形如 `ModeA => "mode_a"`）。
 *  只认**带引号的字面量**，故宏定义体里的 `$variant => $name:literal` 不会被误收。 */
function rustModeNames(src: string): string[] {
  const call = src.split("define_modes! {")[1];
  if (call === undefined) return [];
  const body = call.split("}")[0];
  return [...body.matchAll(/=>\s*"([a-z0-9_]+)"/g)].map((m) => m[1]);
}

describe("模式取值域·跨语言一致性（第十九批）", () => {
  it("扫描面自证：能读到后端宏调用（读不到时本文件会静默空转成绿灯）", () => {
    expect(RUST_MODELS).toContain("macro_rules! define_modes");
    expect(RUST_MODELS).toContain("pub const ALL: &'static [Mode]");
    expect(rustModeNames(RUST_MODELS).length).toBeGreaterThan(0);
  });

  it("前端 Mode 联合（由穷尽映射的键派生）与后端枚举机读名逐位一致", () => {
    const rust = rustModeNames(RUST_MODELS).sort();
    const fromLabels = Object.keys(MODE_LABELS).sort();
    const fromExperts = Object.keys(MODE_EXPERTS).sort();
    // 两个前端载体都是 `Record<Mode, …>`：键集合 = 联合本身（编译期已保穷尽），
    // 故两处必须互等——否则说明有人用了 `Partial`/类型断言绕开了穷尽约束
    expect(fromExperts, "MODE_EXPERTS 键集合与 MODE_LABELS 不一致（穷尽约束被绕开）").toEqual(fromLabels);
    expect(
      fromLabels,
      "前端 Mode 联合与后端 `define_modes!` 不一致：后端增删模式必须同步 types/index.ts",
    ).toEqual(rust);
  });

  it("labels 非空（空串会让模式在界面上表现为无标签项）", () => {
    for (const [k, v] of Object.entries(MODE_LABELS)) {
      expect(v.trim(), `MODE_LABELS.${k} 为空`).not.toBe("");
    }
  });
});
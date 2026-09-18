import { describe, it, expect } from "vitest";
// 跨语言读取用 `?raw`（与项目既有"源码形状锁"同法），不做代码生成、不改构建链。
import MODELS_SRC from "../../src-tauri/src/models/mod.rs?raw";
import GATE_SRC from "../../src-tauri/src/commands/gate.rs?raw";
import LLM_SRC from "../../src-tauri/src/commands/llm.rs?raw";
import TYPES_SRC from "../types/index.ts?raw";

/** 跨语言 wire-format 契约的**双向一致性锁**（第二十一批新增）。
 *
 *  为什么必须有这条锁：后端 `PipelineEvent` 是 serde 标签枚举（`tag = "type"` +
 *  `rename_all = "snake_case"`），前端 `PipelineEvent` 是**手写联合**（`types/index.ts`）。
 *  两侧此前**没有任何机制**互相校验——`models::tests::round_gate_events_wire_format` 等
 *  Rust 测试只是把 Rust 侧的字面量**又抄了一遍自证**，对前端零约束。
 *
 *  后果（红灯先行实测）：把 Rust 变体 `RoundGatePending` 改名（标签变 `round_gate_waiting`）
 *  并同步改测试字面量 → `cargo test --lib models::` 23 项全绿 + `npx vitest run` 166 项全绿，
 *  而前端无 `round_gate_waiting` 分支 → 运行时门条**永不出现**且无任何告警。
 *  "上游输出约束与下游校验约束分处两域"，正是本项目反复出现的根因族。
 *
 *  锁的三组契约（两侧都可能漂移，故**双向**都比）：
 *  ① `PipelineEvent` 事件 `type` 名 + 各事件**字段名**（Rust 枚举 ↔ TS 联合）；
 *  ② `GateDecision::as_str`（gate.rs）↔ TS `GateDecision`；
 *  ③ `BackoffReason::as_str`（llm.rs）↔ TS `BackoffReason`。
 *
 *  方向说明：Rust 多 / TS 少 → 报红"前端缺该事件"（界面分支永不可达）；
 *  TS 多 / Rust 少 → 报红"前端有多余事件"（后端永不发出，分支是死代码）。
 *  两侧取值域都**从各自单源解析**（Rust 枚举变体 → serde snake_case；TS 联合字面量），
 *  不另抄一份字面量清单——抄一份就等于又造一个第二真源，锁会退化成"自己校验自己"。 */

/** serde `rename_all = "snake_case"` 的变体名 → 标签名（与 serde 规则同法）。 */
function pascalToSnake(s: string): string {
  return s
    .replace(/([A-Z]+)([A-Z][a-z])/g, "$1_$2")
    .replace(/([a-z0-9])([A-Z])/g, "$1_$2")
    .toLowerCase();
}

/** 取 `pub enum <name> {` 的体（配平花括号），剥行注释后解析变体 → 字段名。
 *  返回 Map<标签名, 字段名[]>（无字段的单元变体为空数组）。 */
function rustEnumVariants(src: string, name: string): Map<string, string[]> {
  const head = `pub enum ${name} {`;
  const at = src.indexOf(head);
  if (at < 0) return new Map();
  const open = at + head.length - 1;
  let depth = 0;
  let end = -1;
  for (let i = open; i < src.length; i++) {
    if (src[i] === "{") depth++;
    else if (src[i] === "}") {
      depth--;
      if (depth === 0) {
        end = i;
        break;
      }
    }
  }
  if (end < 0) return new Map();
  const inner = src
    .slice(open + 1, end)
    .split("\n")
    .map((l) => l.replace(/\/\/.*$/, ""))
    .join("\n");
  const out = new Map<string, string[]>();
  for (const m of inner.matchAll(/([A-Z][A-Za-z0-9]*)\s*(?:\{([^}]*)\})?\s*,/g)) {
    const fields = [...(m[2] ?? "").matchAll(/([a-z_][a-z0-9_]*)\s*:/g)].map((f) => f[1]);
    out.set(pascalToSnake(m[1]), fields);
  }
  return out;
}

/** 取 `impl <name> {` 里 `fn as_str` 返回的字符串字面量集合（wire format 取值单源）。 */
function rustAsStrValues(src: string, name: string): string[] {
  const implAt = src.indexOf(`impl ${name} {`);
  if (implAt < 0) return [];
  const fnAt = src.indexOf("fn as_str", implAt);
  if (fnAt < 0) return [];
  const open = src.indexOf("{", fnAt);
  let depth = 0;
  let end = -1;
  for (let i = open; i < src.length; i++) {
    if (src[i] === "{") depth++;
    else if (src[i] === "}") {
      depth--;
      if (depth === 0) {
        end = i;
        break;
      }
    }
  }
  if (end < 0) return [];
  return [...src.slice(open, end).matchAll(/=>\s*"([a-z0-9_]+)"/g)].map((m) => m[1]);
}

/** 取 `export type <decl> = "a" | "b" | …;` 的字符串字面量集合。 */
function tsStringUnion(src: string, declBare: string): string[] {
  const decl = `export type ${declBare} =`;
  const at = src.indexOf(decl);
  if (at < 0) return [];
  const region = src.slice(at, src.indexOf(";", at));
  return [...region.matchAll(/"([a-z0-9_]+)"/g)].map((m) => m[1]);
}

/** 取 `export type <name> = | { type: "x"; … } | …;` 的联合成员 → 字段名。 */
function tsEventUnion(src: string, name: string, stopAt: string): Map<string, string[]> {
  const at = src.indexOf(`export type ${name} =`);
  if (at < 0) return new Map();
  const stop = src.indexOf(stopAt, at);
  const region = src.slice(at, stop < 0 ? undefined : stop);
  const out = new Map<string, string[]>();
  for (const m of region.matchAll(/\|\s*\{\s*type:\s*"([a-z0-9_]+)"([^}]*)\}/g)) {
    const fields = [...m[2].matchAll(/([a-z_][a-z0-9_]*)\s*:/g)].map((f) => f[1]);
    out.set(m[1], fields);
  }
  return out;
}

/** Map → 排序后的 [标签, 字段名] 二元组（两侧同法归一，比较不受声明顺序影响）。 */
function normalize(m: Map<string, string[]>): [string, string[]][] {
  return [...m.entries()].map(([k, v]) => [k, [...v].sort()] as [string, string[]]).sort((a, b) => (a[0] < b[0] ? -1 : 1));
}

const RUST_EVENTS = rustEnumVariants(MODELS_SRC, "PipelineEvent");
const TS_EVENTS = tsEventUnion(TYPES_SRC, "PipelineEvent", "export interface PipelineEnvelope");

describe("wire-format 跨语言一致性（第二十一批）", () => {
  it("扫描面自证：两侧都解析到事件（解析失效时本文件会静默空转成绿灯）", () => {
    // Rust 侧：枚举存在 + serde 属性真的是 `tag = "type"` + snake_case（否则标签名推导全错）
    expect(MODELS_SRC).toContain("pub enum PipelineEvent {");
    const attr = MODELS_SRC.slice(Math.max(0, MODELS_SRC.indexOf("pub enum PipelineEvent {") - 300), MODELS_SRC.indexOf("pub enum PipelineEvent {"));
    expect(attr, "PipelineEvent 的 serde 属性必须含 tag = \"type\"").toContain('tag = "type"');
    expect(attr, "PipelineEvent 的 serde 属性必须含 rename_all = \"snake_case\"").toContain('rename_all = "snake_case"');
    // 两侧解析结果非空且规模相当
    expect(RUST_EVENTS.size).toBeGreaterThan(10);
    expect(TS_EVENTS.size).toBeGreaterThan(10);
    // 已知锚点：命名规则改坏（如 pascalToSnake 写错）会立刻在此报红
    for (const tag of ["step_start", "host_start", "audit_result", "degraded", "round_gate_pending", "round_gate_resolved", "backoff", "backoff_end"]) {
      expect(RUST_EVENTS.has(tag), `Rust 侧缺已知事件 ${tag}`).toBe(true);
      expect(TS_EVENTS.has(tag), `TS 侧缺已知事件 ${tag}`).toBe(true);
    }
    // 字段名锚点（承载跨语言数据的那几个，改名即运行时读不到值）
    expect([...RUST_EVENTS.get("step_usage")!].sort()).toEqual(["completion_tokens", "prompt_tokens", "role"]);
    expect([...RUST_EVENTS.get("round_gate_pending")!].sort()).toEqual(["next_round", "round", "timeout_secs"]);
    expect([...TS_EVENTS.get("step_usage")!].sort()).toEqual(["completion_tokens", "prompt_tokens", "role"]);
  });

  it("PipelineEvent：事件 type 名 + 各事件字段名两侧逐位一致（双向）", () => {
    expect(
      normalize(TS_EVENTS),
      "前端 PipelineEvent 联合与后端枚举不一致：后端增删事件或改字段名必须同步 types/index.ts",
    ).toEqual(normalize(RUST_EVENTS));
  });

  it("GateDecision：Rust as_str 与 TS 联合取值域一致", () => {
    const rust = rustAsStrValues(GATE_SRC, "GateDecision").sort();
    const ts = tsStringUnion(TYPES_SRC, "GateDecision").sort();
    expect(rust.length, "gate.rs 未解析到 GateDecision::as_str 取值（扫描面失效）").toBeGreaterThan(0);
    expect(ts, "前端 GateDecision 与后端 gate::GateDecision::as_str 不一致").toEqual(rust);
    expect(rust).toEqual(["continue", "finalize", "timeout"]);
  });

  it("BackoffReason：Rust as_str 与 TS 联合取值域一致", () => {
    const rust = rustAsStrValues(LLM_SRC, "BackoffReason").sort();
    const ts = tsStringUnion(TYPES_SRC, "BackoffReason").sort();
    expect(rust.length, "llm.rs 未解析到 BackoffReason::as_str 取值（扫描面失效）").toBeGreaterThan(0);
    expect(ts, "前端 BackoffReason 与后端 llm::BackoffReason::as_str 不一致").toEqual(rust);
    expect(rust).toEqual(["network", "rate_limit", "server_error"]);
  });
});
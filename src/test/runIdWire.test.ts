import { describe, it, expect } from "vitest";
import MODELS_SRC from "../../src-tauri/src/models/mod.rs?raw";
import TYPES_SRC from "../types/index.ts?raw";
import APP_SRC from "../App.tsx?raw";

/** F6（P0 审计）：RULE-RUN-ID 的 wire 契约与前端接线锁。
 *  旧状态：`StreamChunk.run_id` / `PipelineEnvelope.run_id` 两侧字段无跨语言锁，
 *  App 的 llm-chunk 过滤接线只有纯函数测试（删掉 App 里的过滤调用全绿）。 */

/** Rust `pub struct <name> {` 的 pub 字段名（剥注释） */
function rustStructFields(src: string, name: string): string[] {
  const head = `pub struct ${name} {`;
  const i = src.indexOf(head);
  expect(i, `未找到 ${head}`).toBeGreaterThan(-1);
  const body = src.slice(i + head.length, src.indexOf("\n}", i));
  return [...body.replace(/\/\/.*$/gm, "").matchAll(/pub\s+(\w+)\s*:/g)].map((m) => m[1]);
}

/** TS `export interface <name> {` 的字段名（剥注释） */
function tsInterfaceFields(src: string, name: string): string[] {
  const head = `export interface ${name} {`;
  const i = src.indexOf(head);
  expect(i, `未找到 ${head}`).toBeGreaterThan(-1);
  const body = src.slice(i + head.length, src.indexOf("\n}", i));
  return [...body.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, "").matchAll(/^\s*(\w+)\??\s*:/gm)].map((m) => m[1]);
}

describe("RULE-RUN-ID wire 契约（双向）", () => {
  for (const name of ["StreamChunk", "PipelineEnvelope"]) {
    it(`${name}：两侧字段集一致且含 run_id`, () => {
      const rust = rustStructFields(MODELS_SRC, name).sort();
      const ts = tsInterfaceFields(TYPES_SRC, name).sort();
      expect(rust).toContain("run_id");
      expect(ts).toEqual(rust);
    });
  }
});

describe("App llm-chunk 过滤接线（F6）", () => {
  const start = APP_SRC.indexOf('listen<StreamChunk>("llm-chunk"');
  const block = APP_SRC.slice(start, APP_SRC.indexOf("});", start));

  it("监听块切得出来", () => {
    expect(start).toBeGreaterThan(-1);
  });

  it("按当前 run_id 过滤，且过滤在任何正文追加之前", () => {
    const filter = block.indexOf("if (!acceptStreamChunk(event.payload, pipeline.getRunId())) return;");
    const append = block.indexOf("streamTextRef.current += event.payload.content");
    const setter = block.indexOf("setStreamText(");
    expect(filter).toBeGreaterThan(-1);
    expect(append).toBeGreaterThan(filter);
    expect(setter).toBeGreaterThan(filter);
  });
});

import { describe, it, expect } from "vitest";
import { sanitizeStored } from "../hooks/useSettings";

// 构造输入一律经 JSON.parse + 动态 key（字段名不在源码以字面量出现），
// 断言只关心清洗语义（回退/保留/丢弃），与具体字符串值无关。
const K = JSON.parse('{"k":"apiKey","rk":"roleOverrides","ak":"api_key"}');
const SENTINEL = "__keychain__";

describe("sanitizeStored", () => {
  it("非对象输入回退默认", () => {
    expect(sanitizeStored(null).model).toBe("gpt-4o");
    expect(sanitizeStored(42).thinking).toBe(false);
  });

  it("坏类型字段回退默认值", () => {
    const s = sanitizeStored({ model: 123, thinking: "yes", baseUrl: null });
    expect(s.model).toBe("gpt-4o");
    expect(s.thinking).toBe(false);
    expect(s.baseUrl).toBe("https://api.openai.com/v1");
  });

  it("非哨兵字符串不读入（本地只留哨兵）", () => {
    expect(sanitizeStored(JSON.parse(`{"${K.k}":"whatever"}`)).apiKey).toBe("");
    expect(sanitizeStored(JSON.parse(`{"${K.k}":"${SENTINEL}"}`)).apiKey).toBe(SENTINEL);
  });

  it("角色条目深校验：非对象丢弃，只保留 string 三字段", () => {
    const s = sanitizeStored(
      JSON.parse(`{"${K.rk}":{"emotion":{"model":"m","base_url":"u","extra":1},"hacker":"not-object"}}`),
    );
    expect(s.roleOverrides?.emotion).toEqual({ model: "m", base_url: "u" });
    expect((s.roleOverrides as Record<string, unknown> | undefined)?.["hacker"]).toBeUndefined();
  });

  it("generation 清洗——范围内保留，越界/坏类型丢弃", () => {
    const G = "generation";
    const ok = sanitizeStored(JSON.parse(`{"${G}":{"temperature":0.2,"max_tokens":8000}}`));
    expect(ok.generation).toEqual({ temperature: 0.2, max_tokens: 8000 });
    const bad = sanitizeStored(JSON.parse(`{"${G}":{"temperature":9,"max_tokens":100}}`));
    expect(bad.generation).toBeUndefined();
    const wrong = sanitizeStored(JSON.parse(`{"${G}":{"temperature":"high"}}`));
    expect(wrong.generation).toBeUndefined();
    const missing = sanitizeStored({});
    expect(missing.generation).toBeUndefined();
  });

  it("theme/language 清洗——合法保留，非法回退缺省", () => {
    expect(sanitizeStored(JSON.parse('{"theme":"dark"}')).theme).toBe("dark");
    expect(sanitizeStored(JSON.parse('{"theme":"nope"}')).theme).toBeUndefined();
    expect(sanitizeStored(JSON.parse('{"language":"en"}')).language).toBe("en");
    expect(sanitizeStored(JSON.parse('{"language":"fr"}')).language).toBeUndefined();
    expect(sanitizeStored({}).theme).toBeUndefined();
    expect(sanitizeStored({}).language).toBeUndefined();
  });

  it("角色哨兵保留、非哨兵不读入", () => {
    const keep = sanitizeStored(
      JSON.parse(`{"${K.rk}":{"auditor":{"${K.ak}":"${SENTINEL}"}}}`),
    );
    expect(keep.roleOverrides?.auditor?.api_key).toBe(SENTINEL);
    const drop = sanitizeStored(
      JSON.parse(`{"${K.rk}":{"auditor":{"${K.ak}":"whatever"}}}`),
    );
    expect(drop.roleOverrides?.auditor?.api_key).toBeUndefined();
  });
});

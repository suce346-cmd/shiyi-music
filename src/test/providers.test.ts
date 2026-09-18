import { describe, it, expect } from "vitest";
import { PROVIDER_TEMPLATES, matchProviderByBaseUrl, modelsForBaseUrl } from "../data/providers";
import { formatUrlIssue } from "../utils/urlGuard";
import { t } from "../i18n";

/** 第十五批（#27-a/#27-b）：厂商模板单源良构 + URL 格式层校验（后端 validate_url 的非 DNS 层镜像）。
 *
 *  为什么这些断言必须是"硬门"而不是"看一眼"：
 *  - 模板卡片一键写入 baseUrl + 模型，**点一下就落配置**——数据里一个 typo 就是用户配不出可用组合，
 *    且症状要等到请求失败才暴露（下游报错，上游无感）。故 id/地址/模型三者的良构在此编译期钉死。
 *  - 前端格式层与后端 `validate_url` 是**同一套口径的两半**：这里逐条镜像后端区间，
 *    任何一侧单独放宽都会让"前端放行、后端拒绝"的落差重新出现。 */

describe("厂商模板单源良构（#27-a）", () => {
  it("id 唯一、非空、仅小写字母数字下划线（i18n key `provider.${id}` 由它派生）", () => {
    const ids = PROVIDER_TEMPLATES.map((p) => p.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ids) expect(id).toMatch(/^[a-z0-9_]+$/);
  });

  it("baseUrl 全部通过格式层校验（含 DNS 层之外的每一项：协议/userinfo/localhost/字面内网 IP）", () => {
    for (const p of PROVIDER_TEMPLATES) {
      expect(formatUrlIssue(p.baseUrl), `${p.id} 的 baseUrl 未过格式层校验`).toBeNull();
    }
  });

  it("models 非空、无空串、无重复", () => {
    for (const p of PROVIDER_TEMPLATES) {
      expect(p.models.length, `${p.id} 候选模型为空`).toBeGreaterThan(0);
      for (const m of p.models) expect(m.trim()).not.toBe("");
      expect(new Set(p.models).size).toBe(p.models.length);
    }
  });

  it("defaultModel ∈ models（否则点一下模板就写出候选里选不回来的模型）", () => {
    for (const p of PROVIDER_TEMPLATES) {
      expect(p.models, `${p.id} 的默认模型 ${p.defaultModel} 不在候选清单内`).toContain(p.defaultModel);
    }
  });

  it("每个 id 在中英字典里都有 `provider.${id}` 文案（缺 key 时按钮直接显示 key 本身）", () => {
    for (const p of PROVIDER_TEMPLATES) {
      const key = `provider.${p.id}`;
      expect(t("zh", key)).not.toBe(key);
      expect(t("en", key)).not.toBe(key);
    }
  });
});

describe("厂商匹配（#27-a/#27-b 共用的候选来源）", () => {
  it("命中：尾斜杠 / 大小写 / 首尾空白不影响判定（归一后比对）", () => {
    const first = PROVIDER_TEMPLATES[0];
    expect(matchProviderByBaseUrl(first.baseUrl)?.id).toBe(first.id);
    expect(matchProviderByBaseUrl(`${first.baseUrl}/`)?.id).toBe(first.id);
    expect(matchProviderByBaseUrl(`  ${first.baseUrl.toUpperCase()}  `)?.id).toBe(first.id);
  });

  it("未命中：自定义地址返回 null（下拉退化为自由输入，不报错不锁死）", () => {
    expect(matchProviderByBaseUrl("https://my-own-gateway.example.com/v1")).toBeNull();
    expect(matchProviderByBaseUrl("")).toBeNull();
    expect(matchProviderByBaseUrl("   ")).toBeNull();
    expect(modelsForBaseUrl("https://my-own-gateway.example.com/v1")).toEqual([]);
  });

  it("候选来自命中的模板本体（同源，不是另一份清单）", () => {
    const p = PROVIDER_TEMPLATES[1];
    expect(modelsForBaseUrl(p.baseUrl)).toBe(p.models);
  });
});

describe("URL 格式层校验（镜像后端 models::validate_url 的非 DNS 层）", () => {
  it("合法公网地址：通过（含带端口、带路径、IPv6 公网）", () => {
    expect(formatUrlIssue("https://api.openai.com/v1")).toBeNull();
    expect(formatUrlIssue("https://api.deepseek.com")).toBeNull();
    expect(formatUrlIssue("http://8.8.8.8:8080/v1")).toBeNull();
    expect(formatUrlIssue("https://[2001:4860:4860::8888]/v1")).toBeNull();
    expect(formatUrlIssue("  https://api.moonshot.cn/v1  ")).toBeNull();
  });

  it("空 / 不可解析 / 协议：empty / parse / scheme", () => {
    expect(formatUrlIssue("")).toBe("empty");
    expect(formatUrlIssue("   ")).toBe("empty");
    expect(formatUrlIssue("not a url")).toBe("parse");
    expect(formatUrlIssue("api.openai.com")).toBe("parse"); // 缺协议
    expect(formatUrlIssue("ftp://example.com")).toBe("scheme");
    expect(formatUrlIssue("file:///etc/passwd")).toBe("scheme");
  });

  it("userinfo：任何 @ 形态都拒（含只带用户名）", () => {
    expect(formatUrlIssue("https://user:pass@api.openai.com/v1")).toBe("userinfo");
    expect(formatUrlIssue("https://user@api.openai.com/v1")).toBe("userinfo");
  });

  it("localhost 与 *.localhost 都拒", () => {
    expect(formatUrlIssue("http://localhost:8080")).toBe("localhost");
    expect(formatUrlIssue("http://LOCALHOST/v1")).toBe("localhost");
    expect(formatUrlIssue("http://api.localhost/v1")).toBe("localhost");
  });

  it("IPv4 字面内网/保留：逐条对应后端 is_forbidden_ip 的 V4 分支", () => {
    const forbidden = [
      "http://0.0.0.0",
      "http://0.1.2.3",
      "http://127.0.0.1",
      "http://127.255.255.254",
      "http://10.0.0.1",
      "http://172.16.0.1",
      "http://172.31.255.254",
      "http://192.168.1.1",
      "http://169.254.169.254", // 云元数据
      "http://224.0.0.1",
      "http://239.255.255.255",
      "http://100.64.0.1",
      "http://100.127.255.254",
      "http://192.0.2.1",
      "http://198.51.100.7",
      "http://203.0.113.9",
      "http://198.18.0.1",
      "http://198.19.255.254",
      "http://255.255.255.255",
    ];
    for (const u of forbidden) expect(formatUrlIssue(u), u).toBe("private_ip");
  });

  it("IPv4 边界外一律放行（不宽于后端：172.32/16、100.128/9、198.20/16、240/4 后端也不拒）", () => {
    for (const u of [
      "http://172.32.0.1",
      "http://172.15.0.1",
      "http://100.128.0.1",
      "http://198.20.0.1",
      "http://240.0.0.1",
      "http://223.255.255.255",
      "http://9.255.255.255",
      "http://11.0.0.0",
    ]) {
      expect(formatUrlIssue(u), u).toBeNull();
    }
  });

  it("字面量变形（十进制/十六进制/短写）经 WHATWG URL 归一后仍被拦下", () => {
    // 若用正则切串而非 URL 解析，这三种写法会直接绕过前端校验
    expect(formatUrlIssue("http://2130706433/")).toBe("private_ip"); // 0x7F000001 = 127.0.0.1
    expect(formatUrlIssue("http://0x7f.0.0.1/")).toBe("private_ip");
    expect(formatUrlIssue("http://127.1/")).toBe("private_ip");
  });

  it("IPv6：环回/未指定/唯一本地/链路本地/组播，以及 IPv4-mapped 拆壳后按 IPv4 判", () => {
    for (const u of [
      "http://[::1]",
      "http://[::]",
      "http://[fc00::1]",
      "http://[fd12:3456::1]",
      "http://[fe80::1]",
      "http://[ff02::1]",
      "http://[::ffff:127.0.0.1]", // mapped → 127.0.0.1
      "http://[::ffff:10.0.0.1]", // mapped → 10/8
    ]) {
      expect(formatUrlIssue(u), u).toBe("private_ip");
    }
    expect(formatUrlIssue("http://[::ffff:8.8.8.8]")).toBeNull(); // mapped 公网放行
  });

  it("普通域名一律放行：DNS 层判定（解析到内网）交后端 validate_url，前端不重复", () => {
    // 前端拿不到可靠解析结果，这一层必须留在后端（解析失败即 fail-closed）
    expect(formatUrlIssue("https://internal-only.example.com/v1")).toBeNull();
  });
});

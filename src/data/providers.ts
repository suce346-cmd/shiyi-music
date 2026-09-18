/** 厂商模板（#27-a）——厂商 id / API 地址 / 默认模型 / 候选模型清单的**唯一真源**。
 *
 *  守护（`src/test/providers.test.ts` 锁定，任一不成立即红灯）：
 *   ① id 唯一、非空、`^[a-z0-9_]+$`（i18n key `provider.${id}` 由它派生）；
 *   ② `baseUrl` 必须通过 `formatUrlIssue` 的**格式层**校验（DNS 层由后端 `models::validate_url` 兜底）；
 *   ③ `models` 非空、无空串、无重复；
 *   ④ `defaultModel` ∈ `models`——否则"点一下模板"就写出一个候选里选不回来的模型；
 *   ⑤ 每个 id 在中英字典里都有 `provider.${id}` 文案（否则按钮上直接显示 key 本身）。
 *      **第十九批升级为编译期锁**：`id: ProviderId`（由 `TEMPLATES` 派生）使
 *      `t(locale, `provider.${p.id}`)` 的键类型收敛为字面量联合，缺字典键即 `tsc` 报错；
 *      本测试保留为运行期双保险（并覆盖测试夹具直接拼键的场景）。
 *
 *  地址与模型清单于 2026-09-18 逐条 WebSearch 核实（见 docs「第十五批」段），**勿凭记忆改**；
 *  厂商改名/迁移域名时只改这一处，UI 按钮与下拉候选自动跟随（模板卡片与 SearchableSelect 均从这里取数）。
 */
export interface ProviderTemplate {
  /** 稳定 id：i18n key `provider.${id}` 与匹配锚点 */
  id: ProviderId;
  /** OpenAI 兼容 API 根地址（写入设置即生效，不含占位符） */
  baseUrl: string;
  /** 点击模板时写入的模型 */
  defaultModel: string;
  /** 可搜索下拉的候选清单；**不锁死自由输入**（新模型/私有部署名仍可手打） */
  models: readonly string[];
}

const TEMPLATES = [
  {
    id: "xfyun",
    // 讯飞星辰 MaaS：常规推理域名（本项目实际在用）。Token Plan 走另一域名 maas-token-api…，**不可混用**
    baseUrl: "https://maas-api.cn-huabei-1.xf-yun.com/v2",
    defaultModel: "spark-x2.5",
    models: ["spark-x2.5", "xopdeepseekv4pro", "xopglm52", "xopkimik26", "xopqwen36v35b"],
  },
  {
    id: "deepseek",
    // 官方 OpenAI 兼容 base_url 无 /v1 后缀
    baseUrl: "https://api.deepseek.com",
    defaultModel: "deepseek-flash",
    models: ["deepseek-flash", "deepseek-v4-pro"],
  },
  {
    id: "openai",
    baseUrl: "https://api.openai.com/v1",
    defaultModel: "gpt-4o",
    // 仅登记应用既有默认模型：其余模型名可自由输入（下拉不锁死），避免凭记忆写错型号
    models: ["gpt-4o"],
  },
  {
    id: "dashscope",
    // 阿里云百炼（北京）：存量兼容域名仍可用；官方建议迁 workspace 专属域名（含占位符，故不作模板）
    baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1",
    defaultModel: "qwen3.8-max",
    models: ["qwen3.8-max", "qwen3.7-max", "qwen3.6-plus"],
  },
  {
    id: "zhipu",
    baseUrl: "https://open.bigmodel.cn/api/paas/v4",
    defaultModel: "glm-5.3",
    models: ["glm-5.3", "glm-4.7"],
  },
  {
    id: "moonshot",
    // 国内站；国际站为 api.moonshot.ai/v1，需自行改地址
    baseUrl: "https://api.moonshot.cn/v1",
    defaultModel: "kimi-k3",
    models: ["kimi-k3", "kimi-k2.6", "kimi-k2.7-code-highspeed"],
  },
  {
    id: "volcengine",
    baseUrl: "https://ark.cn-beijing.volces.com/api/v3",
    defaultModel: "doubao-seed-2-1-pro-260628",
    models: ["doubao-seed-2-1-pro-260628"],
  },
  {
    id: "siliconflow",
    // 国内站；国际站为 api.siliconflow.com/v1，需自行改地址
    baseUrl: "https://api.siliconflow.cn/v1",
    defaultModel: "deepseek-ai/DeepSeek-V4-Flash-0731",
    models: ["deepseek-ai/DeepSeek-V4-Flash-0731", "deepseek-ai/DeepSeek-V3", "Qwen/Qwen3-32B"],
  },
] as const;

/** 厂商 id 联合（**单源** = 上面的 `TEMPLATES`，不另写字面量清单）。
 *
 *  用途：把 i18n 键 `provider.${id}` 从"运行期弱守护"升级为**编译期锁**——
 *  `t(locale, `provider.${p.id}`)` 在 `id: ProviderId` 下展开为字面量联合，
 *  字典缺任一 `provider.<id>` 键即 `tsc` 报错（旧写法 `id: string` 下只会
 *  在按钮上把 `provider.foo` 原文显示给用户，`providers.test.ts` ⑤ 是唯一防线）。 */
export type ProviderId = (typeof TEMPLATES)[number]["id"];

/** 厂商模板（对外只读视图；真源 = `TEMPLATES`） */
export const PROVIDER_TEMPLATES: readonly ProviderTemplate[] = TEMPLATES;

/** baseUrl 归一（仅用于**匹配**，不用于写入）：去空白、去末尾斜杠、小写。
 *  归一后比对，使 `https://api.openai.com/v1/` 与模板同判为命中（不因一个尾斜杠丢掉高亮与候选）。 */
function normalizeBaseUrl(raw: string): string {
  return raw.trim().replace(/\/+$/, "").toLowerCase();
}

/** 按 baseUrl 反查厂商模板；未命中返回 null（= 自定义地址，下拉无候选但仍可自由输入） */
export function matchProviderByBaseUrl(baseUrl: string): ProviderTemplate | null {
  const n = normalizeBaseUrl(baseUrl);
  if (!n) return null;
  return PROVIDER_TEMPLATES.find((p) => normalizeBaseUrl(p.baseUrl) === n) ?? null;
}

/** 当前 baseUrl 对应的模型候选（未命中厂商 → 空数组：下拉退化为纯自由输入，不报错不锁死） */
export function modelsForBaseUrl(baseUrl: string): readonly string[] {
  return matchProviderByBaseUrl(baseUrl)?.models ?? [];
}

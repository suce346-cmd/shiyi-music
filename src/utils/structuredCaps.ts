/** P1-2 结构化能力探测（BLUEPRINT_v2 §2.2）：测试连接成功后探测档位并按 capKey 缓存进 settings。 */
import { invoke } from "@tauri-apps/api/core";
import { capKey, STRUCTURED_MODES, type StructuredMode } from "../types";

/** 探测；失败（鉴权/限流/网络）返回 null —— 不把失败记成能力档。 */
export async function probeStructuredCaps(baseUrl: string, apiKey: string, model: string): Promise<StructuredMode | null> {
  try {
    const m = await invoke<StructuredMode>("probe_structured_caps", { baseUrl, apiKey, model });
    return STRUCTURED_MODES.includes(m) ? m : null;
  } catch {
    return null;
  }
}

/** 写入缓存（返回新对象；值不变时返回原对象，避免无谓的 settings 变更） */
export function withProbedCap(
  caps: Record<string, StructuredMode> | undefined,
  baseUrl: string,
  model: string,
  mode: StructuredMode,
): Record<string, StructuredMode> {
  const key = capKey(baseUrl, model);
  const cur = caps ?? {};
  return cur[key] === mode ? cur : { ...cur, [key]: mode };
}

/** 生效档位（与 Rust `llm_caps::resolve_mode` 同序；未知 = undefined，后端走缺省 json_object） */
export function effectiveMode(
  explicit: StructuredMode | undefined,
  caps: Record<string, StructuredMode> | undefined,
  baseUrl: string,
  model: string,
): StructuredMode | undefined {
  return explicit ?? caps?.[capKey(baseUrl, model)];
}

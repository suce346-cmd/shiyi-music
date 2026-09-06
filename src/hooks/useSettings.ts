import { useState, useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppSettings, RoleApiOverride } from "../types";

const STORAGE_KEY = "suno-prompt-settings";
/** localStorage 中的明文 key 哨兵——该字段在钥匙串，本地只存标记 */
const KEYCHAIN_SENTINEL = "__keychain__";
/** 迁移标记（localStorage 明文 → 钥匙串一次性迁移，防止重复执行） */
const MIGRATED_KEY = "suno-prompt-keychain-migrated";

const defaultSettings: AppSettings = {
  apiKey: "",
  model: "gpt-4o",
  baseUrl: "https://api.openai.com/v1",
  // 思考模式默认关闭：保持速度与稳定性；需要时可随时打开（后端按模型能力自动适配）
  thinking: false,
  // 角色级 API 覆盖默认空：所有角色共用全局配置
  roleOverrides: {},
};

/** 清洗 localStorage 旧数据：坏类型字段回退默认值，防透传后端 serde 反序列化失败。
 * apiKey 字段只接受哨兵或空——明文 key 不再从 localStorage 读取（走钥匙串迁移）。
 * export 供单测（纯函数）+ 导入配置清洗复用同一入口。 */
export function sanitizeStored(raw: unknown): AppSettings {
  const base = { ...defaultSettings };
  if (typeof raw !== "object" || raw === null) return base;
  const o = raw as Record<string, unknown>;
  // apiKey：仅哨兵视为"钥匙串有值"（内存回填后替换）；其他字符串视为待迁移明文，这里置空
  if (o.apiKey === KEYCHAIN_SENTINEL) base.apiKey = KEYCHAIN_SENTINEL;
  if (typeof o.model === "string") base.model = o.model;
  if (typeof o.baseUrl === "string") base.baseUrl = o.baseUrl;
  if (typeof o.thinking === "boolean") base.thinking = o.thinking;
  // theme 清洗（非法值回退跟随系统）
  if (o.theme === "light" || o.theme === "dark" || o.theme === "system") base.theme = o.theme;
  // language 清洗（非法值回退中文）
  if (o.language === "zh" || o.language === "en") base.language = o.language;
  // generation 清洗（数值范围收敛，坏值丢弃走后端默认）
  if (typeof o.generation === "object" && o.generation !== null && !Array.isArray(o.generation)) {
    const g = o.generation as Record<string, unknown>;
    const cleaned: { temperature?: number; max_tokens?: number } = {};
    if (typeof g.temperature === "number" && g.temperature >= 0 && g.temperature <= 2) {
      cleaned.temperature = g.temperature;
    }
    if (typeof g.max_tokens === "number" && Number.isInteger(g.max_tokens) && g.max_tokens >= 1000 && g.max_tokens <= 32000) {
      cleaned.max_tokens = g.max_tokens;
    }
    if (cleaned.temperature !== undefined || cleaned.max_tokens !== undefined) {
      base.generation = cleaned;
    }
  }
  if (typeof o.roleOverrides === "object" && o.roleOverrides !== null && !Array.isArray(o.roleOverrides)) {
    // 逐角色深校验：非对象条目丢弃；对象只保留 string 三字段
    // api_key 同 apiKey 处理——仅哨兵保留，明文待迁移
    const cleaned: Record<string, RoleApiOverride> = {};
    for (const [k, v] of Object.entries(o.roleOverrides)) {
      if (v && typeof v === "object" && !Array.isArray(v)) {
        const entry: RoleApiOverride = {};
        const e = v as Record<string, unknown>;
        if (typeof e.model === "string") entry.model = e.model;
        if (e.api_key === KEYCHAIN_SENTINEL) entry.api_key = KEYCHAIN_SENTINEL;
        if (typeof e.base_url === "string") entry.base_url = e.base_url;
        cleaned[k] = entry;
      }
    }
    base.roleOverrides = cleaned;
  }
  return base;
}

/** 从原始 localStorage 数据中提取待迁移的明文 key（全局 + 各角色），不经过 sanitize */
function extractPlaintextKeys(raw: unknown): { global?: string; roles: Record<string, string> } {
  const roles: Record<string, string> = {};
  if (typeof raw !== "object" || raw === null) return { roles };
  const o = raw as Record<string, unknown>;
  let global: string | undefined;
  if (typeof o.apiKey === "string" && o.apiKey !== "" && o.apiKey !== KEYCHAIN_SENTINEL) {
    global = o.apiKey;
  }
  const ro = o.roleOverrides;
  if (typeof ro === "object" && ro !== null && !Array.isArray(ro)) {
    for (const [k, v] of Object.entries(ro)) {
      if (v && typeof v === "object" && !Array.isArray(v)) {
        const ak = (v as Record<string, unknown>).api_key;
        if (typeof ak === "string" && ak !== "" && ak !== KEYCHAIN_SENTINEL) {
          roles[k] = ak;
        }
      }
    }
  }
  return { global, roles };
}

export function useSettings() {
  const [settings, setSettings] = useState<AppSettings>(() => {
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      return stored ? sanitizeStored(JSON.parse(stored)) : defaultSettings;
    } catch {
      return defaultSettings;
    }
  });
  const [showSettings, setShowSettings] = useState(false);
  /** 钥匙串就绪前输入框禁用（防用户在回填前覆盖内存空值） */
  const [secretsReady, setSecretsReady] = useState(false);
  const settingsRef = useRef(settings);
  settingsRef.current = settings;

  /** 基础 updateSettings（只更新内存 + 本地非敏感字段；密钥同步由 useSettingsWithSecrets 包一层） */
  const updateSettings = useCallback((partial: Partial<AppSettings>) => {
    setSettings((prev) => {
      const next = { ...prev, ...partial };
      persistNonSecrets(next);
      return next;
    });
  }, []);

  /** 启动迁移 + 钥匙串回填（一次性） */
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        // 1. 迁移：localStorage 明文 → 钥匙串（仅未迁移过且存在明文时）
        if (!localStorage.getItem(MIGRATED_KEY)) {
          try {
            const raw = localStorage.getItem(STORAGE_KEY);
            if (raw) {
              const { global, roles } = extractPlaintextKeys(JSON.parse(raw));
              if (global) await invoke("keychain_set", { account: "global", secret: global });
              for (const [role, secret] of Object.entries(roles)) {
                await invoke("keychain_set", { account: `role:${role}`, secret });
              }
            }
          } catch {
            // 迁移失败不阻断启动（用户可重新输入，下次启动再试）
          }
          try { localStorage.setItem(MIGRATED_KEY, "1"); } catch { /* 忽略 */ }
        }
        // 2. 回填：钥匙串 → 内存（全局 + 各角色）
        const next: AppSettings = { ...settingsRef.current };
        let changed = false;
        if (next.apiKey === KEYCHAIN_SENTINEL || next.apiKey === "") {
          try {
            const pw = await invoke<string | null>("keychain_get", { account: "global" });
            if (pw) { next.apiKey = pw; changed = true; }
            else if (next.apiKey === KEYCHAIN_SENTINEL) { next.apiKey = ""; changed = true; }
          } catch { /* 钥匙串不可用则保持现状 */ }
        }
        if (next.roleOverrides) {
          for (const role of Object.keys(next.roleOverrides)) {
            const entry = (next.roleOverrides as Record<string, RoleApiOverride>)[role];
            if (entry?.api_key === KEYCHAIN_SENTINEL) {
              try {
                const pw = await invoke<string | null>("keychain_get", { account: `role:${role}` });
                if (pw) { entry.api_key = pw; changed = true; }
                else { entry.api_key = undefined; changed = true; }
              } catch { /* 保持现状 */ }
            }
          }
        }
        if (!cancelled) {
          if (changed) {
            setSettings(next);
            persistNonSecrets(next);
          }
          setSecretsReady(true);
        }
      } catch {
        if (!cancelled) setSecretsReady(true);
      }
    })();
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return { settings, updateSettings, showSettings, setShowSettings, secretsReady };
}

/** 持久化非敏感字段——apiKey/api_key 写哨兵（真实值只在钥匙串） */
function persistNonSecrets(s: AppSettings) {
  try {
    const scrubbed: AppSettings = {
      ...s,
      apiKey: s.apiKey ? KEYCHAIN_SENTINEL : "",
      roleOverrides: Object.fromEntries(
        Object.entries(s.roleOverrides ?? {}).map(([k, v]) => [
          k,
          { ...v, api_key: v.api_key ? KEYCHAIN_SENTINEL : undefined },
        ]),
      ),
    };
    localStorage.setItem(STORAGE_KEY, JSON.stringify(scrubbed));
  } catch { /* 配额等失败静默（与旧行为一致） */ }
}

/** 带密钥同步的设置更新（供 App 层调用处替换 updateSettings 用——本文件默认导出保持兼容） */
export function useSettingsWithSecrets() {
  const base = useSettings();
  const updateSettings = useCallback(async (partial: Partial<AppSettings>) => {
    // 先同步钥匙串，再更新内存 + 本地非敏感字段
    try {
      if (partial.apiKey !== undefined) {
        if (partial.apiKey) {
          await invoke("keychain_set", { account: "global", secret: partial.apiKey });
        } else {
          await invoke("keychain_delete", { account: "global" });
        }
      }
      const ro = partial.roleOverrides;
      if (ro) {
        for (const [role, entry] of Object.entries(ro)) {
          if (entry && typeof entry === "object" && "api_key" in entry) {
            const ak = (entry as RoleApiOverride).api_key;
            if (ak) await invoke("keychain_set", { account: `role:${role}`, secret: ak });
            else await invoke("keychain_delete", { account: `role:${role}` });
          }
        }
      }
    } catch {
      // 钥匙串失败不阻断（内存值仍更新，下次启动回填时以钥匙串为准）
    }
    base.updateSettings(partial);
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) {
        const cur = JSON.parse(raw) as AppSettings;
        persistNonSecrets({ ...cur, ...partial });
      }
    } catch { /* 忽略 */ }
  }, [base]);
  return { ...base, updateSettings };
}

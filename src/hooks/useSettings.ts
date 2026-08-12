import { useState, useCallback } from "react";
import type { AppSettings, RoleApiOverride } from "../types";

const STORAGE_KEY = "suno-prompt-settings";

const defaultSettings: AppSettings = {
  apiKey: "",
  model: "gpt-4o",
  baseUrl: "https://api.openai.com/v1",
  // 思考模式默认关闭：保持速度与稳定性；需要时可随时打开（后端按模型能力自动适配）
  thinking: false,
  // 角色级 API 覆盖默认空：所有角色共用全局配置
  roleOverrides: {},
};

// NOTE: API Key 以明文存储在 WebView localStorage 中。
// 后续可升级为 tauri-plugin-store 加密或 OS Keychain。

/** 清洗 localStorage 旧数据：坏类型字段回退默认值，防透传后端 serde 反序列化失败 */
function sanitizeStored(raw: unknown): AppSettings {
  const base = { ...defaultSettings };
  if (typeof raw !== "object" || raw === null) return base;
  const o = raw as Record<string, unknown>;
  if (typeof o.apiKey === "string") base.apiKey = o.apiKey;
  if (typeof o.model === "string") base.model = o.model;
  if (typeof o.baseUrl === "string") base.baseUrl = o.baseUrl;
  if (typeof o.thinking === "boolean") base.thinking = o.thinking;
  if (typeof o.roleOverrides === "object" && o.roleOverrides !== null && !Array.isArray(o.roleOverrides)) {
    // 逐角色深校验：非对象条目丢弃；对象只保留 string 三字段
    const cleaned: Record<string, RoleApiOverride> = {};
    for (const [k, v] of Object.entries(o.roleOverrides)) {
      if (v && typeof v === "object" && !Array.isArray(v)) {
        const entry: RoleApiOverride = {};
        const e = v as Record<string, unknown>;
        if (typeof e.model === "string") entry.model = e.model;
        if (typeof e.api_key === "string") entry.api_key = e.api_key;
        if (typeof e.base_url === "string") entry.base_url = e.base_url;
        cleaned[k] = entry;
      }
    }
    base.roleOverrides = cleaned;
  }
  return base;
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

  const updateSettings = useCallback((partial: Partial<AppSettings>) => {
    setSettings((prev) => {
      const next = { ...prev, ...partial };
      localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
      return next;
    });
  }, []);

  return { settings, updateSettings, showSettings, setShowSettings };
}

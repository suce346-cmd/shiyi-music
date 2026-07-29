import { useState, useCallback } from "react";
import type { AppSettings } from "../types";

const STORAGE_KEY = "suno-prompt-settings";

const defaultSettings: AppSettings = {
  apiKey: "",
  model: "gpt-4o",
  baseUrl: "https://api.openai.com/v1",
};

// NOTE: API Key 以明文存储在 WebView localStorage 中。
// 后续可升级为 tauri-plugin-store 加密或 OS Keychain。

export function useSettings() {
  const [settings, setSettings] = useState<AppSettings>(() => {
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      return stored ? { ...defaultSettings, ...JSON.parse(stored) } : defaultSettings;
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

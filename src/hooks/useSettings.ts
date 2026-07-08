import { useState, useCallback } from "react";
import type { AppSettings } from "../types";

const STORAGE_KEY = "suno-prompt-settings";

const defaultSettings: AppSettings = {
  apiKey: "",
  model: "gpt-4o",
  baseUrl: "https://api.openai.com/v1",
};

export function useSettings() {
  const [settings, setSettings] = useState<AppSettings>(() => {
    const stored = localStorage.getItem(STORAGE_KEY);
    return stored ? JSON.parse(stored) : defaultSettings;
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
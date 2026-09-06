import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import ErrorBoundary from "./components/ErrorBoundary";
import type { Locale } from "./types";
import "./index.css";

/** 崩溃屏语言（App 外层读不到 settings，直读 localStorage；解析失败回中文） */
function initialLocale(): Locale {
  try {
    const raw = localStorage.getItem("suno-prompt-settings");
    const lang = raw ? (JSON.parse(raw) as { language?: unknown }).language : undefined;
    return lang === "en" ? "en" : "zh";
  } catch {
    return "zh";
  }
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ErrorBoundary locale={initialLocale()}>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);

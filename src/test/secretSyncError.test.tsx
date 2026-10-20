// @vitest-environment jsdom
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { SecretSyncError } from "../components/SecretSyncError";
import { secretErrorMessage } from "../hooks/useSettings";

describe("SecretSyncError（设置界面就地提示）", () => {
  it("有错误 → role=alert 显示前缀 + 后端原因", () => {
    render(<SecretSyncError message="API Key 含空白" locale="zh" />);
    const el = screen.getByRole("alert");
    expect(el.textContent).toContain("Key 未保存到钥匙串");
    expect(el.textContent).toContain("API Key 含空白");
  });
  it("无错误 → 不渲染", () => {
    const { container } = render(<SecretSyncError locale="en" />);
    expect(container.innerHTML).toBe("");
  });
  it("后端 AppError 形态与兜底", () => {
    expect(secretErrorMessage({ kind: "validation", message: "m" })).toBe("m");
    expect(secretErrorMessage("plain")).toBe("plain");
  });
});

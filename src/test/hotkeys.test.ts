import { describe, it, expect } from "vitest";
import {
  isSubmitHotkey,
  isFocusHotkey,
  isSettingsHotkey,
  isImportableFile,
  scrubSettingsForExport,
} from "../utils/hotkeys";

describe("hotkey predicates", () => {
  it("Cmd/Ctrl+Enter 提交（大小写/单键不触发）", () => {
    expect(isSubmitHotkey({ key: "Enter", metaKey: true, ctrlKey: false })).toBe(true);
    expect(isSubmitHotkey({ key: "Enter", metaKey: false, ctrlKey: true })).toBe(true);
    expect(isSubmitHotkey({ key: "Enter", metaKey: false, ctrlKey: false })).toBe(false);
    expect(isSubmitHotkey({ key: "a", metaKey: true, ctrlKey: false })).toBe(false);
  });

  it("Cmd/Ctrl+K 聚焦（大小写兼容）", () => {
    expect(isFocusHotkey({ key: "k", metaKey: true, ctrlKey: false })).toBe(true);
    expect(isFocusHotkey({ key: "K", metaKey: false, ctrlKey: true })).toBe(true);
    expect(isFocusHotkey({ key: "k", metaKey: false, ctrlKey: false })).toBe(false);
  });

  it("Cmd/Ctrl+, 开关设置", () => {
    expect(isSettingsHotkey({ key: ",", metaKey: true, ctrlKey: false })).toBe(true);
    expect(isSettingsHotkey({ key: ".", metaKey: true, ctrlKey: false })).toBe(false);
    expect(isSettingsHotkey({ key: ",", metaKey: false, ctrlKey: false })).toBe(false);
  });
});

describe("isImportableFile", () => {
  it("仅 txt/lrc（大小写兼容）", () => {
    expect(isImportableFile("歌词.txt")).toBe(true);
    expect(isImportableFile("song.LRC")).toBe(true);
    expect(isImportableFile("a.TXT")).toBe(true);
  });

  it("其他后缀/无后缀拒绝", () => {
    expect(isImportableFile("song.mp3")).toBe(false);
    expect(isImportableFile("doc.pdf")).toBe(false);
    expect(isImportableFile("noext")).toBe(false);
    expect(isImportableFile("txt")).toBe(false);
  });
});

describe("scrubSettingsForExport", () => {
  it("移除全局与角色密钥，其他字段保留", () => {
    const out = scrubSettingsForExport({
      apiKey: "whatever",
      model: "m",
      roleOverrides: { auditor: { model: "s", api_key: "whatever", base_url: "u" } },
    });
    expect(out).not.toHaveProperty("apiKey");
    expect(out).toEqual({
      model: "m",
      roleOverrides: { auditor: { model: "s", base_url: "u" } },
    });
  });

  it("非对象输入返回空对象", () => {
    expect(scrubSettingsForExport(null)).toEqual({});
    expect(scrubSettingsForExport(42)).toEqual({});
  });
});

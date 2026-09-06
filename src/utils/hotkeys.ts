/** 快捷键 / 拖拽导入 / 设置导入导出共用的纯函数。 */

/** 快捷键判定：Cmd/Ctrl + Enter 提交 */
export function isSubmitHotkey(e: { key: string; metaKey: boolean; ctrlKey: boolean }): boolean {
  return e.key === "Enter" && (e.metaKey || e.ctrlKey);
}

/** 快捷键判定：Cmd/Ctrl + K 聚焦输入 */
export function isFocusHotkey(e: { key: string; metaKey: boolean; ctrlKey: boolean }): boolean {
  return (e.key === "k" || e.key === "K") && (e.metaKey || e.ctrlKey);
}

/** 快捷键判定：Cmd/Ctrl + , 开关设置 */
export function isSettingsHotkey(e: { key: string; metaKey: boolean; ctrlKey: boolean }): boolean {
  return e.key === "," && (e.metaKey || e.ctrlKey);
}

/** 可导入的歌词文件后缀（仅纯文本） */
const IMPORTABLE_EXTS = ["txt", "lrc"];

/** 拖拽文件是否可导入（按后缀判定，不读内容） */
export function isImportableFile(fileName: string): boolean {
  const dot = fileName.lastIndexOf(".");
  if (dot < 0) return false;
  return IMPORTABLE_EXTS.includes(fileName.slice(dot + 1).toLowerCase());
}

/** 设置导出清洗：移除密钥字段。
 *  输入为 AppSettings 形态的未知对象，输出不含 apiKey / roleOverrides[*].api_key。 */
export function scrubSettingsForExport(settings: unknown): Record<string, unknown> {
  if (typeof settings !== "object" || settings === null) return {};
  const o = settings as Record<string, unknown>;
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(o)) {
    if (k === "apiKey") continue;
    if (k === "roleOverrides" && typeof v === "object" && v !== null && !Array.isArray(v)) {
      const cleaned: Record<string, unknown> = {};
      for (const [rk, rv] of Object.entries(v as Record<string, unknown>)) {
        if (rv && typeof rv === "object" && !Array.isArray(rv)) {
          const { api_key: _drop, ...rest } = rv as Record<string, unknown>;
          cleaned[rk] = rest;
        }
      }
      out[k] = cleaned;
      continue;
    }
    out[k] = v;
  }
  return out;
}

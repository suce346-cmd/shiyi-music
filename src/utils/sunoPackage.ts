/**
 * P1-11：从 render_suno::to_package_text 交付包解析四栏。
 * 锚点与 Rust 单源排版一致：`Style:` / `Exclude:` / `Settings:` / `Lyrics:`（可选 `Title:`）。
 */

export type SunoPackageFields = {
  title: string;
  style: string;
  exclude: string;
  settings: string;
  lyrics: string;
  /** 是否识别为新交付包（否则应整段复制） */
  isPackage: boolean;
};

const ANCHORS = ["Style:", "Exclude:", "Settings:", "Lyrics:"] as const;

/** 抽取单栏：从 `Label:` 起直到下一锚点或文末（Lyrics 吃到文末）。 */
function sliceField(text: string, label: string, nextLabels: string[]): string {
  const re = new RegExp(`(?:^|\\n)${label}\\s*`, "i");
  const m = re.exec(text);
  if (!m) return "";
  const start = m.index + m[0].length;
  let end = text.length;
  for (const n of nextLabels) {
    const nr = new RegExp(`\\n${n}\\s*`, "i");
    const nm = nr.exec(text.slice(start));
    if (nm && start + nm.index < end) end = start + nm.index;
  }
  return text.slice(start, end).replace(/^\n+/, "").replace(/\n+$/, "").trim();
}

export function parseSunoPackage(text: string): SunoPackageFields {
  const hasStyle = /(?:^|\n)Style:\s*/i.test(text);
  const hasLyrics = /(?:^|\n)Lyrics:\s*/i.test(text);
  const isPackage = hasStyle && hasLyrics;
  if (!isPackage) {
    return { title: "", style: "", exclude: "", settings: "", lyrics: "", isPackage: false };
  }
  const titleM = /(?:^|\n)Title:\s*(.+?)(?:\n|$)/i.exec(text);
  return {
    title: titleM?.[1]?.trim() ?? "",
    style: sliceField(text, "Style:", ["Exclude:", "Settings:", "Lyrics:"]),
    exclude: sliceField(text, "Exclude:", ["Settings:", "Lyrics:"]),
    settings: sliceField(text, "Settings:", ["Lyrics:"]),
    lyrics: sliceField(text, "Lyrics:", []),
    isPackage: true,
  };
}

/** Settings 栏是否显式含 Variety=0（P1-11 展示要求） */
export function settingsShowsVarietyZero(settings: string): boolean {
  return /Variety\s*=\s*0\b/i.test(settings);
}

export { ANCHORS };

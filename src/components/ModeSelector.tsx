import { IconFileText, IconBrandTiktok, IconEdit, IconStars } from "@tabler/icons-react";
import type { Mode, Locale } from "../types";
import { t } from "../i18n";

interface Props {
  mode: Mode;
  onChange: (mode: Mode) => void;
  /** 界面语言（缺省中文） */
  locale?: Locale;
}

const MODES: { value: Mode; labelKey: string; descKey: string; icon: typeof IconFileText }[] = [
  { value: "mode_a", labelKey: "mode.a", descKey: "mode.a.desc", icon: IconFileText },
  { value: "mode_b", labelKey: "mode.b", descKey: "mode.b.desc", icon: IconStars },
  { value: "mode_c", labelKey: "mode.c", descKey: "mode.c.desc", icon: IconEdit },
  { value: "mode_d", labelKey: "mode.d", descKey: "mode.d.desc", icon: IconBrandTiktok },
];

export default function ModeSelector({ mode, onChange, locale }: Props) {
  return (
    <div className="flex gap-1.5 w-full">
      {MODES.map((m) => {
        const active = mode === m.value;
        const Icon = m.icon;
        return (
          <button
            key={m.value}
            onClick={() => onChange(m.value)}
            className={`relative flex-1 flex flex-col items-center gap-1 px-2 py-2 rounded-xl text-center
              transition-all duration-200
              ${active
                ? "glass-panel border border-brand-500/30 shadow-sm"
                : "border border-transparent hover:bg-surface-2/50"
              }`}
          >
            <span className={`w-7 h-7 rounded-lg flex items-center justify-center transition-all duration-200
              ${active ? "brand-gradient-btn text-white" : "bg-surface-2/60 text-text-muted"}`}>
              <Icon size={15} />
            </span>
            <span className={`text-[11px] font-medium leading-tight ${active ? "text-text-1" : "text-text-2"}`}>
              {t(locale, m.labelKey)}
            </span>
            <span className="text-[9px] text-text-muted leading-tight">{t(locale, m.descKey)}</span>
          </button>
        );
      })}
    </div>
  );
}

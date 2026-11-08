import { IconMusic, IconSparkles, IconPencil, IconFlame } from "@tabler/icons-react";
import type { Locale, Mode } from "../types";
import { t } from "../i18n";
import type { MessageKey } from "../i18n";

interface Props {
  mode: Mode;
  onChange: (mode: Mode) => void;
  disabled?: boolean;
  locale?: Locale;
}

const modes: { key: Mode; code: string; titleKey: MessageKey; descKey: MessageKey; icon: typeof IconMusic }[] = [
  { key: "mode_a", code: "MODE A", titleKey: "mode.a", descKey: "mode.a.desc", icon: IconMusic },
  { key: "mode_b", code: "MODE B", titleKey: "mode.b", descKey: "mode.b.desc", icon: IconSparkles },
  { key: "mode_c", code: "MODE C", titleKey: "mode.c", descKey: "mode.c.desc", icon: IconPencil },
  { key: "mode_d", code: "MODE D", titleKey: "mode.d", descKey: "mode.d.desc", icon: IconFlame },
];

export default function ModeSelector({ mode, onChange, disabled, locale }: Props) {
  return (
    <div className="grid grid-cols-4 gap-2">
      {modes.map((m) => {
        const Icon = m.icon;
        const active = mode === m.key;
        return (
          <button
            key={m.key}
            onClick={() => onChange(m.key)}
            disabled={disabled}
            className={`group relative flex items-center gap-2.5 px-3 py-2 rounded-lg text-left transition-all duration-150
              ${active
                ? "studio-card border-brand-500/60 bg-surface-2 text-text-1"
                : "bg-surface-0/50 border border-border/50 text-text-2 hover:text-text-1 hover:bg-surface-2/60 hover:border-border-hover"
              }
              ${disabled ? "opacity-40 cursor-not-allowed" : "cursor-pointer active:scale-[0.99]"}`}
          >
            {active && (
              <span className="absolute left-0 top-2 bottom-2 w-[3px] rounded-r bg-brand-400" />
            )}
            <div
              className={`w-7 h-7 rounded-md flex items-center justify-center shrink-0 border transition-colors
                ${active
                  ? "bg-brand-500/15 border-brand-500/40 text-brand-400"
                  : "bg-surface-1 border-border/60 text-text-muted group-hover:text-text-2"
                }`}
            >
              <Icon size={14} strokeWidth={active ? 2.2 : 1.75} />
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-1.5">
                <span className="text-[12px] font-semibold tracking-tight truncate">
                  {t(locale, m.titleKey)}
                </span>
                <span
                  className={`studio-mono text-[9px] px-1 py-0.2 rounded border shrink-0
                    ${active
                      ? "border-brand-500/40 bg-brand-500/10 text-brand-400"
                      : "border-border/60 bg-surface-1 text-text-muted"
                    }`}
                >
                  {m.code}
                </span>
              </div>
              <div className="text-[10px] text-text-muted truncate mt-0.5">
                {t(locale, m.descKey)}
              </div>
            </div>
          </button>
        );
      })}
    </div>
  );
}

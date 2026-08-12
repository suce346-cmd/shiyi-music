import { IconFileText, IconBrandTiktok, IconEdit, IconStars } from "@tabler/icons-react";
import type { Mode } from "../types";

interface Props {
  mode: Mode;
  onChange: (mode: Mode) => void;
}

const modes: { value: Mode; label: string; desc: string; icon: typeof IconFileText }[] = [
  { value: "mode_a", label: "完整方案", desc: "歌词 → 编曲", icon: IconFileText },
  { value: "mode_b", label: "经典创作", desc: "灵感 → 歌曲", icon: IconStars },
  { value: "mode_c", label: "重新填词", desc: "原词 → 重填", icon: IconEdit },
  { value: "mode_d", label: "抖音爆款", desc: "灵感 → 神曲", icon: IconBrandTiktok },
];

export default function ModeSelector({ mode, onChange }: Props) {
  return (
    <div className="flex gap-1.5 w-full">
      {modes.map((m) => {
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
              {m.label}
            </span>
            <span className="text-[9px] text-text-muted leading-tight">{m.desc}</span>
          </button>
        );
      })}
    </div>
  );
}

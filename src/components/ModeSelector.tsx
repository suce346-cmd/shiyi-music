import { IconFileText, IconBrandTiktok, IconEdit, IconStars } from "@tabler/icons-react";
import type { Mode } from "../types";

interface Props {
  mode: Mode;
  onChange: (mode: Mode) => void;
}

const modes: { value: Mode; label: string; desc: string; icon: typeof IconFileText }[] = [
  { value: "mode_a", label: "完整方案", desc: "已有歌词 → 分析 → 编曲", icon: IconFileText },
  { value: "mode_b", label: "经典创作", desc: "灵感 → 从零创作经典歌曲", icon: IconStars },
  { value: "mode_c", label: "重新填词", desc: "原歌词 + 新主题 → 重填", icon: IconEdit },
  { value: "mode_d", label: "抖音爆款", desc: "灵感 → 60-90秒神曲", icon: IconBrandTiktok },
];

export default function ModeSelector({ mode, onChange }: Props) {
  return (
    <div className="space-y-1.5">
      {modes.map((m) => {
        const active = mode === m.value;
        const Icon = m.icon;
        return (
          <button
            key={m.value}
            onClick={() => onChange(m.value)}
            className={`relative w-full flex items-center gap-3 px-3 py-2.5 rounded-xl text-left
              transition-all duration-200
              ${active
                ? "glass-panel border border-brand-500/30"
                : "border border-transparent hover:bg-surface-2/50"
              }`}
          >
            {active && (
              <span className="absolute left-0 top-1/2 -translate-y-1/2 w-[3px] h-6 rounded-full brand-gradient-btn" />
            )}
            <div className={`w-8 h-8 rounded-lg flex items-center justify-center shrink-0 transition-all duration-200
              ${active
                ? "brand-gradient-btn text-white"
                : "bg-surface-2/60 text-text-muted"}`}>
              <Icon size={17} />
            </div>
            <div className="flex-1 min-w-0">
              <div className={`text-[13px] font-medium leading-tight transition-colors
                ${active ? "text-text-1" : "text-text-2"}`}>
                {m.label}
              </div>
              <div className="text-[11px] text-text-muted leading-tight mt-0.5">{m.desc}</div>
            </div>
          </button>
        );
      })}
    </div>
  );
}

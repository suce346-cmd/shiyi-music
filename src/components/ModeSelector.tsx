import { IconFileText, IconBrandTiktok, IconEdit, IconStars } from "@tabler/icons-react";
import type { Mode } from "../types";

interface Props {
  mode: Mode;
  onChange: (mode: Mode) => void;
}

const modes: { value: Mode; label: string; desc: string; icon: typeof IconFileText }[] = [
  {
    value: "mode_a",
    label: "Mode A 完整方案",
    desc: "已有歌词 → 分析 → Style Prompt + 格式化歌词 + 参数",
    icon: IconFileText,
  },
  {
    value: "mode_b",
    label: "Mode B 经典创作",
    desc: "一个灵感 → 从零创作经典歌曲级别全流程",
    icon: IconStars,
  },
  {
    value: "mode_c",
    label: "Mode C 重新填词",
    desc: "原歌词 + 新主题 → 保留字数韵脚意象重新填词",
    icon: IconEdit,
  },
  {
    value: "mode_d",
    label: "Mode D 抖音爆款",
    desc: "一个灵感 → 60-90秒抖音神曲全流程",
    icon: IconBrandTiktok,
  },
];

export default function ModeSelector({ mode, onChange }: Props) {
  return (
    <div className="grid grid-cols-2 md:grid-cols-4 gap-3 mb-6">
      {modes.map((m) => {
        const active = mode === m.value;
        const Icon = m.icon;
        return (
          <button
            key={m.value}
            onClick={() => onChange(m.value)}
            className={`relative group p-4 rounded-2xl border-2 text-left transition-all duration-200
              ${active
                ? "border-brand-500 bg-gradient-to-br from-brand-500/8 to-brand-500/3 shadow-lg shadow-brand-500/10"
                : "border-border/50 bg-surface-2 hover:border-border hover:shadow-md"
              }`}
          >
            <div className={`w-9 h-9 rounded-xl flex items-center justify-center mb-3 transition-colors duration-200
              ${active ? "bg-brand-500 text-white" : "bg-surface-0 text-text-2 group-hover:bg-border"}`}>
              <Icon size={18} />
            </div>
            <div className={`text-sm font-medium mb-0.5 transition-colors ${active ? "text-brand-600" : "text-text-1"}`}>
              {m.label}
            </div>
            <div className="text-xs text-text-muted leading-relaxed">{m.desc}</div>
          </button>
        );
      })}
    </div>
  );
}

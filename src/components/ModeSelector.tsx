import { IconFileText, IconBrandTiktok, IconEdit, IconStars } from "@tabler/icons-react";
import type { Mode, Locale } from "../types";
import { t } from "../i18n";
import type { MessageKey } from "../i18n";

interface Props {
  mode: Mode;
  onChange: (mode: Mode) => void;
  /** 界面语言（缺省中文） */
  locale?: Locale;
}

/** 模式元数据（**穷尽映射**：`Record<Mode, …>`）。
 *
 *  旧写法是 `{value: Mode; …}[]`（数组）——TS 对数组**不做**取值域穷尽检查，于是新增
 *  一个 `Mode` 变体时本组件只是**静默少画一个按钮**：该模式在界面上不可达，而 tsc / 全部
 *  测试照旧全绿（与后端 `rules::ALL_MODES` 漏登记同族，第十九批一并修复）。
 *  改 `Record<Mode, …>` 后，漏配任一模式即**编译期**报错。
 *
 *  渲染顺序 = 此处的声明顺序（`Object.keys` 对字符串键保持插入序），不再另写一份顺序清单
 *  ——那会是同一事实的第二个真源。 */
const MODE_META: Record<Mode, { labelKey: MessageKey; descKey: MessageKey; icon: typeof IconFileText }> = {
  mode_a: { labelKey: "mode.a", descKey: "mode.a.desc", icon: IconFileText },
  mode_b: { labelKey: "mode.b", descKey: "mode.b.desc", icon: IconStars },
  mode_c: { labelKey: "mode.c", descKey: "mode.c.desc", icon: IconEdit },
  mode_d: { labelKey: "mode.d", descKey: "mode.d.desc", icon: IconBrandTiktok },
};

/** 渲染序条目（键类型由 `MODE_META` 的 `Record<Mode, …>` 保证恰为 `Mode`） */
function modeEntries(): [Mode, (typeof MODE_META)[Mode]][] {
  return Object.entries(MODE_META) as [Mode, (typeof MODE_META)[Mode]][];
}

export default function ModeSelector({ mode, onChange, locale }: Props) {
  return (
    <div className="flex gap-1.5 w-full">
      {modeEntries().map(([value, meta]) => {
        const active = mode === value;
        const Icon = meta.icon;
        return (
          <button
            key={value}
            onClick={() => onChange(value)}
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
              {t(locale, meta.labelKey)}
            </span>
            <span className="text-[9px] text-text-muted leading-tight">{t(locale, meta.descKey)}</span>
          </button>
        );
      })}
    </div>
  );
}

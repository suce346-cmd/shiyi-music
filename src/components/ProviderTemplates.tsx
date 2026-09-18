import { PROVIDER_TEMPLATES, matchProviderByBaseUrl, type ProviderTemplate } from "../data/providers";
import { t } from "../i18n";
import type { Locale } from "../types";

interface Props {
  /** 当前 baseUrl（用于高亮命中的厂商；归一后比对，尾斜杠不影响判定） */
  baseUrl: string;
  locale?: Locale;
  /** 点击模板：写入 baseUrl + 该厂商默认模型（**不触碰 API Key**，由调用方落实） */
  onApply: (template: ProviderTemplate) => void;
}

/** 厂商模板卡片（#27-a）：一键把"地址 + 默认模型"填成该厂商的可用组合。
 *  数据全部来自 `data/providers.ts` 单源（地址与模型清单已逐条核实），
 *  故**点不出坏预设**——不会出现"地址填了但模型还是上一家的"这种半截状态。 */
export default function ProviderTemplates({ baseUrl, locale, onApply }: Props) {
  const activeId = matchProviderByBaseUrl(baseUrl)?.id ?? null;
  return (
    <div>
      <label className="text-[11px] text-text-muted block mb-1">{t(locale, "settings.providers")}</label>
      <div className="flex flex-wrap gap-1.5">
        {PROVIDER_TEMPLATES.map((p) => {
          const active = p.id === activeId;
          return (
            <button
              key={p.id}
              type="button"
              onClick={() => onApply(p)}
              aria-pressed={active}
              title={p.baseUrl}
              className={`px-2 py-1 rounded-md text-[10px] border transition-all duration-150 active:scale-95 ${
                active
                  ? "text-brand-500 border-brand-500/50 bg-brand-500/10"
                  : "text-text-2 border-border/40 bg-surface-2/60 hover:bg-surface-3/80"
              }`}
            >
              {t(locale, `provider.${p.id}`)}
            </button>
          );
        })}
      </div>
      <p className="text-[9px] text-text-muted mt-1 leading-relaxed">{t(locale, "settings.providers.hint")}</p>
    </div>
  );
}

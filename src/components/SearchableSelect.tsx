import { useCallback, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { t } from "../i18n";
import type { Locale } from "../types";

/** 设置面板里全局模型输入框的默认样式（与改造前的裸 input 逐字一致，避免样式漂移） */
const DEFAULT_INPUT_CLASS =
  "w-full bg-surface-0 border border-border/60 rounded-lg px-3 py-2 text-[13px] text-text-1 placeholder:text-text-muted/30 focus:outline-none focus:border-brand-500/40 focus:ring-1 focus:ring-brand-500/20 transition-all duration-150 disabled:opacity-50";

interface Props {
  value: string;
  /** 候选模型名（来自当前 baseUrl 命中的厂商模板；空数组 = 无候选，退化为纯自由输入）。
   *  只读：候选来自 `providers.ts` 的单源只读数组，组件不得就地改写。 */
  options: readonly string[];
  /** 每次输入/选择都回调：**自由输入实时透传**（下拉只是提示，不锁死自定义模型名） */
  onChange: (v: string) => void;
  locale?: Locale;
  placeholder?: string;
  ariaLabel?: string;
  disabled?: boolean;
  inputClassName?: string;
}

/** 可搜索模型下拉（#27-b）：既能从候选里选，也保留自由输入。
 *
 *  为什么用 portal + fixed 定位，而不是就地 absolute：
 *  设置面板的角色级覆盖列表是 `max-h-64 overflow-y-auto`，就地 absolute 的下拉会被这个滚动容器
 *  裁掉——看得见选项却点不到，是"假 affordance"的另一种形态。挂到 body 后不受任何祖先
 *  overflow/transform 影响，一处实现同时服务全局与角色级两处输入框。
 *
 *  键盘：↑/↓ 移动高亮、Enter 采用、Esc 关闭；点击外部关闭；窗口滚动/尺寸变化关闭（避免坐标漂移）。
 *  无障碍：combobox + listbox/option 语义，`aria-activedescendant` 指向当前高亮项。 */
export default function SearchableSelect({
  value, options, onChange, locale, placeholder, ariaLabel, disabled, inputClassName,
}: Props) {
  const [open, setOpen] = useState(false);
  /** 正在编辑的文本；null = 未编辑（显示外部 value，候选不过滤 = 全量展示） */
  const [query, setQuery] = useState<string | null>(null);
  const [active, setActive] = useState(0);
  const [pos, setPos] = useState<{ left: number; top: number; width: number } | null>(null);
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const listRef = useRef<HTMLUListElement | null>(null);
  const listId = useId();

  const close = useCallback(() => {
    setOpen(false);
    setQuery(null);
  }, []);

  const openList = useCallback(() => {
    const r = inputRef.current?.getBoundingClientRect();
    if (!r) return;
    setPos({ left: r.left, top: r.bottom + 4, width: r.width });
    setActive(0);
    setOpen(true);
  }, []);

  // 点击外部关闭。刻意不用 onBlur 关闭：点选项会先触发 input 的 blur，选项就永远点不到了
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      const node = e.target as Node;
      if (wrapRef.current?.contains(node) || listRef.current?.contains(node)) return;
      close();
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open, close]);

  // 窗口滚动/尺寸变化 → 关闭（fixed 坐标会漂移）。下拉**自身**滚动不算，否则列表滚不动
  useEffect(() => {
    if (!open) return;
    const onScroll = (e: Event) => {
      const node = e.target as Node | null;
      if (node && listRef.current?.contains(node)) return;
      close();
    };
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onScroll);
    return () => {
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onScroll);
    };
  }, [open, close]);

  const filtered = options.filter((o) => o.toLowerCase().includes((query ?? "").toLowerCase()));
  const activeIdx = filtered.length === 0 ? -1 : Math.min(active, filtered.length - 1);

  const pick = (opt: string) => {
    onChange(opt);
    close();
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) {
        openList();
        return;
      }
      if (filtered.length === 0) return;
      const dir = e.key === "ArrowDown" ? 1 : -1;
      setActive((i) => (Math.min(i, filtered.length - 1) + dir + filtered.length) % filtered.length);
    } else if (e.key === "Enter") {
      if (open && activeIdx >= 0) {
        e.preventDefault();
        pick(filtered[activeIdx]);
      }
    } else if (e.key === "Escape" && open) {
      e.preventDefault();
      close();
    }
  };

  return (
    <div ref={wrapRef} className="relative">
      <input
        ref={inputRef}
        type="text"
        role="combobox"
        aria-expanded={open}
        aria-controls={listId}
        aria-autocomplete="list"
        aria-activedescendant={open && activeIdx >= 0 ? `${listId}-${activeIdx}` : undefined}
        aria-label={ariaLabel}
        value={query ?? value}
        disabled={disabled}
        placeholder={placeholder}
        onChange={(e) => {
          setQuery(e.target.value);
          onChange(e.target.value);
          if (!open) openList();
        }}
        onFocus={() => {
          if (!open) openList();
        }}
        onKeyDown={onKeyDown}
        className={inputClassName ?? DEFAULT_INPUT_CLASS}
      />
      {open && pos && createPortal(
        <ul
          ref={listRef}
          id={listId}
          role="listbox"
          style={{ position: "fixed", left: pos.left, top: pos.top, width: pos.width, zIndex: 60 }}
          className="max-h-52 overflow-y-auto rounded-lg border border-border/60 bg-surface-1 shadow-lg py-1"
        >
          {filtered.length === 0 ? (
            <li className="px-3 py-1.5 text-[11px] text-text-muted">{t(locale, "settings.model.nomatch")}</li>
          ) : (
            filtered.map((o, i) => (
              <li
                key={o}
                id={`${listId}-${i}`}
                role="option"
                aria-selected={i === activeIdx}
                onMouseDown={(e) => e.preventDefault()}
                onClick={() => pick(o)}
                onMouseEnter={() => setActive(i)}
                className={`px-3 py-1.5 text-[12px] cursor-pointer ${
                  i === activeIdx ? "bg-surface-3 text-text-1" : "text-text-2"
                }`}
              >
                {o}
              </li>
            ))
          )}
        </ul>,
        document.body,
      )}
    </div>
  );
}

import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { Locale } from "../types";
import { t } from "../i18n";

export interface DouyinSceneInfo {
  id: string;
  label_zh: string;
  bpm_min: number;
  bpm_max: number;
  is_default: boolean;
}

interface Suggestion {
  scene: string;
  score: number;
  reason: string;
}

interface Props {
  locale?: Locale;
  disabled?: boolean;
  /** 灵感文本（防抖后刷新推荐） */
  inspiration: string;
  value: string | null;
  onChange: (sceneId: string) => void;
}

/** Mode D 场景 chip：默认 daily 高亮；推荐带角标；用户点选后不被推荐覆盖 */
export default function SceneSelector({ locale, disabled, inspiration, value, onChange }: Props) {
  const [scenes, setScenes] = useState<DouyinSceneInfo[]>([]);
  const [suggested, setSuggested] = useState<string[]>([]);
  const userPicked = useRef(false);

  useEffect(() => {
    void invoke<{ scenes: DouyinSceneInfo[]; default_scene: string }>("get_douyin_meta")
      .then((m) => {
        setScenes(m.scenes);
        if (value == null) {
          const d = m.scenes.find((s) => s.is_default)?.id ?? m.default_scene;
          onChange(d);
        }
      })
      .catch(() => {
        // 离线/未接线：本地兜底 daily
        if (value == null) onChange("daily");
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- 仅挂载拉元数据
  }, []);

  useEffect(() => {
    if (userPicked.current) return;
    const text = inspiration.trim();
    const handle = window.setTimeout(() => {
      void invoke<Suggestion[]>("suggest_douyin_scenes", { userInput: text })
        .then((rows) => setSuggested(rows.slice(0, 3).map((r) => r.scene)))
        .catch(() => setSuggested([]));
    }, 300);
    return () => window.clearTimeout(handle);
  }, [inspiration]);

  const ordered = useMemo(() => {
    if (scenes.length === 0) return [];
    const rank = new Map(suggested.map((id, i) => [id, i]));
    return [...scenes].sort((a, b) => {
      const ra = rank.has(a.id) ? rank.get(a.id)! : 100;
      const rb = rank.has(b.id) ? rank.get(b.id)! : 100;
      if (ra !== rb) return ra - rb;
      return a.id.localeCompare(b.id);
    });
  }, [scenes, suggested]);

  const selected = value ?? "daily";

  return (
    <div className="flex flex-col gap-1.5">
      <label className="text-[11px] text-ink-soft flex items-center gap-1">
        {t(locale, "input.douyin.scene")}
      </label>
      <div className="flex flex-wrap gap-1.5">
        {ordered.map((s) => {
          const active = selected === s.id;
          const rec = suggested.includes(s.id);
          const isDefault = s.is_default;
          return (
            <button
              key={s.id}
              type="button"
              disabled={disabled}
              onClick={() => {
                userPicked.current = true;
                onChange(s.id);
              }}
              className={`relative px-2.5 py-1 rounded-lg text-[11px] border transition-colors
                ${active ? "border-brand-400 bg-brand-400/15 text-brand-100" : "border-line text-ink-soft hover:border-brand-400/50"}
                disabled:opacity-40`}
              title={`${s.label_zh} · BPM ${s.bpm_min}–${s.bpm_max}`}
            >
              {s.label_zh}
              {rec && !userPicked.current && (
                <span className="ml-1 text-[9px] text-brand-300">{t(locale, "input.douyin.recommended")}</span>
              )}
              {isDefault && active && !userPicked.current && (
                <span className="ml-1 text-[9px] opacity-70">{t(locale, "input.douyin.default")}</span>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}

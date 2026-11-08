// @vitest-environment jsdom
import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import ResultPanel, { normalizeSunoMeta } from "../components/ResultPanel";
import type { ChatTurn, SongPlan } from "../types";

// 模拟 Rust `plan::suno_profile::get_suno_meta` 真实返回的 `{ limits: [{ name, value, fact }] }` 结构
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "get_suno_meta") {
      return {
        limits: [
          { name: "style_max_chars", value: 1000, fact: { kind: "measured", source: "scan", date: "2026-10-20" } },
          { name: "lyrics_max_chars", value: 5000, fact: { kind: "measured", source: "scan", date: "2026-10-20" } },
          { name: "exclude_max_chars", value: 1000, fact: { kind: "measured", source: "scan", date: "2026-10-20" } },
          { name: "style_tags_min", value: 8, fact: { kind: "measured", source: "scan", date: "2026-10-20" } },
          { name: "style_tags_max", value: 15, fact: { kind: "measured", source: "scan", date: "2026-10-20" } },
          { name: "variety_default", value: 0, fact: { kind: "measured", source: "scan", date: "2026-10-20" } },
        ],
      };
    }
    return null;
  }),
}));

afterEach(cleanup);

const pkg: ChatTurn = {
  role: "assistant",
  timestamp: 2,
  content: `Title: 雨巷

Style: folk, 72 BPM

Exclude: edm

Settings: Weirdness=30 | Style Influence=70 | Variety=0

Lyrics:
[Chorus]
雨落长街
`,
};

describe("ResultPanel 四栏分块复制与蓝图检视台（P1-11 & v6.2）", () => {
  it("normalizeSunoMeta 正确解析后端 Rust `SunoMeta { limits: Vec<Limit> }` wire 格式", () => {
    const parsed = normalizeSunoMeta({
      limits: [
        { name: "style_max_chars", value: 950 },
        { name: "lyrics_max_chars", value: 4800 },
        { name: "exclude_max_chars", value: 900 },
        { name: "style_tags_min", value: 9 },
        { name: "style_tags_max", value: 14 },
        { name: "variety_default", value: 0 },
      ],
    });
    expect(parsed).toMatchObject({
      style_max_chars: 950,
      lyrics_max_chars: 4800,
      exclude_max_chars: 900,
      style_tags_min: 9,
      style_tags_max: 14,
      variety_default: 0,
    });
  });

  it("交付包显示 Style/Exclude/Settings/Lyrics 分块复制与 Variety=0 徽章，并渲染 get_suno_meta 标签区间", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(
      <ResultPanel
        conversation={[{ role: "user", content: "u", timestamp: 1 }, pkg]}
        streamText=""
        status="done"
        onRefine={() => {}}
        validationPassed={true}
      />,
    );
    expect(screen.getByText("Style")).toBeTruthy();
    expect(screen.getByText("Exclude")).toBeTruthy();
    expect(screen.getByText("Lyrics")).toBeTruthy();
    expect(screen.getByText("Variety = 0")).toBeTruthy();
    expect(screen.getByText("✓ 硬校验通过")).toBeTruthy();
    await waitFor(() => {
      expect(screen.getByText(/2\/8–15 tags/)).toBeTruthy();
    });
    fireEvent.click(screen.getByText("Style"));
    expect(writeText).toHaveBeenCalledWith(expect.stringContaining("folk"));
  });

  it("传入 songPlan 与 textEval 时渲染蓝图检视器、逐段声学分轨与七维评估看板", () => {
    const plan: SongPlan = {
      title: "雨巷",
      intent: {
        plot_type: "situational",
        one_thing: "不是放不下你，是气自己还在等",
        to_whom: "副驾驶的前任",
        at_what_moment: "凌晨便利店",
        anchor_object: "冷掉的关东煮",
        rhyme_plan: "全曲主韵言前辙(an/ian)",
      },
      asymmetry: {
        short_line: "别回头",
        chorus_variation: "末遍高八度",
        withholding: "没发出的消息",
      },
      global: {
        mood: "intimate, nocturnal",
        genre: ["indie folk"],
        bpm: 72,
        key: "A minor",
        meter: "4/4",
        vocal: ["V-02"],
        instruments: ["felt piano"],
        arrangement_hook: "muted guitar ostinato",
        mix: ["M-05"],
        negative: ["edm"],
      },
      sections: [
        {
          kind: "chorus",
          label: "Chorus",
          motion: "push",
          subtraction: "drums drop out",
          energy: 8,
          positive_local: ["belted vocals"],
          negative_local: [],
          instruments: ["felt piano"],
          minimal: false,
          target_seconds: 15,
          lines: ["雨落长街"],
        },
      ],
    };
    render(
      <ResultPanel
        conversation={[{ role: "user", content: "u", timestamp: 1 }, pkg]}
        streamText=""
        status="done"
        onRefine={() => {}}
        songPlan={plan}
        textEval={{
          total: 13,
          lines: ["hook:2:抓耳", "imagery:2:冷掉的关东煮具象"],
          small_sample: true,
          note: "小样本参考",
        }}
      />,
    );
    expect(screen.getByText(/不是放不下你，是气自己还在等/)).toBeTruthy();
    expect(screen.getAllByText(/冷掉的关东煮/).length).toBeGreaterThanOrEqual(2);
    expect(screen.getByText(/PUSH · 推进/)).toBeTruthy();
    expect(screen.getByText(/13 \/ 14/)).toBeTruthy();
  });
});

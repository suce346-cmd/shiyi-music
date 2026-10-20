// @vitest-environment jsdom
import { describe, it, expect, afterEach, vi } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import ResultPanel from "../components/ResultPanel";
import type { ChatTurn } from "../types";

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

describe("ResultPanel 四栏分块复制（P1-11）", () => {
  it("交付包显示 Style/Exclude/Settings/Lyrics 分块复制与 Variety=0 徽章", () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    render(
      <ResultPanel conversation={[{ role: "user", content: "u", timestamp: 1 }, pkg]} streamText="" status="done" onRefine={() => {}} />,
    );
    expect(screen.getByText("Style")).toBeTruthy();
    expect(screen.getByText("Exclude")).toBeTruthy();
    expect(screen.getByText("Lyrics")).toBeTruthy();
    expect(screen.getByText("Variety = 0")).toBeTruthy();
    fireEvent.click(screen.getByText("Style"));
    expect(writeText).toHaveBeenCalledWith(expect.stringContaining("folk"));
  });
});

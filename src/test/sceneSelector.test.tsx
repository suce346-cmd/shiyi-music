// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import SceneSelector from "../components/SceneSelector";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "get_douyin_meta") {
      return {
        scenes: [
          { id: "emotion", label_zh: "情绪治愈", bpm_min: 80, bpm_max: 100, is_default: false },
          { id: "daily", label_zh: "日常 Vlog", bpm_min: 100, bpm_max: 120, is_default: true },
          { id: "dance", label_zh: "舞蹈卡点", bpm_min: 120, bpm_max: 140, is_default: false },
        ],
        default_scene: "daily",
      };
    }
    if (cmd === "suggest_douyin_scenes") {
      return [{ scene: "dance", score: 7, reason: "命中 跳舞" }];
    }
    return null;
  }),
}));

describe("SceneSelector（P1-7）", () => {
  beforeEach(() => vi.clearAllMocks());

  it("仅渲染场景 chip，默认 daily，点选覆盖", async () => {
    const onChange = vi.fn();
    render(<SceneSelector inspiration="跳舞卡点" value={null} onChange={onChange} />);
    await waitFor(() => expect(onChange).toHaveBeenCalledWith("daily"));
    await waitFor(() => expect(screen.getByText("舞蹈卡点")).toBeTruthy());
    fireEvent.click(screen.getByText("舞蹈卡点"));
    expect(onChange).toHaveBeenCalledWith("dance");
  });
});

import { describe, it, expect } from "vitest";
import { estimateRefineTargets } from "../utils/refineTargets";

describe("estimateRefineTargets（镜像，后端为准）", () => {
  it("歌词/编曲/情绪/抖音/参数五类", () => {
    expect(estimateRefineTargets("副歌歌词太直白", "mode_b")).toEqual(["lyricist"]);
    expect(estimateRefineTargets("副歌歌词太直白", "mode_c")).toEqual(["reviser"]);
    expect(estimateRefineTargets("唢呐不够炸", "mode_d")).toEqual(["producer", "emotion"]);
    expect(estimateRefineTargets("不够洗脑", "mode_d")).toEqual(["style_analyst"]);
    expect(estimateRefineTargets("不够洗脑", "mode_b")).toEqual(["producer"]);
    expect(estimateRefineTargets("Weirdness 太高", "mode_b")).toEqual(["producer", "emotion"]);
  });

  it("无命中返回空（后端回落全量）", () => {
    expect(estimateRefineTargets("随便改改", "mode_b")).toEqual([]);
  });
});

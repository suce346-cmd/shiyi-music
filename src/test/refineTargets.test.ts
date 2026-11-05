import { describe, it, expect } from "vitest";
import { estimateRefineTargets } from "../utils/refineTargets";

describe("estimateRefineTargets（镜像，后端为准）", () => {
  it("歌词/编曲/情绪/抖音/参数五类（含大小写与 v6.2 声律/分轨/升调/转译词）", () => {
    expect(estimateRefineTargets("副歌歌词太直白", "mode_b")).toEqual(["lyricist"]);
    expect(estimateRefineTargets("副歌歌词太直白", "mode_c")).toEqual(["reviser"]);
    expect(estimateRefineTargets("HOOK 记忆点不够", "mode_b")).toEqual(["lyricist"]);
    expect(estimateRefineTargets("十三辙押韵不顺", "mode_b")).toEqual(["lyricist"]);
    expect(estimateRefineTargets("去掉括号指令和破喻口号", "mode_b")).toEqual(["lyricist"]);
    expect(estimateRefineTargets("前奏减法抽空", "mode_b")).toEqual(["producer"]);
    expect(estimateRefineTargets("终局副歌半音升调，二创分轨干声净化", "mode_b")).toEqual(["lyricist", "producer"]);
    expect(estimateRefineTargets("艺人名转译为四维声学融合", "mode_b")).toEqual(["producer"]);
    expect(estimateRefineTargets("唢呐不够炸", "mode_d")).toEqual(["producer", "emotion"]);
    expect(estimateRefineTargets("不够洗脑", "mode_d")).toEqual(["style_analyst"]);
    expect(estimateRefineTargets("不够洗脑", "mode_b")).toEqual(["producer"]);
    expect(estimateRefineTargets("Weirdness 太高", "mode_b")).toEqual(["producer", "emotion"]);
    expect(estimateRefineTargets("weirdness 太高", "mode_b")).toEqual(["producer", "emotion"]);
  });

  it("无命中返回空（后端回落全量）", () => {
    expect(estimateRefineTargets("随便改改", "mode_b")).toEqual([]);
  });
});

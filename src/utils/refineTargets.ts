/** 前端反馈关键词路由镜像（仅预估展示用，真源在后端 roles_for_feedback）。
 *  双源同步：规则变更两端同 commit；后端为准，前端只做"将重跑"提示。
 *  ModeC 歌词类映射 reviser；抖音类非 D 模式回落 producer；无命中返回空（后端回落全量）。 */
import type { Mode, PipelineRoleKey } from "../types";

export function estimateRefineTargets(feedback: string, mode: Mode): PipelineRoleKey[] {
  const out: PipelineRoleKey[] = [];
  const push = (r: PipelineRoleKey) => {
    if (!out.includes(r)) out.push(r);
  };
  const has = (ks: string[]) => ks.some((k) => feedback.includes(k));
  if (has(["歌词", "词", "句", "韵", "唱", "hook", "副歌", "主歌", "金句"])) {
    push(mode === "mode_c" ? "reviser" : "lyricist");
  }
  if (
    has(["编曲", "配器", "乐器", "伴奏", "BPM", "bpm", "人声", "音色", "混音", "鼓", "吉他", "钢琴", "唢呐"])
  ) {
    push("producer");
  }
  if (has(["情绪", "能量", "感觉", "氛围", "情感", "炸", "软", "嗨"])) {
    push("emotion");
  }
  if (has(["抖音", "传播", "钩子", "洗脑", "爆", "魔性", "循环", "骤停"])) {
    push(mode === "mode_d" ? "style_analyst" : "producer");
  }
  if (has(["参数", "怪异度", "影响度", "Weirdness", "Influence"])) {
    push("producer");
    push("emotion");
  }
  return out;
}

/** 角色中文名（预估展示用，与 ROLE_NAMES 对齐的子集） */
export const REFINE_TARGET_NAMES: Record<PipelineRoleKey, string> = {
  host: "主持人",
  auditor: "校验员",
  emotion: "情感分析师",
  lyricist: "作词人",
  reviser: "改词人",
  producer: "制作人",
  style_analyst: "流行风格分析师",
};

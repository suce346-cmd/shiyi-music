import type { ChatMessage, ChatTurn } from "../types";

/** #10 半成品继续优化的**上游告知**（单源）。
 *
 *  背景：生成中断（出错/取消）时固化的产出是**可能被截断的半成品**（`ChatTurn.partial`）。
 *  用户若以它为基础优化，必须让模型知道"上一版不是完整方案"，否则它会把截断处当完整方案，
 *  在残缺结构上继续加工（下游校验再发现时已浪费一整轮）。
 *
 *  为什么放在 user_input 的【上一版方案】段内（而不是拼进 feedback）：
 *  告知必须**紧贴被描述的对象**——拼进 feedback 会与用户意见混成一句，语义归属不清；
 *  且后端 `extract_previous_plan` 取最后一个 `【上一版方案】` 标记之后全文，
 *  标记原样保留即可让告知随方案一起进入主持人上下文（无需改后端协议）。
 *
 *  为什么是中文常量：本项目的 mode 提示词正文全部为中文（与 rules.rs 单源文案同域），
 *  该告知是**给模型看**的，不随界面语言切换（与面向用户的 i18n 文案分属两个域）。 */

/** 【上一版方案】标记——与后端 orchestrator::extract_previous_plan 的标记**同名同源**（改名即失联） */
export const PREVIOUS_PLAN_MARKER = "【上一版方案】";

/** 半成品告知前缀（紧贴半成品正文之前，位于【上一版方案】段内） */
export const PARTIAL_DRAFT_NOTICE =
  "（注意：以下上一版方案是生成中途中断得到的**半成品**，可能不完整或被截断；" +
  "请在它的基础上补齐并继续完善，不要把它当作完整方案处理。）\n";

/** 组装 refine 的 user_input：标记 + 可选半成品告知 + 上一版正文（唯一拼接点） */
export function composeRefineInput(
  userInput: string,
  lastOutput: string,
  partial: boolean,
): string {
  const notice = partial ? PARTIAL_DRAFT_NOTICE : "";
  return `${userInput}\n\n${PREVIOUS_PLAN_MARKER}\n${notice}${lastOutput}`;
}

/** #10 失败/取消时把已流出的正文固化为"半成品"——**唯一判定处**。
 *
 *  为什么需要：失败前流出的正文此前只活在浮动的流式状态里（可见但不可用）——
 *  没有 assistant 轮就没有复制入口，会话历史又在新 run 开始时被清空，
 *  于是"优化"必然丢掉半成品从头再来。固化 = 把它落成一条 partial 的正式产出。
 *
 *  为什么返回两处写入：可见轮（`turn`，供渲染与复制）与会话历史（`history`，供 refine 取上一版）
 *  必须**同形**——只写一处即产生"看得见但改不了"或"改了却看不见"的偏差。
 *
 *  无正文（空白）→ null：不固化空轮，避免留下一个无内容的气泡（此时由操作条的
 *  "本次没有产出"提示承担引导）。 */
export function freezePartial(
  streamText: string,
  userInput: string,
): { turn: ChatTurn; history: ChatMessage[] } | null {
  if (!streamText.trim()) return null;
  return {
    turn: { role: "assistant", content: streamText, timestamp: Date.now(), partial: true },
    history: [
      { role: "user", content: userInput },
      { role: "assistant", content: streamText, partial: true },
    ],
  };
}
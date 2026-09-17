import type { ChatTurn } from "../types";

/** 修复包 D-1（#13 界面层）：并发审改时专家发言按事件到达序插入，与后端
 * 汇总顺序（模式阵容序）不一致——"发言打架"观感的真瑕疵。
 *
 * 2026-09-18 补洞（生效范围修正）：旧实现 orderSpeechTurns 对**整个数组**做全局
 * 归位，作用域超出了"本轮"。两个后果：
 *   A. 结束态组装（[user, ...speechLog, assistant]）未过归位 → 最终列表回到达序，
 *      规则在用户真正看到的结束态失效；同一份乱序还写进了历史条目。
 *   B. 优化/续跑路径 conversation 不重置（保留上一轮全部 turns），全局归位会把
 *      本轮新到的发言排进**上一轮中间**——越界重排。
 * 故本模块只定义"本轮"语义：一轮 = 一次 run 产生的连续 expert 段，
 * 它以「数组末尾最后一个非 expert 元素之后」为界；跨轮永不重排。 */

/** 角色在阵容中的下标；阵容外（插话确认/未知角色）排末尾，互相保持到达序 */
export function rosterOrder(speakerId: string | undefined, roster: string[]): number {
  if (!speakerId) return roster.length;
  const idx = roster.indexOf(speakerId);
  return idx === -1 ? roster.length : idx;
}

/** expert 发言按阵容序稳定排序（同位次保持到达序） */
function sortExpertsByRoster(experts: ChatTurn[], roster: string[]): ChatTurn[] {
  return experts
    .map((t, i) => ({ t, i }))
    .sort((a, b) => {
      const ka = rosterOrder(a.t.speaker?.id, roster);
      const kb = rosterOrder(b.t.speaker?.id, roster);
      return ka === kb ? a.i - b.i : ka - kb;
    })
    .map(({ t }) => t);
}

/** 本轮专家发言归位：只对**数组末尾的连续 expert 段**做阵容序稳定排序。
 * - 末尾连续 expert 段 = 本轮尚未收尾的发言累积区（user/assistant 为轮次锚点）；
 * - 锚点之前的历史 turns（含上一轮 expert 段）原样保留，绝不跨轮重排；
 * - 段内不足 2 条时直接原样返回（无需排序，也避免无谓的新数组）。
 * roster = MODE_EXPERTS[mode].map(e => e.id)（调用方取当前模式阵容）。 */
export function orderRoundSpeech(turns: ChatTurn[], roster: string[]): ChatTurn[] {
  let start = turns.length;
  while (start > 0 && turns[start - 1].role === "expert") start--;
  if (turns.length - start < 2) return turns;
  return [...turns.slice(0, start), ...sortExpertsByRoster(turns.slice(start), roster)];
}

/** 结束态对话流组装：本轮专家发言先按阵容序归位，再拼接尾部 turns。
 * 三处结束点（生成/优化/续跑）共用此唯一出口。
 * 旧实现直接铺 speechLogRef.current（到达序）再 append 尾部——流式期已归位的顺序会在
 * 运行结束时被整体覆盖回乱序，而这份 allTurns 正是 ResultPanel 渲染的那个数组
 * （不重排），且随条目写进历史（回看历史同样乱）。 */
export function assembleFinalTurns(
  base: ChatTurn[],
  speeches: ChatTurn[],
  tail: ChatTurn[],
  roster: string[],
): ChatTurn[] {
  return [...orderRoundSpeech([...base, ...speeches], roster), ...tail];
}
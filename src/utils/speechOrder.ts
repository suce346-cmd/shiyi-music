import type { ChatTurn } from "../types";

/** 修复包 D-1（#13 界面层）：并发审改时专家发言按事件到达序插入，与后端
 * 汇总顺序（模式阵容序）不一致——"发言打架"观感的真瑕疵。
 * 此模块把 expert 发言稳定重排为阵容序；user/assistant 原样保留。 */

/** 角色 in 阵容的下标；阵容外（插话确认/未知角色）排末尾，互相保持到达序 */
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

/** 稳定排序：expert 发言按阵容序归位，同位次保持到达序；非 expert 不动且相对位置不变。
 * 实现：非 expert 作为"锚点"固定在原位，expert 按阵容序依次填入锚点之间的空隙。
 * roster = MODE_EXPERTS[mode].map(e => e.id)（调用方取当前模式阵容）。 */
export function orderSpeechTurns(turns: ChatTurn[], roster: string[]): ChatTurn[] {
  if (turns.length === 0) return turns;
  if (!turns.some((t) => t.role === "expert")) return turns;
  const sorted = sortExpertsByRoster(turns.filter((t) => t.role === "expert"), roster);
  // 回填：锚点（非 expert）保持原位，expert 依序填入空位
  const out: ChatTurn[] = [];
  let ei = 0;
  for (const t of turns) {
    if (t.role === "expert") {
      out.push(sorted[ei++]);
    } else {
      out.push(t);
    }
  }
  return out;
}

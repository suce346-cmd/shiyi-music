/** 生成队列——纯前端调度（后端仍单任务顺序执行，一次只 invoke 一个）。
 *  设计：
 *  - 提交时若 pipeline 繁忙则入队，否则直接跑；当前任务结束（完成/失败/取消）后自动取队首。
 *  - 取消只杀当前，队列不受影响继续。
 *  - 队列项跨模式各自记录 mode；结果各自独立 HistoryEntry（复用现有归档）。
 *  - 本文件只含纯调度的可测函数 + useQueue hook；UI 在 QueuePanel.tsx。
 */
import { useState, useCallback, useRef } from "react";
import type { HistoryEntry, Mode } from "../types";

export type QueueItemStatus = "queued" | "running" | "done" | "error" | "cancelled";

/** 队列项产出快照（**失败/取消**时固化的产出；成功项走 `historyId` 指向历史条目）。
 *  必要性：完成后立即取队首续跑，新 run 开头会清空会话——若产出只活在会话 state 里，
 *  上一项的产出（尤其失败时的半成品）会在被看见之前就被抹掉。 */
export interface QueueResult {
  /** 中断前已流出的正文 */
  output: string;
  /** 是否"生成中断的半成品"（内容可能被截断；与 ChatTurn.partial 同义） */
  partial: boolean;
}

export interface QueueItem {
  id: string;
  /** 列表展示标签（输入前 30 字） */
  label: string;
  mode: Mode;
  userInput: string;
  extra?: { originalLyrics: string };
  status: QueueItemStatus;
  enqueuedAt: number;
  /** 成功项归档后的历史条目 id（点击查看走**精确**关联，不再按输入前缀模糊匹配） */
  historyId?: string;
  /** 失败/取消项的产出快照（无产出则缺省） */
  result?: QueueResult;
}

/** 展示标签：输入前 30 字（换行压成空格） */
export function queueLabel(input: string): string {
  const flat = input.replace(/\s+/g, " ").trim();
  return flat.length > 30 ? flat.slice(0, 30) + "…" : flat;
}

/** 纯函数：入队（返回新队列；调用方负责忙闲分流） */
export function enqueue(
  queue: QueueItem[],
  item: Omit<QueueItem, "id" | "status" | "enqueuedAt"> & { id: string },
): QueueItem[] {
  return [...queue, { ...item, status: "queued", enqueuedAt: Date.now() }];
}

/** 纯函数：取队首待跑项（queued 第一项；无则 undefined） */
export function dequeueNext(queue: QueueItem[]): QueueItem | undefined {
  return queue.find((q) => q.status === "queued");
}

/** 纯函数：标记状态（返回新队列） */
export function markQueueStatus(
  queue: QueueItem[],
  id: string,
  status: QueueItemStatus,
): QueueItem[] {
  return queue.map((q) => (q.id === id ? { ...q, status } : q));
}

/** 纯函数：移除一项 */
export function removeQueueItem(queue: QueueItem[], id: string): QueueItem[] {
  return queue.filter((q) => q.id !== id);
}

/** 纯函数：清空等待项（running 不动——取消走 cancel 流程） */
export function clearQueued(queue: QueueItem[]): QueueItem[] {
  return queue.filter((q) => q.status !== "queued");
}

/** 纯函数：关联历史条目 id（成功归档后调用） */
export function linkQueueHistory(queue: QueueItem[], id: string, historyId: string): QueueItem[] {
  return queue.map((q) => (q.id === id ? { ...q, historyId } : q));
}

/** 纯函数：写入产出快照（失败/取消时固化；成功项不写，走 historyId） */
export function setQueueResult(queue: QueueItem[], id: string, result: QueueResult): QueueItem[] {
  return queue.map((q) => (q.id === id ? { ...q, result } : q));
}

/** 该项是否有可查看内容——**点击可用性的唯一判定源**（UI 只在为真时给可点击样式）。
 *  此前无条件把 done/error 画成可点击，实际查不到历史时点击静默无响应（假affordance）。 */
export function canViewQueueItem(item: QueueItem): boolean {
  return Boolean(item.historyId) || Boolean(item.result);
}

/** 队列项终态单源：**取消不得被写成"失败"**（旧实现无条件 mark error，覆盖掉取消标记，
 *  用户主动取消的项在列表里变成红灯"失败"——状态失真）。 */
export function terminalQueueStatus(cancelled: boolean): QueueItemStatus {
  return cancelled ? "cancelled" : "error";
}

/** 失败/取消项的产出快照 → 与历史条目**同形**的视图条目（复用"历史视图 + 继续优化"全链）。
 *  快照里的 partial 必须落到 assistant 轮的 `partial` 上——否则从这里再优化时注入层拿不到标记，
 *  不会告知模型"上一版是可能被截断的半成品"。无快照 → null（该项本就没有可查看内容）。 */
export function queueResultEntry(item: QueueItem): HistoryEntry | null {
  const r = item.result;
  if (!r) return null;
  return {
    id: item.id,
    mode: item.mode,
    input: item.userInput,
    output: r.output,
    conversation: [
      { role: "user", content: item.userInput, timestamp: item.enqueuedAt },
      { role: "assistant", content: r.output, timestamp: item.enqueuedAt, partial: r.partial },
    ],
    timestamp: item.enqueuedAt,
  };
}

/** useQueue hook：队列状态 + 操作（执行调度由 App 层完成链驱动） */
export function useQueue() {
  const [queue, setQueue] = useState<QueueItem[]>([]);
  const queueRef = useRef(queue);
  queueRef.current = queue;

  const push = useCallback((item: Omit<QueueItem, "id" | "status" | "enqueuedAt"> & { id: string }) => {
    setQueue((prev) => {
      const next = enqueue(prev, item);
      queueRef.current = next;
      return next;
    });
  }, []);

  const mark = useCallback((id: string, status: QueueItemStatus) => {
    setQueue((prev) => {
      const next = markQueueStatus(prev, id, status);
      queueRef.current = next;
      return next;
    });
  }, []);

  const remove = useCallback((id: string) => {
    setQueue((prev) => {
      const next = removeQueueItem(prev, id);
      queueRef.current = next;
      return next;
    });
  }, []);

  const clearWaiting = useCallback(() => {
    setQueue((prev) => {
      const next = clearQueued(prev);
      queueRef.current = next;
      return next;
    });
  }, []);

  /** 关联历史条目（成功归档后） */
  const linkHistory = useCallback((id: string, historyId: string) => {
    setQueue((prev) => {
      const next = linkQueueHistory(prev, id, historyId);
      queueRef.current = next;
      return next;
    });
  }, []);

  /** 写入产出快照（失败/取消固化） */
  const setResult = useCallback((id: string, result: QueueResult) => {
    setQueue((prev) => {
      const next = setQueueResult(prev, id, result);
      queueRef.current = next;
      return next;
    });
  }, []);

  /** 同步读取当前队列（完成链回调内用，避闭包过期） */
  const peek = useCallback(() => queueRef.current, []);

  return { queue, push, mark, remove, clearWaiting, linkHistory, setResult, peek };
}

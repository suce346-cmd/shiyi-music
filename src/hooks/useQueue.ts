/** F9：生成队列——纯前端调度（后端仍单任务顺序执行，一次只 invoke 一个）。
 *  设计：
 *  - 提交时若 pipeline 繁忙则入队，否则直接跑；当前任务结束（完成/失败/取消）后自动取队首。
 *  - 取消只杀当前（A9 run_id 定向），队列不受影响继续。
 *  - 队列项跨模式各自记录 mode；结果各自独立 HistoryEntry（复用现有归档）。
 *  - 本文件只含纯调度的可测函数 + useQueue hook；UI 在 QueuePanel.tsx。
 */
import { useState, useCallback, useRef } from "react";
import type { Mode } from "../types";

export type QueueItemStatus = "queued" | "running" | "done" | "error" | "cancelled";

export interface QueueItem {
  id: string;
  /** 列表展示标签（输入前 30 字） */
  label: string;
  mode: Mode;
  userInput: string;
  extra?: { originalLyrics: string };
  status: QueueItemStatus;
  enqueuedAt: number;
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

  /** 同步读取当前队列（完成链回调内用，避闭包过期） */
  const peek = useCallback(() => queueRef.current, []);

  return { queue, push, mark, remove, clearWaiting, peek };
}

import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { IconPlayerPlay, IconFlagCheck, IconBolt, IconMessagePlus, IconLoader } from "@tabler/icons-react";
import type { Locale } from "../types";
import { errText } from "../types";
import { t, tf } from "../i18n";

interface Props {
  /** 后端 round_gate_pending 事件内容（round=已完成轮次，nextRound=待决断的下一轮） */
  gate: { round: number; nextRound: number; timeoutSecs: number };
  /** 决断：继续下一轮 / 结束讨论出终稿（上抛错误由调用方处理） */
  onDecide: (decision: "continue" | "finalize") => void;
  /** 一键关闭设置里的轮间确认（本次门按"继续"放行，此后全自动推进） */
  onAutoOff: () => void;
  /** 当前 run_id 读取（插话命令定向用） */
  getRunId?: () => string;
  /** 界面语言（缺省中文） */
  locale?: Locale;
}

/** #12 轮间确认门：流水线在轮末真的停下等用户决断（继续下一轮 / 结束讨论直接出终稿）。
 *  这不是"提示条"——不点就一直等（后端 5 分钟超时后自动继续并记降级）。
 *  暂停期间可插话：选择"继续"时由下一轮开头消费；选择"结束讨论"则不纳入（组件文案已如实告知）。 */
export default function RoundGateBar({ gate, onDecide, onAutoOff, getRunId, locale }: Props) {
  const [note, setNote] = useState("");
  const [sending, setSending] = useState(false);
  const [msg, setMsg] = useState("");

  /** 发送插话（按 run_id 定向，非阻塞存入后端槽；超长由后端 Validation 拦截） */
  const sendInterject = async () => {
    const text = note.trim();
    if (!text || sending) return;
    setSending(true);
    setMsg("");
    try {
      await invoke("interject_feedback", { runId: getRunId?.() ?? "", note: text });
      setNote("");
      setMsg(t(locale, "interject.sent"));
    } catch (e) {
      setMsg(`${t(locale, "interject.fail")}${errText(e)}`);
    } finally {
      setSending(false);
      setTimeout(() => setMsg(""), 4000);
    }
  };

  return (
    <div className="mx-4 mt-3 px-3 py-2.5 rounded-xl glass-panel border border-brand-500/40 bg-brand-500/8 text-[13px]">
      <div className="flex items-start gap-2">
        <IconFlagCheck size={15} className="text-brand-400 mt-0.5 shrink-0" />
        <div className="flex-1 min-w-0">
          <p className="font-medium text-text-1 leading-snug">
            {tf(locale, "gate.title", { round: gate.round, next: gate.nextRound })}
          </p>
          <p className="text-[10px] text-text-muted mt-0.5">
            {tf(locale, "gate.timeout", { secs: gate.timeoutSecs })}
          </p>
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-1.5 mt-2">
        <button onClick={() => onDecide("continue")}
          className="flex items-center gap-1 px-2.5 py-1 rounded-lg text-[11px] font-medium
                     brand-gradient-btn text-white transition-all duration-150 active:scale-95">
          <IconPlayerPlay size={12} />
          {t(locale, "gate.continue")}
        </button>
        <button onClick={() => onDecide("finalize")}
          className="flex items-center gap-1 px-2.5 py-1 rounded-lg text-[11px] font-medium
                     bg-surface-3 hover:bg-border border border-border/50 text-text-2
                     transition-all duration-150 active:scale-95">
          <IconFlagCheck size={12} />
          {t(locale, "gate.finalize")}
        </button>
        <button onClick={onAutoOff}
          className="flex items-center gap-1 px-2 py-1 rounded-lg text-[11px] text-text-muted
                     hover:text-text-2 transition-colors ml-auto">
          <IconBolt size={12} />
          {t(locale, "gate.autoOff")}
        </button>
      </div>

      <div className="mt-2 flex gap-1.5">
        <input value={note} onChange={e => setNote(e.target.value)}
          onKeyDown={e => { if (e.key === "Enter" && !e.nativeEvent.isComposing) sendInterject(); }}
          placeholder={t(locale, "interject.ph")}
          disabled={sending}
          className="flex-1 bg-surface-0 border border-border/60 rounded-lg px-2.5 py-1.5 text-[12px]
                     text-text-1 placeholder:text-text-muted/30 focus:outline-none
                     focus:border-brand-500/40 transition-all duration-150 disabled:opacity-50" />
        <button onClick={sendInterject} disabled={!note.trim() || sending}
          className="flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-[12px] font-medium
                     bg-brand-500/10 hover:bg-brand-500/20 border border-brand-500/30 text-brand-400
                     disabled:opacity-40 disabled:cursor-not-allowed transition-all duration-150 active:scale-95 shrink-0">
          {sending ? <IconLoader size={12} className="animate-spin" /> : <IconMessagePlus size={12} />}
          {t(locale, "interject.send")}
        </button>
      </div>
      <p className="mt-1.5 text-[10px] text-text-muted">
        {msg || t(locale, "gate.interject.hint")}
      </p>
    </div>
  );
}
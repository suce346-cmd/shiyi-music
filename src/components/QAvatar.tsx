import type { ExpertCard } from "../types";

// 真实角色立绘（用户提供，透明背景，已等比缩小）。
// 7 个流水线角色全配图；未来新增角色未配图时走 emoji 占位兜底。
import host from "../assets/avatars/host.png";
import auditor from "../assets/avatars/auditor.png";
import emotion from "../assets/avatars/emotion.png";
import lyricist from "../assets/avatars/lyricist.png";
import reviser from "../assets/avatars/reviser.png";
import producer from "../assets/avatars/producer.png";
import style_analyst from "../assets/avatars/style_analyst.png";

/** 角色 id → 真实立绘图（v2 流水线角色） */
const AVATAR_MAP: Record<string, string> = {
  host,               // 👑 主持人（AI音乐总指挥）
  auditor,            // 🔍 校验员（魔鬼代言人）
  emotion,            // 🎭 情感分析师
  lyricist,           // 📝 作词人（指令规范化师）
  reviser,            // ✍️ 改词人（歌词审查官）
  producer,           // 🎤 制作人（乐器编曲师）
  style_analyst,      // 🔥 流行风格分析师（风格节奏师）
};

interface QAvatarProps {
  expert: Pick<ExpertCard, "id" | "emoji" | "color">;
  status: ExpertCard["status"];
  size?: number;
}

/**
 * 角色立绘（v2 流水线角色）：
 * - 有立绘图：渲染透明 PNG（object-contain 等比）+ 状态光圈 + 角标（✓ / !）
 * - 无图兜底：emoji 占位（未来新增角色未配图时）
 * 动态动画由父级 className 驱动（bob/jump/celebrate/shake）
 */
export default function QAvatar({ expert, status, size = 44 }: QAvatarProps) {
  const img = AVATAR_MAP[expert.id];

  // 状态角标（图片与占位共用）
  const badges = (
    <>
      {status === "done" && (
        <span className="absolute -top-1 -right-1 w-4 h-4 rounded-full bg-success text-white flex items-center justify-center text-[10px] shadow">✓</span>
      )}
      {status === "error" && (
        <span className="absolute -top-1 -right-1 w-4 h-4 rounded-full bg-danger text-white flex items-center justify-center text-[9px] shadow">!</span>
      )}
    </>
  );

  if (img) {
    return (
      <div className="relative inline-block select-none" style={{ width: size, height: size * 1.25 }}>
        {/* 发言光晕 */}
        {status === "working" && (
          <span className="absolute inset-0 rounded-full glow-ring" style={{ boxShadow: `0 0 0 3px ${expert.color}55` }} />
        )}
        <img
          src={img}
          alt={expert.emoji}
          draggable={false}
          className="w-full h-full object-contain"
          style={{ filter: status === "error" ? "grayscale(0.4)" : undefined }}
        />
        {badges}
      </div>
    );
  }

  // 无图兜底：emoji 占位
  return (
    <div
      className="relative inline-block select-none flex items-center justify-center rounded-full bg-surface-2/80 border border-border/40"
      style={{ width: size, height: size * 1.25, fontSize: size * 0.5 }}
    >
      <span>{expert.emoji}</span>
      {badges}
    </div>
  );
}

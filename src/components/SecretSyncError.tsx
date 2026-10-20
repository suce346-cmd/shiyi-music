import { t } from "../i18n";
import type { Locale } from "../types";

/** P0-10 残留：钥匙串同步失败（如 Key 格式被拒）在对应输入框下就地提示——Key 未落盘，重启会丢 */
export function SecretSyncError({ message, locale }: { message?: string; locale: Locale | undefined }) {
  if (!message) return null;
  return (
    <p role="alert" className="mt-1 text-[10px] text-danger leading-relaxed">
      {t(locale, "settings.apikey.saveFailed")}{message}
    </p>
  );
}

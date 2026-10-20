import { useState, useCallback, useEffect, useRef, useMemo } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AppSettings, RoleApiOverride, StructuredMode } from "../types";
import { STRUCTURED_MODES } from "../types";

const STORAGE_KEY = "suno-prompt-settings";
/** localStorage 中的明文 key 哨兵——该字段在钥匙串，本地只存标记 */
const KEYCHAIN_SENTINEL = "__keychain__";
/** 迁移标记（localStorage 明文 → 钥匙串一次性迁移，防止重复执行） */
const MIGRATED_KEY = "suno-prompt-keychain-migrated";

const defaultSettings: AppSettings = {
  apiKey: "",
  model: "gpt-4o",
  baseUrl: "https://api.openai.com/v1",
  // 思考模式默认关闭：保持速度与稳定性；需要时可随时打开（后端按模型能力自动适配）
  thinking: false,
  // 角色级 API 覆盖默认空：所有角色共用全局配置
  roleOverrides: {},
  // #12 轮间人工确认门：**默认开启**——讨论轮之间必须给用户一次"继续/收工"的机会
  // （问题的本体就是"等不到你"）；不想被打断可在门条上一键关闭，此后全自动推进。
  roundGate: true,
};

/** 清洗 localStorage 旧数据：坏类型字段回退默认值，防透传后端 serde 反序列化失败。
 * apiKey 字段只接受哨兵或空——明文 key 不再从 localStorage 读取（走钥匙串迁移）。
 * export 供单测（纯函数）+ 导入配置清洗复用同一入口。 */
export function sanitizeStored(raw: unknown): AppSettings {
  const base = { ...defaultSettings };
  if (typeof raw !== "object" || raw === null) return base;
  const o = raw as Record<string, unknown>;
  // apiKey：仅哨兵视为"钥匙串有值"（内存回填后替换）；其他字符串视为待迁移明文，这里置空
  if (o.apiKey === KEYCHAIN_SENTINEL) base.apiKey = KEYCHAIN_SENTINEL;
  if (typeof o.model === "string") base.model = o.model;
  if (typeof o.baseUrl === "string") base.baseUrl = o.baseUrl;
  if (typeof o.thinking === "boolean") base.thinking = o.thinking;
  // theme 清洗（非法值回退跟随系统）
  if (o.theme === "light" || o.theme === "dark" || o.theme === "system") base.theme = o.theme;
  // language 清洗（非法值回退中文）
  if (o.language === "zh" || o.language === "en") base.language = o.language;
  // #12 轮间确认门清洗（非布尔回退默认；旧数据无此字段 → 默认开启）
  if (typeof o.roundGate === "boolean") base.roundGate = o.roundGate;
  // generation 清洗（数值范围收敛，坏值丢弃走后端默认）
  if (typeof o.generation === "object" && o.generation !== null && !Array.isArray(o.generation)) {
    const g = o.generation as Record<string, unknown>;
    const cleaned: { temperature?: number; max_tokens?: number } = {};
    if (typeof g.temperature === "number" && g.temperature >= 0 && g.temperature <= 2) {
      cleaned.temperature = g.temperature;
    }
    // F-1：上限 30000 与后端 MAX_TOKENS_CAP 同源（后端校验 1000..=30000，32000 会被整请求拒绝）
    if (typeof g.max_tokens === "number" && Number.isInteger(g.max_tokens) && g.max_tokens >= 1000 && g.max_tokens <= 30000) {
      cleaned.max_tokens = g.max_tokens;
    }
    if (cleaned.temperature !== undefined || cleaned.max_tokens !== undefined) {
      base.generation = cleaned;
    }
  }
  if (typeof o.roleOverrides === "object" && o.roleOverrides !== null && !Array.isArray(o.roleOverrides)) {
    // 逐角色深校验：非对象条目丢弃；对象只保留 string 三字段
    // api_key 同 apiKey 处理——仅哨兵保留，明文待迁移
    // O-5/U-1：role 键白名单（与后端 PipelineRole::storage_key 同名单源，keychain account 同名）——
    // 旧规则任意键放行，会配出后端拒绝的 role:{未知} 钥匙串账户
    const KNOWN_ROLES = ["host", "auditor", "emotion", "lyricist", "reviser", "producer", "style_analyst"];
    const cleaned: Record<string, RoleApiOverride> = {};
    for (const [k, v] of Object.entries(o.roleOverrides)) {
      if (!KNOWN_ROLES.includes(k)) continue;
      if (v && typeof v === "object" && !Array.isArray(v)) {
        const entry: RoleApiOverride = {};
        const e = v as Record<string, unknown>;
        if (typeof e.model === "string") entry.model = e.model;
        if (e.api_key === KEYCHAIN_SENTINEL) entry.api_key = KEYCHAIN_SENTINEL;
        if (typeof e.base_url === "string") entry.base_url = e.base_url;
        if (STRUCTURED_MODES.includes(e.structured_mode as StructuredMode)) {
          entry.structured_mode = e.structured_mode as StructuredMode;
        }
        cleaned[k] = entry;
      }
    }
    base.roleOverrides = cleaned;
  }
  // P1-2 探测缓存清洗：只留 string 键 + 合法档位值
  if (typeof o.structuredCaps === "object" && o.structuredCaps !== null && !Array.isArray(o.structuredCaps)) {
    const caps: Record<string, StructuredMode> = {};
    for (const [k, v] of Object.entries(o.structuredCaps)) {
      if (STRUCTURED_MODES.includes(v as StructuredMode)) caps[k] = v as StructuredMode;
    }
    base.structuredCaps = caps;
  }
  return base;
}

/** F1：未验证进钥匙串的明文 */
export interface PlaintextHold { global?: string; roles: Record<string, string> }

/** F1：确保 secret 已在钥匙串（读回相等）；不等则写入并读回校验。成功 null，失败返回原因。 */
export async function ensureInKeychain(account: string, secret: string): Promise<string | null> {
  try {
    const cur = await invoke<string | null>("keychain_get", { account });
    if (cur === secret) return null;
    await invoke("keychain_set", { account, secret });
    const back = await invoke<string | null>("keychain_get", { account });
    return back === secret ? null : "写入后读回不一致";
  } catch (e) {
    return secretErrorMessage(e);
  }
}

/** 从原始 localStorage 数据中提取待迁移的明文 key（全局 + 各角色），不经过 sanitize */
function extractPlaintextKeys(raw: unknown): { global?: string; roles: Record<string, string> } {
  const roles: Record<string, string> = {};
  if (typeof raw !== "object" || raw === null) return { roles };
  const o = raw as Record<string, unknown>;
  let global: string | undefined;
  if (typeof o.apiKey === "string" && o.apiKey !== "" && o.apiKey !== KEYCHAIN_SENTINEL) {
    global = o.apiKey;
  }
  const ro = o.roleOverrides;
  if (typeof ro === "object" && ro !== null && !Array.isArray(ro)) {
    for (const [k, v] of Object.entries(ro)) {
      if (v && typeof v === "object" && !Array.isArray(v)) {
        const ak = (v as Record<string, unknown>).api_key;
        if (typeof ak === "string" && ak !== "" && ak !== KEYCHAIN_SENTINEL) {
          roles[k] = ak;
        }
      }
    }
  }
  return { global, roles };
}

export function useSettings() {
  const [settings, setSettings] = useState<AppSettings>(() => {
    try {
      const stored = localStorage.getItem(STORAGE_KEY);
      return stored ? sanitizeStored(JSON.parse(stored)) : defaultSettings;
    } catch {
      return defaultSettings;
    }
  });
  const [showSettings, setShowSettings] = useState(false);
  /** 钥匙串就绪前输入框禁用（防用户在回填前覆盖内存空值） */
  const [secretsReady, setSecretsReady] = useState(false);
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  /** F1：未验证进钥匙串的明文（account → 明文）。写出时原样保留，绝不哨兵化。 */
  // 同步初始化为 localStorage 中**全部**明文：迁移验证完成前的任何写出都不得把它们哨兵化
  const plaintextHold = useRef<PlaintextHold>(null as unknown as PlaintextHold);
  if (plaintextHold.current === null) {
    try {
      const raw0 = localStorage.getItem(STORAGE_KEY);
      plaintextHold.current = raw0 ? extractPlaintextKeys(JSON.parse(raw0)) : { roles: {} };
    } catch {
      plaintextHold.current = { roles: {} };
    }
    if (plaintextHold.current.global === undefined) delete plaintextHold.current.global;
  }
  /** F1/F5：迁移失败原因（account → 原因），设置界面就地显示 */
  const [migrationErrors, setMigrationErrors] = useState<Record<string, string>>({});

  /** 基础 updateSettings（只更新内存 + 本地非敏感字段；密钥同步由 useSettingsWithSecrets 包一层） */
  const updateSettings = useCallback((partial: Partial<AppSettings>) => {
    setSettings((prev) => {
      const next = { ...prev, ...partial };
      persistNonSecrets(next, plaintextHold.current);
      return next;
    });
  }, []);

  /** 启动迁移 + 钥匙串回填（一次性） */
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        // 1. 迁移：localStorage 明文 → 钥匙串。
        // F1（P0 审计）：明文只有在**钥匙串读回值 = 明文**时才可清除。旧规则凭 MIGRATED_KEY 标记
        // 或 keychain_set 未抛错就判定安全——标记残留 + 钥匙串被清空/不同时，回填结束的脱敏写出
        // 会把唯一一份明文哨兵化（Key 丢失）。现逐 account：读回相等 → 安全；否则写入并读回校验；
        // 仍不相等/出错 → 保留明文（hold）、内存用明文继续可用，并把原因报给设置界面。
        const plain = { global: plaintextHold.current.global, roles: { ...plaintextHold.current.roles } };
        const pending: Array<[string, string]> = [];
        if (plain.global) pending.push(["global", plain.global]);
        for (const [role, secret] of Object.entries(plain.roles)) pending.push([`role:${role}`, secret]);
        const failures: Record<string, string> = {};
        for (const [account, secret] of pending) {
          const reason = await ensureInKeychain(account, secret);
          if (reason === null) {
            // 读回校验通过 → 释放（之后写出才允许哨兵化）
            if (account === "global") delete plaintextHold.current.global;
            else delete plaintextHold.current.roles[account.slice("role:".length)];
          } else {
            failures[account] = reason;
          }
        }
        if (pending.length > 0 && Object.keys(failures).length === 0) {
          try { localStorage.setItem(MIGRATED_KEY, "1"); } catch { /* 忽略 */ }
        }
        if (!cancelled && Object.keys(failures).length > 0) setMigrationErrors(failures);
          // 2. 回填：钥匙串 → 内存（全局 + 各角色）
          const next: AppSettings = { ...settingsRef.current, roleOverrides: { ...settingsRef.current.roleOverrides } };
          let changed = false;
          // 未能进钥匙串的明文：内存直接用明文（Key 仍可用），不再去钥匙串回填它
          const hold = plaintextHold.current;
          if (hold.global) { next.apiKey = hold.global; changed = true; }
          for (const [role, secret] of Object.entries(hold.roles)) {
            const ro = next.roleOverrides as Record<string, RoleApiOverride>;
            ro[role] = { ...(ro[role] ?? {}), api_key: secret };
            changed = true;
          }
          if (!hold.global && (next.apiKey === KEYCHAIN_SENTINEL || next.apiKey === "")) {
            try {
              const pw = await invoke<string | null>("keychain_get", { account: "global" });
              if (pw) { next.apiKey = pw; changed = true; }
              else if (next.apiKey === KEYCHAIN_SENTINEL) { next.apiKey = ""; changed = true; }
            } catch (e) {
              console.error("[keychain] global 读取失败:", e);
            }
          }
          if (next.roleOverrides) {
            for (const role of Object.keys(next.roleOverrides)) {
              const entry = (next.roleOverrides as Record<string, RoleApiOverride>)[role];
              if (entry?.api_key === KEYCHAIN_SENTINEL && !(role in hold.roles)) {
                try {
                  const pw = await invoke<string | null>("keychain_get", { account: `role:${role}` });
                  if (pw) { entry.api_key = pw; changed = true; }
                  else { entry.api_key = undefined; changed = true; }
                } catch (e) {
                  console.error(`[keychain] role:${role} 读取失败:`, e);
                }
              }
            }
          }
          if (!cancelled) {
            if (changed) setSettings(next);
            // 脱敏写出：hold 中的明文原样保留（未验证进钥匙串的绝不哨兵化）
            persistNonSecrets(next, plaintextHold.current);
            setSecretsReady(true);
          }
        } catch (e) {
          if (!cancelled) {
            console.error("[keychain] 启动回填整体失败:", e);
            setSecretsReady(true);
          }
        }
    })();
    return () => { cancelled = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** 用户之后成功同步了某 account → 该明文不再需要保留 */
  const releaseHold = useCallback((account: string) => {
    const h = plaintextHold.current;
    if (account === "global") delete h.global;
    else delete h.roles[account.slice("role:".length)];
    setMigrationErrors((prev) => {
      if (!(account in prev)) return prev;
      const n = { ...prev };
      delete n[account];
      return n;
    });
  }, []);

  return { settings, updateSettings, showSettings, setShowSettings, secretsReady, migrationErrors, releaseHold, plaintextHold };
}

/** 持久化非敏感字段——apiKey/api_key 写哨兵（真实值只在钥匙串）。
 * 唯一写出口统一过 sanitizeStored：读写同一清洗函数，运行期输入的越界值
 * （如 max_tokens 32000）不再原样落盘，后端不会在生成时整请求拒绝（F-1 根治）。
 * export 供单测（唯一写出口的清洗语义必须钉死）。 */
export function persistNonSecrets(s: AppSettings, hold?: PlaintextHold) {
  try {
    const scrubbed: AppSettings = {
      ...s,
      apiKey: s.apiKey ? KEYCHAIN_SENTINEL : "",
      roleOverrides: Object.fromEntries(
        Object.entries(s.roleOverrides ?? {}).map(([k, v]) => [
          k,
          { ...v, api_key: v.api_key ? KEYCHAIN_SENTINEL : undefined },
        ]),
      ),
    };
    // hold 中的明文要原样落盘：sanitizeStored 只保留哨兵，故清洗后再写回明文
    const out = sanitizeStored(scrubbed) as AppSettings;
    if (hold?.global) out.apiKey = hold.global;
    for (const [k, secret] of Object.entries(hold?.roles ?? {})) {
      out.roleOverrides = { ...out.roleOverrides, [k]: { ...(out.roleOverrides?.[k as keyof typeof out.roleOverrides] ?? {}), api_key: secret } };
    }
    localStorage.setItem(STORAGE_KEY, JSON.stringify(out));

  } catch { /* 配额等失败静默（与旧行为一致） */ }
}

/** 全局 Key 留空 = 沿用（AINA 参考 SettingsPage.tsx:383：留空 → undefined → 后端沿用旧值）。
 * 旧规则空值直接 keychain_delete——输入框受控值在清空/重输间隙即毁掉真 key（数据丢失回路根因）。
 * 全局 Key 留空永远不动钥匙串（防输入间隙误删）；显式清除走 clearGlobalApiKey（P1-12 / D-04）。 */
async function withRestoredGlobalKey(partial: Partial<AppSettings>): Promise<Partial<AppSettings>> {
  if (partial.apiKey === undefined || partial.apiKey) return partial;
  try {
    const pw = await invoke<string | null>("keychain_get", { account: "global" });
    if (pw) return { ...partial, apiKey: pw };
  } catch (e) {
    console.error("[keychain] global 回读失败:", e);
  }
  return partial;
}

/** 带密钥同步的设置更新（供 App 层调用处替换 updateSettings 用——本文件默认导出保持兼容） */
export function useSettingsWithSecrets() {
  const base = useSettings();
  /** 防抖定时器表（按 account 分键）：输入框每按键都触发同步，不防抖会把按键中间态
   * （如 "ak"、"ak-1"）写进钥匙串，中途退出即留下残缺 key */
  const syncTimers = useRef<Record<string, ReturnType<typeof setTimeout>>>({});
  /** 防抖窗口内每个 account 的最终待写值（null=删除）。timer fire 或 flush 后移除。 */
  const pendingSecrets = useRef<Record<string, string | null>>({});
  /** 立即执行单个 account 的同步（timer fire 与 flush 共用同一出口，避免双写分叉） */
  /** P0-10 残留：同步失败（如 Key 格式被后端拒绝）按 account 记录，设置界面就地显示；成功即清除。
   * 旧规则只 console.error——用户看到输入框有值，重启后 Key 消失，无任何提示。 */
  const [secretErrors, setSecretErrors] = useState<Record<string, string>>({});
  const { releaseHold } = base;
  const invokeSync = useCallback((account: string, secret: string | null) => {
    const op = secret
      ? invoke("keychain_set", { account, secret })
      : invoke("keychain_delete", { account });
    op.then(
      () => (releaseHold(account), setSecretErrors((prev) => {
        if (!(account in prev)) return prev;
        const next = { ...prev };
        delete next[account];
        return next;
      })),
      (e) => {
        console.error(`[keychain] ${account} 同步失败:`, e);
        setSecretErrors((prev) => ({ ...prev, [account]: secretErrorMessage(e) }));
      },
    );
  }, [releaseHold]);
  /** 防抖窗口内退出/卸载时 pending 同步会随 timer 一起死——最终值丢失（与"中间态写入"同族缺口）。
   * flush：把所有 pending 的最终值立即落盘（卸载 cleanup 调用）。 */
  const flushPendingSyncs = useCallback(() => {
    const timers = syncTimers.current;
    const pendings = pendingSecrets.current;
    for (const account of Object.keys(timers)) {
      clearTimeout(timers[account]);
      const secret = pendings[account];
      delete pendings[account];
      if (secret !== undefined) invokeSync(account, secret);
    }
    syncTimers.current = {};
  }, [invokeSync]);
  /** 防抖同步：停顿 800ms 后才落钥匙串；secret 为 null = 删除（仅角色级留空语义使用） */
  const scheduleSync = useCallback((account: string, secret: string | null) => {
    pendingSecrets.current[account] = secret;
    clearTimeout(syncTimers.current[account]);
    syncTimers.current[account] = setTimeout(() => {
      delete pendingSecrets.current[account];
      invokeSync(account, secret);
    }, 800);
  }, [invokeSync]);
  /** 卸载 flush：pending 的最终值立即落钥匙串（退出竞态闭合） */
  useEffect(() => {
    return () => { flushPendingSyncs(); };
  }, [flushPendingSyncs]);
  const updateSettings = useCallback(async (partial: Partial<AppSettings>) => {
    partial = await withRestoredGlobalKey(partial);
    if (partial.apiKey !== undefined && partial.apiKey) {
      scheduleSync("global", partial.apiKey);
    }
    const ro = partial.roleOverrides;
    if (ro) {
      for (const [role, entry] of Object.entries(ro)) {
        if (entry && typeof entry === "object" && "api_key" in entry) {
          // 角色级留空 = 清除覆盖、继承全局（UI 明示"API Key（留空继承全局）"），删除语义保留
          const ak = (entry as RoleApiOverride).api_key;
          scheduleSync(`role:${role}`, ak || null);
        }
      }
    }
    base.updateSettings(partial);
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) {
        const cur = JSON.parse(raw) as AppSettings;
        persistNonSecrets({ ...cur, ...partial }, base.plaintextHold.current);
      }
    } catch { /* 忽略 */ }
  }, [base, scheduleSync]);
  // F1/F5：迁移失败原因与同步失败同一显示通道（同 account 以最新同步结果为准）
  const mergedErrors = useMemo(() => ({ ...base.migrationErrors, ...secretErrors }), [base.migrationErrors, secretErrors]);

  /** P1-12 / D-04：显式清除全局 Key（二次确认由 UI 负责）。删钥匙串 global + 清空内存。 */
  const clearGlobalApiKey = useCallback(async () => {
    // 取消进行中的防抖写，避免清完又被 pending 写回
    if (syncTimers.current.global) {
      clearTimeout(syncTimers.current.global);
      delete syncTimers.current.global;
    }
    delete pendingSecrets.current.global;
    try {
      await invoke("keychain_delete", { account: "global" });
      releaseHold("global");
      setSecretErrors((prev) => {
        if (!("global" in prev)) return prev;
        const next = { ...prev };
        delete next.global;
        return next;
      });
    } catch (e) {
      console.error("[keychain] global 清除失败:", e);
      setSecretErrors((prev) => ({ ...prev, global: secretErrorMessage(e) }));
      throw e;
    }
    base.updateSettings({ apiKey: "" });
  }, [base, releaseHold]);

  return { ...base, updateSettings, clearGlobalApiKey, secretErrors: mergedErrors };
}


/** 后端 AppError 序列化为 {kind, message}；其他形态兜底转字符串 */
export function secretErrorMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e && typeof (e as { message: unknown }).message === "string") {
    return (e as { message: string }).message;
  }
  return String(e);
}

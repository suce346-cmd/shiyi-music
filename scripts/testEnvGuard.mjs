// C-01（BLUEPRINT_v2 P0-5）测试环境守卫：`npm test` 前置（pretest）。
// 根因见 docs/qa/vitest-root-cause.md：
//   1) 工程位于 iCloud Drive 同步目录（macOS“桌面与文稿”同步）→ node_modules 读取经 fileproviderd，
//      并发 worker 冷启动时读延迟可达分钟级（实测 `cat` 报 Operation timed out），
//      vitest 报 “Failed to start forks worker / Timeout waiting for worker to respond”。
//   2) Node 版本低于依赖 engines（vitest 5 / jsdom 30 / undici）→ 运行期 API 缺失
//      （Node 20：webidl.util.markAsUncloneable is not a function）。
// 纯函数导出供单测；CLI 入口在文件末尾。
import { execFileSync } from "node:child_process";
import { dirname, resolve, sep } from "node:path";
import { homedir } from "node:os";

/** 与 package.json engines.node 同源（单测断言二者一致） */
export const NODE_RANGES = [
  [22, 22, 2, 23],
  [24, 15, 0, 25],
  [26, 0, 0, Infinity],
];
export const ENGINES_NODE = "^22.22.2 || ^24.15.0 || >=26.0.0";

/** @param {string} version 形如 "v22.23.3" */
export function nodeVersionOk(version) {
  const m = /^v?(\d+)\.(\d+)\.(\d+)/.exec(version);
  if (!m) return false;
  const [maj, min, pat] = m.slice(1).map(Number);
  return NODE_RANGES.some(([lo, lmin, lpat, hiExcl]) => {
    if (maj < lo || maj >= hiExcl) return false;
    if (maj > lo) return true;
    return min > lmin || (min === lmin && pat >= lpat);
  });
}

/**
 * 判断路径是否处于 iCloud Drive 管理下。
 * @param {string} dir 绝对路径
 * @param {(p: string) => boolean} hasFileProviderAttr 该目录是否带 com.apple.file-provider-domain-id 扩展属性
 * @param {string} home
 */
export function iCloudManagedAncestor(dir, hasFileProviderAttr, home) {
  const mobileDocs = resolve(home, "Library", "Mobile Documents") + sep;
  if ((resolve(dir) + sep).startsWith(mobileDocs)) return mobileDocs;
  let cur = resolve(dir);
  for (;;) {
    if (hasFileProviderAttr(cur)) return cur;
    const parent = dirname(cur);
    if (parent === cur) return null;
    cur = parent;
  }
}

function darwinHasFileProviderAttr(p) {
  try {
    execFileSync("xattr", ["-p", "com.apple.file-provider-domain-id", p], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

export function runGuard({ cwd, version, platform, env, home, hasAttr }) {
  const errors = [];
  if (!nodeVersionOk(version)) {
    errors.push(
      `Node ${version} 不满足 engines.node "${ENGINES_NODE}"（vitest 5 / jsdom 30 运行期 API 依赖）。` +
        `请切换版本（见 .nvmrc）：nvm use`,
    );
  }
  if (platform === "darwin" && env.SHIYI_ALLOW_ICLOUD_TESTS !== "1") {
    const hit = iCloudManagedAncestor(cwd, hasAttr, home);
    if (hit) {
      errors.push(
        `工程位于 iCloud Drive 同步目录（${hit}）。node_modules 读取经 iCloud 文件提供程序，` +
          `并发 worker 冷启动会超时，测试结果随同步状态随机红绿（C-01）。\n` +
          `  处置：把工程移出 iCloud 同步目录（如 ~/dev/shiyi-music），或关闭“桌面与文稿文件夹”同步。\n` +
          `  确需在此运行：SHIYI_ALLOW_ICLOUD_TESTS=1 npm test（结果不作为门禁依据）。`,
      );
    }
  }
  return errors;
}

const isCli = process.argv[1] && resolve(process.argv[1]) === resolve(new URL(import.meta.url).pathname);
if (isCli) {
  const errors = runGuard({
    cwd: process.cwd(),
    version: process.version,
    platform: process.platform,
    env: process.env,
    home: homedir(),
    hasAttr: darwinHasFileProviderAttr,
  });
  if (errors.length) {
    for (const e of errors) console.error(`[test-env-guard] ✗ ${e}`);
    process.exit(1);
  }
}

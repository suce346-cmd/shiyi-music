import { describe, it, expect } from "vitest";
import { dictKeys } from "../i18n";

/**
 * 文案键守护·第三层（运行期，2026-09-18 第十九批）。
 *
 * 前两层在 `src/i18n.ts` 已由类型系统锁定，本文件不重复：
 *   ① `MessageKey = keyof typeof zh`（键集合单源）；
 *   ② `en: Record<MessageKey, string>` + `t/tf(key: MessageKey)`（穷尽 + 引用，编译期）。
 *
 * 本文件负责静态类型照不到的那一半——**动态族**与**孤儿键**：
 *
 *   A. 动态族（`provider.${id}` / `settings.url.err.${issue}` / `settings.theme.${v}` …）：
 *      静态类型只能保证"取值域成员都已登记"，不能保证"字典里确实存在"。族的存在性在此
 *      逐前缀断言——前缀拼错（如 `providerX.${id}`）会立刻红灯，而不是在界面上把
 *      `providerX.xfyun` 原文显示给用户。
 *
 *   B. 孤儿键（旧规则清理）：字典里每个键都必须"有主"——要么在非测试源码中以**带引号
 *      字面量**出现（静态引用），要么落在某个**源码里真实存在的动态前缀**之下。
 *      两者皆无 = 已下线文案长期滞留成第二真源，必须删除。
 *
 * 扫描面是**声明式**的（整个 `src/**`，自动纳入新文件），不硬编码文件清单——
 * 与后端 `rules::PROSE_CARRIERS` 同一思路：硬编码清单会静默漏掉新增载体。
 *
 * **扫描面自身的假阴性/假阳性（第十九批实测修正）**：`import.meta.glob` 的键**相对本文件**，
 * 同目录文件形如 `./x.test.ts`（**不带 `/test/` 段**）——原判定 `!path.includes("/test/")`
 * 因此漏掉全部兄弟测试文件，后果两条：
 * ① 假红：测试里 `` `MODE_LABELS.${k}` `` 这类**非文案**模板串被当成动态键族；
 * ② **假阴性**（更危险）：一个只被测试文件引用的字典键会被判为"有主"——而"孤儿键清理"
 * 正是本文件 B 段要守的东西，假阴性等于该段整体失效。
 */

/** 是否为测试文件。`path` 是 glob 键（相对本文件）：同目录 = `./x.test.ts`，子目录 = `../hooks/x.ts`。
 *  三条判据覆盖本项目可能出现的全部形态：同目录兄弟文件、`test/tests/__tests__` 目录、
 *  `*.test.*` / `*.spec.*` 文件名。 */
const isTestFile = (path: string) =>
  path.startsWith("./") ||
  /(^|\/)(test|tests|__tests__)\//.test(path) ||
  /\.(test|spec)\.[jt]sx?$/.test(path);

/** 非测试源码全文（`src/**`，排除测试文件与字典自身）。
 *  用 `import.meta.glob` 而非逐文件 `?raw`：新增源文件自动入网，无需改本文件。 */
const SOURCES: Record<string, string> = import.meta.glob("../**/*.{ts,tsx}", {
  query: "?raw",
  import: "default",
  eager: true,
});

const NON_TEST_SOURCES = Object.entries(SOURCES).filter(
  ([path]) => !isTestFile(path) && !path.endsWith("/i18n.ts"),
);

const CONCATENATED = NON_TEST_SOURCES.map(([, text]) => text).join("\n");

/** 字典键的合法形态：**首段以小写字母开头**、点分段、段内可含 camelCase（如 `gate.autoOff`）。
 *  既是良构断言，也是下面"跳过首段大写前缀"的**前提**：字典键首段必为小写字母 ⇒ 任何首段大写的
 *  模板串都不可能匹配到字典键 ⇒ 跳过它**不会漏掉**任何真实动态族（可证明，不是经验判断）。
 *  前提一旦不成立，`dynamicPrefixes` 自动停用跳过（fail-safe：宁可多报，不可静默漏网）。 */
const KEY_SHAPE = /^[a-z][A-Za-z0-9_]*(\.[A-Za-z0-9_]+)*$/;

/** 源码里真实出现的动态键前缀。两种写法都收：
 *  ① 模板串 `` `prefix.${expr}` ``（本项目现役写法）；
 *  ② 字符串拼接 `"prefix." + expr`（防改用拼接后动态族整体漏出扫描面）。
 *  前缀必须**含点且以点结尾**——否则会把 `` `u${x}` `` 这类与文案无关的模板串当族。 */
function dynamicPrefixes(): Set<string> {
  const out = new Set<string>();
  const canSkipUpper = dictKeys("zh").every((k) => KEY_SHAPE.test(k));
  for (const [, text] of NON_TEST_SOURCES) {
    for (const m of text.matchAll(/`([A-Za-z0-9_]+(?:\.[A-Za-z0-9_]+)*\.)\$\{/g)) out.add(m[1]);
    for (const m of text.matchAll(/"([A-Za-z0-9_]+(?:\.[A-Za-z0-9_]+)*\.)"\s*\+/g)) out.add(m[1]);
  }
  if (canSkipUpper) {
    // 首段大写 = 对**常量对象**取字段（`MODE_LABELS.${k}`），不是文案键族
    for (const p of [...out]) if (!/^[a-z0-9_]/.test(p)) out.delete(p);
  }
  return out;
}

describe("i18n 键守护·动态族与孤儿键（第十九批）", () => {
  it("扫描面自证（glob/过滤失效时本文件会静默空转——故先自证）", () => {
    expect(NON_TEST_SOURCES.length).toBeGreaterThan(5);
    expect(CONCATENATED).toContain("MessageKey");
    // 反面自证：不得有任何测试文件混入（混入即孤儿键检查假阴性）
    const leaked = NON_TEST_SOURCES.map(([p]) => p).filter(isTestFile);
    expect(leaked, `测试文件混入扫描面：${leaked.join(", ")}`).toEqual([]);
    // 目录级覆盖（不点名具体文件，避免改名即失效的第二真源）
    expect(NON_TEST_SOURCES.some(([p]) => p.startsWith("../components/"))).toBe(true);
    expect(NON_TEST_SOURCES.some(([p]) => p.startsWith("../hooks/"))).toBe(true);
  });

  it("字典键形态：首段小写字母（跳过首段大写前缀的前提）", () => {
    const bad = dictKeys("zh").filter((k) => !KEY_SHAPE.test(k));
    expect(bad, "以下键不满足首段小写字母——`dynamicPrefixes` 的跳过判定前提不再成立").toEqual([]);
  });

  it("动态族前缀必须解析到真实键（前缀拼错/族被改名即红灯）", () => {
    const keys = dictKeys("zh");
    const prefixes = [...dynamicPrefixes()];
    // 现役动态族（第十九批核实）：任一消失都说明写法变了，需同步更新本断言
    for (const required of ["provider.", "settings.url.err.", "settings.theme."]) {
      expect(prefixes, `动态族 ${required} 未在源码中发现——若已改写请同步本测试`).toContain(required);
    }
    for (const p of prefixes) {
      expect(
        keys.some((k) => k.startsWith(p)),
        `动态前缀 \`${p}$\{…}\` 在字典里没有任何键——界面上会显示键原文`,
      ).toBe(true);
    }
  });

  it("孤儿键检查：每个键都必须有主（静态字面量引用，或落在动态前缀之下）", () => {
    const keys = dictKeys("zh");
    const prefixes = [...dynamicPrefixes()];
    const orphans = keys.filter((k) => {
      if (CONCATENATED.includes(`"${k}"`)) return false; // 静态引用（带引号字面量）
      return !prefixes.some((p) => k.startsWith(p)); // 动态族成员
    });
    expect(
      orphans,
      `以下键既无静态引用、也不属于任何动态族——已下线文案应删除（勿留第二真源）：\n${orphans.join("\n")}`,
    ).toEqual([]);
  });

  it("中英字典键集合逐位一致（运行期双保险；编译期由 Record<MessageKey, string> 穷尽锁定）", () => {
    expect(dictKeys("en")).toEqual(dictKeys("zh"));
  });
});

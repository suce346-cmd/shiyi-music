/** URL **格式层**校验（#27-a 配套）——镜像后端 `models::validate_url` 的**非 DNS 层**。
 *
 *  为什么前端只做格式层，而不是把后端那套整个搬过来：
 *  - DNS 解析在 WebView 里不可靠（无同步解析、可能被代理/DNS over HTTPS 改写），而 SSRF 判定的
 *    关键在于「域名**最终**解析到哪个 IP」——只有真正发请求的一侧（Rust）拿得到，
 *    故 `validate_url` 在解析失败时 fail-closed；前端**不重复**这一层，避免两侧口径分裂。
 *  - 前端的职责是「坏输入在写进配置的那一刻就被拦下」，不必等到请求发出才报错。
 *  - 两层共用同一套拒绝项与区间（协议 / userinfo / localhost / 字面 IP 的内网判定），
 *    与后端 `validate_url` 逐条对应——上游产出与提示、下游准入由此保持同一套口径，
 *    不出现"前端放行、后端拒绝"的落差。
 *
 *  后端对应项（`src-tauri/src/models/mod.rs::validate_url` + `is_forbidden_ip`）：
 *    协议 ∈ {http, https} / username|password 非空即拒 / localhost 与 *.localhost 拒 /
 *    逐 IP：未指定·环回·私有·链路本地·组播·0/8·CGNAT·文档段×3·基准段·广播，
 *    IPv6 环回/未指定/唯一本地/链路本地/组播，以及 IPv4-mapped 拆壳后按 IPv4 判定。
 */

/** 拒绝原因（i18n key = `settings.url.err.${issue}`；null = 格式层通过） */
export type UrlIssue = "empty" | "parse" | "scheme" | "userinfo" | "localhost" | "private_ip";

/** 内网/保留 IPv4 判定——**逐条对应**后端 `is_forbidden_ip` 的 V4 分支
 *  （`is_unspecified`/`is_loopback`/`is_private`/`is_link_local`/`is_multicast` 的展开写法）。
 *  刻意不合并区间（如写成 `a >= 224`）：宽于后端即为口径分裂，宁可逐条照抄。 */
function isForbiddenV4(o: number[]): boolean {
  const [a, b, c, d] = o;
  return (
    a === 0 || // 0.0.0.0/8（含未指定 0.0.0.0）
    a === 127 || // 127.0.0.0/8 环回
    a === 10 || // 10/8 私有
    (a === 172 && b >= 16 && b <= 31) || // 172.16/12 私有
    (a === 192 && b === 168) || // 192.168/16 私有
    (a === 169 && b === 254) || // 169.254/16 链路本地（含云元数据 169.254.169.254）
    (a >= 224 && a <= 239) || // 224/4 组播
    (a === 100 && b >= 64 && b <= 127) || // 100.64/10 CGNAT 共享段
    (a === 192 && b === 0 && c === 2) || // 192.0.2/24 文档段
    (a === 198 && b === 51 && c === 100) || // 198.51.100/24 文档段
    (a === 203 && b === 0 && c === 113) || // 203.0.113/24 文档段
    (a === 198 && (b === 18 || b === 19)) || // 198.18/15 基准测试段
    (a === 255 && b === 255 && c === 255 && d === 255) // 广播
  );
}

/** IPv6 段展开。URL 解析器已把 `::ffff:127.0.0.1` 归一成 `::ffff:7f00:1`，
 *  故这里只需处理十六进制段（点分内嵌不会出现）。展开失败返回 null。 */
function parseIpv6Segments(host: string): number[] | null {
  const parts = host.split("::");
  if (parts.length > 2) return null; // 多个 "::"
  const compressed = parts.length === 2;
  const head = parts[0] ? parts[0].split(":") : [];
  const tail = compressed && parts[1] ? parts[1].split(":") : [];
  const missing = 8 - head.length - tail.length;
  if (missing < 0) return null;
  if (!compressed && missing !== 0) return null;
  const segs = [...head, ...(compressed ? new Array<string>(missing).fill("0") : []), ...tail];
  if (segs.length !== 8) return null;
  const nums = segs.map((s) => (/^[0-9a-f]{1,4}$/.test(s) ? parseInt(s, 16) : -1));
  return nums.includes(-1) ? null : nums;
}

/** 内网/保留 IPv6 判定——对应后端 `is_forbidden_ip` 的 V6 分支（含 IPv4-mapped 拆壳） */
function isForbiddenV6(seg: number[]): boolean {
  // IPv4-mapped（::ffff:a.b.c.d）：拆壳按 IPv4 判，与后端 to_ipv4_mapped 同序
  if (seg[0] === 0 && seg[1] === 0 && seg[2] === 0 && seg[3] === 0 && seg[4] === 0 && seg[5] === 0xffff) {
    return isForbiddenV4([seg[6] >> 8, seg[6] & 0xff, seg[7] >> 8, seg[7] & 0xff]);
  }
  const allZero = seg.every((s) => s === 0);
  const loopback = seg.slice(0, 7).every((s) => s === 0) && seg[7] === 1;
  return (
    allZero || // :: 未指定
    loopback || // ::1
    (seg[0] & 0xfe00) === 0xfc00 || // fc00::/7 唯一本地
    (seg[0] & 0xffc0) === 0xfe80 || // fe80::/10 链路本地
    (seg[0] & 0xff00) === 0xff00 // ff00::/8 组播
  );
}

/** 格式层校验：返回拒绝原因，null = 通过（普通域名一律通过——DNS 层交后端）。
 *
 *  用 WHATWG `URL` 解析而非正则切串：它与后端 `reqwest::Url` 同源，会把
 *  `http://2130706433/`、`http://0x7f.0.0.1/`、IPv6 压缩式等**先归一**再交给我们判定，
 *  否则"十进制 IP 绕过前端"这类变形会直接漏过去。 */
export function formatUrlIssue(raw: string): UrlIssue | null {
  const s = raw.trim();
  if (!s) return "empty";
  let url: URL;
  try {
    url = new URL(s);
  } catch {
    return "parse";
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return "scheme";
  if (url.username !== "" || url.password !== "") return "userinfo";
  const host = url.hostname.replace(/^\[|\]$/g, "").toLowerCase();
  if (!host) return "parse";
  if (host === "localhost" || host.endsWith(".localhost")) return "localhost";
  const v4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(host);
  if (v4) {
    const oct = v4.slice(1).map(Number);
    if (oct.every((n) => n <= 255)) return isForbiddenV4(oct) ? "private_ip" : null;
  }
  if (host.includes(":")) {
    const seg = parseIpv6Segments(host);
    if (seg && isForbiddenV6(seg)) return "private_ip";
  }
  return null;
}

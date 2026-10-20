import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// C-01（P0-5）：根因是环境（iCloud 同步目录 I/O、Node 版本），不是并发——不设 maxWorkers 掩盖。
// 守卫：scripts/testEnvGuard.mjs（pretest）。详见 docs/qa/vitest-root-cause.md。
// 显式固定：默认 node 环境（DOM 测试逐文件 `// @vitest-environment jsdom` 声明），pool=forks（vitest 5 默认，写明防漂移）。
export default defineConfig({
  plugins: [react()],
  test: {
    environment: "node",
    pool: "forks",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});

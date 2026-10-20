# 审计交付物索引（2026-10-19）

| 文件 | 说明 |
|---|---|
| `BLUEPRINT_v2.md` | 执行蓝图 v2（与仓库同步；P0/P1 步骤与规则定义） |
| `REPORT.md` | 主报告（Markdown 源） |
| `REPORT.html` | 主报告（自包含样式，适合浏览器阅读/打印） |
| `REPORT.pdf` | 主报告（26 页，A4 打印版） |
| `build.py` | 由 `REPORT.md` 重新生成 `REPORT.html`（`python build.py`） |
| `repro/01-keychain-test-destroys-key.sh` | 复现 S0：单测删除真实钥匙串条目（有防二次伤害守卫） |
| `repro/02-envelope-fallback-rate.py` | 统计信封降级率（只读） |
| `repro/03-mode-a-pass-rate.py` | 统计分模式硬校验通过率（只读） |
| `repro/04-test-suite-concurrency.sh` | 复现默认并发失败 / 限并发通过（只读，约 2.5 分钟） |
| `repro/05-suno-scenario-leak.py` | 统计 suno_rules 未识别场景（只读） |
| `repro/qa_replay.rs` | 40 例历史输出回放测试（复制到 `src-tauri/tests/` 后运行） |

> 审计未修改任何源码；`repro/` 只读脚本可安全重跑。`01` 会写删一个哑值，若检测到真实 `global` 条目会拒绝执行。

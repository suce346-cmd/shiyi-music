# P3 扫仓 + P0–P1 独立复审（2026-10-20，Asia/Shanghai）

对照：`BLUEPRINT_v2.md`。范围：代码与文档一致性、残留旧规则、§5.1 退役进度。  
判定：在修复本轮发现的硬漂移后关闭本批；软缺口登记为 follow-up。

## 1. 本批已删 / 已改

| 项 | 动作 | 提交 |
|---|---|---|
| `output_lang` 三态 / `mix` / `wants_style_en` / `wants_all_en` / 阶段2 mix 注入 | 收敛 `LyricLang` zh\|en；IPC `"mix"` 反序列化失败；前端下拉/i18n/sanitize；守护 `no_mix_output_lang` | `1efb19e` |
| §3.7 Style 恒英文硬门缺失 | 新增 `plan/style_lang.rs`、空表 `style_cjk_allow.csv`、`validate` 挂载；守护独立于 output_lang | `55c81d5` |
| 未引用 `DOUYIN_V2_DURATION_SEC_*` | 删除（零调用方） | `55c81d5` |
| 「产出语言三态」过时注释 | orchestrator / knowledge 注释改两态 | `55c81d5` |
| Mac 用户数据 v1 | `history.json`（裸数组）与旧 `pipeline_checkpoint_*.json` 改名备份丢弃（非兼容迁移） | 本机操作 2026-10-20 20:42 |

## 2. 刻意保留（及理由）

| 资产 | 理由 |
|---|---|
| 事件 `PipelineEnvelope` / `run_id` | 与已退役的「方案信封 / enforce_envelope」不同概念；取消/插话/门控/流式隔离依赖 |
| `STYLE_PROMPT_MAX_CHARS` / `DESC_LINE_MAX_CHARS` / CHECKLIST_*（§5.1#6） | 仍为 `plan::validate` 与 prompt 插值单源，非死代码 |
| 旧名 `DOUYIN_*` 标量（时长 60–90、`HOOK_WINDOW`、`INTRO_SPAN` 等）（§5.1#7） | 仍注入 mode_d / 角色 prompt；与 `DOUYIN_V2_*` 硬门并存（见 §3 软漂移） |
| `validate_lyric_fill` / `style_prompt_length_issues` / `desc_line_length_issues` / `check_bpm_range` | IR 调用；§5.1#5 已证保留 |
| `docs/qa/results_v{3,4}_final.jsonl`、b02 / vitest 根因 / text-eval 冻结集 | 审计与 P1-13 对照资产；hygiene 防重复 |
| `coverage/` | 已 gitignore，非仓内资产 |
| no-legacy 守卫内拼接禁用词 | 防回流，不是生产路径 |

## 3. §5.1 退役进度（复审）

| # | 状态 |
|---|---|
| 1–5 信封 / 转写保真 / 文本终稿校验 | 已退役；`no_legacy_*` 绿 |
| 6 常量 STYLE/DESC/CHECKLIST | **保留**（生产单源） |
| 7 旧 DOUYIN_* → 仅 V2 | **部分**：V2 硬门在 `plan/douyin`；prompt 仍吃旧标量 |
| 8 RULE_REGISTRY | 执行者已改挂 `check_*` / `validate_douyin_v2`；自检绿 |
| 9 prompt 快照 | 本批未改 prompt 正文；快照测试绿 |
| 10 前端历史迁移块 | P1-10 已删 |
| 11 suno_rules 说明行/350 | 行仍描述现行门限（与 rules 常量一致），保留 |
| 12 用户 v1 历史/检查点 | Mac 已备份丢弃 |
| 13 无主 dead_code allow | `no_blanket_dead_code_allows` 绿 |
| 14 mix / wants_* | **本批完成** |

## 4. P0–P1 落地 vs 蓝图：发现与处置

| ID | 发现 | 处置 |
|---|---|---|
| A1 | §3.8 mix 仍在 IPC/前端/指令 | **已修** `1efb19e` |
| A2 | §3.7 `validate_style_english` / `style_lang.rs` 缺失 | **已修** `55c81d5`（allow 表空，待 P2-S12） |
| A3 | `validate_lang`（歌词语言 vs `mode_meta.lyrics_lang`）未实现 | **登记**：软门；误报风险高，不在本批硬拦；讨论层指令已按 `lyric_lang()` 二态 |
| A4 | Prompt `DOUYIN_DURATION` 60–90 与调研/V2 建议 45–75 文案不一致 | **登记**：硬门用 V2 clip/intro；成曲时长旧标量为软指引。全量迁 V2 属 #7 后续，避免本批改快照无探针 |
| A5 | `DOUYIN_INTRO_SPAN=7`（动态表）vs `DOUYIN_V2_INTRO_MAX=2`（硬门） | **非矛盾**：前者为能量表「前 N 秒」窗口文案，后者为 Intro 段 `target_seconds` 上限 |
| A6 | ResultPanel 仍有「说明行」能量摘除逻辑 | **保留**：兼容自由文本展示；定稿主路径已是 `render_suno` 四栏 |
| A7 | 事件名含 envelope | **保留**（见 §2） |

## 5. 门禁与同步

- 每批提交后 `bash scripts/gates.sh` 绿（§3.8、§3.7 各一次）。
- Mac：`df462b62…`，`git am` 同步见同期操作记录；App Support v1 已备份丢弃。

## 6. 结论（Verdict）

**P0–P1 相对 BLUEPRINT_v2：硬漂移（mix 三态、Style 英文硬门缺失）已关闭；P3 #1–5/#10/#12/#14 完成。**  
遗留软项：`validate_lang`、§5.1#6–#7 常量双轨（旧 DOUYIN 软指引 + V2 硬门）。不阻塞本批关闭；#7 全量迁 V2 建议在 P2 探针或单独 chore 批执行。


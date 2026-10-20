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
| 7 旧 DOUYIN_* → 仅 V2 | **完成**（第二批）：非 V2 名零残留，`no_legacy_douyin_v1_names` |
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
| A3 | `validate_lang` 未实现 | **已修**（第二批，见 §7） |
| A4 | Prompt 成曲 60–90 秒 vs §6.2 45–75 秒 | **已修**（第二批） |
| A5 | 动态表「Intro/前7秒」vs 硬门 Intro≤2 秒 | **已修**：统一引用 `${DOUYIN_V2_INTRO_MAX}`（复审改判：同一 Intro 概念两套数即矛盾） |
| A6 | ResultPanel 仍有「说明行」能量摘除逻辑 | **保留**：兼容自由文本展示；定稿主路径已是 `render_suno` 四栏 |
| A7 | 事件名含 envelope | **保留**（见 §2） |

## 5. 门禁与同步

- 每批提交后 `bash scripts/gates.sh` 绿（§3.8、§3.7 各一次）。
- Mac：`df462b62…`，`git am` 同步见同期操作记录；App Support v1 已备份丢弃。

## 6. 第二批（fix_soft_now，2026-10-20 21:xx Asia/Shanghai）

### 6.1 validate_lang
- 实现：`plan/validate.rs::validate_lang`，`validate()` 全模式调用；`plan_flow::stamp_douyin_scene` 盖写 `mode_meta.lyrics_lang = req.lyric_lang()`（模型不得自选）。
- 规则：en → 任一歌词行含 CJK 即 Hard（逐行 path）；zh → 全曲无中文行即 Hard（允许英文和声/拟声行）。只读 `sections[].lines`。
- 测试：`validate_lang_two_states`、`validate_lang_ignores_style`、`stamp_writes_lyrics_lang_from_request`；§3.7 `style_english_rule_independent_of_output_lang` 保持绿。
- 变异（`cargo test --lib validate_lang`）：M1 删 validate() 接线 → 红；M2 en 条件改 zh → 红；M3 zh 全英文判据失效 → 红。3/3 红，复原绿。

### 6.2 §5.1#7 DOUYIN V1→V2
| 旧 | 新（`plan::douyin`） | 值变化 |
|---|---|---|
| `DOUYIN_LINE_MAX_CHARS` / `${DOUYIN_LINE_MAX}` | `DOUYIN_V2_LINE_MAX_CHARS` / `${DOUYIN_V2_LINE_MAX}` | 12 不变 |
| `DOUYIN_BPM_MIN` | `DOUYIN_V2_BPM_MIN` | 90→80（场景带最低，测试锁） |
| `DOUYIN_DURATION_SEC_*` | `DOUYIN_V2_DURATION_SEC_*` | 60–90→45–75 |
| `DOUYIN_HOOK_WINDOW_SEC` | `DOUYIN_V2_HOOK_LAND_SEC` | 3→8 |
| `DOUYIN_INTRO_SPAN_SEC` | `DOUYIN_V2_INTRO_MAX_SEC` | 7→2 |
| `DOUYIN_HOOK_LOOP_MIN/MAX` | `DOUYIN_V2_HOOK_REPEAT_MIN` / `DOUYIN_V2_HOOK_LOOP_MAX` | 3–4 不变 |
| `DOUYIN_ENERGY_*`、`WEIRD/STYLE_*`、`ARC_FORBID_PHRASE`、`DYNAMIC_TABLE(_INDENTED)` | 同名 `DOUYIN_V2_*` | 不变（参数区间仍 provisional，待 P2） |
| `HOOK_MIN_COUNT`、`HOOK_REPEAT_MIN/MAX`、`VERSE_MAX_LINES`（D 专用） | 改为 V2 别名 | Hook 2→3、强化区间 2–4→3–4 |
- 知识表：`suno_rules.csv` `hook_first`（前 3 秒→Intro≤2 秒 + Hook 8 秒落地）、`bpm_match` douyin（90–170→80–170 + 场景带）。
- 快照：`mode_d.txt` 重生成（9 行，全部为上表数值）。
- 守护：`no_legacy_douyin_v1_names`（src/tests/knowledge/前端扫 `DOUYIN_` 非 `V2_` 前缀；注入一行即红，已实测）；`bpm_min_is_scene_floor`。

### 6.3 复审追加发现
| ID | 发现 | 处置 |
|---|---|---|
| B1 | `CHECKLIST_D`/primer「Hook≥2次」与 V2 硬门 ≥3 矛盾 | 已修（别名） |
| B2 | `bpm_match` douyin 下限 90 与 emotion 场景 80–100 矛盾（情绪场景合法 BPM 被提示词判违规） | 已修 |
| B3 | `hook_first`「前 3 秒必须出现 Hook」与 Hook 8 秒落地矛盾 | 已修 |
| B4 | 文本护栏字面量 `BPM≥90` 随值迁移 | 改为 80 |

## 7. 结论（Verdict）

**P0–P1 + P3 相对 BLUEPRINT_v2：无已知漂移、矛盾或旧规则残留。** §5.1 #1–#14 全部关闭（#6 常量为现行单源、#11 suno_rules 行描述现行门限，均非残留）。  
仅剩用户侧：`scripts/mac-verify.sh` 真模型回放 / smoke，与 §7 P2 Suno 探针（结果回填 `suno_profile` 与 `style_cjk_allow.csv`、D 参数区间）。

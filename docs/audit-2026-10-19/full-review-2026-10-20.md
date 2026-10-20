# 全量复审（2026-10-20）

基线：`3bedf9a`（用户口述 "e73bedf" 应为笔误；Mac 同内容 `a99896c`，本次同步时按盒子哈希对齐）。
时间均为 Asia/Shanghai（UTC+8）。凭据：真实模型配置只在 `/workspace/.secrets/shiyi.env`（600，仓库外），本文与提交中不含任何 Key。

## 1. 新规则落地表

| 编号 | 规则 | 定义点（单源） | 作用域 | 守卫测试 | 状态 |
|---|---|---|---|---|---|
| D1 | 自用不分发 | `docs/security-self-use.md` | 文档/发布 | — （无发布产物） | ✅ |
| D2 | 原生结构化输出 json_schema→json_object→none 探测 | `llm_caps.rs`（`probe_body`/`classify_probe_status`/**新** `classify_probe_body`）、`llm.rs::call_llm_structured` | 全部结构化阶段 | `probe_tiers_schema_object_none`、`probe_status_classes`、**新** `probe_body_content_aware`、`structured_wrappers_stripped_only_think_and_fence` | ✅（本次加固，见 §4 F-F） |
| D3 | 只做 Suno | `plan::render_suno`（唯一渲染器） | 渲染 | `no_legacy_envelope_symbols`；rg 零命中 Mureka/海绵/天音/`enum Platform` | ✅ |
| D4 | 旧格式一刀切 | §5 退役清单 | 全仓 | `no_legacy_envelope_symbols`、`no_legacy_text_terminal_validators` | ✅ |
| D5 | 旧历史/检查点不兼容 | `history.rs::HISTORY_SCHEMA_VERSION=2`、`checkpoint.rs` | 落盘 | history v2 单测 + **新** `ipc_commands::history_roundtrip_keeps_song_plan_and_rendered` | ✅ |
| D6 | 钥匙串只读，永不写 global/role:* | `keychain.rs` | 测试 | `keychain_tests_never_write_protected`、`cli_get_global_is_read_only` | ✅（本次所有测试零钥匙串写） |
| D7 | Suno 探针用户手动 | `suno_profile.rs`（事实源 research 文档） | 参数上限 | `suno_meta_non_empty`（新） | ⏳ 待用户手动探针 |
| D8/D10/D14 | 清垃圾、旧计划文档与 Store 图标删除 | — | 仓库 | `repo_hygiene::removed_junk_assets_stay_removed` | ✅ |
| D9 | 抖音 V2 重设计 | `plan::douyin::DOUYIN_V2_*` | D 模式 | `douyin_v2_happy_path_and_negatives`、`intro_and_hook_land_boundaries` | ✅ |
| D11 | 场景选择（推荐+用户选，默认 daily） | `douyin.rs::DouyinScene`、`SceneSelector.tsx`、`stamp_douyin_scene` | D 模式 | `douyin_scene_default_is_daily`、`sceneSelector.test.tsx`、**新** `uiControls` D 默认 daily、实网 D 断言 scene=dance | ✅ |
| D12 | Style 恒英文 | `plan::style_lang::validate_style_english` | 全模式 | `style_english_rule_independent_of_output_lang` | ✅ |
| D13 | LyricLang zh/en 两态 | `models::LyricLang`、`plan::validate::validate_lang` | 歌词 | `output_lang_rejects_mix`、`no_mix_output_lang`、`validate_lang_two_states` | ✅ |
| A-1 | P1-3/P1-4 合并 | 提交 83cbb99/872b2ff | — | — | ✅ |
| A-2 | clippy -D warnings | `scripts/gates.sh` 末步 | 全仓 | gates `CLIPPY_CLEAN` | ✅ |
| A-3 | P0 补丁 F1–F9 | 见 REPORT | — | run_id wire 锁 `runIdWire.test.ts` 等 | ✅ |
| 用户裁定 | 抖音 60–90s / Intro≤3s / Hook 10s 内落地 / Hook≥2（≤4） / BPM≥80 | `DOUYIN_V2_DURATION_*`、`INTRO_MAX`、`HOOK_LAND`、`HOOK_REPEAT_MIN/LOOP_MAX`、`BPM_MIN` | D 模式 | `intro_and_hook_land_boundaries`（含变异检查）| ✅ |
| 用户裁定 | 歌词仅 zh/en | 同 D13 | | | ✅ |
| 用户裁定 | 只做 Suno | 同 D3 | | | ✅ |

## 2. 旧规则处理（退役证据）

`rg` 全仓零命中（不含守卫字面量与本审计文档）：Mureka、海绵、天音、`enum Platform`、`PROMPT_PACK_IR`、`PLAN_ENVELOPE_CONTRACT`、`enforce_envelope`、`envelope_fallback`、`parse_plan_sections`、`PlanSections`、`check_transcription_fidelity`、`TRANSCRIPTION_CONTRACT`、`wants_style_en`/`wants_all_en`、`HISTORY_MIGRATED_KEY`、`"mix"` 输出语言、旧 `DOUYIN_*` 名。
仅出现在守卫/注释的：`validate_for_mode`、`validate_production`、`collect_hard_issues`、`param_range_issues`、`check_audio_influence`（守卫 `no_legacy_text_terminal_validators`）；`StoreLogo`、`tauri.svg`（守卫 `removed_junk_assets_stay_removed`）。
本次清理：`rules.rs` 两处注释仍把 `validate_for_mode`/`validate_production` 当现役执行者描述 → 改指 `plan::validate`。

## 3. 变更点清单（ba7d0d5..3bedf9a，29 提交）+ 本次

- **审计/文档**：adfa13d、fcbee50、8f0fdd8
- **结构化管线 P1-2/3/4**：8bb7882、a3e532a、399ca2e、872b2ff、7ab00b0、83cbb99、ca04090、9585727
- **受控词表/场景**：cda6342（vocal_mix）、4d6e61e（scenario）、6e478f9/87a79b5/2c9b510（抖音 V2 + 场景选择）
- **P1-8..13 功能**：c4c89b8（C 押韵建议）、3361cb1（文本层+Audio Influence）、e9d6aa6（历史/检查点 v2）、4a20dd7（四栏复制）、07b3732（lint/清 Key/用量）、1941e30（文本评估 Judge）
- **退役与收敛**：94440ba（文本终稿校验）、1efb19e（mix）、55c81d5（Style 英文）、4e730e4（validate_lang）、1f161b6（DOUYIN_* 迁 V2）、84dd6c5（用户数值）
- **工具**：3bedf9a（mac-verify.sh）
- **本次**：见 §4。

功能点：四模式圆桌（A 短词成曲 / B 灵感成曲 / C 逐行填词 / D 抖音）、结构化 SongPlan 定稿 + plan::validate 硬门 + render_suno、能力探测、轮间确认门、插话、取消、续跑（检查点）、退避播报、队列、历史（筛选/导出/删除/清空/再精修）、四栏复制、能量条、场景推荐、文本评估 Judge、多角色模型覆盖、提供商模板、钥匙串存 Key、日志/配置目录打开。

## 4. 逻辑审计：矛盾与修复

| # | 矛盾 / 缺陷 | 位置 | 修复 |
|---|---|---|---|
| F-A | 代理 fake-IP（198.18.0.0/15）DNS 下合法域名被 `validate_url` 判内网拒绝 → 实网全挂（Mac 跑 Clash 同险） | `models::validate_url` | 仅"域名解析结果"豁免 198.18/15；IP 字面量仍拒；`fake_ip_range_only_for_resolved_domains` + `docs/security-self-use.md` |
| F-B | `HistoryEntry.song_plan/rendered` 类型存在、文档称 RULE-HISTORY-V2，但**从不写入**；定稿 plan 被丢弃 | orchestrator `_final_plan`、App.tsx | 新事件 `final_plan{song_plan}`（wire 锁自动覆盖）；前端 `finalPlanRef` + `historyPlanFields`，生成/精修两路落盘 |
| F-C | 定稿未过显示"格式问题 N 条"（旧文本格式口径） | usePipeline.ts | 改"硬校验未通过 N 条" |
| F-D | 注释称现役执行者为已删函数 | rules.rs | 改指 plan::validate |
| F-E1 | `DESC_LINE_CONTRACT` 要求说明行末写中文"能量:X"、"禁英文 energy"；而 SongPlan 用 `energy` 字段、render_suno 输出 `energy N`——提示词与渲染器/Style 英文门互相矛盾 | rules.rs | 契约改为"能量只填 energy 字段，渲染器输出 energy N，勿写进文本字段" |
| F-E2 | `plan_energy_range` / `plan_bpm_value` 按文本扫描，生产输入却是**单行紧凑 SongPlan JSON**：能量区间被 schema_version/variety 等小数字拉偏，BPM 恒取不到 → 配器/知识注入过滤失真 | energy.rs、knowledge.rs | JSON 形态读 `sections[].energy` / `global.bpm`；单测 `song_plan_json_uses_section_energy_fields`、`plan_bpm_value_reads_song_plan_json` |
| F-E3 | 结果页能量条：render_suno 段头为单行 `[Verse 1 | …, energy 7]`，`TAG_RE` 只认 `[Verse 1]` → 所有能量条标签都是"开场" | ResultPanel.parseEnergy | 新 `HEAD_RE` 取同行标签；测试补 |
| F-F | 探测仅凭 HTTP 200 判支持（网关静默忽略 response_format 时误判）；`parse_structured` 不容忍 `<think>`/```json 外壳；推理字段无测试 | llm.rs、llm_caps.rs | 200 时看正文：非空且非 JSON → Rejected；空（reasoning 吃光 token）→ Supported；只剥 think/围栏两种外壳；`reasoning_fields_never_leak_into_content` |
| F-G | 实网无头测试 A/B/D 只断言非空，D 未开场景/Judge | tests/headless_modes.rs | 订阅 pipeline 事件：末次 audit_result.pass、无 degraded、final_plan 过 schema + plan::validate + render_suno；D 用 scene=dance + Judge 开并断言 text_eval |
| F-H | json_schema 档不在提示中带 schema；网关 200 但不强制 → 模型把 `positive_local` 等数组写成字符串 | llm.rs `structured_body` | json_schema/json_object 两档都注入 schema 文本；契约写明三字段为字符串数组；`structured_body_always_carries_schema_text` |
| F-I | strictify 把全部属性标 required；模型省略 Option/默认字段（persona、checked、changes、reason）→ Parse | llm.rs `parse_structured` | `fill_omitted_optionals`：只给**缺席**属性补位（可空→null，数组→[]，字符串→""），类型错误不修；对象后跟尾随文字只取首个 JSON 值；`omitted_optional_fields_filled_but_type_errors_kept` |
| F-J | 审改 JSON 偶发非法 → 角色意见直接作废（call_degraded） | plan_flow 审改阶段 | Parse 时同请求重发一次（无修复提示，D2 不变） |
| F-K | Style/段头超长：审核员 2 次修正不收敛（376>350、213>200） | validate / validator / plan_flow | 打回文案给出"须再删 ≥N 字符"与段头拼接组成；`AUDITOR_FIX_ATTEMPTS` 2→3 |
| F-L | Judge 在终稿后运行却沿用流水线预算并预留 120s → D 长链必 "可用 0ns" | orchestrator `maybe_run_text_eval`、text_eval | 独立旁路预算 `TEXT_EVAL_BUDGET`=180s、reserve=0 |
| F-M | Judge max_tokens 1024 被 reasoning 吃光 → content 空 | text_eval | `JUDGE_MAX_TOKENS`=4096 |
| F-N | 实网 C 断言对整包文本跑 lyric_fill（含 Title/Style/段头）必误报；预算 8 分钟四路并发不够 | tests/headless_modes.rs | 改在 final_plan 歌词行上比对；A/B/C 12 分钟、D 15 分钟（=生产） |
| 记录 | `get_suno_meta` 已注册 IPC 但前端无调用 | lib.rs | 保留（参数上限元数据，待 D7 探针后接 UI），列为遗留 |
| 记录 | HistoryPanel 筛选页签"全部"硬编码中文未走 i18n | HistoryPanel.tsx | 遗留（低） |

实网探测（curl，deepseek-v4.1-flash）：json_schema / json_object / 无 response_format 在 max_tokens 64 与 1024 下均 200 且正文为合法 JSON；message 含 `reasoning`、`reasoning_details` 字段 → 客户端只读 `content`，已加测试锁。

## 4.1 实网 A/B/C/D 回合记录（deepseek-v4.1-flash，经 amr-link 网关）

| 回合 | A | B | C | D | 主因 → 修复 |
|---|---|---|---|---|---|
| 1 | ✗ | ✗ | ✗ | ✗ | 数组写成字符串（F-H）；C 断言误报、B 预算（F-N） |
| 2 | ✗ | ✗ | ✗ | ✗ | 省略可空字段（F-I）；C Style/段头超长 |
| 3 | ✗ | ✗ | ✓ | ✗ | 审改非法 JSON/尾随文字（F-I/F-J）；D 预算 |
| 4 | ✓ | ✗ | ✗ | ✗ | 超长不收敛（F-K）；Judge 预算（F-L） |
| 5 | ✓ | ✓ | ✓ | ✗ | Judge 空正文（F-M） |
| 6 | — | — | — | 见最终报告 | |

“讨论轮次上限强制收敛”是设计内软降级，测试单独打印不判失败；其余降级一律判失败。

## 5. 按钮 → 数据流矩阵

| 控件 | TS 状态/请求构建 | IPC | Rust 处理 | 存储/事件 | 测试 |
|---|---|---|---|---|---|
| 模式切换 ×4 | ModeSelector → App mode | — | — | settings | uiControls ModeSelector、modeDomain |
| 生成 | InputPanel.handleSubmit → buildPipelineRequest | pipeline_generate | validate_request → run_pipeline | pipeline 事件流 → 历史 | uiControls InputPanel(A/C/D)、buildPipelineRequest、实网 headless A/B/C/D |
| 场景 chip | SceneSelector → douyinScene | get_douyin_meta / suggest_douyin_scenes | douyin.rs | 请求 douyin_scene → stamp_douyin_scene | sceneSelector、ipc_commands、实网 D |
| Judge 开关 | settings.textEvalJudge | （随请求）| text_eval | text_eval 事件 → 发言 | text_eval.rs、实网 D |
| 停止 | StatusIndicator.onCancel → usePipeline.cancel | cancel_pipeline | cancel.rs 按 run_id | 取消错误 | uiControls、cancel.rs 单测 |
| 重试 / 从上次继续 | onRetry / resume | pipeline_generate / pipeline_resume | checkpoint | — | uiControls、ipc_commands resume |
| 插话 | StatusIndicator.sendInterject | interject_feedback | validate_feedback → interject 队列 | 轮边界消费 | uiControls、ipc_commands |
| 轮间门 继续/定稿 | RoundGateBar | round_gate_decide | gate.rs | round_gate_resolved | roundGateBar、usePipelineGate、ipc_commands |
| 精修 | ResultPanel refine → usePipeline.refine | pipeline_refine | validate_request(feedback) | 更新历史条目（含 song_plan/rendered） | refineTargets、historyRefine |
| 四栏复制 | ResultPanel | — | — | 剪贴板 | resultPanelActions/Fields |
| 历史 筛选/选中/删除/清空(二次确认)/关闭 | HistoryPanel → App | history_save/flush | history.rs v2 | history.json | uiControls HistoryPanel、ipc_commands roundtrip |
| 历史 导出 | exportEntry | history_export_text → dialog.save → fs.write | history.rs | 文件 | uiControls HistoryPanel |
| 队列 清空/移除/查看 | QueuePanel → useQueue | — | — | 内存 | uiControls QueuePanel、useQueue、queueItemResult |
| 设置 测试连接 | App testConnection | test_api → probe_structured_caps | llm.rs + validate_url | settings.structuredCaps | settingsWiring、structuredCaps、ipc_commands bad URL |
| Key 保存/清除 | useSettings | keychain_set/get/delete | keychain.rs | 钥匙串 | useSettingsSecrets（mock；真实写钥匙串禁止） |
| 提供商模板 / 模型下拉 | ProviderTemplates / SearchableSelect | — | — | settings | providerTemplates、searchableSelect |
| 打开日志/配置目录 | App | open_log_dir / open_config_dir | logdir.rs | 系统文件管理器 | 仅 GUI 实点 |
| 密钥同步失败提示 | SecretSyncError | keychain_get | | | secretSyncError |

## 6. 需 GUI 实点的遗留

打开日志/配置目录（调起 Finder）、拖拽 .txt/.lrc 导入、导出对话框真实路径、钥匙串真实写入（测试禁止）、窗口层 AppHandle<Wry> 命令（history_* 实盘、pipeline_generate/cancel 经 webview）、Suno 端粘贴实听（D7）。

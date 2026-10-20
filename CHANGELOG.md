# Changelog

## Unreleased

- **[P1-13] 文本评估 Judge（M-09/M-10）**：七维评分卡 + 圆桌同模型 Judge；设置 **文本评估 Judge**（`textEvalJudge`）默认关闭，仅开启时终稿后打分并发 `text_eval` 事件；失败 `text_eval_degraded` 不阻断。20 条冻结提示词、校准模板与歧义裁定见 `docs/qa/text-eval-*.md` / `p1-13-ambiguities.md`；`tests/text_eval.rs` 离线绿、在线需 `SHIYI_TEXT_EVAL=1`。

- **[§5.1#5] 退役文本终稿校验**：变异实验证明 `plan::validate` 覆盖原文本硬门后，删除 `validate_for_mode` / `validate_production` / 文本 `validate_douyin` / `param_range_issues` / `check_audio_influence` / `declared_minimal_sections`；保留 `validate_lyric_fill`、`style_prompt_length_issues`、`desc_line_length_issues`、`check_bpm_range` 供 IR 调用。RULE_REGISTRY 执行者改挂 `check_*` / `validate_douyin_v2` / `section_floor`；无头/矩阵改走 `plan::validate`；扩展 no-legacy 守卫。对照与变异表见 `docs/audit-2026-10-19/p3-retire-text-validators.md`。

- **[P1-12] D-01/D-04/D-05**：CI 加 clippy/ESLint/vitest coverage；设置面板「清除已保存的 Key」+ 二次确认；校验员审查首调 usage 先入账（重试失败不丢 tokens）。
- **[P1-11] 结果区四栏分块复制 + Settings 显示 Variety 0**：`parseSunoPackage` 按 Style/Exclude/Settings/Lyrics 锚点拆分；ResultPanel 分块复制按钮；Settings 条显式 Variety=0 徽章；vitest。
- **[P1-10] 历史/检查点 v2（RULE-HISTORY-V2 / B-03/C-09/D-03）**：`history.json` 改为 `{schema_version:2,entries}`；读到 v1 裸数组/非 2 版本直接备份丢弃；上限 200；`history_flush` + 前端串行写队列与关闭前 flush；删除 App 本地迁移块；检查点 schema=2 + TTL 7 天 + 启动清扫；HistoryEntry 增 `song_plan`/`rendered`。
- **[P1-9] 发音/Persona/系列文本层 + Audio Influence 可选（M-08/M-05/M-11/M-12）**：SongPlan 增 `pronunciation_hints`/`persona`/`series_context`（默认 null=关闭，不渲染）；`SunoSettings.reference_audio` 默认 false；为 true 时 A/B 允许 `audio_influence≠0`；校验清单/plan_field_contract/suno_rules/prompts 同步；schema 快照更新。
- **[P1-8] Mode C 韵脚咨询（C-03）**：`plan::validate::advise_rhyme` 用拼音韵母（十三辙近似）对照原词押韵位置；仅 `Severity::Warn`（多音字取首读，不打回）；`pinyin` crate；README 改为「字数硬校验、韵脚提示」。
- **[P1-7] D 模式 V2 + 场景选择**：`plan/douyin.rs`（DouyinScene/BPM 带/suggest_scenes/validate_douyin_v2）、`DOUYIN_V2_*` 常量；`mode_meta.douyin`；请求 `douyin_scene` 后端盖写；`douyin_scene_keywords.csv`；前端 `SceneSelector`；vocal_mix 增 M-06 phone-speaker friendly，D 硬门要求 M-01+M-06；Exclude 必含 long intro/slow build/fade out。
- **[补丁] 抖音叙事弧线专行（add_douyin_arc）**：新增 10 条 `style_arc_d_*`（scenario=douyin，Style Influence 与 A/B 弧线同值），供 D 叙事型回落在 suno_rules 内看到弧线引导；A/B/C 仍只吃 `style_arc_*`（normal）。职责与子集绑定同步；对照表见 `docs/qa/p1-6-scenario-reclass.md`。
- **[P1-6] Scenario 词表闭合（C-02，RULE-SCENARIO-VOCAB）**：`knowledge::Scenario` 登记 `all/douyin/normal`；`suno_rules.csv` 26 行改值（17 行未知值重分类 + 抖音/中文改 token，逐行表见 `docs/qa/p1-6-scenario-reclass.md`）；未知值 fail-closed（不注入 + 启动 error + 守护测试红）；审改角色 suno_rules 子集路径补过场景门（旧实现绕过）。mode_d 注入 27/40（旧 38/40）。
- **[残留清理] 定稿人设与前端标签**：`roles::auditor()` 人设重写为结构化 PlanIR 定稿人（输出完整 SongPlan JSON，规则全走 `${占位符}`），`produce_final_text` 改读该单源并按角色绑定注入 suno_rules；删除前端 `envelope_fallback`「格式降级」标签（旧历史不再兼容回放）；UI/README/i18n「格式输出」措辞改为定稿；守护测试扩至前端 TS/TSX 与旧端口措辞。
- **[P1-5] vocal_mix 受控词表（M-07）**：新增 `knowledge/vocal_mix.csv`（人声 4 + 混音 5，label_en 去否定写法）；`SongPlan.global.vocal/mix` 只填词表 id，`render_suno` 展开为英文标签；`plan::validate` 新增覆盖门（各类 ≥1、未知 id / 类别错位为硬问题；删表即跳过）；主持人阶段 0/汇总、制作人/校验员审改、校验员定稿注入词表，超 `VOCAL_MIX_INJECT_MAX_CHARS` 只注入 id+label_en；覆盖目录张数判据改为 `EMBEDDED_TABLE_COUNT`=9。
- **[P1-3/4 复审修复] 信封/转写保真全仓退役**：模式 prompt 输出节改为 SongPlan 字段映射（原文仍要求 `<<<NOTES>>>` 等四节文本，与结构化 schema 冲突）；删除 `envelope_spec`/`parse_plan_sections`/`envelope_defect_mode`/`plan_envelope_enabled`、`check_transcription_fidelity`、`TRANSCRIPTION_CONTRACT`、`auditor_format_prompt_mode_c`、`collect_hard_issues` 及 cfg(test) 旧辅助函数与对应测试；结构化调用补发 usage 与退避事件；refine 上一版非 SongPlan JSON 时回落阶段 0；守护测试扩至 `src/**` + prompt 快照。
- **[S2 修复] 结构化能力探测误判**：400 类响应仅当 body/code 明示与 `response_format`/`json_schema`/`json_object` 相关时才降档或记 `none`；缺模型/欠费等不再污染能力缓存。探测 `max_tokens` 提至 64。
- **[S2 修复] schema strictify 误删字段名**：只剥 schema 元键 `description`/`format`，不再误删 `properties` 里同名的字段定义。
- **[文档] 审计交付入库（P1-J 余）**：`docs/audit-2026-10-19/`（README/REPORT/BLUEPRINT_v2）与 `docs/research/`；BLUEPRINT 对齐 F1 读回校验、C-12=DNS、P0-3 验证列=`runIdWire`、§3.7 英文硬门在 validate（不写 schema description）、`LyricLang` 唯一定义点、§2.1 Interlude+minimal+lyrics_lang、P1-1 位置列去掉 validate/render。
- **[工程] gates.sh 调用 pretest 同源 `testEnvGuard.mjs`**；suno_profile 来源改为 research 文档（待探针），不再自引用蓝图。

- **[S0 修复] 测试不再删除用户真实 API Key（A-01）**：`keychain::tests::cli_round_trip` 旧实现以生产账户 `global` 做写→删往返，跑一次 `cargo test` 即删除用户 Key。现改用 `test:shiyi:{pid}:{nanos}` 隔离账户 + Drop 清理；新增 RULE-KEYCHAIN-RO 双层守护（测试构建下写/删 `global`/`role:*` 即 panic；静态扫描测试模块写/删实参）。生产行为不变。
- **[S2 修复] 续跑丢产出语言（C-04）**：`resume` 手写请求字面量漏传 `output_lang`，后端缺省 `zh`，en/mix 设置在续跑被静默忽略。现三路径（generate/refine/resume）统一由 `buildPipelineRequest(kind, …)` 单源组装，`PipelineRequest.output_lang` 改为必填类型（漏传即 TS 编译失败）；新增 `buildPipelineRequest.test.ts` 7 项。
- **[S2 修复] run_id 规则（C-05/C-06/D-06，RULE-RUN-ID）**：删除空 id 全局取消位（旧规则无复位点，无 run_id 时按“停止”后“测试连接”永远返回已取消）；`cancel_pipeline` 空 id 改为明确报错，`test_api` 用独立 id；`llm-chunk` 载荷增加 `run_id`，前端 `acceptStreamChunk` 只收当前 run 的流式片段（防取消/队列交错串流）；run 结束（含续跑）移除取消条目；run_id 解析单源 `resolve_run_id`；取消注册表只对活动 run 生效（`begin`/`finish`），结束后迟到的取消为 no-op、不留条目。
- **[S2 修复] SSE 跨 chunk UTF-8 截断（C-07）**：流式行缓冲改为字节级，按 `\n` 切完整行后再解码；中文等多字节字符被网络 chunk 切开时不再变成 U+FFFD（夹具覆盖全部切点）。
- **[S2 修复] vitest 不可复现根因（C-01）**：根因为工程位于 iCloud 同步目录（node_modules 读取经文件提供程序，worker 冷启动超时）与 Node 版本低于依赖 engines，非并发竞态（`--maxWorkers=2` 实测仍失败）。新增 `vitest.config.ts`（显式 node 环境/forks，不设 maxWorkers）、`engines`/`.nvmrc`、`pretest` 环境守卫 `scripts/testEnvGuard.mjs`，根因记录 `docs/qa/vitest-root-cause.md`。
- **[S1 修复] 迁移后 localStorage 残留明文 Key（C-10）**：启动回填只在内存有变化时写出，明文 apiKey 不触发回填 → 明文永久留存。现钥匙串持有密钥（迁移成功或此前已迁移）即无条件写出脱敏副本；迁移失败则不写出（修复潜在丢 Key：角色回填触发写出会把未入钥匙串的明文哨兵化）。新增 3 项单测。
- **[S3 修复] 大型矩阵测试路径与口径（C-11）**：结果目录改为工程相对 `target/large-test`（原写死 `/Users/hakayi/Desktop/...`，工程移动即失效）；测量改调生产终稿门 `collect_hard_issues`（原只调 `validate_for_mode`，漏 Style Prompt 长度门等，读数虚高）；新增离线 dry-run 测试。
- **[S2 修复] B-02 配器门归因 + 计数口径（P0-9）**：11 例配器失败逐例归因（`docs/qa/b02-root-cause.md`）：0 例纯真违规，主因为极简段无出口（9/11）与计数口径（8/11）。修复 `parse_section_instruments`：英文 `energy:` 尾项与能量解析同口径（原整行计为乐器，误报“配器过多 8 件”）、否定/退出项（`no drums`、`bass drops out`）不计。门阈值调整随 P1-4 SongPlan 门一次落地。
- **[S2 修复] 自用安全基线（C-08/C-12/D-02/D-07，P0-10）**：钥匙串写入改走 `security -i` stdin（密钥不再出现在进程参数，写后读回校验）；前端写文件权限从全盘 `**` 收窄到 `$APPDATA`/`$DOWNLOAD`/`$DOCUMENT`（保存对话框路径运行期自动授权）；信封门日志不再记录模型输出正文；DNS 重绑定等残余风险记录于 `docs/security-self-use.md`。Key 格式不合规（空白/引号/反斜杠/非 ASCII）时写入前拒绝，设置界面在对应输入框下显示“Key 未保存到钥匙串”及原因（原先只打控制台，重启后 Key 静默丢失）。
- **[S3 修复] 变更记录与 QA 数据重复（C-13/C-14）**：补齐 0.5.5 版本段与本 Unreleased 段；删除 `docs/qa/results_12round_final.jsonl`（与 `results_v4_final.jsonl` 字节相同，MD5 `0ddd1ecc…`）；新增守护单测：当前版本必须有 CHANGELOG 段、`docs/qa/*.jsonl` 不得内容重复。
- **[S3 清理] 仓库垃圾清理（P1-J，§8 / D10 / D14）**：删除 8 份旧计划文档、`public/tauri.svg`、移动端图标（android/ios）与 Windows Store 图标（均零引用）；`speechOrder.test.ts` 移入 `src/test/`；`.gitignore` 增 `graphify-out/`、`.workbuddy/` 与审计派生物；9 处无主 `#[allow(dead_code)]` 逐个判定（测试专用改 `#[cfg(test)]`，无调用者 `logging::log_dir` 删除，其余为有调用者的 pub 项直接去掉）；Linux 构建的未使用导入告警修复。新增两项守护测试防回流。
- **[新架构] SongPlan IR 类型与 Schema（P1-1，§2.1 / RULE-SUNO-PROFILE）**：新增 `src-tauri/src/plan/`（`mod.rs` 类型唯一定义点、`schema.rs` schemars 生成 + strict 后处理 + jsonschema 本地校验 + 快照、`suno_profile.rs` 平台限制带事实来源 + `get_suno_meta` 命令）。依赖 `schemars`、`jsonschema`。
- **[S1 修复] 明文 Key 清除前读回校验（F1，P0 审计）**：旧规则凭迁移标记或 `keychain_set` 未报错即把 localStorage 明文哨兵化——标记残留且钥匙串为空/不同值时唯一一份 Key 丢失。现逐账户“读回 = 明文”才清；否则写入并读回校验，仍不一致则保留明文（内存继续可用），原因显示在对应 Key 输入框下；迁移验证完成前任何设置写出都不会哨兵化明文。
- **[S2 修复] 活动 run RAII 守卫 + begin 前取消（F2/F3）**：`cancel::begin` 返回守卫，`run_pipeline`/`pipeline_resume` 统一经 `with_active_run`，future 被丢弃/超时 abort 也会 finish；前端已生成 run_id、后端尚未登记时的“停止”记为 pending，run 一开始即取消；`cancel_pipeline` 返回是否生效，界面显示“停止未产生效果”或失败原因。
- **[S3 修复] 审计补丁 F5–F9**：Key 格式门只作用于 macOS `security -i` 路径（Windows/Linux 不误拒）；特殊字符 token 测试 + Mac 实写验收；`StreamChunk`/`PipelineEnvelope` 的 `run_id` 跨语言双向锁与 App chunk 过滤接线锁；钥匙串静态守卫防变量别名；`double/triple stops` 不再被当作乐器退出；注释归位。
- **[工程] clippy 零告警门**：存量告警清零，`scripts/gates.sh` 以 `cargo clippy --all-targets -- -D warnings` 收尾；新增 `scripts/mac-verify.sh`（同步后钥匙串/特殊字符实写/离线门/GUI 验收）与 `docs/qa/p1-3-4-acceptance.md`（真模型 12 例 smoke + 40 例回放标准）。
- **[新架构] 结构化调用 + 能力探测（P1-2，§2.2）**：新增 `llm_caps.rs`（`json_schema`/`json_object`/`none` 三档；解析顺序 角色显式 → `(base_url, model)` 探测缓存 → 缺省 `json_object`）与 `call_llm_structured`（任何档位都做本地 schema 二次校验；失败为 Parse 并带 JSON 路径，不静默降级、不自由文本兜底；仅网络/截断允许 1 次同请求重发；`none` 档不发请求，直接提示换模型）。新命令 `probe_structured_caps`（测试连接成功后自动探测，鉴权/限流/5xx 不记档）；设置页显示探测结果（`none` 为醒目告警），角色配置可显式选择档位；新增 `PhaseProgress` 事件（结构化阶段以进度+预览代替 token 流）。离线 mock 网关覆盖三档、探测、截断重发与校验失败。

## 0.5.5（2026-09-14 ~ 2026-10-04）钥匙串数据安全 · 产出语言 · 审计批修复

- **[S0 修复] 钥匙串 Key 数据丢失回路**（v0.5.5 / v0.5.5.1）：全局 Key 留空 = 沿用（不再 `keychain_delete`）；密钥同步防抖；退出竞态 flush；`security` CLI 重写。
- **[功能] 产出语言**：`PipelineRequest.output_lang`（zh/mix/en）、设置面板下拉、orchestrator 讨论层/终稿层语言指令拆分。
- **[修复] 字数/降级规则根治**：D 模式行上限 10→12、和音虚词计数豁免、suno_rules 场景过滤、Degraded 分型中文标签。
- **[修复] 审计报告第二至二十六批**：规则单源化、轮间人工确认门（#12）、结果操作条与产出落点单源（#10/#11/#30/#31）、厂商模板与可搜索模型下拉（#27）、请求限流 UI 与 429 分类、i18n/模式域/wire-format 守护、历史结果反馈接线与插话提示（#9–#13）。
- **[修复] 平台**：Windows 打包 beforeBuildCommand 引号、CRLF 检出下生产段提取（含 `.gitattributes`）、集成测试 `make_request` 补 `output_lang`。

## 0.5.4（2026-09-12）方案信封契约 + 回炉责任分离——大型实测驱动

40 例大型实测（四模式×10 冻结提示词，全链路事件捕获）定位根因后落地的结构修复：

- **方案信封契约（plan_envelope_contract）**：主持人方案强制 `<<<LYRICS>>>/<<<STYLE>>>/<<<PARAMS>>>/<<<NOTES>>>` 分节，双入口（阶段0/汇总）注入规范、出口过信封门（不合契约带缺陷清单纠错重写一次，独立额度）。保真校验只比对 LYRICS 节——围栏/表格/元话语物理上进不了比对，**误报占比 79%→2%**。解析器容错版（顺序无关、前置散文忽略），信封降级 run 跳过保真（硬校验照常），不再制造启发式噪音。
- **回炉责任分离**：打回 issue 按归属拆双通道——转写类（保真/格式要素/截断）走校验员循环 ≤2；方案内容类（配器/字数/Hook/骤停/行宽/Style 长度）走主持人循环 ≤2 携领地指引，修复后新方案重转写。**validation 通过率 30%→58%**（D 0/10→7/10）。
- **说明行单一责任人（TERRITORY R-4→制作人）**：信封门机械前置校验说明行长度（写入点左移），制作人提示词领地声明同步。**Style Prompt 标签在场 40/40**。
- 新增大型实测基建：冻结提示词池 + 40 例批量 harness（全链路事件落盘/断点续跑/逐例 JSONL）+ 7 维产品评分卡（docs/qa/）。
- 开关 `PLAN_ENVELOPE_CONTRACT=0` 回退旧行为。lib 262 全绿（+14 新测试）。

## 0.5.3（2026-09-08）GUI 实测修复——导出 ACL · 校验结论落盘

0.5.2 发布后按验收流程在本机跑了四模式 GUI 实测（D/A/B/C 各一次），实网暴露两个只在真实 GUI 链路出现的缺陷，本版修复：

- **导出功能不可用（P1）**：导出/设置备份走 `plugin:fs|write_text_file`，但 capabilities 只配了 `fs:default`（只读权限集），点击导出必报 "not allowed by ACL"。0.5.1 起即 latent，GUI 实测首次踩中。修复：capabilities 增加 `fs:allow-write-text-file`（scope `**`——路径来自用户自己的保存对话框）。
- **history.validation_passed 恒缺（P1）**：保存历史条目时读 `pipeline.validation`（React state 闭包在生成点击时定格为 null），audit_result 的更新对保存点不可见——四模式实测记录全部缺校验结论、导出头部声明恒为"结论未知"。与 usageRef 当初的归零完全同构。修复：新增 `validationRef`（audit_start 清零 / audit_result 写入 / run 启动与 reset 复位），保存改读 ref，与 degradedRef/usageRef 同模式。

## 0.5.2（2026-09-08）圆桌逻辑升级——规则闭环 · 转写契约 · 诚实降级

吸收《多Agent协作对话系统》任务书机制设计，对圆桌流水线做逻辑升级（原计划文档 `docs/2026-09-08-roundtable-logic-upgrade-plan.md` 已于 P1-J 删除，可经 `git log --all --full-history -- docs/2026-09-08-roundtable-logic-upgrade-plan.md` 找回；ADR-1/2/3 已 Accepted）。

### 修复（按类根治）

- **参数区间承诺与执行闭环（C1）**：A/B/抖音模式的 Weirdness/Style Influence 区间此前只有提示词承诺、校验代码零执行（50/50 双双通过）。现新增参数硬门 + RULE_REGISTRY 规则注册表（15 条规则唯一执行者）+ 死常量守护测试——删执行点 CI 即红；叙事型例外条款与硬校验同口径（≥2 叙事段且写明'叙事型'归类方可回落 A/B 弧线区间）。
- **收敛-终稿断层（C2）**：阶段 2 校验员从"重新创作"约束为"受控转写"（转写契约）；新增转写保真校验（歌词行 multiset diff，报告精确行号与两侧原文）+ 定点重写（只重写违规段，其余段落字节不动）+ TRANSCRIPTION_ISSUE 回炉通道（收敛方案硬伤交主持人修正，不静默修复）。generate/resume 两链路共用同一终稿产出实现。功能开关：`SHIYI_TRANSCRIPTION_FIDELITY=0` 一键回 v0.5.1 行为。
- **提示词手写数字归零（C3）**：60 处手写规则数字（弧线表 10 组、模式参数区间、350/80/90 等）退役为 `${占位符}`，经 `rules::interpolate` 从常量单源生成；弧线表由 ARC_PARAMS 运行时生成；守护测试禁止手写数字回流；快照测试锁定渲染输出与升级前逐字节一致；阶段 0 主持人纳入校验清单注入（与审改员/校验员同口径）。
- **多角色修订冲突无裁决（C4）**：主持人汇总输入新增冲突预检——同 target 跨角色互不相容修订对在双侧条目前注入 ⚠️ 裁决指引（含 TERRITORY 领地表全文），并在人设/表之间双向锁定测试；包含关系视为细化不误报。
- **诚实降级聚合（C5）**：三降级源（call/budget/gate）事件化透传 → 历史记录降级标记 + 列表徽章 + 导出文本头部完整度声明（"存在降级：… / 全环节正常"）；硬校验结论随记录落盘；v0.5.1 旧记录零迁移直接读。

### 工程

- headless 四模式实网测试新增终稿硬校验断言（收敛承诺传递到终稿的实网观测点）。
- 完整测试套件：cargo 254+（lib 248 / headless_offline 7）+ 前端 46，全部通过；零编译告警。
- **实网验证记录（四模式 × 5 轮）**：B/C 稳定通过全部硬校验；A/D 出现轮换性违规（说明行缺失/配器超限/Hook 次数），每次均被硬门打回、耗尽后诚实降级并给出行号级原因（§6.4 第二分支）。实网暴露并已修复两个真实缺陷：①转写标记在打回循环内泄漏进交付包（重写输出未剥离）→ 已修并加分类器防御；②转写契约与打回修正指令自相矛盾 → 已修（清单项修正优先于原样保留）。ADR-1 重审触发（打回率>30%）如实记录：机制正确，flash 档模型硬门遵守率为当前方差主因，建议生产配置更强模型或观察 GUI 实测。

## 0.5.1（2026-09-07）四链路按类根治

- 设置清洗单出口（sanitizeStored）；usage 同源持久化（usageRef 累计真源）；版本单源 Cargo.toml + CI Version guard（终结 0.3.0 静默事故）；Style Prompt 标签回退（B 模式三连打回根治）。

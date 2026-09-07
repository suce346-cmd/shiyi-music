# 全链路深审矛盾修复计划（P0×1 / P1×6 / P2×20）

日期：2026-09-07 | 状态：Proposed（待批准后按批施工） | 基线：`main@2798b63`
落盘：`docs/2026-09-07-deep-review-fix-plan.md`

> 范围：本次七路并行逐行深审（orchestrator 主链 / 角色人设+模式指令 / 校验单源+知识库 /
> 网关预算错误 / 前端主链 / 外围模块 / 安全纵深）的全部置信度≥80 发现。
> 不含 Mimosa 密封扫描替代项；Mimosa deep（`scan-2026-09-06T17-23-34.820Z-6563a683e228`，
> seal `sha256:6e2c2a7e…`）结论仍为参考：HIGH×1 为测试占位串，1 包命中 2 条 advisory（包名未给出）。
> 合规：凭据只从环境变量/密钥服务读；服务端 URL 仅 http/https，发请求前校验 host，
> 拒绝 localhost、环回、私有、保留地址。本计划无真实凭据写入。

---

## 0. 基线与门限

- 本地：P4 + Q1–Q6 + 审理漂移修复，共 8 个本地提交（`91c2ee6…2798b63`），工作区干净，领先 `origin/main`。
- 测试基线：后端 198 项 / 离线 `headless_offline` 7 项 / 前端 9 文件 41 项 / `tsc --noEmit` 退出 0。
- 实网基线：讯飞网关全量无头 5 项全绿（A 834 / B 2198 / C 583 / D 1255 字，一次产出，约 88 秒）。
- 每批门限：后端 198+新增全绿、离线 7 全绿、前端 41（+新增）全绿、`tsc` 退出 0；P0/P1 项加跨分支用例。
- 回滚：每批单提交，退化即 `git revert` 当批。

---

## 1. 发现总览

| 级别 | 编号 | 一句话 | 位置 |
|------|------|--------|------|
| P0 | S-1 | base_url 双入口无 host 校验，Bearer 可被带往内网 | `models/mod.rs:513-519`、`orchestrator.rs:174-184`、`llm.rs:324/371/850 + 248/387/856` |
| P0 | S-2 | `test_api` 完全无 URL 校验 | `llm.rs:839-863` |
| P1 | F-1 | 前端 max_tokens 放 32000，后端只认 30000 | `App.tsx:702`、`useSettings.ts:44` 对 `models/mod.rs:433-455` |
| P1 | L-1 | Mode A 初稿弧线五选一 vs primer/审改十选一 | `prompts.rs:59/296` 对 `rules.rs:95/79`、`roles.rs:113/210`、`orchestrator.rs:721-726` |
| P1 | L-2 | Mode C 终稿 system 允尾部 2 行 vs user 补丁总数完全一致 | `roles.rs:82` 对 `orchestrator.rs:890-894`、`validator.rs:266-268`、`rules.rs:39/80` |
| P1 | L-3 | 单段 3–7 与最弱段≥2 自矛盾，且“3”无单源无代码门 | `rules.rs:79`、`roles.rs:82/93` 对 `rules.rs:13-27`、`validator.rs:143-168` |
| P1 | L-4 | 主持人汇总丢 Mode C 原词链路 | `orchestrator.rs:786-803/808-846`（对照 `498-500/580-584/730-732/883-895`） |
| P1 | L-5 | `role_overrides.base_url` 零校验（S-1 的第二入口，逻辑侧） | `orchestrator.rs:174-184`、`models/mod.rs:510-519` |
| P2 | B-1 | 审改/校验重试成功仍发首次用量，token 漏记 | `orchestrator.rs:537-561/676-700` |
| P2 | B-2 | 反馈路由大小写不一（`Hook`/`weirdness` 漏路由） | `orchestrator.rs:66/70/85` |
| P2 | B-3 | 检查点 mode 双轨（阶段 2 用 Debug 拼装） | `orchestrator.rs:1292/1313/1449` |
| P2 | B-4 | `headless_modes.rs:112` Mode C 座位注释=4（真源=3） | `headless_modes.rs:112` |
| P2 | B-5 | 模式锁单测只断言前提，未锁分支行为 | `orchestrator.rs:1901-1910` 对 `1434-1454` |
| P2 | B-6 | `reset` 具名连带清全局位，跨任务串扰 | `cancel.rs:36-41` 对 `orchestrator.rs:1059-1061` |
| P2 | R-1 | Mode B 参数三方不一（固定区间 vs 10 弧线，制作人缺 B 条款） | `prompts.rs:1056`、`rules.rs:45-56/79`、`roles.rs:92/113/210` |
| P2 | R-2 | Mode D 作词人与风格分析师 Hook 同轮互盲重复提案 | `roles.rs:129/231-232`、`orchestrator.rs:28-35/1206-1246` |
| P2 | R-3 | 情感与制作人对人声/参数双重管辖无主副 | `roles.rs:113 item6-7`、`roles.rs:210 item5/8` |
| P2 | K-1 | ARC 注释“Weirdness 散见 CSV”失实（9/10 零命中） | `rules.rs:41-43` 对 `suno_rules.csv` |
| P2 | K-2 | 弧线单测只锁 Weirdness，Style 9/10 无 verbatim 锁 | `rules.rs:144-153` |
| P2 | K-3 | 清单 Audio=0、禁 `/` 与 `、`、标点全半角无代码门 | `rules.rs:79-81` 对 `validator.rs` 全文件 |
| P2 | G-1 | 终稿 reserve=ZERO 仍报文案“终稿保底截断”虚构 | `llm.rs:102-106/129-133`（闸门 `69-77` 有双文案可复用） |
| P2 | G-2 | 重发等待被预算打断 `break` 丢因，返回旧 Parse | `orchestrator.rs:744-749`、`llm.rs:285-287` |
| P2 | G-3 | `test_api` 全错误坍缩为 Network，kind 丢失 | `llm.rs:853-864` |
| P2 | U-1 | `sanitizeStored` 角色键无白名单 | `useSettings.ts:55-63` 对 `usePipeline.ts:155-157` |
| P2 | U-2 | 钥匙串迁移失败仍置标记，明文残留 | `useSettings.ts:123-137` |
| P2 | U-3 | Mode C 首败“重试”死按钮 | `App.tsx:512-525` 对 `StatusIndicator.tsx:78-86` |
| P2 | U-4 | 启动回填窗口哨兵当真 key 发出 | `useSettings.ts:29/141-147`、`App.tsx:33/646-652`、`usePipeline.ts:435` |
| P2 | O-1 | 日志“保留 3 天”注释失实，无界增长 | `logging.rs:2` 对 `:13-17` |
| P2 | O-2 | 检查点 `load` 吞 IO 错误，误报无检查点 | `checkpoint.rs:63-68` 对 `orchestrator.rs:1442-1447` |
| P2 | O-3 | 锁中毒静默丢取消/插话 | `cancel.rs:19-33`、`interject.rs:16-28`、`orchestrator.rs:1419-1429` |
| P2 | O-4 | 插话槽无上限堆积 | `interject.rs:15-19` 对 `orchestrator.rs:1501-1525` |
| P2 | O-5 | keychain account 无校验 | `keychain.rs:20/28/42` |
| P2 | O-6 | CI 测试门禁只跑 macOS | `build.yml:12-40` 对 `:42-56` |
| P2 | O-7 | 依赖高危（nanoid/postcss 各 2 条 audit） | `package-lock.json`（`npm audit` 实证），Rust 侧待 `cargo audit` 定级 |

注：L-3 与校验侧“单段 3–7”同源，B-3 与 O 侧 Debug 拼装同源，B-6 与 O-5（reset 串扰）同源，
O-5（keychain）与安全侧 account 白名单同源，合并施工不重复计数。

---

## 2. R1：堵 P0 + 前端上限（安全优先，可独立上线）

### S-1/S-2/L-5：URL 双入口 host 校验 + `test_api` 收口

- 问题原因：`validate_request` 只做 `http(s)` 前缀检查，无 `Url::parse`、无 host 解析、
  无 localhost/环回/私有/169.254/保留拒绝；且只查全局 `base_url`，`role_overrides[*].base_url`
  零校验；`test_api` 完全不调校验。
- 有何影响：恶意配置导入即可让后端以 Bearer 身份请求内网/云元数据，key 随请求外发（P0）。
- 怎么修复：
  1. 新增 `validate_url()`（建议放 `models/mod.rs` 或新建 `src-tauri/src/net_guard.rs`）：
     `Url::parse` → scheme 仅 http/https → 拒绝含 `username/password/@` →
     DNS 解析全部 IP → 拒绝 `is_loopback/is_private/is_link_local/is_multicast/is_unspecified`
     及 169.254/保留段。纯函数单测。
  2. `validate_request` 内对 `base_url + role_overrides.values().base_url` 逐个校验（含空串拒绝）。
  3. `test_api` 入口复用同一函数；`headless_modes.rs` 的 `test_config` 同口径复用（P2 安全项顺带收）。
  4. `reqwest` 关闭重定向或重定向后复检（若当前默认跟随，至少加注释+单测锁定行为）。
- 旧规则处理：前缀检查保留为第一道，host 校验为第二道；`ftp://` 非法用例继续通过。
- 新规则握手：URL 规则单源 `validate_url`，三入口（generate/refine/resume）+ `test_api` 共用；
  前端 `sanitizeStored` 对 baseUrl 加同口径提示（不硬拦，后端为准）。
- 预期：内网/保留地址显式 `Validation` 拒绝；合法公网网关不受影响。
- 验证：新增 `validate_url` 单测（localhost/127/10/192.168/169.254/`@`/ftp 全拒，公网放行）；
  `role_overrides` 绕过用例；`test_api` 非法 URL 用例；既有 198 项全过。
- 关联链路：`resolve_api` 透传不变；`send_with_retry` 不动；前端设置页错误文案原样展示。
- 文件行：`models/mod.rs:513-519`、`orchestrator.rs:174-184`、`llm.rs:324/371/850/248/387/856/839-863`、
  `headless_modes.rs:17-29`。

### F-1：前端 max_tokens 32000→30000

- 问题原因：Q5 后端上限统一 30000，前端输入框与清洗仍放 32000。
- 影响：填 30001–32000 每次生成都被后端 `Validation` 拦截，UI 承诺失信。
- 修复：`App.tsx:702 max={32000}`→`30000`；`useSettings.ts:44 <= 32000`→`<= 30000`；
  `sanitizeStored` 测试补 30000/30001 边界。
- 旧规则：32000 上限废止。
- 新握手：上限唯一真源后端 30000，前端镜像。
- 验证：前端边界单测；后端 `generation_config` 单测已锁 30000。
- 文件行：`App.tsx:702`、`useSettings.ts:44`、`models/mod.rs:433-455`。

### G-3：`test_api` kind 透传（与 S-2 同文件顺手收）

- 修复：`.map_err` 保留原 `e.kind` 仅包装 message；取消/超时不再显示为网络失败。
- 验证：单测断言 Cancelled/Timeout kind 透传。

R1 验收：后端 198+新增、离线 7、前端 41+新增、`tsc` 0；SSRF 用例全红转绿。

---

## 3. R2：修 P1 逻辑矛盾（ prompt/校验/汇总三处）

### L-4：汇总补 Mode C 原词

- 原因：`build_summarize_user_prompt` 签名无 `req`，体内零引用原词；审改/校验/初稿/终稿四处都有原词，
  唯独合并点没有，行数字数漂移在此引入。
- 影响：对齐好的修订在合并点被洗掉，下一轮打回烧预算。
- 修复：传入 `req: &PipelineRequest`，按 `build_audit_review_user_prompt` 同口径追加原词块 + Mode C 四条约束；
  调用点 `run_host_summarize` 同步改；补单测断言汇总含原词行。
- 旧规则：无原词合并废止。
- 新握手：五处原词口径统一（初稿/审改/校验/汇总/终稿）。
- 验证：汇总单测；Mode C 无头对照一次产出。
- 文件行：`orchestrator.rs:786-803/808-846`。

### L-1：Mode A 弧线五选一 vs 十选一

- 原因：`mode_a_system_prompt` 的 Step1/Step2 JSON 枚举锁死 5 弧线，primer 与审改按 10 弧线评审，
  同一 system 内双口径。
- 影响：新增 5 弧线初稿无法合规产出，必多轮返工。
- 修复（三选一，推荐 A）：A 指令升级为 10 弧线枚举（补 5 新增定义 + 参数区间 + `推荐弧线类型` 10 选）。
  备选 B：`PRIMER_AB` 拆 A-5/B-10 + `checklist("mode_a")` 与初稿枚举一致。本计划按 A 施工。
- 旧规则：五选一封闭枚举废止。
- 新握手：初稿/primer/清单/审改四处同为 10 弧线。
- 验证：枚举文本单测；A 无头对照。
- 文件行：`prompts.rs:59/296`、`rules.rs:95/79`、`roles.rs:113/210`、`orchestrator.rs:721-726`。

### L-2：Mode C 尾部行数口径统一

- 原因：终稿 system 允尾部≤2 行，user 补丁 1/3/4 条说总数完全一致、禁止增删；硬校验站 system 侧。
- 影响：尾部多 1–2 行时行为随机，用户补丁与打回口径不一。
- 修复：统一为“前 N 行逐行等字数，仅允许尾部≤2 行收尾，除此之外禁止增删行”；
  `orchestrator.rs:890-894` 的 1/3/4 按此放宽，`roles.rs:82` 保持，`CHECKLIST_C` 不动。
- 旧规则：user 过严三条废止，换统一句。
- 新握手：system/user/validator/清单四处同口径。
- 验证：尾部 +1/+2 通过、+3 打回用例；C 无头对照。
- 文件行：`roles.rs:82`、`orchestrator.rs:890-894`、`validator.rs:266-268`、`rules.rs:39/80`。

### L-3：单段 2–7 统一（含 K 侧 CSV 描述）

- 原因：清单同句“单段 3–7”与“最弱段≥2”互斥；“3”无常量无代码门；CSV `instrument_max` 描述写“弱段 3 件起”。
- 影响：讨论轮按 3 打回、终稿按 2 放行，口径震荡。
- 修复：统一为单段 2–7（与弱段下限一致）：`CHECKLIST_AB` 改“单段 2–7 件”，两处 auditor 人设同改，
  CSV 该行描述改建议语气（“弱段建议 3 件起，硬门以≥2 为准”）。
- 旧规则：“3”下限废止，不立新常量（弱段 2 已覆盖）。
- 新握手：清单/人设/CSV/硬门四处同口径。
- 验证：2 件弱段全链通过用例；`checklist_contains_single_source_numbers` 同步。
- 文件行：`rules.rs:79`、`roles.rs:82/93`、`validator.rs:143-168`、CSV `instrument_max` 行。

### R-1：Mode B 参数优先级（P2，顺手收，防 B 选边缘弧线打架）

- 修复：三处人设写明“B 以 20-35/75-85 为硬框，弧线区间仅框内细调，出框须说明理由”；
  制作人 item8 补 B 条款。
- 文件行：`prompts.rs:1056`、`rules.rs:45-56/79`、`roles.rs:92/113/210`。

R2 验收：后端 198+新增、离线 7、前端 41、`tsc` 0；A/B/C 各一次无头对照。

---

## 4. R3：流程诚实与单源收口

| 编号 | 修复 | 文件行 |
|------|------|--------|
| B-1 用量漏记 | 重试分支对 `resp2` 同调 `emit_usage`（审改+校验两处） | `orchestrator.rs:537-561/676-700` |
| G-2 break 丢因 | `break` 前置 `Timeout("预算打断")`（阶段 0 + 非流式 R9） | `orchestrator.rs:744-749`、`llm.rs:285-287` |
| G-1 终稿文案 | `reserve.is_zero()` 分支复用闸门双文案口径 | `llm.rs:102-106/129-133`（复用 `69-77`） |
| B-3/B-5/O | 检查点 mode 单源 `to_str_name()`；`load` 分 `NotFound`/IO（warn+Internal）；模式锁抽纯函数共用 | `orchestrator.rs:1292/1313/1449`、`checkpoint.rs:63-68` |
| B-6/reset | 具名 reset 只删自身，仅空参清全局 | `cancel.rs:36-41`、`orchestrator.rs:1059-1061` |
| O-3 锁中毒 | `lock().unwrap_or_else(\|e\| e.into_inner())` 或失败打 error 日志 | `cancel.rs:19-33`、`interject.rs:16-28` |
| O-4 插槽上限 | 单 run 最多 10 条/总计 10000 字，超限 `Validation`/丢最旧+warm | `interject.rs:15-19` |
| O-5/keychain | account 非空 + `global`/`role:已知角色` 白名单；`set` 拒空 secret | `keychain.rs:20/28/42`、`useSettings.ts:55-63`（U-1 同改） |
| B-2 路由大小写 | feedback 转小写后匹配，关键词全小写 | `orchestrator.rs:66/70/85` |
| B-4/B-5 注释单测 | headless C 座位注释=3；模式锁行为单测 | `headless_modes.rs:112`、`orchestrator.rs:1901-1910` |
| K-1/K-2 注释单测 | ARC 注释正名（9 条 Weirdness 系 prose 共识）；循环追加 Style 全片段断言 | `rules.rs:41-43/144-153` |
| K-3 清单无门 | Audio=0 加参数行解析硬门（双向）或清单降级为软建议（二选一，施工时定；默认加硬门） | `rules.rs:79-81`、`validator.rs` |
| R-2/R-3 领地 | D：作词管文字金句形态、风格管次数位置骤停；人声终裁制作人、参数终裁情感弧线（prompt 首段互斥声明） | `roles.rs:129/231-232/113/210` |

R3 验收：后端 198+新增、离线 7、前端 41+新增、`tsc` 0。

---

## 5. R4：体验运维与供应链（可独立上线）

| 编号 | 修复 | 文件行 |
|------|------|--------|
| U-2 迁移标记 | 仅成功后置标记；失败分支哨兵覆写本地明文 + 修正注释 | `useSettings.ts:123-137` |
| U-3 C 重试死按钮 | 无 `lastFeedback` 时禁用重试或回退解析重走生成 | `App.tsx:512-525`、`StatusIndicator.tsx:78-86` |
| U-4 哨兵门控 | 生成/测试按钮加 `secretsReady` 门控或哨兵映射空+提示 | `useSettings.ts:29/141-147`、`App.tsx:33/646-652`、`usePipeline.ts:435` |
| O-1 日志保留 | 注释改“按日滚动不自动删”或补启动清理（3 天）+ 单测 | `logging.rs:2/13-17` |
| O-6 CI | `test` 加 `windows-latest` 一份（或矩阵化） | `build.yml:12-56` |
| O-7 依赖 | `npm audit fix`（nanoid>3.3.18、postcss>8.5.22 锁定重跑）；Rust 跑 `cargo audit/deny` 定级 | `package-lock.json`、`Cargo.lock` |

R4 验收：前端 41+新增、`tsc` 0；`npm audit` 复查；CI 双平台绿。

---

## 6. 施工顺序与交付

```
R1（安全）：validate_url + 双入口 + test_api + 前端30000 + kind透传 → 回归 → 本地提交
R2（矛盾）：汇总C原词 + A弧线10选 + C尾部统一 + 单段2-7 + B优先级 → 回归 + A/B/C无头 → 本地提交
R3（诚实）：用量/丢因/文案/检查点/锁/槽/校验/路由/注释单测 → 回归 → 本地提交
R4（体验）：迁移/重试/哨兵/日志/CI/依赖 → 回归 → 本地提交
→ 推送 origin/main → 打 tag v0.5.0 → 云端 test→三平台→draft Release → 下载验证
```

门限：每批后端 198+新增、离线 7、前端 41+新增全绿，`tsc` 0；R2 加 A/B/C 无头对照；
P0 项加红转绿用例。不改流水线阶段语义、不改阈值数值（除 F-1 前端对齐）、不新增角色与外部依赖
（`url` crate 若尚未引入，R1 允许新增这一个依赖并记录 ADR）。

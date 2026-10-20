# shiyi-music（拾音）交付前审计报告

> 审计日期：2026-10-19　审计对象：`<repo>`　HEAD：`766e747`（2026-10-04）
> 方法：`project-audit` 五步（定性 → 四梁取证 → 门禁反证 → 需求收敛 → 施工图）
> 证据只认三类：可复现命令 + 原始输出、`file:line`、外部权威文档。本报告未修改任何源码；所有变异在 `/tmp` 的 git worktree 中完成。

---

## 0. 第一屏

### 0.1 审计面声明

```
本次审计面 = 四梁 × M（AI 管线，主层）× F（前端，次层）
未覆盖：
  1) 真实模型端到端四模式全链路（无可用 API Key；未产生外部调用费用）
  2) Tauri 打包 / dmg / Windows 安装包（未执行 tauri build，签名与跨平台实机不具备）
  3) 真实 GUI 窗口交互（未启动 WebView；只做组件测试与静态审查）
  4) 第三方安全扫描（Mimosa）与 GitHub Release 实际状态（需账号）
  5) 15 分钟级长跑 / 内存曲线（未跑满预算的真实流水线）
  6) 人工 7 维产品评分（无评分执行人与评分产物）
```

### 0.2 一句话定性

**这是一个工程纪律明显高于同类个人项目的仓库；核心风险不在“代码烂”，而在“门禁与证据的口径比实现更乐观”——最严重的三件事是：跑一次 `cargo test` 会删掉用户真实 API Key、方案信封契约约七成 host 输出未通过而保真校验因此被跳过、模式 A 在唯一 40 例真实数据里硬校验通过率只有 30%。**

### 0.3 结论摘要（10 行）

1. **S0（实测）**：`keychain::tests::cli_round_trip` 直接操作生产账户 `global`，先删后写再删；用哑值实测“测试前存在（exit 0）→ 测试后消失（exit 44）”，测试仍报 ok。
2. **S1（实测）**：40 例真实运行里 107/156 次 host 输出（68.6%）在纠错重写后仍不合信封 → `envelope_fallback`，该路径**跳过保真校验**；v4 的 12 例更高（74.3%）。
3. **S1（实测）**：v3 的 40 例硬校验通过率 57.5%，但 **Mode A 仅 3/10（30%）**；失败集中在配器件数（14 次提及）。对外“B/C 稳定通过”与数据不符（B 5/10）。
4. **S1（实测）**：v4 与 12round 两份 QA 结果**字节完全相同**（MD5 一致），实际是同一批 12 例；每模式 n=3，且无 7 维评分卡产物。
5. **S1（静态）**：历史迁移在标记缺失时用 localStorage 数组**整体覆盖** `history.json`，存在数据丢失路径。
6. **S2（实测）**：默认 `npm test` 在 28 个测试文件中 9 个 worker 启动超时，退出码 1；`--maxWorkers=2` 时 28/28、170 测试全绿——门禁不可复现。
7. **S2（实测）**：`suno_rules.csv` 40 行中 17 行的 scenario 值代码不认识（创作/叙事/high/高开低走…）→ 保守注入所有模式；Mode D 实际拿到 38/40 行，与“mode_d 只吃抖音行”的提交口径不符。
8. **S2（静态）**：`resume` 不传 `output_lang`，英文产出设置在续跑路径被静默忽略；`llm-chunk` 事件不带 run_id；流式按 chunk 做 `from_utf8_lossy` 有中文截断风险。
9. **S2（静态）**：README 声称 Mode C “保留字数韵脚”，字数有硬门，**韵脚零代码校验**。
10. **S3**：无 ESLint/Prettier/rustfmt/clippy/coverage；无 lint 门禁；CHANGELOG 停在 0.5.4，而 HEAD 已领先 v0.5.5 标签 47 个提交、+11,304 行。

### 0.4 立即行动（与审计动作直接相关）

> **请先确认你的 API Key。** 本次审计执行了 `cargo test --lib`（359 项全绿），其中 `cli_round_trip` 会删除钥匙串 `service=shiyi-music, account=global` 的条目。审计后查询该条目为 `exit 44`（不存在）。若你原本存有 Key，它已被该测试删除，请打开应用重新输入一次；这不是你的操作失误。

---

## 1. 定性六格与事实底座

### 1.1 fingerprint 原始输出（原样入报告）

```
REPO   <repo>
TIME   2026-10-19 00:08:35 +0800
[1] 仓库状态
HEAD    766e747 2026-10-04 [fix] 补集成测试 make_request 的 output_lang 字段——CI Rust 测试失败真因
branch  main
first   2026-07-09
commits 187
dirty   1 files
        ?? graphify-out/
[2] 规模（已排除依赖与构建产物）
ext      files      lines
rs          26      17773
tsx         24       3970
ts          31       3587
TOTAL       81      25330
markdown   123 files
[3] 技术栈指纹
  + package.json
  + package-lock.json
[4] 测试 / 构建入口
  + tsconfig.json
  scripts:
[5] 体积构成
total  5.3G
  5.0G  src-tauri/
  297M  node_modules/
   13M  graphify-out/
  1.6M  src/
  1.6M  docs/
  1.5M  dist/
  1.2M  target/
  8.0K  public/
[6] 可疑物
大文件 (>50M): 无
敏感文件名候选: 无
.gitignore 关键覆盖: node_modules dist
```

补充读数：`.git` 19 MB；`src-tauri/target` 4.9 GB（构建缓存，占总体积 92%）；提交节奏 2026-07 = 16、08 = 1、09 = 169、10 = 1（单人 + AI 的脉冲式开发）。

### 1.2 六格

| 格 | 判定 | 依据 |
|---|---|---|
| 产出形态 | 桌面应用（Tauri 2）+ 本地 LLM 管线 + React 前端 | `src-tauri/tauri.conf.json`、`package.json`、`src-tauri/src/lib.rs` |
| 谁在用 | 作者自用为主，README/LICENSE/Release 表明有对外分发意图 | MIT、GitHub Actions 三平台产物、draft release |
| 生死线 | ① 用户 API Key 丢失（已发生）② 产出提示词质量不达标 ③ 长任务卡死/烧钱 | 0.4 节 S0、40 例数据、预算/超时机制 |
| 规模 | L：25,330 行（rs 17,773 / tsx 3,970 / ts 3,587），81 个源文件，8 张知识表 346 行 | fingerprint |
| 年龄 | 3 个月（2026-07-09 → 2026-10-04），187 commits，9 月占 169 | `git log` |
| 承诺强度 | 高：README 功能承诺 + 4 条 CHANGELOG 记录（0.5.1–0.5.4）+ 三平台 Release + 量化指标（58%、40/40、79%→2%） | README、CHANGELOG、workflow |

### 1.3 规模档位与取样

`L`（>20k 行）：按技能要求读入口 + 核心 + 最可疑三处，不做“假装读全”。实际通读/精读：

- 后端核心：`orchestrator.rs`（4,332）、`validator.rs`（1,880）、`rules.rs`（3,069 关键段）、`knowledge.rs`（2,215 关键段）、`llm.rs`（1,044）、`models/mod.rs`（请求/URL/限额）、`keychain.rs`、`history.rs`、`cancel.rs`、`gate.rs`、`interject.rs`、`checkpoint.rs`、`budget.rs`、`logging.rs`
- 前端核心：`App.tsx`（1,194 关键路径）、`usePipeline.ts`（716 全文）、`useSettings.ts`（301 全文）、`useQueue.ts`（177 全文）、`types/index.ts`、`urlGuard.ts`、`data/providers.ts`
- 测试与基建：`headless_offline.rs`、`headless_modes.rs`、`large_scale_matrix.rs`、CI workflow、capabilities、CSV 数据

### 1.4 审的是工作树还是 HEAD？

`git status --porcelain` 仅 `?? graphify-out/`（未跟踪的分析产物）；`git diff --stat` 为空。**源码工作树 == HEAD**，未提交改动不改变行为。本报告结论对 `766e747` 成立。

---

## 2. 宣称清单（30 条）

> 来源：README、CHANGELOG 0.5.1–0.5.4、`docs/2026-09-14-问题总清单`、`docs/qa/*`、CI workflow、代码注释中的对外口径。
> 判定：✅ 落点存在且行为一致；◐ 部分成立/有未披露前提；✗ 不成立；? 未验证。

| # | 宣称 | 落点 | 判定 |
|---|---|---|---|
| C01 | Tauri 2 + React 19 + Rust 桌面应用 | `Cargo.toml:26`、`package.json:21-22` | ✅ |
| C02 | MIT 许可 | `LICENSE:1` | ✅ |
| C03 | 圆桌流水线 v3，≤3 轮收敛 | `orchestrator.rs:1987` `MAX_DISCUSSION_ROUNDS=3` | ✅ |
| C04 | 四模式 A/B/C/D | `steps_for_mode`、`prompts.rs`、测试 | ✅ |
| C05 | 角色级 API（逐字段回退全局） | `resolve_api:177-187` + 3 项单测 | ✅ |
| C06 | 轮间人工确认门（可关，5 分钟上限） | `gate.rs`、`ROUND_GATE_*`、6 项测试 | ✅ |
| C07 | 同轮修订可见 + 冲突裁决 | 阵容序串行 + `detect_revision_conflicts` | ✅ |
| C08 | 方案信封契约强制分节 | `enforce_envelope:1102` | ◐ 69% host 输出降级 |
| C09 | 转写保真校验（歌词 multiset diff） | `check_transcription_fidelity` | ◐ 信封降级/Mode C 时跳过 |
| C10 | 保真误报 79%→2% | CHANGELOG 0.5.4 | ? 无基线数据；且 69% 靠“跳过检查” |
| C11 | validation 通过率 30%→58% | CHANGELOG 0.5.4 | ◐ 23/40=57.5%；Mode A 仅 30% |
| C12 | Style Prompt 标签 40/40 在场 | CHANGELOG 0.5.4 | ✅ 独立复核 40/40、12/12 |
| C13 | 说明行 80→200 单源 | `DESC_LINE_MAX_CHARS=200` | ✅ 三处门禁变异均按预期红 |
| C14 | 参数区间硬门 + 规则注册表（0.5.2 称 15 条，现 20 条） | `RULE_REGISTRY`、`param_gate_*` | ✅ 去接线变异 2 项红 |
| C15 | 思维资产“引用必达”不被上限截断 | `render_with_gate` | ✅ 变异后 1 项红，报错精确 |
| C16 | trigger 双域（阶段 × 条件） | `craft_inject_gate`、登记表 | ✅ 测试覆盖 |
| C17 | 关键词表结构可达性审计 | `reachability_violations` | ✅ 数据侧无单字键 |
| C18 | 字数/配器数值单源，禁止手写回流 | `handwritten_rule_number_hits` | ✅ 守护测试绿 |
| C19 | suno_rules 场景过滤：D 只吃抖音行 | `scenario_allows_mode` | ✗ D 实际 38/40 |
| C20 | Mode C 保留字数与韵脚 | README:24、`prompts.rs:409-413` | ◐ 字数有硬门，韵脚零执行者 |
| C21 | 全局 Key 留空 = 沿用，不误删 | `withRestoredGlobalKey` | ✅ |
| C22 | 迁移标记仅全部成功才置位 | `useSettings.ts:135-153` | ✅ 行为锁测试 |
| C23 | 明文 Key 迁移后删除 | `persistNonSecrets` | ◐ 失败/仅角色键时明文残留 |
| C24 | 导出 ACL 已修 | `capabilities/default.json:9-12` | ✅ |
| C25 | 版本单源 + CI version guard | `Cargo.toml:3`、`build.yml:89-102` | ✅ |
| C26 | 测试全绿 | 359 lib + 7 offline + 170 前端 | ◐ 默认 `npm test` 退出码 1 |
| C27 | 零编译告警 | `cargo build --lib` | ✅ 未观察告警 |
| C28 | 40 例 harness 断点续跑 + 逐例 JSONL | `large_scale_matrix.rs` | ◐ 硬编码作者绝对路径 |
| C29 | 7 维产品评分卡 | `docs/qa/2026-09-09…md` | ✗ 只有评分标准，无评分结果 |
| C30 | 三平台构建 + Release | `.github/workflows/build.yml` | ? 未本地/远端验证 |

---

## 3. 四梁取证

### 3.1 梁一 · 真实性

**1）宣称落点（逐条）**：见第 2 节。30 条中 ✅ 17、◐ 8、✗ 3、? 2。

**2）能力边界（3 个应被拒绝的输入）**

| 输入 | 期望 | 实测 | 判定 |
|---|---|---|---|
| `base_url = http://127.0.0.1:8080` | 拒绝（SSRF） | `validate_url` 返回“解析到内网/环回/保留地址” | ✅ 单测覆盖 |
| `base_url = ftp://…` | 拒绝 | “协议仅允许 http/https” | ✅ |
| `feedback` 长度 2001 | 拒绝且文案统一 | `too_long("优化反馈", n, 2000)` | ✅ |
| `keychain account = "role:hacker"` | 拒绝 | 白名单拒绝 | ✅ |
| 空 `run_id` 插话 | 拒绝 | “缺少 run_id” | ✅ |

结论：入口拒绝能力真实存在，不是“只判非空”。

**3）占位与宣传错位**

- 全仓 `TODO/FIXME/unimplemented!/todo!` = **0**（`grep -rniE ... | wc -l` = 0），没有“假实现当能力写”的典型痕迹。
- 但存在**能力级空缺**：韵脚（C20）、7 维评分（C29）、信封合规率（C08）——不是 TODO，而是“文档写了、执行者没有/未测量”。

**4）命名承诺**

- `check_audio_influence`、`param_range_issues`、`desc_line_length_issues`、`settle_craft_refs` 名实一致，且有负路径测试。
- `validate_*` 系列不是“只判非空”：均解析数值/结构后再判定。

**5）版本与成熟度**

- `package.json:4` = 0.5.5；`Cargo.toml:3` = 0.5.5；tag `v0.5.5` = `7c7b5b5`（2026-09-14）。
- **CHANGELOG 最新条目是 0.5.4（`CHANGELOG.md:3`），没有 0.5.5 条目**；HEAD 距 tag 47 个提交、67 文件、+11,304/−955 行，版本号未变。
- 结论：源码树 ≠ 发布产物；对外“最新版 0.5.5”只覆盖 tag 那一点，之后五周的改动没有版本叙事。

### 3.2 梁二 · 正确性

**1）不可替代资产**

一句话：**“把中文歌词创作方法论 + Suno 规则约束，压成一条可审计的多角色提示词流水线”**。删掉它，剩下的只是一个聊天框。非平凡实现确实在仓库里：

- 4 套模式原版指令（`prompts.rs` 1,116 行）
- 7 个角色人设 + 领地声明（`roles.rs` 928 行）
- 8 张知识表 346 行 + 按角色/模式/能量/关键词的检索注入（`knowledge.rs`）
- 20 条硬规则与执行者注册表（`rules.rs:248-289`）
- 信封/保真/打回/预算/取消/门/检查点（`orchestrator.rs`）

**2）量化判据（自己跑的，不抄文档）**

| 判据 | 读数 | 样本/命令 |
|---|---|---|
| 历史 QA 判定可复现率 | **38/40（95%）** | `cargo test --test qa_replay`（用当前校验器回放 v3 的 40 份真实输出） |
| 不可复现的 2 例 | 均为 `FAIL→PASS`，原因是说明行上限 80→200（有文档、有单源） | P4'、P11（mode_d） |
| 当前校验器仍判失败 | **15/40** | 与当时 17 例失败相比，差额恰为上述 2 例 |
| Style Prompt 标签在场 | **40/40、12/12** | 正则独立复核 `output` 字段 |
| 真实历史记录字段完整度 | conversation 23/23、usage 22/23、validation_passed 14/23（9 条旧记录 None） | 读取 `~/Library/Application Support/com.shiyi.music/history.json` |
| 知识表数据质量 | 8 表无重复 id、无空单元格（suno_rules 的数值列按设计为空） | Python CSV 抽检 |

这条梁的结论：**校验器本身没有静默漂移**，40 份历史真实输出的判定 95% 可复现，差额可解释；0.5.3 的 `validation_passed` 修复在真实历史里也确实生效（新记录有值，旧记录为 None）。

**3）跑真实输入，读产物**

- 我用当前校验器回放了 40 份真实模型输出（不是 mock）。产物可读、结构完整（Style Prompt + 段落标签 + 说明行 + 歌词 + 参数行）。
- 但“能读”不等于“能用”：15/40 仍违反硬规则，其中 11 例是配器件数；Mode A 只有 3/10 过门。

**4）守恒与不变量**

- 歌词行对齐（Mode C）：`validate_lyric_fill` 做行数 + 逐行字数守恒，C3/C4 两例失败被正确捕获。
- 保真 multiset：`lyric_multiset_issues` 做双向差集（缺失/新增），报告行号与两侧原文。
- 预算守恒：`Budget` 用 deadline，`remaining_for_discussion` 预留终稿 120s；`spawn_guarded` 超时 abort 并 await。
- **破缺**：信封降级后保真不再守恒（跳过），这不是“误差”，是“不检查”。

**5）单一真相源（抽查）**

- 数值：`DESC_LINE_MAX_CHARS`、`LYRIC_LINE_*`、`ARC_PARAMS`、`MODE_B_*` 均有单源常量 + 扫描网。
- 规则：`RULE_REGISTRY` 20 条，执行者存在性用“调用点”而非“名字出现”校验。
- **破缺 1**：`suno_rules.scenario` 的取值域（16 种）与代码识别域（3 种）不一致（见 C19）。
- **破缺 2**：前端 `output_lang` 在 `run/refine` 传、`resume` 不传（见 C04）。
- **破缺 3**：模式名在 Rust 枚举/`ALL_MODES`/前端 `MODE_EXPERTS` 三处，有测试锁，但前端的 `ROLE_NAMES/ROLE_EMOJIS` 仍是可被后端 meta 改写的可变全局对象（初始化后写、多处读），属于“运行期单源”，不是结构单源。

### 3.3 梁三 · 可信度

**1）门禁变异（Step 2 详表见第 4 节）**：3 个核心门禁均在预期位置变红。

**2）假绿四型**

| 型 | 抽查结果 |
|---|---|
| 条件写反 | 未发现；`validate_url`、`param_range_issues`、`desc_line_length_issues` 的负路径测试都真实失败 |
| 容差大于精度 | 未发现数值容差型断言；`backoff_jitter_secs` 是行为参数不是判据 |
| 只判非空 | 未发现关键门禁只判非空（`is_lyric_line` 有分类器） |
| `or` 短路 | 未发现 `A == 0 || B` 类假绿；`is_conflict_pair` 的“包含即细化”是有意语义 |

**但有两个“绿色但无效”的变体：**

- **绿着删数据**：`cli_round_trip` 测试通过，同时删除生产密钥（见 A-01）。
- **绿着不测量**：40 例 harness 的 `validation_passed` 只调用 `validate_for_mode`，不含生产 `collect_hard_issues` 的 Style 长度/BPM 门（`large_scale_matrix.rs:234-240` vs `orchestrator.rs:1960-1980`）。本批数据恰好没有 Style/BPM 违规，所以读数没被扭曲；但口径确实窄于生产。

**3）删断言实验**

未做全量“把实现改成返回常量”的 10 项实验（会大面积破坏编译）；改用**定点变异**（第 4 节），3/3 在预期断言上失败，信息可定位到具体常量与行号。

**4）覆盖盲区**

仓库没有覆盖率工具（无 tarpaulin/llvm-cov/istanbul 配置），因此**没有任何覆盖率读数**。359 + 170 个测试是“数量”，不是“覆盖”。按技能口径，这属于“无法读 missing 行”，记 S3。

**5）替身覆盖**

- `headless_modes.rs` 用真实网关（环境变量），无 key 自动跳过；`large_scale_matrix.rs` 同理。
- 前端测试用 jsdom + Testing Library，不 mock Tauri invoke 的地方会直接失败；`wireProtocol.test.ts` 锁事件形状。
- 没有“mock 掉唯一会坏的地方”的明显痕迹。

**6）确定性**

- `cargo test --lib` 连跑通过；`qa_replay` 两次读数一致。
- **不确定项**：默认 `npm test` 的 worker 启动超时是资源竞争型 flaky（同一代码两次结果不同：19/28 失败 vs 28/28 通过）。这不是算法不确定，是门禁不确定。

**7）回归基线**

- 有：`tests/fixtures/prompts/*.txt` 快照 + `mode_prompts_byte_identical_to_snapshot`。
- 有：40 例 JSONL 历史产物。
- 缺：评分卡基线（7 维分数）——所以“变好/变坏”只能看 pass/fail，不能看产品维度。

### 3.4 梁四 · 交付性

| 探针 | 结果 |
|---|---|
| 干净环境 | `npm ci` 未重跑（会改 node_modules）；`npm ls --depth=0` 依赖完整；`tsc --noEmit` = 0；`vite build` = 0（JS 338.31 kB / gzip 102.41 kB；CSS 48.03 / 8.68） |
| 外部依赖 | 仅 LLM 网关；失败有分类（Auth/RateLimit/Network/Parse/Timeout）与退避播报；`validate_url` fail-closed |
| 长跑与重启 | 预算 15 分钟 + spawn 超时 abort；检查点每轮落盘、终稿成功即清；**残留 1 个 2026-09-19 的检查点无 TTL** |
| 并发 | 队列顺序执行（一次只 invoke 一个），单窗口；但 `llm-chunk` 无 run_id、全局取消位可跨命令污染 |
| 安全入口 | 1 个窗口、16 个命令；URL 双层校验；keychain account 白名单；`fs:allow-write-text-file` scope `**` |
| 数据落盘 | Key 在钥匙串（但测试会删）；历史在 app_data（394 KB / 23 条）；日志 3 天滚动（当前 6 KB） |
| 回滚 | git 单提交/revert 可行；无数据库迁移；历史 JSON 只增字段，旧记录可读 |
| 跨平台 | CI 矩阵 macOS arm64/x64 + Windows；Windows 集成测试因 WebView2 DLL 只编译不执行；本地仅 macOS 验证 |

---

## 4. 门禁反证（Step 2）

> 方法：`git worktree add /tmp/shiyi-audit HEAD`，在临时工作区改一处 → 跑对应测试 → 记录“期望红/实际红” → `git checkout --` 还原。被测仓库源码零改动。

| # | 检查名 | 它声称保证什么 | 变异方式 | 期望报什么 | 实际报什么 | 判定 |
|---|---|---|---|---|---|---|
| M1 | `DESC_LINE_MAX_CHARS` 单源族 | 说明行上限 200 在 checklist/CSV/envelope 三处同源，且边界 200/201 正确 | 200 → 80 | `desc_line_max_matches_csv_structured_value`、`desc_line_examples_within_limit`、`desc_line_limit_fits_strongest_section_contract` 红 | 3 项红，报错含“left: 200 right: 80”与具体超限示例行 | ✅ 按预期红 |
| M2 | 引用必达 | prompt 点名的 LC/CC 编号必须强制投递，不被 8 条上限截断 | `i32::MAX - pos` → `0` | `craft_required_rows_force_injected_beyond_cap` 红，指出必达行被截 | 1 项红，报错“引用必达行被上限截掉: … 含引用必达 1 条 … 前 2 条” | ✅ 按预期红 |
| M3 | 参数区间硬门 | A/B 的 Weirdness/Style Influence 越界必须打回 | 删除 `issues.extend(param_range_issues(mode, text));` | `param_gate_mode_b_out_of_range_fails`、`param_gate_mode_a_arc_union_rejects_wild_values` 红 | 2 项红，报错“越界参数应被拒（issues: []）” | ✅ 按预期红 |
| M4 | 门禁本身的安全性 | 测试不应触碰生产数据 | 预置哑值 `global=AUDIT-DUMMY-KEY-2026` → 跑 `cli_round_trip` | 测试后 `global` 应仍在（或测试使用隔离账户） | 测试前 `exit 0`，测试后 `exit 44`；测试输出 `ok` | ❌ **绿着删数据（S0）** |

**结论：这套门禁能拦住“规则执行点被删/常量被改/引用被截断”，拦不住“测试污染生产状态”和“测量口径窄于生产”。**

补充反证：
- 变异均在预期**断言名**上红，没有红在无关测试或异常堆栈上。
- 没有发现“永远为真”的条件表达式；最接近的是 40 例 harness 的 `validation_passed`（口径窄，不是恒真）。
- 测试自身的验证器（`has_call_site_distinguishes_definition_from_invocation`、`test_reference_helpers_selfcheck`）存在且绿，说明“门禁的门禁”有自检。

---

## 5. 问题清单

> 字段：`ID | 梁/层 | 严重度 | 证据强度 | 现象 | 证据 | 影响 | 修复方向 | 规模`
> 证据强度：E1 实测 / E2 静态可证 / E3 外部文档 / E4 推断。

### S0（安全 / 数据损坏 / 核心不可用）

| ID | 梁/层 | 严重度 | 证据 | 现象 | 证据位置 | 影响 | 修复方向 | 规模 |
|---|---|---|---|---|---|---|---|---|
| A-01 | 梁四 / 交付 | **S0** | E1 | 单元测试 `cli_round_trip` 直接操作生产钥匙串账户 `global`：先 `cli_delete`，再写入 `test-key-12345`，再删除。运行 `cargo test` 即删除用户真实 API Key，测试仍报 ok | `src-tauri/src/commands/keychain.rs:240-255`；实测：预置哑值 → `exit 0`，跑测试 → `exit 44` | 用户凭据不可逆丢失；默认开发/CI 命令触发；本次审计已触发 | 测试改用隔离账户（如 `test:global:<pid>`，测试内直接调 `cli_*` 不受白名单限制），或 `#[ignore]` 且显式 opt-in；**绝不触碰 `global`** | S |

### S1（宣称不成立 / 核心产出错误）

| ID | 梁/层 | 严重度 | 证据 | 现象 | 证据位置 | 影响 | 修复方向 | 规模 |
|---|---|---|---|---|---|---|---|---|
| B-01 | 梁一/二 · 管线 | **S1** | E1 | 40 例真实运行中，107/156 次 host 输出（68.6%）在“纠错重写一次”后仍不合信封 → `envelope_fallback`；该路径按设计**跳过保真校验**。v4 的 12 例为 26/35（74.3%），8/12 run 至少一次 | `orchestrator.rs:1152-1159`、`validator.rs:585-587`；数据：`docs/qa/results_v3_final.jsonl` / `v4` 的 `degraded.flag=envelope_fallback` | “方案强制分节”名存实亡；约七成 run 的歌词转写无保真保护；“误报 79%→2%”部分来自“不检查”而非“更准” | 把 envelope 合规率作为一等指标打进 QA 报告；对不合规输出二选一：继续纠错（独立额度）或明确降级并在产物头部标注；不要用“跳过”当默认 | M |
| B-02 | 梁二 · 正确性 | **S1** | E1 | v3 40 例硬校验通过率 23/40（57.5%），但 Mode A 仅 3/10（30%）、Mode B 5/10；17 例失败中 11 例是配器件数（14 次 issue 提及）。v4 的“改善”只有 12 例（每模式 n=3），且与 v3 同题的 12 例是 9/12→11/12（1 例回归） | 数据统计；`validator.rs:226-274`；`large_scale_matrix.rs:82-149` | Mode A 是主用法之一，三成通过率意味着多数产出靠“打回耗尽降级”交付；对外“B/C 稳定”不成立 | 先做 Mode A 配器门的根因分析（Intro/Outro 弱段是否应豁免/降下限），再改 prompt 或门；用同 40 例重跑验证，不用 12 例替代 | M |
| B-03 | 梁四 · 落盘 | **S1** | E2 | 启动迁移在 `HISTORY_MIGRATED_KEY` 缺失且 localStorage 有非空历史时，直接 `history_save(parsed)` **整体覆盖** `history.json`，不读现有文件、不合并 | `src/App.tsx:266-276`、`history.rs:89-97` | 标记丢失 + 旧 localStorage 残留的组合下，历史记录被旧数据覆盖（不可逆） | 先 `history_load`，仅当文件为空才迁移；或按 id 合并去重；迁移后立即清 localStorage | S |

### S2（结构性风险 / 假绿 / 无基线）

| ID | 梁/层 | 严重度 | 证据 | 现象 | 证据位置 | 影响 | 修复方向 | 规模 |
|---|---|---|---|---|---|---|---|---|
| C-01 | 梁三 · 门禁 | S2 | E1 | 默认 `npm test`（`vitest run`，无 config）在本机 28 个测试文件中 9 个 worker 启动超时，`Test Files 19 passed` 但 `Errors 9`、退出码 1；`--maxWorkers=2` 时 28/28、170 测试、退出码 0 | `package.json:10`、`vite.config.ts`（无 `test` 段）；两次运行记录 | 门禁不可复现：同一代码“全绿/全红”取决于机器负载；CI 与本地可能给出相反结论 | 加 `vitest.config.ts`：`maxWorkers: 2` 或 `pool:'forks', maxForks:2`；开 `isolate:false` 可把 70s 降到 ~15s；CI 固定并发 | S |
| C-02 | 梁二 · 单源 | S2 | E1 | `suno_rules.csv` 40 行有 16 种 scenario 值，代码只识别 `抖音/中文`，其余 17 行（创作/叙事/high/高开低走/平摊/起伏/阶梯上升/渐进爆发/U型/单峰/回环/全模式/无参考音频）全部按“注入所有模式”处理 → D 实际 38/40、A/B/C 33/40 | `knowledge.rs:494-500`；CSV 统计；提交 8e54ba4 的“D 只吃抖音行”口径 | D 模式被灌入 A/B 弧线规则与“无参考音频”规则，增加噪音与自相矛盾风险；单源承诺不成立 | 把 scenario 词表升为单源常量 + 加载期校验（未知值即报错/告警）；为弧线行引入 `mode_d` 排除规则；加守护测试断言“每行 scenario ∈ 已登记词表” | M |
| C-03 | 梁一 · 真实性 | S2 | E2 | README 称 Mode C “保留字数韵脚”；字数有 `validate_lyric_fill` 硬门，**韵脚零代码校验**（validator/rules 无任何 韵/押韵/rhyme 逻辑），仅存在于 prompt 文案 | `README.md:24`、`prompts.rs:409-413`；`grep -rn "韵脚" validator.rs rules.rs` = 空 | 用户以为韵脚被保证；实际只能靠模型自觉，转写保真对 C 也跳过 | 二选一：① 实现轻量韵脚门（提取句尾拼音韵母，比对押韵位置）② 把 README 改为“提示词约束”，并在评分卡里人工评 | M |
| C-04 | 梁二 · 契约 | S2 | E2 | `resume` 构造请求时**不传 `output_lang`**，后端 serde 缺省 `"zh"`；用户在设置里选 en/mix，续跑终稿变中文 | `usePipeline.ts:665-683`、`types/index.ts:229`（可选字段）、`models/mod.rs:689-695` | 同一任务两条路径行为不一致，且静默 | 请求组装单源：抽 `buildRequest()`，run/refine/resume 共用；类型改必填 | S |
| C-05 | 梁四 · 状态 | S2 | E2 | `test_api` 传 `run_id=""` → `send_with_retry` 查全局取消位；`cancel_pipeline` 空 id 会置全局位且**没有复位点**（具名 run 的 reset 不清全局） | `llm.rs:1019-1027`、`llm.rs:161`、`cancel.rs:14-22`、`orchestrator.rs:2500-2509` | 用户“停止”在无 run_id 时按下后，“测试连接”永远返回“已取消”，直到重启 | 取消位只认具名 run；空 id 分支删除或改为 no-op；`test_api` 传独立 run_id | S |
| C-06 | 梁四 · 并发 | S2 | E2 | `llm-chunk` 事件不带 run_id，前端只靠“监听注册 token”过滤；取消后旧流/新 run 重叠时，旧 chunk 可被新监听器接收 | `llm.rs:591-596`、`App.tsx:135-141` | 队列连续跑或取消瞬间，正文可能串到下一项 | `StreamChunk` 加 run_id，前端按当前 run 过滤（与 pipeline 事件同口径） | S |
| C-07 | 梁二 · 正确性 | S2 | E2 | 流式解码 `String::from_utf8_lossy(&chunk)` 逐 chunk 转字符串；多字节 UTF-8 被 TCP/TLS 边界切开时，半字符变 U+FFFD | `llm.rs:583` | 中文歌词偶发乱码（“�”），且不可复现、难归因 | 用 `Vec<u8>` 累积 + 按行切分后 `from_utf8`；或 `encoding_rs` 增量解码 | S |
| C-08 | 梁四 · 安全 | S2 | E2 | 密钥通过 `security add-generic-password … -w <secret>` 的命令行参数传递（同机进程可 `ps` 看到）；`-A` 允许任意应用读取 | `keychain.rs:85-86` | 本机其他进程/用户可短时窥见或长期读取密钥 | `-A` 改回默认 ACL（需解决 adhoc 签名）；密码改走 stdin/临时文件（若 CLI 支持）；至少在文档里声明风险 | M |
| C-09 | 梁四 · 落盘 | S2 | E2 | 历史保存 500ms 防抖，窗口关闭/进程退出无 flush；并发保存时旧数组的 in-flight 写可能晚于新数组落盘 | `App.tsx:95-99`、`App.tsx:146-154`（只清 llm 监听） | 生成后立刻退出丢最后一条；极端情况下历史回退 | 监听 `beforeunload`/Tauri close 事件 flush；保存串行化（单写队列）或带版本号 | S |
| C-10 | 梁四 · 安全 | S2 | E2 | 迁移只在 `changed=true` 时调用 `persistNonSecrets`；若只有角色级 Key（或 keychain 回读失败），明文 Key 留在 localStorage，且迁移标记已置 1 | `useSettings.ts:135-186` | 明文凭据长期驻留，违背“迁移后删除明文”的承诺 | 迁移成功后无条件 `persistNonSecrets`（即使 `changed=false`）；迁移后校验 localStorage 不含明文 | S |
| C-11 | 梁二/三 · 证据 | S2 | E2 | 40 例 harness 把输出目录硬编码为 `<repo>/target/large-test`；且 `validation_passed` 只跑 `validate_for_mode`，不含生产 `collect_hard_issues` 的 Style 长度/BPM 门 | `large_scale_matrix.rs:165`、`:234-240`；`orchestrator.rs:1960-1980` | 换机/CI 不可复跑；测量口径窄于生产，通过率可能高估 | 路径用 `CARGO_TARGET_DIR`/`env!("CARGO_MANIFEST_DIR")`；测量改调 `collect_hard_issues` 或抽公共函数 | S |
| C-12 | 梁四 · 安全 | S2 | E2 | `validate_url` 在准入时 `to_socket_addrs` 解析并判内网，但真正请求由 reqwest 重新解析；DNS rebinding 可让两次解析结果不同 | `models/mod.rs:787-797`、`llm.rs:226-233`（`redirect none` 已防跳转） | 恶意域名可绕过 SSRF 防线访问内网 | 解析后固定 IP（`resolve` + `host` 覆盖）或请求后校验对端 IP；对用户自填地址风险可接受但需记录 | M |
| C-13 | 梁一 · 版本 | S2 | E1 | CHANGELOG 最新 0.5.4，`Cargo.toml`/`package.json` 0.5.5；HEAD 领先 `v0.5.5` 47 commits / +11,304 行，版本未 bump | `CHANGELOG.md:3`、`Cargo.toml:3`、`git rev-list` | 源码构建与发布产物不一致；用户无法从版本号判断能力 | 补 0.5.5 条目；为未发布改动开 `## Unreleased`；发版前 bump 到 0.6.0 并打 tag | S |
| C-14 | 梁三 · 证据 | S2 | E1 | `results_v4_final.jsonl` 与 `results_12round_final.jsonl` MD5 完全相同（`0ddd1ecc…`），是同一批 12 例；两份文件未被任何文档引用；7 维评分卡只有标准无结果 | `md5` 实测；`docs/qa/` | 证据被误读为两次独立实验；“12 轮”叙事缺乏数据支撑 | 删除/合并重复文件并在 CHANGELOG 注明；补评分卡结果或删掉“7 维评分卡”表述 | S |

### S3（体验与整洁）

| ID | 现象 | 证据 | 影响 | 修复方向 |
|---|---|---|---|---|
| D-01 | 无 ESLint/Prettier/rustfmt/clippy/coverage；CI 只跑 tsc/test/build | `package.json:7-13`、`build.yml` | 风格与静态缺陷靠人眼；无覆盖率读数 | 加 `cargo clippy -- -D warnings` + ESLint + `vitest --coverage` |
| D-02 | `fs:allow-write-text-file` scope `**` | `capabilities/default.json:9-12` | WebView 若被攻破可写任意用户文件（CSP 已降低概率） | 收窄到应用数据目录 + 对话框路径；或改用 Rust 命令写文件 |
| D-03 | 历史无上限/归档（394 KB、23 条）；检查点无 TTL（残留 1 个 09-19 的文件） | `history.rs:89`、`checkpoint.rs:50-98`、app_data 实查 | 长期使用后写放大、残留堆积 | 历史上限 + 归档；检查点 7 天 TTL |
| D-04 | 全局 API Key 无删除入口（留空=沿用，只能覆盖） | `useSettings.ts:221-233` | 用户无法从应用内清除凭据 | 设置面板加“清除 Key”（显式确认 → keychain_delete） |
| D-05 | `execute_review` 重试路径若第二次调用报错，首调 usage 未入账 | `orchestrator.rs:712-742` | token 统计偏低 | `?` 前先 emit 首调 usage，或 RAII 记账 |
| D-06 | `CANCELLED_RUNS` 只增不减（除同 id reset） | `cancel.rs:8-45` | 极小的内存泄漏；长期运行累积 | run 结束时 remove；或改用有界 LRU |
| D-07 | 日志记录 host 输出前 300 字 | `orchestrator.rs:1119-1123` | 用户创作内容进本地日志（3 天） | 脱敏或降为 debug；文档声明 |
| D-08 | v0.5.5 Release 仍是 draft（公开最新为 0.5.4） | `docs/2026-09-14…md:55` | 对外版本落后 | 用户手动 Publish；或改为自动发布 |

---

## 6. 量化数据

### 6.1 测试与构建（全部为本机实测）

| 项 | 命令 | 读数 |
|---|---|---|
| Rust 单测 | `cargo test --lib` | **359 passed / 0 failed / 3 ignored**，85.07s |
| Rust 离线集成 | `cargo test --test headless_offline` | **7 passed**，0.00s |
| Rust 历史回放 | `cargo test --test qa_replay` | 38/40 一致；2 例 FAIL→PASS（80→200）；0 例 PASS→FAIL |
| 前端类型 | `tsc --noEmit` | exit 0 |
| 前端测试（默认） | `vitest run` | 19/28 文件、113 tests、**9 unhandled errors、exit 1** |
| 前端测试（限并发） | `vitest run --maxWorkers=2` | **28/28 文件、170 tests、exit 0**，69.66s |
| 生产构建 | `vite build` | exit 0；JS 338.31 kB（gzip 102.41）、CSS 48.03 kB（gzip 8.68） |
| Rust 编译告警 | `cargo build --lib` | 未观察 warning |

### 6.2 QA 数据集（`docs/qa/*.jsonl`）

| 数据集 | 例数 | 通过 | 通过率 | 分模式 |
|---|---|---|---|---|
| v3 | 40 | 23 | **57.5%** | A 3/10 (30%)、B 5/10、C 8/10、D 7/10 |
| v4（=12round，同 MD5） | 12 | 11 | 91.7% | A 3/3、B 3/3、C 3/3、D 2/3 |

- 失败类型（v3）：配器 14 次提及、说明行超限 3、字数 2、Hook 1、行数 1。
- 信封降级：v3 107 次 / 156 次 host 输出（68.6%），28/40 run；v4 26/35（74.3%），8/12 run。
- 同题对比（12 个相同 mode+prompt）：v3 9/12 → v4 11/12；P5/P6/P2 由失败转通过，D-P1 由通过转失败（说明行 93>80）。
- Style Prompt 标签：v3 40/40、v4 12/12。
- 重试：v3 host 49 次、auditor 9 次；其中保真类重试仅 2 次。v4 host 9 次、保真 0。

### 6.3 真实用户数据（只读）

| 项 | 读数 |
|---|---|
| `history.json` | 394,045 B，23 条 |
| conversation 字段 | 23/23 |
| usage 非零 | 22/23 |
| validation_passed | True 11 / False 3 / None 9（旧记录） |
| degraded 非空 | 9/23 |
| app_data 总量 | 408 KB |
| 日志 | 1 个文件 6,239 B（2026-10-04） |
| 残留检查点 | 1 个（2026-09-19） |

---

## 7. 未验证项（明确列出，不当作没问题）

1. **真实模型端到端**：没有 API Key，未跑 `headless_modes`（3 个 ignored）与 40 例 harness；所有“模型遵守率”结论来自历史 JSONL 回放，不是本次新跑。
2. **打包**：未跑 `npm run tauri build`，dmg/NSIS 产物、签名、公证、图标、Windows 实机均未验证。
3. **GUI**：未启动真实窗口，插话、门条、队列、导出对话框的交互只做了组件测试与代码审查。
4. **第三方扫描**：Mimosa 的 `runStatus/ completeness` 结论未复核（需 ZCode 环境）。
5. **Release 状态**：未查 GitHub API，v0.5.5 draft 状态引用自仓库文档（E3）。
6. **长跑/内存**：未跑满 15 分钟预算，未观测内存/句柄曲线。
7. **覆盖率**：无工具，未测。
8. **跨平台**：Windows 只看了 CI 配置与 `#[cfg]` 分支，未实机。

---

## 8. 需求收敛（Step 3）

### 8.1 Question Log（本轮 3 问，按“最影响方案分叉”排序）

| # | 问题 | 选项 | 为什么必须先答 |
|---|---|---|---|
| Q1 | 这个仓库的终局是什么？ | A 自用工具继续演进（默认假设）／B 对外发布给他人用／C 演示或一次性交付 | 决定 S1 是否必须清零、是否投入打包/签名/更新器 |
| Q2 | 验收主判据选哪个？ | A 真实模型 40 例通过率（要花 API 费用）／B 离线门禁全绿 + 构建可复现／C 人工 7 维评分 | 决定 B-01/B-02 的“改好”如何证明；三者成本差一个量级 |
| Q3 | 第一批范围？ | A 只修数据安全（A-01/B-03/C-10）／B 连产出正确性（+B-01/B-02）／C 连工程可复现（+C-01/C-13/C-14） | 决定批次顺序与停机时间 |

### 8.2 五槽位

| 槽位 | 现状 | 置信度影响 |
|---|---|---|
| 目标 | **未定**（自用 vs 对外） | 缺，-15% |
| 成功判据 | **未定**（真模型/离线/人工） | 缺，-15% |
| 范围与非目标 | 部分已知：不做架构重写；不接更新器 | 半满 |
| 硬约束 | 已知：不改用户数据、真实模型验收、分批停、AI 不 push | 满 |
| 决策权与节奏 | 已知：用户拍板，每批停 | 满 |

**当前置信度：60%（上限 70%，因两槽位空缺）。施工图只能算草案，不作为施工依据。**

### 8.3 用户签字句（待确认）

> “我确认 Q1=__、Q2=__、Q3=__；同意按第 9 节草案调整为正式施工图。”

---

## 9. 施工图（草案，置信度 60%，待 Q1–Q3 拍板）

> **已被定稿取代**：Q1–Q8 拍板后的最终施工图见 `docs/audit-2026-10-19/BLUEPRINT.md`（置信度 95%，6 个批次，38/38 问题归位）。本节保留为审计当时的草案，不作为施工依据。
>
> 排序固定：安全与数据正确性 → 宣称兑现 → 架构收敛 → 体验与整洁。重构不入第一批。

### 批次 1（P0，半天–1 天）：止血——不再丢数据

| 项 | 内容 |
|---|---|
| 目的 | 消除唯一的“跑测试就删真实密钥”路径；堵住历史覆盖与明文残留 |
| 触及文件 | `src-tauri/src/commands/keychain.rs`、`src/App.tsx`、`src/hooks/useSettings.ts` |
| 动作 | ① `cli_round_trip` 改用隔离账户 `test:global:<pid>`，结束清理；② 历史迁移仅当 `history_load` 为空才写，否则按 id 合并；③ 迁移成功后无条件 `persistNonSecrets`；④ 设置面板加“清除 Key”（可选，若 Q1=B） |
| 验收命令 + 期望读数 | `security add-generic-password -U -A -s shiyi-music -a global -w DUMMY && cargo test --lib keychain && security find-generic-password -s shiyi-music -a global >/dev/null; echo $?` → **0（条目仍在）**；`npm test` 不涉及；迁移脚本用临时 app_data 目录跑两条：文件空→写入、文件非空→不覆盖 |
| 回滚 | 单提交 revert；无数据迁移 |
| 前置 | 无 |
| 规模 | S |

### 批次 2（P1，2–3 天）：把“七成降级”和“Mode A 三成”变成可测量的真问题

| 项 | 内容 |
|---|---|
| 目的 | 让信封合规率与分模式通过率成为报告的一等指标；决定是“修提示词/门”还是“改宣称” |
| 触及文件 | `src-tauri/tests/large_scale_matrix.rs`、`docs/qa/*`、`src-tauri/src/commands/orchestrator.rs`、`validator.rs`、`rules.rs`、`README.md` |
| 动作 | ① harness 输出 `envelope_fallback` 次数/率、分模式 pass rate、Style/BPM 门（改调 `collect_hard_issues`）；② Mode A 配器失败逐例归因（Intro/Outro 是否应豁免），给出“改门”或“改 prompt”的方案；③ 韵脚：实现轻量门或改 README 措辞；④ scenario 词表单源化（C-02） |
| 验收命令 + 期望读数 | `SHIYI_TEST_* … cargo test --test large_scale_matrix -- --ignored` → 报告含 `envelope_fallback_rate`、`pass_rate_by_mode`；离线：`cargo test --lib` 全绿 + 新增 scenario 守护测试（塞入未知值即红）；`npm test` 全绿 |
| 回滚 | 每项独立提交；harness 改动不影响生产 |
| 前置 | Q2 选定（是否愿意花 40 次真实调用） |
| 规模 | M |

### 批次 3（P2，1 天）：让门禁可复现、让版本可解释

| 项 | 内容 |
|---|---|
| 目的 | 同一代码在 CI 与本地给出同一结论；版本号能解释源码树 |
| 触及文件 | `vitest.config.ts`（新）、`package.json`、`CHANGELOG.md`、`Cargo.toml`、`docs/qa/` |
| 动作 | ① `maxWorkers:2` / `isolate:false`；② 补 0.5.5 条目 + `Unreleased`；③ 删重复 JSONL 或注明同源；④ 补评分卡结果或删“7 维评分卡”表述 |
| 验收命令 + 期望读数 | `npm test` 连跑两次 → exit 0、28/28、170；`git rev-list --count v0.5.5..HEAD` 与 CHANGELOG 条目对应；`md5 docs/qa/*.jsonl` 无重复 |
| 回滚 | 纯配置/文档，revert 安全 |
| 前置 | Q3 含 C |
| 规模 | S |

### 批次 4（P3，持续）：契约与体验收口

- C-04 `buildRequest()` 单源（resume 带 output_lang）
- C-05 取消位只认具名 run
- C-06 `llm-chunk` 加 run_id
- C-07 流式字节缓冲解码
- C-08 keychain argv/`-A` 风险处置（文档或改 ACL）
- C-09 关闭前 flush + 串行写
- C-12 DNS pinning（可选）
- D-01 lint/clippy/coverage
- D-02 fs scope 收窄
- D-03 历史/检查点 TTL
- D-05 usage 记账

### 不做清单

| 不做 | 代价 |
|---|---|
| 重写圆桌架构/换框架 | 现有 359+170 测试与 40 例证据全部作废，收益无法用现有判据证明 |
| 为“100% 信封合规”放宽/删硬门 | 会重演“改判据绕过”，假绿比红危险 |
| 现在接更新器/签名/公证 | 无 Apple 账号；与当前风险排序不符 |
| 用 12 例替代 40 例做验收 | 样本量不足，且两份文件同源 |
| 直接改 `global` 钥匙串做“迁移验证” | 已发生一次，不能再有第二次 |

### 风险登记（最可能翻车的三处）

| 风险 | 早期信号 | 缓解 |
|---|---|---|
| 修信封策略导致重试/成本上升 | 单 run host 输出 >5、时长 >8 分钟、fallback 率不降 | 先只加测量不改策略；用 12 例小样本试跑 |
| 放宽配器下限导致产品变差 | 评分卡“编曲规范性/合理性”下降 | 先做逐例归因，改 prompt 优先于改门 |
| 测试并发调低掩盖真实竞态 | 测试全绿但 GUI 队列串流 | 单独加一条并发 e2e（两个 run 交错）作为补偿 |

### 半天最小切片（证明方向正确）

1. 修 `cli_round_trip` 隔离账户（30 分钟）。
2. 修历史迁移“文件非空不覆盖”（30 分钟）。
3. 迁移后无条件 scrub localStorage（15 分钟）。
4. 跑 `repro/01`、`repro/02`、`repro/03`，确认 S0 不再复现、B-01/B-02 读数可打印（1–2 小时）。

---

## 10. 附录

### 10.1 repro 脚本（`docs/audit-2026-10-19/repro/`）

| 脚本 | 用途 | 安全性 |
|---|---|---|
| `01-keychain-test-destroys-key.sh` | 证明单测删除 `global` | 先用 `security find-generic-password` 检查；若已有条目则**拒绝执行**（避免二次伤害），否则写入哑值→跑测试→断言消失 |
| `02-envelope-fallback-rate.py` | 统计信封降级率 | 只读 JSONL |
| `03-mode-a-pass-rate.py` | 统计分模式通过率 | 只读 JSONL |
| `04-test-suite-concurrency.sh` | 复现默认并发失败/限并发通过 | 只读（会跑测试，不改源码） |
| `05-suno-scenario-leak.py` | 统计 scenario 未识别行 | 只读 CSV |

### 10.2 证据文件索引

- `docs/qa/results_v3_final.jsonl`（40 例，2026-09-12）
- `docs/qa/results_v4_final.jsonl` = `results_12round_final.jsonl`（12 例，2026-09-13，同 MD5）
- `docs/qa/test-prompts.md`（10 条冻结提示词 × 4 模式）
- `docs/2026-09-14-问题总清单-重构输入.md`（历史问题账本，1244 行）
- `src-tauri/tests/large_scale_matrix.rs`（40 例 harness）
- `src-tauri/tests/qa_replay.rs`（本次审计新增的回放测试；已归档到 `repro/qa_replay.rs`，复制到 `src-tauri/tests/` 即可运行）

### 10.3 关键命令速查

```bash
# 事实底座
bash <audit-tool>/fingerprint.sh <repo>

# 测试
cd <repo>/src-tauri && cargo test --lib
cargo test --test headless_offline
cd .. && ./node_modules/.bin/tsc --noEmit && ./node_modules/.bin/vitest run --maxWorkers=2

# 门禁变异（临时工作区）
git worktree add /tmp/shiyi-audit HEAD
# 改一处 → CARGO_TARGET_DIR=/tmp/shiyi-audit-target cargo test --lib <filter> → git checkout --
```

---

## 11. 自检（技能提交前清单）

| 项 | 结果 |
|---|---|
| 一句话说清靠什么活着 | 是：中文歌词方法论 + Suno 规则 → 可审计的多角色提示词流水线 |
| 每条结论可重跑 | 是：命令/JSONL/file:line 均在文中 |
| 至少 3 个门禁红过 | 是：M1/M2/M3 按预期红；M4 反向证明门禁绿着删数据 |
| 量化数字自己跑的 | 是：测试、回放、通过率、降级率、构建体积均为本机读数 |
| 五槽位填满 | **否**（目标/判据空缺，置信度 60%） |
| 每批有验收/回滚 | 是（草案） |
| 明确写未验证 | 是（第 7 节 8 项） |
| 只修三件知道哪三件 | 是：A-01、B-03、C-10（半天切片） |

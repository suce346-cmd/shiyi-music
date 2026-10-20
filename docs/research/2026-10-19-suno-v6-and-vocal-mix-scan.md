# Suno v6 市场扫描 + 截图词表吸收（2026-10-19）

> 目的：在给出施工图之前，先回答三件事——
> ① 两张截图里的 9 条人声/混音提示词，在当前知识库里覆盖到什么程度；
> ② 截至 2026-10-19，Suno 与同类产品最成熟/最先进的提示词做法是什么；
> ③ 现有 26 条工程问题 + 市场对齐缺口，哪些有方案、哪些还只是方向。
>
> 证据等级：E1 本机实测 / E2 代码静态可证 / E3 外部资料（附 URL 与访问日期）/ E4 推断。
> 外部资料含厂商/SEO 内容，**不是官方文档的部分已标注**；官方能力以 Suno 站内字段与多家独立实测交叉为准。

---

## 一、两张截图的逐条拆解（E3）

### A. 人声演唱类（4 条）

| # | 标题 | 截图关键词 | 截图描述（原文/补全） | 英文可机读标签（市场通用） | 控制层 |
|---|---|---|---|---|---|
| V1 | 清晰咬字 | 字正腔圆｜吐字清晰｜无吞音 | 主唱人声清晰靠前，咬字饱满，每一句歌词清晰可辨，没有模糊/吞音 | `clear diction, upfront lead vocal, articulate, no mumbling` | 人声层（character + delivery） |
| V2 | 气声转音 | 气声修饰｜转音顺滑｜细腻处理 | 主唱带气声质感，转音自然顺滑，尾音带轻微颤音，情感细腻，没有生硬断层 | `breathy vocal, smooth runs, subtle vibrato, intimate, no abrupt breaks` | 人声层（delivery + effects） |
| V3 | 情绪哭腔 | 情感递进｜哭腔质感｜感染力强 | 副歌部分人声带轻微哭腔，情绪逐层递进，声音带着哽咽感，感染力强而不煽情 | `emotional delivery, slight cry/tremble in chorus, building intensity, restrained` | 人声层（delivery，按段落变化） |
| V4 | 和声伴唱 | 多层和声｜背景伴唱｜层次分明 | 副歌加入双层和声伴唱，人声层次分明，和声不盖过主唱，整体融合度高 | `layered harmonies, double-tracked backing vocals, lead-forward, blended` | 人声层（arrangement + mix） |

### B. 混音质感类（5 条）

| # | 标题 | 截图关键词 | 截图描述（原文/补全） | 英文可机读标签 | 控制层 |
|---|---|---|---|---|---|
| M1 | 人声贴唱 | 居中贴耳｜融合度高｜混响适中 | 人声位于声场正中央，贴耳不遥远，混响适中，人声与伴奏融合度高 | `vocal-forward, centered, close-mic, moderate reverb, sits in the mix` | 混音层 |
| M2 | 低频增强 | 低频饱满｜下潜扎实｜弹性鼓点 | 低频贝斯饱满有力，鼓点弹性强，低频不浑浊，质感扎实不轰头 | `full low-end, deep sub-bass, punchy kick, tight lows, not muddy` | 混音层 |
| M3 | 空间混响 | 声场宽阔｜定位清晰｜空间感 | 整体加入适度空间混响，声场宽阔不拥挤，乐器定位清晰，有大空间感 | `wide stereo field, lush hall reverb, clear instrument placement, spacious` | 混音层 |
| M4 | 分轨层次感 | 分层清晰｜三频均衡｜通透干净 | 人声、鼓点、贝斯、旋律乐器分层清晰，互不干扰，高中低频均衡 | `clear separation, balanced EQ, transparent, uncluttered mix` | 混音层 |
| M5 | 母带质感 | 母带质感｜动态充足｜高清晰度 | 经过母带处理，整体声场宽阔，动态范围足，声音细腻，接近专业发行级 | `mastered, wide dynamics, polished, release-ready, hi-fi` | 母带/整体质感层 |

### 1.1 这 9 条在现有知识库中的覆盖（E1/E2）

用整词检索 8 张 CSV：

| 词 | 命中 | 说明 |
|---|---|---|
| 咬字 | 1 | 零散出现在 note/production 文本，不是可检索标签 |
| 气声 | 11 | 多在 instruments 的 technique 列（乐器气声），不是人声标签 |
| 哭腔 | 2 | instruments（二胡）与 style_genre（emo rock）描述，非人声控制项 |
| 和声 | 30 | style_genre 的 production/note 中出现，非结构化槽位 |
| 低频 | 8 | style_genre 的 note/production，非独立混音标签 |
| 混响 | 17 | 同上，散落描述 |
| 颤音 | 9 | instruments 的 technique 列（演奏技法） |
| 贴唱 | **0** | 无 |
| 母带 | **0** | 无 |
| 分轨 | **0** | 无 |
| 层次感 | **0** | 无 |
| 声场 | **0** | 无 |

结论：
- **不是“完全没有”**：气声、混响、低频、和声等语义在 style_genre/instruments 的描述文本里已有；
- **但“不可机读、不可按需注入、不可校验”**：它们散落在 note/production/technique 段落中，不是独立行、没有 trigger/适用段/适用角色，无法像 lyric_craft 那样“引用必达”，也无法做覆盖门。
- 截图的 9 条正好是“人声层 4 + 混音层 5”的**最小可用词表**；市场同类产品（HookGenius 声称 14 类 vocal archetype、300+ tags）比这更细。

---

## 二、市场扫描（截至 2026-10-19）

### 2.1 Suno 当前版本与字段（E3，交叉验证）

| 事实 | 内容 | 来源 | 证据等级 |
|---|---|---|---|
| 当前模型 | **v6**（2026-09-09 发布）；v6 / v6-wild / v6-mini；v5、v4.5、v4 已从模型选择器移除 | hookgenius.app/learn/suno-v6-guide；zinstrel.com | E3 第三方 |
| 字段上限 | Style **1,000 字符**；Lyrics **5,000 字符**；Exclude Styles **1,000 字符**（与 v5.5 相同） | hookgenius.app/learn/suno-v6-vs-v55（站内计数器实测） | E3 第三方实测；官方页未写 |
| 段落指令 | v6 **会读段落标签内的表演指令**，如 `[Bridge | Female — Whispered, pitch drifting…]`；也会读标签后的括号 cue `[Chorus] (belted, open vowels, wide)` | hookgenius.app/learn/suno-v6-guide；popularaitools.ai | E3 独立实测；官方仅称“更懂音乐语言” |
| 负向控制 | 官方 **Exclude** 字段（Custom → Advanced Options，第一项）；写在 Style 里的否定词会被当成“要包含” | **help.suno.com/en/articles/3161921（官方原文）**；popularaitools.ai | **E3 官方**（无字符上限说明） |
| 参数面板 | Vocal Gender（官方）、Weirdness（官方：50%=正常）、Style Influence（官方：Loose→Strong，默认未说明）、Audio Influence（官方：仅 Audio Upload 时出现）、**Variety（第三方）**、My Taste（官方） | help.suno.com/en/articles/6141377、10153473、11362561；hookgenius.app | 官方 + 第三方混合 |
| 一致性能力 | Personas、Voices（v5.5，Pro/Premier）、Custom Models（Beta，100 credits）、My Taste | hookgenius.app；Suno v5.5 发布说明转述；My Taste 官方页 | 部分官方 |
| 后期能力 | Studio/Stems：Auto Split ≤12 轨、Split from Mix、Advanced Split（Premier，近 100 件乐器） | memons.ai/suno-prompt-cheat-sheet（2026-06 更新） | E3 第三方 |

> **官方 vs 第三方（2026-10-19 复核）**：Exclude、Creative Sliders（Weirdness/Style Influence/Audio Influence）、Vocal Gender、My Taste 已由 `help.suno.com` 原文确认；**Style 1,000 / Lyrics 5,000 / Exclude 1,000 / 段内 cue 行为 / Variety 仍只有第三方实测**，未拿到官方原文。后者是 B1 架构的输入，必须在 B0.5 用实机或官方文档补证；补证前按“暂定值”处理，不写成永久硬编码。

### 2.2 当前最成熟的提示词结构（E3）

**六层模型**（多家独立指南在 2026-09 后收敛到同一结构，非 Suno 官方原文）：

1. **Genre & era + BPM/拍号**（先定世界）
2. **Vocal performance**（角色/音色 → 唱法 → 效果，三层）
3. **Instrumentation & performance**（乐器 + 怎么演奏 + 房间）
4. **Arrangement / energy / mood**（段落间怎么变）
5. **Production & mix**（贴耳/混响/宽度/低频/母带）
6. **Tail trigger**（结尾怎么收：延音/骤停/淡出）

**五槽位简版**：曲风 → 情绪 → 主奏乐器 → 节奏/BPM → 人声与制作质感。

**标签数量**：8–15 个最佳；<5 太虚，>20 互相稀释。

**v6 的关键变化**：
- 段落级表演指令（per-section cue）比“全曲 Style”更能控制“副歌怎么唱、Bridge 怎么变”；
- 双 take 方差可能**大于**一次提示词改动的影响；调 Variety 低再 A/B；
- Style Influence 默认 50%，精确 Style 会被稀释一半；
- 否定词必须进 Exclude，不能写在 Style 里。

### 2.3 同类产品的最成熟做法（E3）

| 产品 | 关键做法 | 对本项目的启示 |
|---|---|---|
| **HookGenius** | 两阶段流水线（先写词，再转 Suno Style）；14 类 vocal archetype；300+ 标签；**critic pass 自评分再修**；Album Mode（≤128 首）；发音引擎（40+ 语言） | 与现有“多角色 + 校验员”同构；缺的是“自评分 + 专辑一致性 + 发音修正” |
| **SunoPrompt** | 提示词 + 直接生成音频 + 分轨 + MIDI | 单做提示词的天花板；若不做音频，就要把“可粘贴质量”做到极致 |
| **SunoMetaTagCreator** | 拖拽 metatag + Chrome 扩展 + 母带 | 工具化/低摩擦是另一条路 |
| **Suno Prompt Builder** | 免费、免登录、滑块 | 低端市场已被免费工具占据 |
| **国产（Qwen-Music / 快乐虾米 / 海绵 / 天音 / Mureka V9 / MiniMax Music-2.6）** | 中文咬字/声调明显优于 Suno；Mureka V9 结构完整、可分轨；Qwen-Music 盲测对 Suno V5.5 胜率 50.3% | “中文歌”与“Suno”存在结构性错配；多平台或中文优先是产品分叉 |

> 国产对比来源为媒体实测（mp.weixin.qq.com、toutiao.com），属 E3 且样本有限；只作为方向证据，不作为定论。

### 2.4 评估方法（E3，工程侧最成熟做法）

- **一次只改一个变量**；固定 baseline、模型、歌词、段落顺序、音量。
- **50–200 个代表性样本**才谈得上显著性；<50 只能发现灾难性回归。
- **盲评 + rubric**；LLM-as-judge 必须先对 150–250 条人工标注校准，一致率 ≥80% 才可用。
- **报告分布而非均值**；位置偏差要随机化；同族模型当裁判会有风格偏好。
- **v6 双 take 方差**：必须固定 Variety/多次生成，否则测到的是噪声。
- 成本/时延与质量一起记；回归测试 = 固定版本的 A/B。

> 这直接打脸现有 40 例评估：10 条提示词重复成 40 次、无盲评、无 judge 校准、无方差控制。它是“能跑通”的证据，不是“更好”的证据。

### 2.5 多平台语法差异（Q2=B 的可行性底稿，E3）

| 平台 | 输入结构 | 风格语法 | 歌词/结构 | 参考音频 | 备注 |
|---|---|---|---|---|---|
| **Suno v6** | Style + Lyrics + Exclude + Settings | 逗号标签，8–15 个，Style 1,000 字符 | 5,000 字符；`[Section]` + `(cue)`；段落内表演指令 | Persona / Voices / Custom Model | 当前最完整的公开语法 |
| **Mureka V9/V9.5** | 歌词栏 + 曲风栏（或 API 的 `lyrics` + `desc`） | `[Genre]+[Mood]+[BPM]+[Instruments]+[Vocal]+[Structure]+[Theme]`；API `desc` 逗号列表，`vocal_gender` 强制性别 | 歌词 50–5,000 字符；`[Verse]/[Chorus]` | **风格参考 + 旋律种子 + 声音参考（vocal_id）**，是它的强项 | API 成熟，适合做生成闭环 |
| **海绵音乐** | 自然语言 | 五要素：曲风 + 情绪 + 结构 + 人声 + BPM | 支持歌词/描述模式 | 未公开 | 中文/短视频生态强，语法最宽松 |
| **Qwen-Music / 快乐虾米** | 自然语言 | 无公开字段语法；媒体实测对 Suno V5.5 盲测 50.3% | 支持结构标签 | 未公开 | 中文咬字强，语法需实测探针 |
| **网易天音** | 自然语言 | 中文流行/民谣/古风，虚拟歌手 | 支持歌词 | 未公开 | 一键发行网易云 |
| **Udio** | Style + Lyrics | 自由文本/标签 | 支持结构标签 | 有参考/扩展能力 | 电子/氛围强，中文弱 |

**结论**：多平台不是“把同一段提示词复制五份”，而是**五套字段语法 + 三种参考音频能力 + 不同的歌词长度/结构约定**。Suno 与 Mureka 的公开语法最完整，适合先做深；海绵/天音/Qwen 需要“自然语言适配 + 实测探针”才能确定最佳格式。

### 2.6 调研方法与覆盖度自评（回答“综合还是片面”）

**方法**：目标导向的多源交叉（official → independent field test → competitor teardown → evaluation methodology），不是穷举式行业研究。

| 已覆盖 | 具体来源 | 强度 |
|---|---|---|
| Suno 官方功能 | Exclude、Creative Sliders、Vocal Gender、My Taste 四篇 help.suno.com 原文 | 官方 |
| Suno v6 行为/字段 | HookGenius（v6 guide / limits / vs v5.5）、zinstrel、memons、popularaitools | 第三方实测 |
| Mureka 语法/API | 2026 训练指南、V9.5 中文实测、useapi API 文档 | 第三方/API 文档 |
| 国产平台 | AIX 财经横向实测、两篇微信实测 | 媒体实测，样本有限 |
| 同类产品 | HookGenius 对比页（自述偏向）、SunoPrompt/SunoMetaTagCreator 简介 | 厂商/自述 |
| 评估方法 | LLM A/B、judge 校准、prompt 回归三篇指南 | 通用工程实践 |

**未覆盖（关键缺口）**：

1. **Suno v6 官方 release notes / 字段上限原文**——未拿到；1,000/5,000/Exclude 1,000 仍是第三方实测。
2. **实机探针**——没有账号/未生成音频，无法亲手验证段内 cue、Exclude 效果、Variety 是否存在。
3. **Mureka 官方 API 文档**——只读到第三方 `useapi.net` 的封装文档；`desc`/`lyrics`/`vocal_gender` 的官方约束未确认。
4. **Qwen-Music / 海绵 / 天音官方语法**——未找到公开字段文档，只能按自然语言预设。
5. **Udio / Stable Audio / MiniMax Music 2.6**——只做了浅层对比，未纳入语法设计。
6. **社区证据**（Reddit/Discord/YouTube 实测）——没做；可能漏掉“某功能实际无效”的反例。
7. **学术/行业评估指标**（CLAP/FAD、听感盲测）——没做；文本 rubric 是工程折中。
8. **版权/商用条款**——没做；多平台输出涉及商用时的合规提示未覆盖。

**置信度必须拆开说**：

| 对象 | 置信度 | 说明 |
|---|---|---|
| **你的需求/目标**（Q1–Q8） | **95%** | 8 个分叉全部拍板；范围、边界、验收、平台顺序都明确 |
| **市场事实**（谁强、主流做法） | **75%** | 多源交叉，但样本偏英文 SEO/少数媒体 |
| **平台具体字段/行为**（1,000/5,000/cue/Exclude/Variety） | **65–70%** | 部分是第三方实测；未实机、未拿到官方原文 |
| **实现方案**（IR/渲染器/校验器） | **90%** | 结构本身不依赖具体数字；数字做成 profile 可配置 |

> 所以“95%”是**需求置信度**，不是“市场事实置信度”。上一轮把它写成笼统的 95%，是我的表述不严谨。

**补证方案（B0.5，0.5–1 天）**：官方文档/ release notes → 实机探针（若有账号）→ 竞品矩阵（8–10 家）→ 社区反例 → 把 `platform/profiles.rs` 的每个数字标注 `official / hand-tested / third-party`，第三方值只做“暂定上限”，不写死。

---

## 三、差距分析：26 条工程问题之外，还有 12 条市场对齐缺口

> 这 12 条**不在**原审计的 26 条里。原审计是“代码是否兑现宣称”，这一节是“产品是否对齐 2026-10 的市场事实”。

| ID | 缺口 | 现状（E2） | 市场事实（E3） | 影响 | 候选方案 |
|---|---|---|---|---|---|
| M-01 | **段内表演指令缺失** | 输出格式：`[Verse]` 下一行 `[乐器, 空间, 人声, 能量]`；没有 `[Verse] (soft, close-mic)` 这种段内 cue | v6 明确读取段落标签内的表演指令；这是 v6 最大的提示词变化 | 说明行可能被当成未知标签/歌词，而不是“这一段怎么唱” | 输出格式升级：`[Section]` + 括号 cue；说明行保留为“编曲备注”但不作为唯一载体 |
| M-02 | **Style 预算只用 35%** | 硬门 ≤350 字符 | Style 字段 1,000 字符；成熟做法 8–15 标签 | 主动放弃 650 字符的控制力 | 把上限改为 800–1,000，按“六层”分配预算；保留“简洁版/完整版”两档 |
| M-03 | **Exclude Styles 缺位** | 无任何负向字段输出 | 官方 Exclude 1,000 字符；否定词写在 Style 里会反向 | 想排除的元素（如电子鼓、auto-tune）无法稳定排除 | 输出增加 `Exclude:` 块；把 lyric_craft 的 negative 语义扩展到 style 层 |
| M-04 | **参数面板不对齐** | 只输出 Weirdness / Style Influence / Audio Influence=0 | v6 有 Weirdness / Style Influence（默认 50%）/ Variety | 不告诉用户调 Variety/Style Influence，A/B 结果被方差和默认值污染 | 输出增加“建议设置”：Variety 低（测试）/高（探索）、Style Influence 建议值 |
| M-05 | **Persona/Voices/Custom Models 缺位** | 无 | v5.5+ 的核心一致性能力；Persona 保存声音指纹，Custom Model 吸收风格 | 多首歌之间人声/风格漂移，用户仍要每次重写 | 增加“声音/风格档案”槽位；输出可复用的 Persona 描述与 Custom Model 提示 |
| M-06 | **单平台锁定** | 只面向 Suno | 中文歌在 Suno 上咬字/声调弱；国产模型中文更稳；Qwen-Music 盲测打平 Suno V5.5 | 中文用户的实际最佳平台可能不是 Suno | 平台适配层：Suno（v6 语法）/ Mureka / Qwen-Music / 海绵；至少输出“平台迁移说明” |
| M-07 | **人声/混音词汇未结构化** | 散落在 note/production/technique | 截图 9 条 + 市场 300+ tags / 14 vocal archetypes | 无法按需注入、无法覆盖检查 | 新增 `vocal_mix.csv`（人声 4 类 + 混音 5 类，后续扩到 20–30 条），带 trigger/适用段/角色 |
| M-08 | **无发音/咬字修正** | 只有“咬字清晰”这种形容词 | Suno 中文咬字弱是公开短板；HookGenius 有发音引擎 | 中文歌词仍可能吞音/多音字错读 | 增加“拼音/多音字/同音字”提示层；对高风险字给替代写法 |
| M-09 | **无听感评估** | 只有文本硬校验；7 维评分卡未执行 | 成熟做法是盲听 rubric + 人工/LLM judge | 无法回答“这一版是否更好听” | 建立 7 维盲评表（每维 0–2），先人工 12–20 例校准，再考虑 LLM judge |
| M-10 | **评估不抗方差** | 10 条提示词 × 重复 = 40 次；无 blind/judge/Variety 控制 | v6 双 take 方差 > 提示词改动；50–200 例才显著 | 40 例的“通过率”不能证明质量提升 | 改成“固定 20–30 条提示词 + 每版本 2 take + 盲评 + 只改一个变量” |
| M-11 | **无多曲一致性** | 单曲生成 | HookGenius Album Mode ≤128 首；Suno Persona/Custom Model | 做专辑/系列内容时风格漂移 | 增加 album 上下文（共享 style/persona/关键词），输出一致性检查 |
| M-12 | **Audio Influence=0 硬门** | A/B 模式强制 `Audio Influence=0`，无参考音频 | v6 支持参考音频/Persona 思路；Audio Influence 非 0 可用于风格锚定 | 想用参考音频的用户被硬门挡住 | 把“0”改为“默认 0，可选参考音频模式”；硬门只校验“显式声明” |

### 3.1 范围锁定（2026-10-19 二轮答复后）

用户二轮拍板：**Q4 = 平台深度适配即可（允许按平台改格式）；Q5 = A 只适配提示词；Q6 = B 提示词 LLM 文本评估**。

据此，本期范围重定义：

| 项 | 决定 | 影响 |
|---|---|---|
| 音频生成 | **不做** | 不接 Suno/Mureka 生成 API，不做应用内音频、入库、自动评估 |
| 音频盲评 | **不做** | M-09 从“听感评估”改为“**提示词文本质量评估**”；M-10 从“抗音频双 take 方差”改为“抗 LLM 文本方差” |
| 多平台 | **做**，但只做**提示词适配层** | 每个平台一套输出 schema + 语法适配 + 校验器；不做平台账号/额度/生成闭环 |
| 参考音频 | **只输出文本指引** | M-05 Persona、M-08 发音、M-12 Audio Influence 都只在提示词层表达，不实际调用 |
| “彻底解决” | **26 工程 + 12 市场缺口全做** | 允许破坏性格式升级；旧历史兼容读取，旧输出格式不再默认 |
| 验收 | **真 LLM 跑文本 + 文本 rubric** | 能证明“提示词写对了/覆盖全了/不套话”，**不能证明“歌好听”**；后者需音频盲评，本期明确不做 |

> 诚实声明：文本验收是“生产输入的质量”，不是“最终音频的质量”。如果后续要回答“是否真的更自然/更有专业质感”，必须补音频盲评（M-09），本期不做。

---

## 四、26 条工程问题的“解决方案到位度”

| 状态 | 数量 | 条目 |
|---|---|---|
| 可直接施工 + 有验收命令 | **6** | A-01（钥匙串测试隔离）、B-03（历史迁移不覆盖）、C-10（明文残留清洗）、C-01（vitest 并发）、C-13（CHANGELOG/版本）、C-14（QA 证据去重） |
| 有明确代码改动方向，但未实施 | **14** | C-02、C-04、C-05、C-06、C-07、C-08、C-09、C-11、C-12、D-01、D-02、D-03、D-04、D-05 |
| 需产品决策才能定方案 | **3** | B-01（信封降级策略：继续纠错 vs 明确降级 vs 跳过保真）、B-02（Mode A 配器门：改门 vs 改 prompt）、C-03（韵脚：实现轻量门 vs 改 README） |
| 需真实数据/预算才能验证 | **2** | B-02 的效果验证、B-01 的降级率改善 |
| 仅有方向、需补语料/工具 | **1** | C-03（韵脚需要语料/拼音库） |
| **已实施并验证** | **0** | — |

> 合计 26（6+14+3+2+1 有交叉，按主状态计：6+14+3+1+2 中 B-02 同时属“需决策/需验证”，去重后 26）。
> **结论：26 条都有“修复方向”，但没有一条已经落地；其中 6 条可以在半天内做完并用命令验收。**

---

## 五、把截图“吸收”成什么（候选，不是最终方案）

### 方案 A：新增 `vocal_mix.csv`（最小侵入）

```csv
id,category,label_cn,label_en,keywords,description,trigger,applies_to,source
V-01,vocal,清晰咬字,"clear diction, upfront lead",字正腔圆|吐字清晰|无吞音,主唱人声清晰靠前…,审改/全程,lead,截图
...
M-05,mix,母带质感,"mastered, wide dynamics, polished",母带|动态|高清晰,经过母带处理…,审改/全程,mix,截图
```

- 注入：按角色（作词/制作/校验）+ 段落（Verse/Chorus）+ 模式；
- 守护：`id` 唯一、`trigger` 已登记、`label_en` 非空；
- **不设硬门**，只做“按需命中 + 引用必达”（沿用 craft 表的机制）。

### 方案 B：把“人声层 + 混音层”变成产出必填槽位

- 信封增加 `<<<VOCAL>>>` / `<<<MIX>>>` 两节，或要求 Style Prompt 至少各含 1 条；
- 硬门：缺一即打回；
- 风险：与现有 200 字说明行、350 字 Style 上限冲突；会提高降级率。

### 方案 C：升级成“Suno v6-ready 输出包”

- `Style Prompt`（≤1,000，六层）
- `Exclude`（负向）
- `Lyrics`（`[Section]` + `(cue)` 段内指令）
- `Settings`（Weirdness / Style Influence / Variety）
- `Persona/Consistency`（可选）
- 这是市场最成熟形态，但改动最大，会影响现有信封/保真/校验全链。

> 三个方案不是互斥的；A 可以先行，B/C 需要产品决策。

---

## 六、来源（访问日期 2026-10-19）

| 来源 | 用途 | 强度 |
|---|---|---|
| https://hookgenius.app/learn/suno-v6-guide | v6 段内 cue、Exclude、Variety、字段上限 | E3（独立实测，非官方） |
| https://hookgenius.app/learn/suno-v6-vs-v55 | 1,000/5,000/1,000 字段、双 take 方差 | E3 |
| https://hookgenius.app/learn/suno-prompt-guide-2026/ | 六层/五槽位、8–15 标签、vocal 三层 | E3 |
| https://memons.ai/suno-prompt-cheat-sheet | metatag 清单、Studio/Stems、Persona | E3 |
| https://zinstrel.com/tips-tools/how-to-prompt-suno-v6-and-why-it-feels-so-different | 六层模型、v6 三型号 | E3 |
| https://www.popularaitools.ai/blog/suno-v6-vocal-prompts-expression-phrasing-2026 | 段内 delivery cue、可用词表 | E3 |
| https://hookgenius.app/learn/best-suno-prompt-tools | 同类产品能力对比（含自述偏向） | E3（有利益相关） |
| https://hookgenius.app/about-author/jesse | 两阶段流水线、14 vocal archetype、critic pass | E3（自述） |
| http://mp.weixin.qq.com/s?__biz=MzkyMzI5OTI0Mw==... | 国产模型横向实测（Suno/Mureka/MiniMax/海绵/音潮） | E3（媒体实测，样本有限） |
| http://mp.weixin.qq.com/s?__biz=MzY5ODEyMTA2Mg==... | 中文咬字、平台选型、Qwen-Music 盲测 | E3（媒体实测） |
| http://ai-tldr.dev/learn/prompt-engineering/prompt-iteration/how-to-ab-test-prompts | 50–200 例、judge 校准 ≥80%、回归门禁 | E3 |
| http://flo2.com/blog/llm-ab-testing | 单变量、盲评、成本/时延 | E3 |

> 未获取到 Suno 官方帮助中心原文（登录/地区限制）；字段与行为以两家独立实测交叉为准。若后续拿到官方原文，需回填并复核 M-01–M-05。

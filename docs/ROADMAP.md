# 拾音（Shiyi Music）开源 Suno 工程深度研究与 v6.2.0 全链路落地报告

> **定位**：本文档沉淀 `v6.1.0` 发布后，针对全网头部开源 Suno Agent Skills 仓库源码（含 `SKILL.md`、`references/*.md`、校验门与混音母带链）的**源码级深度拆解**，本报告中的 10 大核心声学与工程法则已在 `v6.2.0` 全链路落地（涵盖知识库 CSV、编译器 `render_suno.rs`、双层校验器 `validate.rs`、四模式系统提示词与审改角色、前端工作台快捷胶囊与反馈路由），并严格对齐 [docs/BLUEPRINT_v3_CONTRACT.md](BLUEPRINT_v3_CONTRACT.md) 的四象限 IR 与诚实编译器契约。

---

## 一、全网开源 Suno Agent Skills 源码级调研基线

社交平台图卡中的“Suno 最火 10 个 skill”（`01 melody-prompt` ～ `10 stem-craft`）是自媒体对 10 类创作任务的概括命名；而在 GitHub 与 Agent Skills 社区中，真正实现这些能力且具备深厚工程积累的开源仓库共有 **6 个核心项目（合计 70+ 份内部参考规范文件）**。本次深度研究完整拆解了以下源码与内部手册：

1. **[bitwize-music-studio/claude-ai-music-skills](https://github.com/bitwize-music-studio/claude-ai-music-skills)**（53 个子 Skill 的工业级全链路制作插件）
   - 核心拆解文件：
     - `skills/suno-engineer/SKILL.md`（Suno v6 / v6-wild / v6-mini 提示词工程与参数表）
     - `skills/lyric-writer/SKILL.md`（13 项自动质量门 + 3 轮 Tighten/Strengthen/Flow 精修日志 + 概念专辑跨曲目 Motif 回调）
     - `skills/lyric-reviewer/SKILL.md`（14 项生成前 QC 门禁，含 Verse-Chorus Echo 拦截与同形异音词强制转写）
     - `skills/voice-checker/SKILL.md`（11 类 AI 机器味模式识别器：歌词 1–7 类 + 文案 8–11 类）
     - `skills/mix-engineer/SKILL.md`（12 轨分轨独立 EQ/压缩/立体声宽度后处理参数链）
     - `reference/suno/best-practices.md`（722 行 Suno v6 实测手册：含 K-Pop 段落切风、小节数靶向、环境音效双端锚定、模型偏置词黑名单）
     - `reference/suno/creative-sliders.md`（Weirdness × Style Influence × Audio Influence × Variety 四维交互矩阵与故障诊断树）
     - `reference/suno/tips-and-tricks.md`（人声显嫩/显平庸的三级破局法、局部替换单变量法则）
     - `reference/suno/structure-tags.md` & `voice-tags.md` & `instrumental-tags.md` & `artist-blocklist.md`（80+ 禁用艺人声学转译字典）
2. **[NuNaught/suno-songwriting-skill](https://github.com/NuNaught/suno-songwriting-skill)**（及作者在 r/SunoAI 的完整架构说明书）
   - 核心拆解：单括号合并指令机制（Combined Bracket Embedding）、6 大类段头 Enriched Cues 词典、乐器 8 大职能动词体系（8-Role Action Vocabulary）、跨流派融合四要素公式（Anchor + Contrast + Bridge Mechanism + Production Binder）。
3. **[regiellis/suno-songwriter-agent-skill](https://github.com/regiellis/suno-songwriter-agent-skill)**
   - 核心拆解：`references/v6-production-direction.md`（主歌掉能量修复、结尾防截断缓冲段、Directed Brief 结构化长指令）、`references/songwriting-craft.md`（Hook 载体选择、物体承载情绪法、按症状修订诊断表 `Revision By Symptom`）。
4. **[noravoid-log/suno-prompt-crafter](https://github.com/noravoid-log/suno-prompt-crafter)**
   - 核心拆解：`artist-translation-bank.md`（音乐/歌词/情绪三分离艺人转译法）、Free/Pro/Premier 计划能力门控。
5. **[frankxai/creator-skills (`suno-ai-mastery`)](https://github.com/frankxai/creator-skills/blob/main/skills/music/suno-ai-mastery/SKILL.md)**
   - 核心拆解：五层提示词权重金字塔、8 分钟长曲七乐章结构、Add Vocals / Add Instrumentals / Covers / Persona 闭环。
6. **[ismilewolf-eng/suno-fanqie-music](https://github.com/ismilewolf-eng/suno-fanqie-music)**
   - 核心拆解：中文苦情/草原/现实题材的“把空泛副歌改回人话”诊断法、“物件/动作 + 矛盾”开篇锁定法。

---

## 二、从开源仓库深挖出的 10 项底层机制与工程盲区（核心干货）

对比咱们 `shiyi-music v6.1.0` 现有代码，以下是开源社区通过数千次 Suno v5.5/v6 实测踩坑总结出、而咱们当前代码尚未完全覆盖或存在旧规则冲突的 **10 项关键机制**：

### 发现 1：真实艺人名在 Suno v6 的“后台静默重写”陷阱（与咱们旧规则直接冲突！）
- **开源实测结论**（`suno-engineer` + `artist-blocklist.md` + `suno-prompt-crafter`）：
  - 在 Suno v5.5/v6 中，只要 Style 栏出现任何真实歌手或乐队名（如 `Taylor Swift`、`The Weeknd`、`BLACKPINK` 等），Suno 会弹出 `"Artist name '…' replaced"` 并**在服务端强行用模板改写你的整个 Style 框**，导致精心设计的声学标签被冲掉。
- **咱们代码现状与隐患**：
  - 在 `src-tauri/src/commands/prompts.rs` 与 `src-tauri/knowledge/suno_rules.csv` 中，仍保留着早期规则：*“艺人参考：like X meets Y，可选，直接写艺人名”*。
- **v6.2.0 升级方案**：
  1. **彻底废除“直接写艺人名”提示**：改为**「艺人四维声学转译法（Artist 4D Translation）」**——当用户在输入中提到“像某歌手”时，主持人必须在 `global` 中将其拆解为：`年代与流派分支 (Era/Genre)` + `音域与嗓音颗粒度 (Register/Grain)` + `标志性律动配器 (Signature Groove/Instrument)` + `空间混音质感 (Mix Texture)`，严禁将真名写入 `GlobalStyle` 或 `Section`。
  2. **在 `validate.rs` 新增 `style_artist_name_leak` 检查**：内置高频中西/日韩艺人名黑名单（并提示对应的声学替代词）。

### 发现 2：歌词框圆括号 `(...)` 被当成和声唱出的物理陷阱
- **开源实测结论**（`suno-engineer/SKILL.md` § Lyrics Box Warning）：
  - Suno 会把歌词框里圆括号 `(...)` 内的文字**当成背景和声或台词直接唱出来**！例如写 `(Instrumental break)` 或 `(drums drop, guitars explode)`，Suno 真的会找个人声唱出“instrumental break”。
  - 圆括号 `(...)` 唯一合法的用途是**尾音回声伴唱**，如 `I'm on fire (on fire!)`。一切舞台指示、器乐指令、情绪说明都必须写在方括号 `[Label | cue, cue]` 内部。
- **咱们代码现状与隐患**：
  - `render_suno.rs` 的 `SectionHeader::PipeInline` 把段头编译成了安全的 `[Label | cue]`，但：① `render_suno.rs` 仍留有未使用的 `BracketThenParen` 分支；② `validate.rs` 的 `check_lyric_lines` 只拦截了行首尾 `[...]` 泄漏（`lyric_bracket_leak`），**没有拦截模型在 `sections[].lines` 歌词行里夹带 `(吉他独奏)`、`(停顿两拍)`、`(whispered)` 等圆括号舞台指示**！
- **v6.2.0 升级方案**：
  - 在 `check_lyric_lines` 中新增圆括号舞台指示检测（`lyric_paren_direction`）：允许真实的 2–4 字回声衬词（如 `(oh yeah)`、`(不退)`），但若圆括号内出现乐器名、奏法词、混音词或“间奏/独奏/停顿/旁白/渐弱”等舞台调度词，立即触发 `Severity::Hard` 打回。

### 发现 3：`[Intro]` 裸标签失控与“小节数靶向语法（Bar Count Targeting）”
- **开源实测结论**（`structure-tags.md` & `best-practices.md`）：
  - 单独写 `[Intro]` 在 Suno v6 的 `Duration: Auto` 下极不稳定，常生成长达 30–45 秒的冗长器乐前奏，甚至导致第一段主歌人声被吞（Lyrics Not Audible）。
  - 开源验证的两种高可靠写法：
    1. 使用 `[Short Instrumental Intro]` 或 `[Intro - Spoken]` 替代裸 `[Intro]`；
    2. **小节数靶向语法（Bar Count Targeting）**：在段名后紧跟目标小节数，如 `[Intro 4 | ...]`、`[Verse 1 8 | ...]`、`[Pre-Chorus 4 | ...]`、`[Chorus 8 | ...]`，能显著改善前奏/尾奏拖沓失控。
- **v6.2.0 升级方案**：
  - 在 `render_suno.rs` 中，当 `SectionKind::Intro` 为纯器乐且 `label == "Intro"` 时，若 `target_seconds` ≤ 10 秒或处于紧凑曲式，自动优化段头提示为 `short instrumental intro`；并可在 `Section` 可选支持小节数标注。

### 发现 4：主歌末句与副歌首句的“回声泄气效应（Verse-Chorus Echo）”
- **开源实测结论**（`lyric-writer/SKILL.md` & `lyric-reviewer/SKILL.md` 第 13 条铁律）：
  - 如果 Verse 或 Pre-Chorus 的最后 1–2 行，提前使用了紧接着的 Chorus 首行的**核心意象词、Hook 原句或完全相同的押韵尾字**，副歌砸下来的一瞬间就会丧失悬念与反差冲击力（"The verse gives away the chorus before it hits"）。
- **咱们代码现状与隐患**：
  - 咱们的 `validate.rs` 已经锁死了同段内的同音挤韵（`rhyme_homophone_collision`）和跨段同职能韵部统一，但**尚未检查「主歌/导歌末行 ↔ 副歌首行」交界处的尾字重字或核心短语剧透**。
- **v6.2.0 升级方案**：
  - 在 `check_prosody_and_rhyme` 或 `check_anti_ai_tells` 中新增 `verse_chorus_echo` 检查：比对每个 `Verse/PreChorus` 的末行与紧随其后的 `Chorus/Hook` 首行，若两行尾字完全相同或复用了连续 ≥4 字的相同短语，给出告警或修正指引。

### 发现 5：人声“形容词失效定律”与三级声学破局法（Vocal Age & Character Escalation）
- **开源实测结论**（`tips-and-tricks.md` + `voice-tags.md`）：
  - **为什么写 `mature, deep` 还是出幼态甜妹音？** 因为同提示词里的 `intimate, breathy, delicate, pop` 在 Suno 训练集里与年轻声线强绑定，权重直接盖过了抽象的年龄词 `mature`。
  - **为什么写 `powerful, defiant, emotional` 还是出平庸罐头流行嗓？** 因为情绪形容词描述的是“心理意图”而非“声带物理振动”，Suno 无法把 `defiant` 映射成频谱，只能回退到默认流行音源。
  - **三级破局法**：
    1. **物理音域 + 声带纹理词替代年龄/情绪词**：想要成熟沧桑女声必须写 `contralto, low register, smoky, weathered, resonant`（并删掉 `breathy/delicate`）；想要力量撕裂感必须写 `mezzo-soprano / baritone, gritty chest voice, rasp on belted notes`。
    2. **Exclude 栏反向去幼态/去罐头**：在 `global.negative` 填入 `youthful vocals, generic polished studio pop vocal`。
    3. **段头内联声线强锚定（Inline Vocal Metatag Escalation）**：对顽固流派，在每段 `positive_local` 重复锚定 `husky contralto vocal`。
- **v6.2.0 升级方案**：
  - 将这套“音域（`contralto/baritone`）+ 物理颗粒度（`smoky/weathered/gravelly`）+ Exclude 反向排斥”的映射规则写入 `vocal_mix.csv` 与情感/制作人 Prompt。

### 发现 6：`Voice-Checker` 的 7 类歌词 AI 机器味精细分类学
- **开源实测结论**（`voice-checker/SKILL.md` Classes 1–7）：
  - 除咱们已实现的“抽象词堆砌（Class 1）”和“大团圆强行升华（Class 3）”外，`voice-checker` 还抓出了 3 个极准的 AI 写词通病：
    - **Class 2 · 比喻自解（Over-Explained Metaphors）**：上一句刚写了一个好意象（如“茶杯早就凉透”），下一句立刻用说明文解码（“就像我们的感情已经走到尽头”），把听众的留白想象空间彻底堵死。
    - **Class 5 · 廉价励志口号升级（Cliche Escalation Phrases）**：副歌动辄“冲破枷锁 / 逆风飞翔 / 重写命运 / 照亮黑暗 / 撕碎沉默”。
    - **Class 7 · 主歌句法镜像（Twin Verses / Syntactic Mirroring）**：Verse 1 与 Verse 2 不仅字数对齐（格律好），连语法结构都一模一样（如每句都是“在XX的XX里，我XX着XX”的同构换词复读）。
- **v6.2.0 升级方案**：
  - 在 `check_anti_ai_tells` 与 `lyric_craft.csv` 中补齐「禁比喻自解（写物即写情，禁止下一行用“就像/代表/说明”解释上一行物象）」与「廉价励志套语黑名单」。

### 发现 7：跨流派融合四要素公式（4-Part Fusion Builder）
- **开源实测结论**（`NuNaught/suno-songwriting-skill`）：
  - 把“国风、电子、爵士”三个词并列塞进 Style，Suno 只会产出混乱的四不像。成功的跨流派融合必须拆成四个明确分工的槽位：
    1. **Anchor（律动引擎）**：唯一主干节拍与低频，如 `808 trap drums` 或 `bossa nova syncopated groove`；
    2. **Contrast（异质色彩主奏）**：1–2 件跨界特色乐器，如 `guzheng arpeggios`、`shakuhachi`、`suona`；
    3. **Bridge Mechanism（粘合机制）**：让两种流派咬合的共同音乐动作，如 `pentatonic ostinato`、`call-and-response`、`pedal drone`；
    4. **Production Binder（空间胶水）**：把它们装进同一个声学房间的混音质地，如 `warm analog tape saturation` 或 `glossy nocturnal club sub`。
- **v6.2.0 升级方案**：
  - 写入主持人与制作人的跨流派融合标准模板，对应图中 `03 genre-blender`。

### 发现 8：乐器 8 大职能动词体系（补齐 `Counterline` 与 `Texture Glue`）
- **开源实测结论**（`NuNaught/suno-songwriting-skill`）：
  - 咱们 `prompts.rs` 目前有 6 类乐器角色动词（Pulse carrier / Groove anchor / Harmonic bed / Signature hook / Impact layer / Contrast color）。
  - `NuNaught` 体系中多出两个在副歌和氛围段极其关键的职能：
    - **Counterline（复调对话线）**：`weaves, replies, shadows, circles, descends`（在副歌或桥段围绕主唱旋律游走对话，如大提琴或竹笛副旋律，能彻底解决副歌伴奏只有和弦块没有旋律流动感的问题）；
    - **Texture glue（纹理胶水层）**：`washes, smears, saturates, shimmers, breathes`（负责粘合频段间隙）。

### 发现 9：主歌掉能量空洞（Verse Drop-Out）与结尾吞字缓冲（Ending Buffer）
- **开源实测结论**（`regiellis/v6-production-direction.md`）：
  - **主歌掉伴奏**：Suno v6 在生成大动态曲目时，常把 Verse 1/2 的伴奏抽得过于干净，导致主歌像清唱干拔。对需要持续律动的曲风（摇滚/流行/燃曲/舞曲），需显式声明 `active bass and drums under every verse, verses maintain forward momentum`。
  - **结尾吞尾字**：Suno v6 偶发在最后一句歌词还没唱完时就提前切音。必须在最后一段歌词后留出器乐缓冲段（`[Outro | short instrumental outro, fade out]` + `[End]`），或在 Style 中声明 `complete every written lyric before a short instrumental outro`。
  - **模型偏置词警报（Token Biases）**：Suno 底层权重极度偏爱塞入 `Neon, Echo, Ghost, Silver, Shadow, Whisper, Crystal, Velvet` 等词，英文写作或审查时需重点防范。

### 发现 10：Suno 12 轨分轨（Stem Splitting）的真实串音物理边界与预混音策略
- **开源实测结论**（`mix-engineer/SKILL.md` + `best-practices.md § Stem Extraction`）：
  - Suno 的分轨有三种模式：`Auto Split`（一次拆 12 轨）、`Split from Mix`（单拆人声+伴奏 2 轨）、`Advanced Split`（从约 100 种乐器中单拎 1 轨）。
  - **关键实测真相**：`Auto Split` 拆出的 12 轨在物理上仍带有频谱串音（bleed/crosstalk）和共享混响尾音；且因为 Suno 音轨是神经网络合成而非麦克风实录，**对分轨做降噪（`noise_reduction`）反而会削掉人声辅音字头和呼吸声**！
  - **对提示词工程的反向指导（对应图中 `10 stem-craft`）**：如果用户生成歌曲的目的是拆伴奏翻唱或进 DAW 混音，必须在生成阶段做**「分轨预净化（Pre-Stem Isolation Prompting）」**：
    1. `global.vocal` 锁定 `dry close-mic lead vocal, natural pitch`；
    2. `global.negative` 强制排除 `crowd vocals, choir, gang vocals, heavy hall reverb, delay throws, autotune`（防止群唱和长混响串进伴奏轨）。

---

## 三、图中 10 个 Skill 升级版全景落地方案（v6.2.0 路线图）

结合上述 10 项底层发现，将图中的 10 个热门能力转化为咱们 `shiyi-music` 可直接落地的工程特性：

| 图中 Skill 名称 | 咱们 `v6.1.0` 已具备基础 | 结合开源深度研究后的 `v6.2.0` 升级动作 |
| :--- | :--- | :--- |
| **01 `melody-prompt`**<br>一句词生成完整歌 | `Mode B` + 立意五问 `SongIntent` + 直通管线 | 引入 `[Short Instrumental Intro]` 默认保护与结尾 `[Outro] + [End]` 防吞字缓冲 |
| **02 `lyric-weaver`**<br>主题情绪扩写歌词 | `Mode B` + 十三辙全局硬门 + 同旋律格律锁死 | 在 `validate.rs` 新增 **`verse_chorus_echo`（禁主歌末行剧透副歌首行/同字撞韵）** 与 **比喻自解（Class 2）** 检测 |
| **03 `genre-blender`**<br>多流派跨界混搭 | `render_suno` 流派数量收敛 | 在制作人 Prompt 与 `compose_craft.csv` 植入 **「四要素融合公式（Anchor 律动锚 + Contrast 异质乐器 + Bridge 共同音型 + Binder 混音胶水）」** |
| **04 `beat-forge`**<br>先定鼓点节奏再编曲 | `global.bpm` / `meter` + `style_genre.csv` | 引入 **Pulse & Pocket 切分词汇表**（`half-time shift`, `locked groove`, `behind-the-beat swing`）与主歌不断拍保护（`active bass and drums under verse`） |
| **05 `vocal-magic`**<br>男女声线与演唱质感 | `VocalGender` 枚举 + `vocal_mix.csv` (`V-01..V-08`) | ① 落地 **「物理音域 + 声带颗粒度 + Exclude 排斥幼态/罐头音」三级声线锚定法**；<br>② 当 `vocal_gender == Duet` 时强制各段 `positive_local` 标明男女轮唱与合唱分工 |
| **06 `hook-maker`**<br>聚焦副歌上头记忆点 | `Mode D` + `hooks.csv` + `arrangement_hook` | 引入 **「Bridge 抽空 + Final Chorus 半音升调（`key modulation up a half-step`）」** 与 **第 7 类乐器职能 `Counterline`（副歌复调对话线）** |
| **07 `mood-shifter`**<br>同首歌切换治愈/燃炸 | `Mode A` 锁词换编曲 + 结果面板 `Refine` | 在结果面板提供 **「平行宇宙一键换风格（治愈民谣 / 燃系摇滚 / 夜店电音 / 复古蒸汽波）」** 预设重跑按钮 |
| **08 `soundtrack-lab`**<br>短片/Vlog/游戏配乐 | 纯器乐段头自动补 `instrumental` | 新增 **「纯器乐 OST / BGM 模式开关」**：开启后跳过作词门与 `vocal` 覆盖门，采用 `[Intro 4]` → `[Main Theme 8]` → `[Bridge 4]` → `[Outro 4]` → `[End]` 小节数靶向结构 |
| **09 `cover-remix`**<br>旧旋律重做编曲翻唱 | `settings.reference_audio` + `audio_influence` | ① **废除直接写艺人名，全面升级为「艺人四维声学转译（`artist_blocklist` 拦截）」**；<br>② 开启 `reference_audio` 时自动锁定 `variety = 0`、`audio_influence = 75`，防止翻唱跑调 |
| **10 `stem-craft`**<br>拆分人声与伴奏二创 | 单段 ≤5 件乐器硬门 + `M-04` 分轨层次感 | 新增 **「二创分轨预净化（Stem-Clean）」** 选项：自动配置干声贴耳主唱并在 `global.negative` 排除 `choir, crowd vocals, heavy hall reverb, delay throws` 防分轨串音 |

---

## 四、垂直流派基因库预备行（`style_genre.csv` 14 列规格）

以下 2 条高频流派已彻底剥离视觉类伪提示词，符合 `src-tauri/knowledge/style_genre.csv` 14 列结构与双字词元可达性守卫：

```csv
二次元燃系摇滚,anisong J-rock,160-185,overdriven electric guitar fast piano arpeggio live drums strings,热血 疾走 番剧OP,清亮 高亢 穿透力,18-28,82-90,高速钢琴琶音+双吉他失真Riff+弦乐齐奏 预副歌半拍停顿后副歌全开王道进行 适合热血番剧主题曲 忌拖沓慢铺垫,十六分疾走双踩 预副歌切分急停,王道进行 IV-V-iii-vi,4/4,双吉他宽声场 钢琴高频穿透 鼓组瞬态锐利,二次元 二次元动漫 动漫主题曲 日系燃曲 J-Pop J-Rock 番剧OP
新中式禅意疗愈,zen ambient,55-72,guqin xiao singing bowl sparse felt piano,空山 晨雾 茶席,气声 吟唱 器乐为主,15-25,80-88,古琴泛音+洞箫长气息+颂钵余韵 大量留白呼吸 适合冥想/茶空间/东方写意疗愈 忌密集鼓点与现代电音合成器,散板 自由呼吸 极慢脉冲,五声宫商角徵羽 悬浮持续音,4/4 或散板,超长自然堂音 极简分轨 动态轻柔,新中式禅意 禅意音乐 冥想音乐 疗愈轻音乐 茶空间音乐 古琴冥想
```

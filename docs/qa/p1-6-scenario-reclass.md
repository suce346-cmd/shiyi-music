# P1-6 suno_rules scenario 重分类（RULE-SCENARIO-VOCAB，C-02）

词表：`all`（全模式）/ `douyin`（仅 mode_d）/ `normal`（仅 mode_a/b/c）；未知值不注入 + 启动 error + 守护测试红。

注入行数：mode_d 27/40（旧 38/40），mode_a/b/c 33/40。

| rule | 旧值 | 新值 | 理由 |
|---|---|---|---|
| audio_influence | 无参考音频 | all | 无参考音频是全模式前提 |
| min_energy_gap | 创作 | normal | 能量差门仅 A/B（C 随 A/B 文本组） |
| hook_min_count | 抖音 | douyin | 原 抖音 → 词表 token |
| verse_max_lines | 抖音 | douyin | 原 抖音 → 词表 token |
| line_max_chars | 抖音 | douyin | 原 抖音 → 词表 token |
| line_chars_verse | 中文 | normal | 原 中文(普通模式) → 词表 token |
| line_chars_chorus | 中文 | normal | 原 中文(普通模式) → 词表 token |
| style_arc_dry | 叙事 | normal | A/B 弧线引导度，D 用抖音区间 |
| style_arc_high | high | normal | A/B 弧线引导度，D 用抖音区间 |
| style_arc_highlow | 高开低走 | normal | A/B 弧线引导度，D 用抖音区间 |
| style_arc_flat | 平摊 | normal | A/B 弧线引导度，D 用抖音区间 |
| style_arc_drama | 起伏 | normal | A/B 弧线引导度，D 用抖音区间 |
| style_arc_stair | 阶梯上升 | normal | A/B 弧线引导度，D 用抖音区间 |
| style_arc_buildup | 渐进爆发 | normal | A/B 弧线引导度，D 用抖音区间 |
| style_arc_u | U型 | normal | A/B 弧线引导度，D 用抖音区间 |
| style_arc_singlepeak | 单峰 | normal | A/B 弧线引导度，D 用抖音区间 |
| style_arc_loop | 回环 | normal | A/B 弧线引导度，D 用抖音区间 |
| hook_first | 抖音 | douyin | 原 抖音 → 词表 token |
| stop_mark | 抖音 | douyin | 原 抖音 → 词表 token |
| desc_line_max | 全模式 | all | 全模式同门；描述去掉信封门措辞 |
| verse2_new | 创作 | all | 创作类规则 D 也写词 |
| imagery_unity | 创作 | all | 同上 |
| anti_cliche | 创作 | all | 同上 |
| hook_protect | 抖音 | douyin | 原 抖音 → 词表 token |
| turn_point | 创作 | all | 同上 |
| bpm_match | 抖音 | douyin | 原 抖音 → 词表 token |

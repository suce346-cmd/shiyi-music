# §5.1#5 文本校验退役前对照（2026-10-20）

## 文本路径（`validate_for_mode` → production / lyric_fill / douyin）vs `plan::validate`

| 硬门（文本） | 文本执行点 | IR/`plan::validate` 等价 | 备注 |
|---|---|---|---|
| Style Prompt 存在 | `validate_production` | schema + render 必有 style 串；长度门 `style_prompt_length_issues` via `check_style_and_headers` | 存在性由结构化字段保证 |
| Style 长度 ≤350 / ≥30 | `style_prompt_length_issues`（曾由 collect_hard_issues 接线） | 同函数，经 `check_style_and_headers` | **保留** helper |
| 结构标签 ≥N | `extract_section_tags` | `check_structure` sections.len | |
| 能量差 ≥3 | `extract_energy_values` | `check_arc` energy | |
| 配器差/弱强段/上限/极简 | `extract_section_instrument_counts` | `check_arc` + `section_floor` | |
| Audio Influence=0 | `check_audio_influence` 文本 | `check_params` + `reference_audio` | 文本版退役 |
| Weirdness/Style 区间 A/B | `param_range_issues` | `check_params` | 文本版退役 |
| 断句禁 /、 | `forbidden_break_marks` | `check_lyric_lines` | |
| 说明行 ≤200 | `desc_line_length_issues` | `header_too_long`（段头 cue） | 文本扫描退役 |
| Mode C 字数/行数 | `validate_lyric_fill` | **仍调用** `validate_lyric_fill(orig, rendered.lyrics)` | helper **保留**，仅 IR 入口 |
| Mode C 韵脚 | — | `advise_rhyme` Warn | P1-8 |
| Mode D Hook/Verse/骤停/行长/BPM/参数 | `validate_douyin` 文本 | `check_douyin`→`validate_douyin_v2` + `check_params` + `check_lyric_lines` | 文本版退役 |
| 保真 | 文本 fidelity | `check_fidelity` | |
| vocal/mix 覆盖 | — | `check_vocal_mix_coverage` | IR 新增 |

## 调用方（退役前）

- 生产终稿：`orchestrator` → `plan::validate`（已切）
- 矩阵：`measure_plan` / `measure_from_events`（已禁 `collect_hard_issues`）
- 残留：`validate_for_mode` — headless_modes / headless_offline / validator 单测

## 变异实验（见 CHANGELOG / 本文件续）

对 `plan::validate` 关键门逐一短路，对应单测须红。

## 变异实验结果（2026-10-20 UTC+8）

对 `plan::validate` 关键门逐一短路，对应单测须红（生产段恢复后全绿）。

| 变异 | 短路点 | 对应单测 | 结果 |
|---|---|---|---|
| M-param_range_b | `check_params` mode_b 区间 `if false &&` | `params_by_mode` | 红 |
| M-audio_influence | `reference_audio` 条件改 `&& false` | `params_by_mode` | 红 |
| M-energy_gap | `check_arc` energy_gap 条件 `if false &&` | `energy_gap_and_instrument_gap_and_cap` | 红 |
| M-instrument_gap | 改 code 字符串（仅生产段） | 同上 | 红 |
| M-instrument_max_global | global cap `if false &&` | 同上 | 红 |
| M-break_mark | `check_lyric_lines` 断句条件 | `break_marks_and_header_length` | 红 |
| M-header_too_long | DESC_LINE 条件 `if false &&` | 同上 | 红 |
| M-douyin | 跳过 `check_douyin` 调用 | `douyin_structure_ported_to_ir` | 红 |
| M-lyric_fill | 跳过 `validate_lyric_fill` 接线 | `mode_c_lyric_fill_runs_on_rendered_lyrics` | 红 |
| M-style_length | 跳过 `style_prompt_length_issues` 接线 | `style_length_wired_through_plan_validate`（本批补） | 红 |

缺口：原先无 IR 层 `style_length` 接线单测 → 已补 `style_length_wired_through_plan_validate`。

结论：文本硬门在 `plan::validate` 均有可红覆盖；可删 `validate_for_mode` / `validate_production` / 文本 `validate_douyin` / `param_range_issues` / `check_audio_influence`。`validate_lyric_fill` + `style_prompt_length_issues` + `desc_line_length_issues`（供 lyric_fill）保留为内部助手。`collect_hard_issues` 已不在 orchestrator。

## 退役落地（同日）

已删除文本终稿入口与专用助手；调用方改 `plan::validate` / `AuditResult`；`RULE_REGISTRY` 执行者改挂 IR；矩阵/ validator 生产段 no-legacy 守卫扩展。`collect_hard_issues` 此前已不在 orchestrator（矩阵仍禁其符号）。

# 文本评估评分卡与校准（P1-13 / M-09）

## 七维（每维 0–2）

与 `docs/qa/2026-09-09-large-scale-test-plan.md` 产品维度冻结同口径：

| id | 0 | 1 | 2 |
|---|---|---|---|
| arrangement_norm | 违反硬线 | 贴线 | 全在区间 |
| arrangement_diversity | 配方重复感强 | 部分重叠 | 独立配方 |
| coherence | 配器/能量自相矛盾 | 局部矛盾 | 全曲自洽 |
| non_cliche | 结构/金句套路 | 部分套路 | 独立结构 |
| rhyme | 押韵混乱 | 部分押韵 | 一韵到底或有意换韵 |
| lyric_norm | 字数/行数违规 | 贴线 | 全部合规 |
| hook_quality | 无记忆点 | 有 Hook 无差异 | 记忆点+变化 |

总分 0–14。实现：`text_eval::DIMENSIONS` / `parse_judge_response`。

## 校准（R-10）

1. 人工标注 ≥40 条（模板 `text-eval-calibration-template.csv`）
2. 同批终稿跑 Judge，算逐维一致率 `agreement_rate`
3. `n≥40` 且一致率 ≥80% → `calibration_trusted`；此前报告恒标 **小样本**

## 成本闸门

- 设置项 **`textEvalJudge`（文案：文本评估 Judge）** 默认 **OFF**
- 请求字段 `text_eval_judge: true` 才调用；否则零 Judge 请求
- Judge 使用圆桌同一 `api_key` + `model`（不读写钥匙串 `global`）

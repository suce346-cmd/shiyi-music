# P1-13 蓝图歧义裁定（2026-10-20 UTC+8）

| 蓝图原文 | 裁定 | 理由 |
|---|---|---|
| judge「双模型」 | **单模型 = 圆桌同 key+model**（`same_as_roundtable`） | 用户拍板；自用场景控成本与配置面 |
| 「成本 → 用户控制」 | 设置 **文本评估 Judge**（`textEvalJudge`）默认 **关闭**；仅显式开启才调用 | 用户要求 opt-in default OFF |
| 20×4×2 全量矩阵 | 在线 harness 旁路：`SHIYI_TEXT_EVAL=1` + 凭据才跑；日常 CI 只跑离线解析/校准单测 | 全量约 160 次流水线，不可进默认门禁 |
| GUI 与 harness 关系 | GUI：开关开启后**每次终稿**追加 1 次 Judge；harness：批量矩阵 | 旁路不阻塞主路径；失败发 `text_eval_degraded` |
| 人工 40 条校准 | 提供模板与 `agreement_rate`/`calibration_trusted`；仓库不塞 40 条标注 | 自用数据不入库；未达标前报告标小样本 |
| 钥匙串 | Judge **只读**请求里的 key（与圆桌相同来源）；禁止写/改/删 `global` | 用户硬约束 |

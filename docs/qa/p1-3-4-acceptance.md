# P1-3+P1-4 合并批次：真模型验收（Mac 同步后执行）

蓝图修订 A-1（BLUEPRINT_v2 §5.2）：P1-3 与 P1-4 合并实施；box 上只跑离线门，真模型回放与 smoke 在 Mac 同步后执行。
凭据只从环境变量读，不进仓库、不进日志。

## 前置

```bash
cd /Users/hakayi/xiangmu/shiyi-music
scripts/mac-verify.sh            # 钥匙串/离线门/GUI 基线先过
export SHIYI_TEST_BASE_URL=...  SHIYI_TEST_MODEL=...
read -rs SHIYI_TEST_API_KEY && export SHIYI_TEST_API_KEY   # 不进 shell 历史
```

## 1. 12 例 smoke（与 v4 基线同用例）

```bash
cd src-tauri
LSM_CASES="a:P1,a:P5,a:P9,b:P2,b:P6,b:P10,c:C1,c:C4,c:C7,d:P3,d:P7,d:P8" \
  cargo test --test large_scale_matrix -- --ignored --nocapture
cp ../target/large-test/results.jsonl ../docs/qa/results_p134_smoke.jsonl
```

> 用例 id 以 `docs/qa/results_v4_final.jsonl` 实际 `mode:prompt_id` 为准（执行前用下行核对并替换）：
> `python3 -c "import json;print(','.join(json.loads(l)['mode']+':'+json.loads(l)['prompt_id'] for l in open('../docs/qa/results_v4_final.jsonl')))"`

**通过标准（全部满足）**
| # | 指标 | 门槛 | 基线（v4，12 例） |
|---|---|---|---|
| S1 | 产出成功（`ok=true`） | 12/12 | 12/12 |
| S2 | 终稿硬校验通过（`validation_passed=true`，口径 = `plan::validate`） | ≥ 11/12 | 11/12 |
| S3 | 结构化解析失败（事件含 Parse / schema 校验失败）导致的 Failed | 0 | —（新指标） |
| S4 | `envelope_fallback` / `call_degraded` 降级事件 | 0 / ≤1 | —（envelope 已退役） |
| S5 | 渲染预算：Style ≤ `SUNO_STYLE_MAX`、Lyrics ≤ 5000、Exclude ≤ 1000、标签 8–15 | 12/12 | — |
| S6 | 单例耗时 p50 | ≤ v4 p50 × 1.3 | 见 v4 `dur_secs` |

## 2. 40 例回放对比（与 v3 基线同提示词池 `docs/qa/test-prompts.md`）

```bash
cd src-tauri
cargo test --test large_scale_matrix -- --ignored --nocapture
cp ../target/large-test/results.jsonl ../docs/qa/results_p134_replay.jsonl
```

**通过标准**
| # | 指标 | 门槛 | 基线（v3，40 例） |
|---|---|---|---|
| R1 | 硬校验通过 | ≥ 30/40 且每模式不低于 v3 | v3：A 3/10、B 5/10、C 8/10、D 7/10（23/40） |
| R2 | A 模式配器门失败（B-02 族） | ≤ 2/10 | v3：11 例（含误报，见 b02-root-cause.md） |
| R3 | 结构化 Parse 失败 | ≤ 1/40 | — |
| R4 | 人工抽检 12 例讨论质量（每模式 3 例，盲评新旧两版） | 新版不劣于旧版 ≥ 9/12 | — |

不达标 → 整批 revert（A-1：无双轨），结论写入 `docs/qa/p1-3-4-replay.md`。

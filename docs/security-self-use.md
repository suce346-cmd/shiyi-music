# 安全基线（自用版，BLUEPRINT_v2 D1 / P0-10，2026-10-20）

本应用为**个人自用**、不对外分发（D1）。以下为已做的最小修复与**明确接受**的残余风险；
对外分发前，“接受”项全部需要重新评估。审计条目 C-08 / C-12 / D-02 / D-07 归入本页。

## 已修复
| 项 | 旧行为 | 现行为 | 守护 |
|---|---|---|---|
| 钥匙串写入的密钥暴露在进程参数 | `security add-generic-password … -w <明文>`，写入期间 `ps` 可见 | `security -i` 从 stdin 读命令，明文只经管道；写后读回校验，不一致即报错 | `keychain::tests::cli_set_never_passes_secret_in_argv`、`interactive_set_command_shape_and_rejects` |
| 前端写文件权限全盘 `**` | 任意路径可写 | 仅 `$APPDATA/**`、`$DOWNLOAD/**`、`$DOCUMENT/**`；保存对话框所选路径由 dialog 插件运行期加入 scope（导出到任意目录仍可用） | `repo_hygiene::fs_write_scope_is_narrowed` |
| 日志记录模型输出正文 | 信封门 warn 日志记录方案前 300 字（用户创作内容进日志文件） | 只记缺陷类型与字数 | `repo_hygiene::logs_do_not_record_model_output_head` |

密钥字符限制（新增）：为不依赖 `security -i` 的引号语义，Key 限定为无空白/引号/反斜杠的可打印 ASCII；
不满足时 `keychain_set` 返回明确错误（主流厂商 Key 均满足，含空白多为粘贴错误）。

## 接受的残余风险
| 项 | 说明 | 接受理由 | 分发前须做 |
|---|---|---|---|
| DNS 重绑定 | `validate_url` 校验时解析与实际连接时解析是两次查询 | 地址由用户本人填写，非外部输入 | 解析一次并按已校验 IP 连接 |
| 钥匙串条目 `-A`（任意应用可读） | adhoc 签名每次构建签名变化，限定应用 ACL 会导致自身读不到 | 不买 Apple 开发者账号（F6）；本机单用户 | 正式签名后去掉 `-A`，改应用 ACL |
| 无代码签名 / 公证 / 自动更新 | — | 自用不分发（D1） | 签名、公证、更新器 |

## 待用户在 Mac 上验证（box 为 Linux，macOS 分支不编译）
1. `cargo test --lib keychain`（`cli_round_trip` 用隔离账户 `test:shiyi:*` 走新的 stdin 写入路径，**不触碰 `global`**）。
2. GUI：设置中修改一次 Key → 重启 → Key 仍在。
3. GUI：导出设置 / 导出历史到桌面等任意目录成功。

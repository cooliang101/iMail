# Rust 服务迁移 R5 资源基线

日期：2026-08-10
状态：离线 runtime 基线通过；R6 已完成同构 HTTP 空闲 Node 对照，真实网络长稳待完成

## 测试边界

本报告只记录 Rust 嵌入式同步运行时的离线空闲基线，不把它解释成完整生产负载结果。测试使用 release 构建、2 个持久 worker slot、关闭 scheduler 和网络 watcher，指向带 `backup-manifest.json` 的 `runtime-final-copy` 数据副本；运行前要求 queued job 为 0。测试没有打开当前 `.data`，没有连接邮箱，也没有执行同步任务。

可重复命令：

```powershell
npm.cmd run rust:runtime-soak -- output/rust-migration-tests/r5-preflight-2026-08-10/runtime-final-copy --duration-seconds 60 --max-growth-mib 16 --report <新的非覆盖报告路径>
```

`imail-runtime-soak` 拒绝目录名为 `.data` 的目标、拒绝缺少备份清单的数据目录、拒绝有 queued job 的副本，并拒绝覆盖已有报告。默认时长为 60 秒，允许范围为 5 秒至 24 小时；默认末次相对首次 RSS 增长预算为 16 MiB。测试结束还必须满足优雅关闭、零任务执行、零遗留 worker heartbeat 和零遗留 queued job。

这里显式使用 `npm.cmd`，避免 Windows PowerShell 把脚本参数误解析成 npm 自身配置；不带参数运行时仍可使用普通的 `npm run`。

## 结果

60 秒报告保存在 Git 忽略的 `output/rust-migration-tests/r5-preflight-2026-08-10/runtime-soak-60s.json`，SHA-256 为 `f001f433f8caa76d628c1bc96f2d33cdf055635c5734e07028bc3a20932b00c3`。

| 指标 | 结果 |
| --- | ---: |
| 样本数 | 60 |
| 首次 RSS | 9,633,792 bytes |
| 末次 RSS | 9,621,504 bytes |
| 峰值 RSS | 9,670,656 bytes |
| 计入预算的增长 | 0 bytes |
| 增长预算 | 16,777,216 bytes |
| CPU | 采样值均为 0% |
| 执行任务 | 0 |
| 关闭后 worker / queued job | 0 / 0 |
| 优雅关闭 | 通过 |

另有 10 秒烟雾报告 `runtime-soak-10s.json`：RSS 从 9,637,888 bytes 到 9,670,656 bytes，增长 32 KiB，峰值 9,670,656 bytes，同样无任务且优雅关闭。

Windows 上当前 `sysinfo` 版本不能从该接口返回线程/任务数，因此报告中的 `threadCount` 为 `null`；worker 生命周期改由持久 heartbeat 清零与 runtime health 共同验证。Windows 的 `virtualBytes` 口径在本次采样中不适合作为 Working Set 替代，本报告不据此作结论。

## 尚未满足的资源门禁

- 60 秒只能作为工具和稳定空闲路径的烟雾基线，不能替代数小时/数天的 IMAP IDLE、断网恢复和周期校准测试。
- R6 已在相同机器、全新隔离数据、production Web/Gateway/MCP、50 次 HTTP 预热和相同 3-worker 空闲拓扑下完成 Node/Rust 进程树 Working Set 对照，见 `docs/rust-migration-r6-resource-comparison.md`。该结果仍不包含真实邮箱连接数与同步吞吐，不能外推为最终生产容量。
- 尚未执行专用真实邮箱验收，资源测试中也没有启动 watcher、OAuth 刷新、IMAP、SMTP 或 MIME 大附件路径。
- Node 当前本地服务包含 HTTP 宿主与独立同步 worker，而本次 Rust 目标是无 HTTP 的进程内运行时；最终报告必须同时给出交付形态总进程占用和同负载核心占用，不能只比较单进程数字。

在这些项目完成前，Node 继续作为正式运行路径和当前数据的唯一写入者。

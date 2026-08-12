# Rust 服务迁移 R8 已安装数据预检报告

日期：2026-08-11
状态：前三阶段通过；已完成在线一致性备份、离线副本校验、无网络嵌入式 Host 首启及桌面切换事务实现，尚未切换真实数据写入者

## 边界

- 源目录：当前 Windows 内测安装的 `local-service/data`。
- 预检期间 `enabled=true`、`hold=false`，旧 Node manager 及两个 service 继续运行；没有停止、替换或删除任何进程和运行文件。
- 源数据库只通过 SQLite backup API 读取。所有新写入均位于仓库 `output/rust-migration-tests/r8-installed-preflight-2026-08-11/`。
- 备份和预检目标在执行前不存在；维护工具使用不可覆盖语义创建目标。
- 没有发送邮件、连接 OAuth provider 或执行真实 IMAP/SMTP 操作。

## 环境预检

- 数据目录、`imail.sqlite`、`master.key`、`instance-id` 均存在。
- 源数据库大小为 64,532,480 字节；源卷预检时可用空间约 82.55 GB，足以保留多份当前规模快照。
- 当前 schema 为 v6，与 Rust 支持版本一致。

## 一致性备份与恢复副本

`imail-maintenance upgrade-preflight` 已完成：

- `activeDataUntouched=true`；
- backup manifest 完整性校验通过；
- backup schema v6，副本迁移后仍为 v6；
- SQLite quick check 与 foreign key check 通过；
- backup 与 preflight 数据库 SHA-256 均为 `ACD290FC8E33CD7311A715AA999162A4C526279385A7670FD3ACE0F350F1B56D`。

## 模型摘要对照

backup 与 preflight 副本完全一致：

- 账户 4；用户 2；Session 20；
- 邮件 1,398；联系人 266；草稿 0；
- mailbox sync state 16；sync job 27,281；sync event 6,697；sync policy 4；
- developer token 1，账户绑定 1，scope 1；
- Logo 采集记录 197；安全审计 19。

Node 模型摘要的 accounts、messages、drafts、contacts、developerTokens 五项 digest 在 backup/preflight 间逐项一致。凭据兼容读取也一致：4 个账户全部成功解密；只记录字段类别和数量，不记录任何 token、密码或密文。

## 保留物

- `backup/`：不可覆盖的一致性备份，带完整性 manifest；
- `preflight/`：从 backup 恢复并由 Rust 迁移预检打开的离线副本；
- 当前安装数据和仓库 `.data` 均保留且未被替换。

## 嵌入式 Host 首启

新增 `imail-embedded-preflight` 专用门禁：

- 必须显式提供 backup manifest，且离线数据库 SHA-256 必须与 manifest 一致；
- 拒绝 `.data` 和父目录存在 `enabled` 标记的活动数据目录；
- 要求数据库、主密钥和实例身份同时存在；
- 以 `syncWorker=false` 启动 `EmbeddedServiceHost`，不创建 HTTP listener、不启动同步或网络连接；
- 通过 Rust 只读仓储读取 inventory、模型计数、兼容 digest 和凭据兼容摘要；
- 关闭 Host 后再次核对数据库、主密钥和实例身份哈希，任何变化均使预检失败。

当前 preflight 副本实测通过：`noHttpListener=true`、`syncWorker=false`、`protectedFilesUnchanged=true`，实例 ID 保持 `d1bf7a02-98d5-4139-9a33-ae7ba88a9d9b`；模型计数、digest 和四账户凭据摘要与备份阶段完全一致。

自动化测试另覆盖活动目录拒绝和 manifest 不匹配拒绝，测试只清理自身唯一临时目录。

## 下一门禁

桌面切换事务现已实现：

- Tauri 本地模式默认选择进程内 Rust，不再依赖隐藏的构建或运行时环境变量；Web 与桌面远程模式仍使用 HTTP。
- 首次本地领域调用以单次异步门闩执行切换，先读取受管理的旧配置，再移除 `enabled`、注销启动项，并同时确认旧 API 与 supervisor 已退出。
- 旧写入者停止后在 `local-service/migration-snapshots/rust-switch-<time>-<uuid>/` 创建不可覆盖的一致性快照；随后验证 SQLite quick check、外键、schema、全部账户凭据解密和 `syncWorker=false` 的无 HTTP Rust Host 首启。
- 数据库哈希在无网络首启前后必须一致，成功后写入私有 `embedded-switch.json`；`daemon.json`、control token、旧二进制、日志和快照均不删除。
- 备份、密钥、完整性或无网络首启失败时，只有数据库哈希保持不变才允许恢复旧 `enabled`、启动注册和 supervisor；如果发现 Rust 预检产生了数据库变化，则保持旧服务停止并保留失败现场，避免不安全双写。
- 已有旧 HTTP Session 只在旧服务停止且 `daemon.json` 数据目录、回环地址均匹配时导入内存，经数据库确认有效后才落入私有 `embedded-session`；token 不返回 WebView。

新增自动化覆盖：合法数据无 HTTP 首启且数据库哈希不变、无效 master key 在 Host 启动前失败、快照目标存在时拒绝覆盖且源数据不变、旧 Session 一次性安全导入。future schema、迁移事务回滚和守护进程停机超时继续由存储层及旧守护生命周期测试覆盖；真实安装覆盖升级仍是下一门禁。

## 下一门禁

1. 运行完整 Node、Rust workspace、Tauri、格式、严格 Clippy 和 production build 回归。
2. 生成仍保留旧 Node 恢复来源的 Windows 内测安装包，并在真实安装数据上执行覆盖升级切换。
3. 首次 Rust 启动后验证已有 Session、登录用户、4 个账户、公开模型计数、无业务 HTTP listener 和同步运行时；失败不得删除旧运行文件或任何测试数据。
4. 真实切换成功并取得切换后快照后，才从后续安装包移除 Node SEA、Worker 和旧端口恢复 UI。

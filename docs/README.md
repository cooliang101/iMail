# iMail 文档

这里仅保留当前实现、交付和维护仍会使用的文档。已经完成的 Node → Rust 迁移阶段报告、临时路线图和一次性验收记录不再常驻仓库；需要追溯时使用 Git 历史。

## 使用与部署

- [部署模式](deployment-modes.md)：Windows 本地嵌入、桌面远程连接与 Docker 服务端的边界。
- [运维手册](operator-runbook.md)：环境变量、Docker/HTTPS、备份恢复、升级和故障排查。
- [内部测试](internal-testing.md)：Windows NSIS 与服务端 Docker 的构建、验收和交付要求。
- [邮件验收协议](rust-mail-acceptance.md)：会产生真实邮箱副作用的受控 IMAP/SMTP 验收流程。

## 架构与集成

- [架构说明](architecture.md)：模块、数据、安全、同步、MCP 与联系人 Logo 边界。
- [CP0 平台审计](platform-audit.md)：操作系统耦合、平台接口、Owner、CI 命名与 Windows 回归基线。
- [MCP 接入指南](mcp-integration.md)：授权、工具、推荐调用顺序和安全约束。
- [工程交接](handoff.md)：当前能力、不变量与提交前验证基线。
- [ADR](adr/)：仍有效或被后续决策修订的架构决定。

## 后续开发计划

- [跨平台支持路线](cross-platform-support-roadmap.md)：Docker、Linux 和 macOS 的分阶段扩展计划与验收门禁。

## 前端与主题

- [样式系统](style-system.md)：主题令牌、排版、布局、组件和响应式维护规则。
- [自定义主题](custom-theme.md)：安全主题 JSON 的字段和生成约束。
- [Preact 迁移方案](preact-migration.md)：从 React 渐进迁移到 Preact 的阶段、门禁、性能指标与回滚策略。

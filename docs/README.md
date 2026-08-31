# iMail 文档

这里仅保留当前实现、交付、维护及有效规划仍会使用的文档。已完成或失效的迁移方案、路线图、阶段审计和交接快照通过 Git 历史追溯，不再常驻仓库。

## 产品规划

- [功能扩展路线图](product-roadmap.md)：写信、搜索、规则、会话、发件箱与 Agent 自动化的范围、实施顺序和验收边界；各项状态见正文，候选条目不代表交付承诺。

## 使用与部署

- [写信与会话](composition-and-conversations.md)：回复全部、密送、签名、模板、附件提醒、会话关联和引用折叠的行为与安全边界。
- [部署模式](deployment-modes.md)：Windows 本地嵌入、桌面远程连接与 Docker 服务端的边界。
- [运维手册](operator-runbook.md)：环境变量、Docker/HTTPS、备份恢复、升级和故障排查。
- [内部测试](internal-testing.md)：Windows NSIS 与服务端 Docker 的构建、验收和交付要求。
- [邮件验收协议](rust-mail-acceptance.md)：会产生真实邮箱副作用的受控 IMAP/SMTP 验收流程。

## 架构与集成

- [架构说明](architecture.md)：模块、数据、安全、同步、MCP 与联系人 Logo 边界。
- [MCP 接入指南](mcp-integration.md)：授权、工具、推荐调用顺序和安全约束。
- [附件预览](attachment-preview.md)：支持格式、缓存、预览会话与 ZIP 安全边界。
- [ADR](adr/)：仍有效或被后续决策修订的架构决定。

## 前端与主题

- [样式系统](style-system.md)：主题令牌、排版、布局、组件和响应式维护规则。
- [自定义主题](custom-theme.md)：安全主题 JSON 的字段和生成约束。

# iMail

iMail 将多邮箱邮件能力集中在独立服务中，并允许多种受信客户端连接同一用户空间。

## Language

**iMail 服务**:
独立运行并拥有邮箱凭据、邮件数据、同步任务、HTTP API、Gateway 与 MCP 能力的服务端。
_Avoid_: 桌面后端、sidecar

**iMail 客户端**:
连接 iMail 服务的 Web 或桌面界面；只保存界面偏好和服务地址，不拥有邮箱服务端状态。
_Avoid_: 本地服务、桌面服务

**服务地址**:
iMail 客户端用于访问一个 iMail 服务实例的 HTTP 或 HTTPS 基地址。
_Avoid_: sidecar 地址、固定端口

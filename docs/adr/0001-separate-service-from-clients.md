---
status: superseded by ADR-0002
---

# 服务端与客户端独立部署

iMail 服务独立拥有邮箱凭据、SQLite、同步 Worker、HTTP API、Gateway 与 MCP；Windows、macOS 和 Web 均作为可配置服务地址的客户端。桌面包不再携带 Node sidecar，因为把服务生命周期绑定到桌面进程会造成端口冲突、数据位置分裂，并使无人值守同步依赖客户端是否正在运行；代价是跨设备部署必须配置 HTTPS、CORS 与服务可达性。

# iMail HTTP service

该目录是 iMail 的独立 HTTP 部署入口，不包含第二套业务实现。

- `src/main.rs`：装配并启动 HTTP bridge。
- `Dockerfile`、`compose*.yml`、`deploy/`：容器与 HTTPS 部署配置。
- 通用 Web API、MCP、Gateway 与 Web 托管实现位于 `crates/imail-http/`。
- 账户、存储、邮件、OAuth 和同步等底层服务位于 `crates/`，Windows Tauri 应用与本目录共同复用。

从仓库根目录运行：

```powershell
cargo run --locked -p imail-http-service -- --host 127.0.0.1 --port 8787
docker build --file http-service/Dockerfile --tag imail-http-service .
docker compose -f http-service/compose.example.yml up -d
```

独立服务入口默认启用 HTTP；`--host` 和 `--port` 可省略，默认监听 `127.0.0.1:8787`。Gateway 与 MCP 的开关、授权和领域行为继续由通用 Rust 服务实现维护。

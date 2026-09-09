use std::io;

use crate::ConfigError;

#[derive(Debug, thiserror::Error)]
pub enum HttpAdapterError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("iMail 服务实例身份文件无效")]
    InvalidInstanceId,
    #[error("无法读取或创建 iMail 服务实例身份：{0}")]
    InstanceIo(#[from] io::Error),
    #[error("HTTP listener 失败：{0}")]
    Listener(io::Error),
    #[error("HTTP 服务失败：{0}")]
    Serve(io::Error),
    #[error("同步运行时启动失败：{0}")]
    SyncRuntimeStart(String),
    #[error("同步运行时未能优雅关闭：{0:?}")]
    SyncRuntimeShutdown(Vec<String>),
    #[error("Apple HME 会话保活任务启动失败：{0}")]
    AppleHmeKeepaliveStart(String),
    #[error("Apple HME 会话保活任务未能优雅关闭")]
    AppleHmeKeepaliveShutdown,
    #[error("发件箱运行时启动失败：{0}")]
    OutboxRuntimeStart(String),
    #[error("发件箱运行时未能优雅关闭")]
    OutboxRuntimeShutdown,
}

#[derive(Debug, Clone)]
pub struct EmbeddedOperationError {
    pub status: u16,
    pub message: String,
}

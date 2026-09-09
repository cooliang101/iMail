//! Network implementation of mail ports with a shared timeout/cancellation runtime.
use error::NetworkError;
use imail_mail::{ProtocolFailure, ProtocolStage};
use std::{future::Future, io, sync::Arc, time::Duration};
use tls::tls_connector;
use tokio::{
    runtime::{Builder as RuntimeBuilder, Runtime},
    time::timeout,
};
use tokio_rustls::TlsConnector;
mod error;
mod imap;
mod smtp;
mod sync;
mod tls;
mod tunnel;
pub use tunnel::TunnelStream;
const OPERATION_TIMEOUT: Duration = Duration::from_secs(120);

pub trait NetworkCancellation: Send + Sync + 'static {
    fn is_cancelled(&self) -> bool;
}

pub struct NetworkMailAdapter {
    runtime: Runtime,
    cancellation: Option<Arc<dyn NetworkCancellation>>,
    tls_connector: TlsConnector,
}

impl NetworkMailAdapter {
    pub fn new() -> io::Result<Self> {
        Self::with_tls_connector(tls_connector())
    }

    pub fn with_tls_connector(tls_connector: TlsConnector) -> io::Result<Self> {
        Ok(Self {
            runtime: RuntimeBuilder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?,
            cancellation: None,
            tls_connector,
        })
    }

    pub fn with_cancellation(cancellation: Arc<dyn NetworkCancellation>) -> io::Result<Self> {
        let mut adapter = Self::new()?;
        adapter.cancellation = Some(cancellation);
        Ok(adapter)
    }

    pub fn with_cancellation_and_tls_connector(
        cancellation: Arc<dyn NetworkCancellation>,
        tls_connector: TlsConnector,
    ) -> io::Result<Self> {
        let mut adapter = Self::with_tls_connector(tls_connector)?;
        adapter.cancellation = Some(cancellation);
        Ok(adapter)
    }

    fn run<T>(
        &self,
        stage: ProtocolStage,
        operation: impl Future<Output = Result<T, NetworkError>>,
    ) -> Result<T, ProtocolFailure> {
        self.run_with_timeout(stage, OPERATION_TIMEOUT, operation)
    }

    fn run_with_timeout<T>(
        &self,
        stage: ProtocolStage,
        maximum_wait: Duration,
        operation: impl Future<Output = Result<T, NetworkError>>,
    ) -> Result<T, ProtocolFailure> {
        let cancellation = self.cancellation.clone();
        let has_cancellation = cancellation.is_some();
        self.runtime.block_on(async move {
            tokio::select! {
                result = timeout(maximum_wait, operation) => result
                    .map_err(|_| ProtocolFailure::from_provider(stage, Some("TIMEOUT"), "网络操作超时"))?
                    .map_err(|error| error.into_protocol_failure(stage)),
                () = wait_for_cancellation(cancellation), if has_cancellation => {
                    Err(ProtocolFailure::from_provider(stage, Some("CANCELLED"), "网络操作已取消"))
                }
            }
        })
    }
}

async fn wait_for_cancellation(cancellation: Option<Arc<dyn NetworkCancellation>>) {
    let Some(cancellation) = cancellation else {
        std::future::pending::<()>().await;
        return;
    };
    while !cancellation.is_cancelled() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[cfg(test)]
mod tests;

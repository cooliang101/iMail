use std::{collections::HashMap, future::pending, sync::Mutex};
use tokio::sync::watch;

#[derive(Default)]
pub struct RequestCancellationRegistry {
    active: Mutex<HashMap<uuid::Uuid, watch::Sender<bool>>>,
}

impl RequestCancellationRegistry {
    pub fn begin(&self, request_id: Option<&str>) -> Result<Option<watch::Receiver<bool>>, String> {
        let Some(request_id) = request_id else {
            return Ok(None);
        };
        let request_id = parse_request_id(request_id)?;
        let (sender, receiver) = watch::channel(false);
        let mut active = self
            .active
            .lock()
            .map_err(|_| "请求取消状态不可用".to_string())?;
        if let Some(previous) = active.insert(request_id, sender) {
            let _ = previous.send(true);
        }
        Ok(Some(receiver))
    }

    pub fn cancel(&self, request_id: &str) -> Result<bool, String> {
        let request_id = parse_request_id(request_id)?;
        let active = self
            .active
            .lock()
            .map_err(|_| "请求取消状态不可用".to_string())?;
        let Some(sender) = active.get(&request_id) else {
            return Ok(false);
        };
        let _ = sender.send(true);
        Ok(true)
    }

    pub fn finish(&self, request_id: Option<&str>) {
        let Some(request_id) = request_id.and_then(|value| uuid::Uuid::parse_str(value).ok())
        else {
            return;
        };
        if let Ok(mut active) = self.active.lock() {
            active.remove(&request_id);
        }
    }
}

fn parse_request_id(value: &str) -> Result<uuid::Uuid, String> {
    uuid::Uuid::parse_str(value).map_err(|_| "请求标识无效".to_string())
}

pub async fn wait_for_request_cancellation(receiver: Option<watch::Receiver<bool>>) {
    let Some(mut receiver) = receiver else {
        pending::<()>().await;
        return;
    };
    if *receiver.borrow() {
        return;
    }
    let _ = receiver.changed().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancels_and_removes_registered_requests() {
        let registry = RequestCancellationRegistry::default();
        let request_id = uuid::Uuid::new_v4().to_string();
        let receiver = registry.begin(Some(&request_id)).expect("register request");
        assert!(registry.cancel(&request_id).expect("cancel request"));
        wait_for_request_cancellation(receiver).await;
        registry.finish(Some(&request_id));
        assert!(!registry.cancel(&request_id).expect("request was removed"));
    }
}

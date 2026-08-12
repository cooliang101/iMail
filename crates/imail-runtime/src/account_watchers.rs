use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{atomic::Ordering, Arc},
    thread::{self, JoinHandle},
    time::Duration,
};

use chrono::Utc;
use imail_mail::MailboxWakeReason;
use imail_storage_sqlite::{SyncEnqueue, SyncRuntimeStore};

use crate::{sync_workers::RuntimeMetrics, CancellationToken, SyncWorkerConfig};

#[derive(Clone)]
pub struct AccountWakeContext {
    shutdown: CancellationToken,
    account_cancelled: CancellationToken,
}

impl AccountWakeContext {
    pub fn is_cancelled(&self) -> bool {
        self.shutdown.is_cancelled() || self.account_cancelled.is_cancelled()
    }

    pub fn wait_cancelled(&self, maximum_wait: Duration) -> bool {
        let deadline = std::time::Instant::now() + maximum_wait;
        while !self.is_cancelled() {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            self.account_cancelled
                .wait_cancelled(remaining.min(Duration::from_millis(100)));
        }
        self.is_cancelled()
    }
}

pub trait AccountWakeExecutor: Send + Sync + 'static {
    fn wait_for_wake(
        &self,
        account_id: &str,
        context: &AccountWakeContext,
        maximum_wait: Duration,
    ) -> Result<MailboxWakeReason, String>;
}

struct AccountWatcherTask {
    cancellation: CancellationToken,
    handle: JoinHandle<()>,
}

pub(crate) fn run_account_watcher_manager(
    config: &SyncWorkerConfig,
    executor: Arc<dyn AccountWakeExecutor>,
    shutdown: CancellationToken,
    metrics: Arc<RuntimeMetrics>,
) {
    let mut watchers = BTreeMap::<String, AccountWatcherTask>::new();
    while !shutdown.is_cancelled() {
        if let Ok(store) = SyncRuntimeStore::open_database(&config.database_path) {
            if let Ok(account_ids) = store.enabled_sync_account_ids() {
                reconcile_watchers(
                    &mut watchers,
                    account_ids.into_iter().collect(),
                    config,
                    Arc::clone(&executor),
                    &shutdown,
                    Arc::clone(&metrics),
                );
            }
        }
        if shutdown.wait_cancelled(config.watcher_reconcile_interval) {
            break;
        }
    }
    for (_, watcher) in watchers {
        watcher.cancellation.cancel();
        let _ = watcher.handle.join();
    }
}

fn reconcile_watchers(
    watchers: &mut BTreeMap<String, AccountWatcherTask>,
    desired: BTreeSet<String>,
    config: &SyncWorkerConfig,
    executor: Arc<dyn AccountWakeExecutor>,
    shutdown: &CancellationToken,
    metrics: Arc<RuntimeMetrics>,
) {
    let removed = watchers
        .keys()
        .filter(|account_id| !desired.contains(*account_id))
        .cloned()
        .collect::<Vec<_>>();
    for account_id in removed {
        if let Some(watcher) = watchers.remove(&account_id) {
            watcher.cancellation.cancel();
            let _ = watcher.handle.join();
        }
    }
    for account_id in desired {
        if watchers.contains_key(&account_id) {
            continue;
        }
        let cancellation = CancellationToken::default();
        let watcher_cancel = cancellation.clone();
        let watcher_shutdown = shutdown.clone();
        let watcher_executor = Arc::clone(&executor);
        let database_path = config.database_path.clone();
        let maximum_wait = config.watcher_maximum_wait;
        let retry_wait = config.watcher_retry_interval;
        let task_account_id = account_id.clone();
        let watcher_metrics = Arc::clone(&metrics);
        let handle = thread::Builder::new()
            .name(format!("imail-watch-{}", safe_thread_suffix(&account_id)))
            .spawn(move || {
                watcher_metrics
                    .active_watchers
                    .fetch_add(1, Ordering::Relaxed);
                let _active = ActiveWatcherGuard(Arc::clone(&watcher_metrics));
                let context = AccountWakeContext {
                    shutdown: watcher_shutdown,
                    account_cancelled: watcher_cancel,
                };
                run_account_watcher(
                    &database_path,
                    &task_account_id,
                    watcher_executor.as_ref(),
                    &context,
                    maximum_wait,
                    retry_wait,
                    &watcher_metrics,
                );
            });
        if let Ok(handle) = handle {
            watchers.insert(
                account_id,
                AccountWatcherTask {
                    cancellation,
                    handle,
                },
            );
        }
    }
}

struct ActiveWatcherGuard(Arc<RuntimeMetrics>);

impl Drop for ActiveWatcherGuard {
    fn drop(&mut self) {
        self.0.active_watchers.fetch_sub(1, Ordering::Relaxed);
    }
}

fn run_account_watcher(
    database_path: &std::path::Path,
    account_id: &str,
    executor: &dyn AccountWakeExecutor,
    context: &AccountWakeContext,
    maximum_wait: Duration,
    retry_wait: Duration,
    metrics: &RuntimeMetrics,
) {
    let mut disconnected = false;
    let mut consecutive_disconnects = 0_u32;
    while !context.is_cancelled() {
        match executor.wait_for_wake(account_id, context, maximum_wait) {
            Ok(reason) if !context.is_cancelled() => {
                if disconnected {
                    metrics.watcher_reconnects.fetch_add(1, Ordering::Relaxed);
                    disconnected = false;
                }
                consecutive_disconnects = 0;
                let reason = match reason {
                    MailboxWakeReason::Changed => "recovery",
                    MailboxWakeReason::Reconcile => "scheduled",
                };
                enqueue_wake(database_path, account_id, reason, retry_wait, context);
            }
            Ok(_) => break,
            Err(_) => {
                if !context.is_cancelled() && !disconnected {
                    metrics.watcher_disconnects.fetch_add(1, Ordering::Relaxed);
                    disconnected = true;
                }
                consecutive_disconnects = consecutive_disconnects.saturating_add(1);
                if context.wait_cancelled(watcher_retry_delay(retry_wait, consecutive_disconnects))
                {
                    break;
                }
            }
        }
    }
}

fn watcher_retry_delay(initial: Duration, consecutive_disconnects: u32) -> Duration {
    const MAXIMUM: Duration = Duration::from_secs(30);
    let exponent = consecutive_disconnects.saturating_sub(1).min(6);
    initial
        .checked_mul(1_u32 << exponent)
        .unwrap_or(MAXIMUM)
        .min(MAXIMUM)
}

fn enqueue_wake(
    database_path: &std::path::Path,
    account_id: &str,
    reason: &str,
    retry_wait: Duration,
    context: &AccountWakeContext,
) {
    let input = SyncEnqueue {
        account_id: account_id.into(),
        mailbox: None,
        mailbox_role: "inbox".into(),
        reason: reason.into(),
        priority: if reason == "recovery" { 50 } else { 0 },
        not_before: None,
    };
    while !context.is_cancelled() {
        let queued = SyncRuntimeStore::open_database(database_path)
            .and_then(|mut store| store.enqueue(&input, Utc::now()))
            .is_ok();
        if queued || context.wait_cancelled(retry_wait) {
            break;
        }
    }
}

fn safe_thread_suffix(account_id: &str) -> String {
    account_id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
        .take(32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watcher_reconnect_delay_matches_the_bounded_node_curve() {
        let initial = Duration::from_millis(500);
        assert_eq!(watcher_retry_delay(initial, 1), Duration::from_millis(500));
        assert_eq!(watcher_retry_delay(initial, 2), Duration::from_secs(1));
        assert_eq!(watcher_retry_delay(initial, 6), Duration::from_secs(16));
        assert_eq!(watcher_retry_delay(initial, 7), Duration::from_secs(30));
        assert_eq!(watcher_retry_delay(initial, 100), Duration::from_secs(30));
    }
}

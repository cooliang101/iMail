use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    thread,
    time::Duration,
};

use chrono::Utc;
use imail_protocol::SyncJobReadModel;
use imail_storage_sqlite::{SyncCompletion, SyncFailure, SyncRuntimeStore};

use crate::{
    account_watchers::run_account_watcher_manager, AccountWakeExecutor, CancellationToken,
    RuntimeError, RuntimeSupervisor, ShutdownReport,
};

#[derive(Debug, Clone)]
pub struct SyncWorkerConfig {
    pub database_path: PathBuf,
    pub worker_count: usize,
    pub poll_interval: Duration,
    pub lease_duration: Duration,
    pub reconciliation_minutes: i64,
    pub host_name: String,
    pub watcher_reconcile_interval: Duration,
    pub watcher_maximum_wait: Duration,
    pub watcher_retry_interval: Duration,
    pub scheduler_enabled: bool,
    pub scheduler_interval: Duration,
    pub scheduler_startup_delay: Duration,
    pub event_signal: Option<SyncEventSignal>,
}

impl SyncWorkerConfig {
    pub fn new(database_path: impl Into<PathBuf>) -> Self {
        Self {
            database_path: database_path.into(),
            worker_count: 2,
            poll_interval: Duration::from_millis(500),
            lease_duration: Duration::from_secs(180),
            reconciliation_minutes: 30,
            host_name: "local".into(),
            watcher_reconcile_interval: Duration::from_secs(15),
            watcher_maximum_wait: Duration::from_secs(60),
            watcher_retry_interval: Duration::from_millis(500),
            scheduler_enabled: true,
            scheduler_interval: Duration::from_secs(5),
            scheduler_startup_delay: Duration::from_secs(1),
            event_signal: None,
        }
    }

    fn validate(&self) -> Result<(), SyncWorkerStartError> {
        if !self.database_path.is_file() {
            return Err(SyncWorkerStartError::MissingDatabase(
                self.database_path.clone(),
            ));
        }
        if !(1..=16).contains(&self.worker_count)
            || self.poll_interval.is_zero()
            || self.lease_duration < Duration::from_secs(3)
            || self.reconciliation_minutes <= 0
            || self.host_name.trim().is_empty()
            || self.watcher_reconcile_interval.is_zero()
            || self.watcher_maximum_wait < Duration::from_secs(1)
            || self.watcher_maximum_wait > Duration::from_secs(60)
            || self.watcher_retry_interval.is_zero()
            || self.scheduler_interval.is_zero()
        {
            return Err(SyncWorkerStartError::InvalidConfiguration);
        }
        Ok(())
    }
}

#[derive(Clone, Default)]
pub struct SyncEventSignal {
    inner: Arc<(Mutex<u64>, Condvar)>,
}

impl std::fmt::Debug for SyncEventSignal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SyncEventSignal")
            .finish_non_exhaustive()
    }
}

impl SyncEventSignal {
    pub fn sequence(&self) -> u64 {
        *self
            .inner
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    pub fn notify(&self) {
        let mut sequence = self
            .inner
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *sequence = sequence.wrapping_add(1);
        self.inner.1.notify_all();
    }

    pub fn wait_since(&self, previous: u64, maximum_wait: Duration) -> u64 {
        let sequence = self
            .inner
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if *sequence != previous {
            return *sequence;
        }
        let (sequence, _) = self
            .inner
            .1
            .wait_timeout_while(sequence, maximum_wait, |value| *value == previous)
            .unwrap_or_else(|error| error.into_inner());
        *sequence
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SyncWorkerStartError {
    #[error("找不到 iMail 数据库：{0}")]
    MissingDatabase(PathBuf),
    #[error("同步 worker 配置无效")]
    InvalidConfiguration,
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}

#[derive(Clone)]
pub struct JobExecutionContext {
    shutdown: CancellationToken,
    lease_lost: Arc<AtomicBool>,
}

impl JobExecutionContext {
    pub fn is_cancelled(&self) -> bool {
        self.shutdown.is_cancelled() || self.lease_lost.load(Ordering::SeqCst)
    }

    pub fn lease_lost(&self) -> bool {
        self.lease_lost.load(Ordering::SeqCst)
    }
}

pub trait SyncJobExecutor: Send + Sync + 'static {
    fn execute(
        &self,
        job: &SyncJobReadModel,
        context: &JobExecutionContext,
    ) -> Result<SyncCompletion, SyncFailure>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeHealthSnapshot {
    pub jobs_started: u64,
    pub jobs_succeeded: u64,
    pub jobs_failed: u64,
    pub jobs_cancelled: u64,
    pub scheduler_scans: u64,
    pub scheduler_errors: u64,
    pub active_watchers: u64,
    pub watcher_disconnects: u64,
    pub watcher_reconnects: u64,
}

#[derive(Default)]
pub(crate) struct RuntimeMetrics {
    pub jobs_started: AtomicU64,
    pub jobs_succeeded: AtomicU64,
    pub jobs_failed: AtomicU64,
    pub jobs_cancelled: AtomicU64,
    pub scheduler_scans: AtomicU64,
    pub scheduler_errors: AtomicU64,
    pub active_watchers: AtomicU64,
    pub watcher_disconnects: AtomicU64,
    pub watcher_reconnects: AtomicU64,
}

impl RuntimeMetrics {
    fn snapshot(&self) -> RuntimeHealthSnapshot {
        RuntimeHealthSnapshot {
            jobs_started: self.jobs_started.load(Ordering::Relaxed),
            jobs_succeeded: self.jobs_succeeded.load(Ordering::Relaxed),
            jobs_failed: self.jobs_failed.load(Ordering::Relaxed),
            jobs_cancelled: self.jobs_cancelled.load(Ordering::Relaxed),
            scheduler_scans: self.scheduler_scans.load(Ordering::Relaxed),
            scheduler_errors: self.scheduler_errors.load(Ordering::Relaxed),
            active_watchers: self.active_watchers.load(Ordering::Relaxed),
            watcher_disconnects: self.watcher_disconnects.load(Ordering::Relaxed),
            watcher_reconnects: self.watcher_reconnects.load(Ordering::Relaxed),
        }
    }
}

pub struct PersistentSyncRuntime {
    supervisor: RuntimeSupervisor,
    worker_count: usize,
    metrics: Arc<RuntimeMetrics>,
}

impl PersistentSyncRuntime {
    pub fn start(
        config: SyncWorkerConfig,
        executor: Arc<dyn SyncJobExecutor>,
    ) -> Result<Self, SyncWorkerStartError> {
        config.validate()?;
        let mut supervisor = RuntimeSupervisor::default();
        let metrics = Arc::new(RuntimeMetrics::default());
        for index in 0..config.worker_count {
            let worker_config = config.clone();
            let worker_executor = Arc::clone(&executor);
            let worker_metrics = Arc::clone(&metrics);
            supervisor.spawn(format!("sync-worker-{index}"), move |shutdown| {
                run_worker(
                    index,
                    &worker_config,
                    worker_executor.as_ref(),
                    shutdown,
                    &worker_metrics,
                );
            })?;
        }
        if config.scheduler_enabled {
            let scheduler_config = config.clone();
            let scheduler_metrics = Arc::clone(&metrics);
            supervisor.spawn("sync-scheduler", move |shutdown| {
                run_sync_scheduler(&scheduler_config, shutdown, &scheduler_metrics);
            })?;
        }
        Ok(Self {
            supervisor,
            worker_count: config.worker_count,
            metrics,
        })
    }

    pub fn start_with_watchers(
        config: SyncWorkerConfig,
        executor: Arc<dyn SyncJobExecutor>,
        wake_executor: Arc<dyn AccountWakeExecutor>,
    ) -> Result<Self, SyncWorkerStartError> {
        let mut runtime = Self::start(config.clone(), executor)?;
        let watcher_metrics = Arc::clone(&runtime.metrics);
        runtime
            .supervisor
            .spawn("account-watcher-manager", move |shutdown| {
                run_account_watcher_manager(&config, wake_executor, shutdown, watcher_metrics);
            })?;
        Ok(runtime)
    }

    pub fn worker_count(&self) -> usize {
        self.worker_count
    }

    pub fn shutdown(&mut self, maximum_wait: Duration) -> ShutdownReport {
        self.supervisor.shutdown(maximum_wait)
    }

    pub fn health(&self) -> RuntimeHealthSnapshot {
        self.metrics.snapshot()
    }
}

fn run_sync_scheduler(
    config: &SyncWorkerConfig,
    shutdown: CancellationToken,
    metrics: &RuntimeMetrics,
) {
    if shutdown.wait_cancelled(config.scheduler_startup_delay) {
        return;
    }
    run_scheduler_scan(config, "startup", metrics);
    while !shutdown.wait_cancelled(config.scheduler_interval) {
        run_scheduler_scan(config, "scheduled", metrics);
    }
}

fn run_scheduler_scan(config: &SyncWorkerConfig, reason: &str, metrics: &RuntimeMetrics) {
    metrics.scheduler_scans.fetch_add(1, Ordering::Relaxed);
    let result = SyncRuntimeStore::open_database(&config.database_path)
        .and_then(|mut store| store.enqueue_due_syncs(reason, Utc::now()));
    if result.is_err() {
        metrics.scheduler_errors.fetch_add(1, Ordering::Relaxed);
    }
}

fn run_worker(
    index: usize,
    config: &SyncWorkerConfig,
    executor: &dyn SyncJobExecutor,
    shutdown: CancellationToken,
    metrics: &RuntimeMetrics,
) {
    let worker_id = format!("{}-{}-{index}", config.host_name, std::process::id());
    let started_at = Utc::now();
    let mut store = loop {
        match SyncRuntimeStore::open_database(&config.database_path) {
            Ok(store) => break store,
            Err(_) if shutdown.wait_cancelled(config.poll_interval) => return,
            Err(_) => {}
        }
    };
    while !shutdown.is_cancelled() {
        let now = Utc::now();
        let _ = store.heartbeat(
            &worker_id,
            i64::from(std::process::id()),
            &config.host_name,
            started_at,
            now,
        );
        if let Ok(Some(job)) = store.claim_next(&worker_id, config.lease_duration, now) {
            run_job(
                &mut store, config, executor, &shutdown, &worker_id, &job, metrics,
            );
        }
        if shutdown.wait_cancelled(config.poll_interval) {
            break;
        }
    }
    let _ = store.remove_heartbeat(&worker_id);
}

fn run_job(
    store: &mut SyncRuntimeStore,
    config: &SyncWorkerConfig,
    executor: &dyn SyncJobExecutor,
    shutdown: &CancellationToken,
    worker_id: &str,
    job: &SyncJobReadModel,
    metrics: &RuntimeMetrics,
) {
    let mailbox = job
        .mailbox
        .clone()
        .unwrap_or_else(|| format!("@role:{}", job.mailbox_role));
    if !mark_job_started(store, config, shutdown, worker_id, job, &mailbox) {
        return;
    }
    if let Some(signal) = &config.event_signal {
        signal.notify();
    }
    metrics.jobs_started.fetch_add(1, Ordering::Relaxed);

    let lease_lost = Arc::new(AtomicBool::new(false));
    let lease_stop = CancellationToken::default();
    let lease_handle = spawn_lease_heartbeat(
        &config.database_path,
        worker_id,
        job,
        config,
        shutdown.clone(),
        lease_stop.clone(),
        Arc::clone(&lease_lost),
    );
    let context = JobExecutionContext {
        shutdown: shutdown.clone(),
        lease_lost: Arc::clone(&lease_lost),
    };
    let result = executor.execute(job, &context);
    let mut finalize_attempts = 0_u32;
    while !context.lease_lost() {
        finalize_attempts += 1;
        let persisted = if shutdown.is_cancelled() {
            store.cancel(job, &mailbox, Utc::now()).map(|_| {
                metrics.jobs_cancelled.fetch_add(1, Ordering::Relaxed);
            })
        } else {
            match &result {
                Ok(completion) => store
                    .complete(job, completion, config.reconciliation_minutes, Utc::now())
                    .map(|_| {
                        metrics.jobs_succeeded.fetch_add(1, Ordering::Relaxed);
                    }),
                Err(failure) => store.fail(job, failure, Utc::now()).map(|_| {
                    metrics.jobs_failed.fetch_add(1, Ordering::Relaxed);
                }),
            }
        };
        if persisted.is_ok() {
            if let Some(signal) = &config.event_signal {
                signal.notify();
            }
            break;
        }
        if shutdown.is_cancelled() && finalize_attempts >= 10 {
            break;
        }
        thread::sleep(config.poll_interval.min(Duration::from_millis(100)));
    }
    lease_stop.cancel();
    let _ = lease_handle.join();
}

fn mark_job_started(
    store: &mut SyncRuntimeStore,
    config: &SyncWorkerConfig,
    shutdown: &CancellationToken,
    worker_id: &str,
    job: &SyncJobReadModel,
    mailbox: &str,
) -> bool {
    loop {
        if store.mark_started(job, mailbox, Utc::now()).is_ok() {
            return true;
        }
        if shutdown.is_cancelled() {
            let _ = store.cancel(job, mailbox, Utc::now());
            return false;
        }
        match store.renew_lease(
            job.id.as_str(),
            worker_id,
            config.lease_duration,
            Utc::now(),
        ) {
            Ok(true) => {}
            Ok(false) | Err(_) => return false,
        }
        if shutdown.wait_cancelled(config.poll_interval) {
            let _ = store.cancel(job, mailbox, Utc::now());
            return false;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_lease_heartbeat(
    database_path: &Path,
    worker_id: &str,
    job: &SyncJobReadModel,
    config: &SyncWorkerConfig,
    shutdown: CancellationToken,
    stop: CancellationToken,
    lease_lost: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    let database_path = database_path.to_path_buf();
    let worker_id = worker_id.to_string();
    let job_id = job.id.clone();
    let lease = config.lease_duration;
    let interval = (lease / 3).max(Duration::from_secs(1));
    thread::spawn(move || {
        let store = match SyncRuntimeStore::open_database(database_path) {
            Ok(store) => store,
            Err(_) => {
                lease_lost.store(true, Ordering::SeqCst);
                return;
            }
        };
        while !shutdown.is_cancelled() && !stop.wait_cancelled(interval) {
            match store.renew_lease(&job_id, &worker_id, lease, Utc::now()) {
                Ok(true) => {}
                Ok(false) | Err(_) => {
                    lease_lost.store(true, Ordering::SeqCst);
                    break;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeSet,
        sync::atomic::{AtomicUsize, Ordering},
        sync::Mutex,
        time::{SystemTime, UNIX_EPOCH},
    };

    use imail_mail::MailboxWakeReason;
    use imail_protocol::CURRENT_SCHEMA_VERSION;
    use imail_storage_sqlite::{SyncEnqueue, SyncPolicySettings};
    use rusqlite::{params, Connection};

    use super::*;

    static FIXTURE_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn event_signal_wakes_immediately_and_times_out_without_polling() {
        let signal = SyncEventSignal::default();
        let sequence = signal.sequence();
        let notifier = signal.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            notifier.notify();
        });
        let started = std::time::Instant::now();
        let next = signal.wait_since(sequence, Duration::from_secs(2));
        thread.join().unwrap();
        assert_ne!(next, sequence);
        assert!(started.elapsed() < Duration::from_secs(1));
        let timeout_started = std::time::Instant::now();
        assert_eq!(signal.wait_since(next, Duration::from_millis(25)), next);
        assert!(timeout_started.elapsed() >= Duration::from_millis(20));
    }

    struct Fixture {
        path: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::SeqCst);
            let path = std::env::temp_dir().join(format!(
                "imail-runtime-{}-{nonce}-{sequence}.sqlite",
                std::process::id()
            ));
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(include_str!(
                    "../../imail-storage-sqlite/sql/schema-v10.sql"
                ))
                .unwrap();
            connection
                .execute_batch(include_str!(
                    "../../imail-storage-sqlite/sql/migration-v11-message-sources.sql"
                ))
                .unwrap();
            connection
                .execute_batch(include_str!(
                    "../../imail-storage-sqlite/sql/migration-v14-rules.sql"
                ))
                .unwrap();
            connection
                .execute(
                    "INSERT INTO metadata(key,value) VALUES ('schema_version',?1)",
                    [CURRENT_SCHEMA_VERSION.to_string()],
                )
                .unwrap();
            connection.execute("INSERT INTO accounts(id,provider,email,display_name,group_name,color,settings_json,encrypted_secret,created_at,status,user_id) VALUES ('account-1','custom','owner@example.com','Owner','personal','#000','{}','cipher','2026-08-10T00:00:00.000Z','connected','user-1')", []).unwrap();
            Self { path }
        }

        fn enqueue(&self) -> String {
            let mut store = SyncRuntimeStore::open_database(&self.path).unwrap();
            store
                .enqueue(
                    &SyncEnqueue {
                        account_id: "account-1".into(),
                        mailbox: None,
                        mailbox_role: "inbox".into(),
                        reason: "manual".into(),
                        priority: 100,
                        not_before: None,
                    },
                    Utc::now(),
                )
                .unwrap()
                .id
        }

        fn wait_for_status(&self, job_id: &str, expected: &str) {
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            loop {
                let store = SyncRuntimeStore::open_database(&self.path).unwrap();
                if store.job(job_id).unwrap().unwrap().status == expected {
                    return;
                }
                assert!(std::time::Instant::now() < deadline, "job did not finish");
                thread::sleep(Duration::from_millis(10));
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    struct SuccessfulExecutor(AtomicUsize);

    impl SyncJobExecutor for SuccessfulExecutor {
        fn execute(
            &self,
            _job: &SyncJobReadModel,
            context: &JobExecutionContext,
        ) -> Result<SyncCompletion, SyncFailure> {
            assert!(!context.is_cancelled());
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(SyncCompletion {
                mailbox: "INBOX".into(),
                uid_validity: Some("1".into()),
                highest_modseq: Some("2".into()),
                last_seen_uid: 3,
                synced: 1,
                created: 1,
                updated: 0,
                deleted: 0,
                message_changes: vec![],
            })
        }
    }

    #[test]
    fn worker_pool_claims_and_completes_persistent_jobs() {
        let fixture = Fixture::new();
        let job_id = fixture.enqueue();
        let executor = Arc::new(SuccessfulExecutor(AtomicUsize::new(0)));
        let mut config = SyncWorkerConfig::new(&fixture.path);
        config.scheduler_enabled = false;
        config.worker_count = 2;
        config.poll_interval = Duration::from_millis(10);
        config.lease_duration = Duration::from_secs(3);
        config.host_name = "test-host".into();
        let event_signal = SyncEventSignal::default();
        config.event_signal = Some(event_signal.clone());
        let mut runtime = PersistentSyncRuntime::start(config, executor.clone()).unwrap();
        fixture.wait_for_status(&job_id, "succeeded");
        assert!(event_signal.sequence() >= 2);
        let health = runtime.health();
        assert_eq!(health.jobs_started, 1);
        assert_eq!(health.jobs_succeeded, 1);
        assert_eq!(health.jobs_failed, 0);
        let report = runtime.shutdown(Duration::from_secs(1));
        assert!(report.graceful());
        assert_eq!(executor.0.load(Ordering::SeqCst), 1);
        let store = SyncRuntimeStore::open_database(&fixture.path).unwrap();
        assert!(store.worker_health(Utc::now()).unwrap().workers.is_empty());
        assert_eq!(store.events(0, 10).unwrap().len(), 2);
    }

    struct FailingExecutor;

    impl SyncJobExecutor for FailingExecutor {
        fn execute(
            &self,
            _job: &SyncJobReadModel,
            _context: &JobExecutionContext,
        ) -> Result<SyncCompletion, SyncFailure> {
            Err(SyncFailure {
                mailbox: "INBOX".into(),
                code: "IMAP_UNAVAILABLE".into(),
                message: "connection reset".into(),
                auth_required: false,
                retry_minutes: Some(1),
            })
        }
    }

    #[test]
    fn worker_persists_classified_failures() {
        let fixture = Fixture::new();
        let job_id = fixture.enqueue();
        let mut config = SyncWorkerConfig::new(&fixture.path);
        config.scheduler_enabled = false;
        config.worker_count = 1;
        config.poll_interval = Duration::from_millis(10);
        config.lease_duration = Duration::from_secs(3);
        let mut runtime = PersistentSyncRuntime::start(config, Arc::new(FailingExecutor)).unwrap();
        fixture.wait_for_status(&job_id, "failed");
        assert!(runtime.shutdown(Duration::from_secs(1)).graceful());
        let store = SyncRuntimeStore::open_database(&fixture.path).unwrap();
        let job = store.job(&job_id).unwrap().unwrap();
        assert_eq!(job.error_code.as_deref(), Some("IMAP_UNAVAILABLE"));
        let state = store.mailbox_state("account-1", "INBOX").unwrap().unwrap();
        assert_eq!(state.sync_state, "backoff");
    }

    struct CooperativeExecutor(AtomicUsize);

    impl SyncJobExecutor for CooperativeExecutor {
        fn execute(
            &self,
            _job: &SyncJobReadModel,
            context: &JobExecutionContext,
        ) -> Result<SyncCompletion, SyncFailure> {
            self.0.fetch_add(1, Ordering::SeqCst);
            while !context.is_cancelled() {
                thread::sleep(Duration::from_millis(5));
            }
            Err(SyncFailure {
                mailbox: "INBOX".into(),
                code: "CANCELLED".into(),
                message: "cancelled".into(),
                auth_required: false,
                retry_minutes: None,
            })
        }
    }

    #[test]
    fn shutdown_cancels_an_in_flight_job_without_recording_a_failure() {
        let fixture = Fixture::new();
        let job_id = fixture.enqueue();
        let executor = Arc::new(CooperativeExecutor(AtomicUsize::new(0)));
        let mut config = SyncWorkerConfig::new(&fixture.path);
        config.scheduler_enabled = false;
        config.worker_count = 1;
        config.poll_interval = Duration::from_millis(10);
        config.lease_duration = Duration::from_secs(3);
        let mut runtime = PersistentSyncRuntime::start(config, executor.clone()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while executor.0.load(Ordering::SeqCst) == 0 {
            assert!(std::time::Instant::now() < deadline, "job did not start");
            thread::sleep(Duration::from_millis(5));
        }
        assert!(runtime.shutdown(Duration::from_secs(1)).graceful());
        fixture.wait_for_status(&job_id, "cancelled");
        let store = SyncRuntimeStore::open_database(&fixture.path).unwrap();
        assert_eq!(
            store.job(&job_id).unwrap().unwrap().error_code,
            None,
            "shutdown must not be reported as an IMAP failure"
        );
    }

    struct WakeExecutor {
        calls: AtomicUsize,
        waiting: AtomicUsize,
    }

    impl AccountWakeExecutor for WakeExecutor {
        fn wait_for_wake(
            &self,
            _account_id: &str,
            context: &crate::AccountWakeContext,
            _maximum_wait: Duration,
        ) -> Result<MailboxWakeReason, String> {
            match self.calls.fetch_add(1, Ordering::SeqCst) {
                0 => return Err("connection reset".into()),
                1 => return Ok(MailboxWakeReason::Changed),
                _ => {}
            }
            self.waiting.fetch_add(1, Ordering::SeqCst);
            context.wait_cancelled(Duration::from_secs(5));
            self.waiting.fetch_sub(1, Ordering::SeqCst);
            Err("disconnected".into())
        }
    }

    #[test]
    fn watcher_manager_reconciles_enabled_accounts_and_queues_wakes() {
        let fixture = Fixture::new();
        let store = SyncRuntimeStore::open_database(&fixture.path).unwrap();
        store
            .ensure_policy("account-1", "user-1", Utc::now())
            .unwrap();
        assert_eq!(store.enabled_sync_account_ids().unwrap(), ["account-1"]);
        drop(store);

        let job_executor = Arc::new(SuccessfulExecutor(AtomicUsize::new(0)));
        let wake_executor = Arc::new(WakeExecutor {
            calls: AtomicUsize::new(0),
            waiting: AtomicUsize::new(0),
        });
        let mut config = SyncWorkerConfig::new(&fixture.path);
        config.scheduler_enabled = false;
        config.worker_count = 1;
        config.poll_interval = Duration::from_millis(10);
        config.lease_duration = Duration::from_secs(3);
        config.watcher_reconcile_interval = Duration::from_millis(10);
        config.watcher_retry_interval = Duration::from_millis(10);
        config.watcher_maximum_wait = Duration::from_secs(1);
        let mut runtime = PersistentSyncRuntime::start_with_watchers(
            config,
            job_executor.clone(),
            wake_executor.clone(),
        )
        .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while job_executor.0.load(Ordering::SeqCst) == 0
            || wake_executor.waiting.load(Ordering::SeqCst) == 0
        {
            let queued = SyncRuntimeStore::open_database(&fixture.path)
                .and_then(|store| store.worker_health(Utc::now()))
                .map(|health| health.queued_jobs)
                .unwrap_or(-1);
            assert!(
                std::time::Instant::now() < deadline,
                "watcher did not enqueue: calls={}, waiting={}, jobs={}, queued={queued}",
                wake_executor.calls.load(Ordering::SeqCst),
                wake_executor.waiting.load(Ordering::SeqCst),
                job_executor.0.load(Ordering::SeqCst)
            );
            thread::sleep(Duration::from_millis(10));
        }
        let health = runtime.health();
        assert_eq!(health.active_watchers, 1);
        assert_eq!(health.watcher_disconnects, 1);
        assert_eq!(health.watcher_reconnects, 1);

        let mut store = SyncRuntimeStore::open_database(&fixture.path).unwrap();
        store
            .update_policy(
                "account-1",
                &SyncPolicySettings {
                    enabled: false,
                    ..SyncPolicySettings::default()
                },
                Utc::now(),
            )
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while wake_executor.waiting.load(Ordering::SeqCst) != 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "disabled watcher did not stop"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert!(runtime.shutdown(Duration::from_secs(1)).graceful());
        assert!(wake_executor.calls.load(Ordering::SeqCst) >= 3);
    }

    #[test]
    fn scheduler_expands_standard_policy_without_a_frontend_connection() {
        let fixture = Fixture::new();
        let mut store = SyncRuntimeStore::open_database(&fixture.path).unwrap();
        store
            .ensure_policy("account-1", "user-1", Utc::now())
            .unwrap();
        store
            .update_policy(
                "account-1",
                &SyncPolicySettings {
                    folder_mode: "standard".into(),
                    ..SyncPolicySettings::default()
                },
                Utc::now(),
            )
            .unwrap();
        drop(store);

        let executor = Arc::new(SuccessfulExecutor(AtomicUsize::new(0)));
        let mut config = SyncWorkerConfig::new(&fixture.path);
        config.worker_count = 1;
        config.poll_interval = Duration::from_millis(10);
        config.lease_duration = Duration::from_secs(3);
        config.scheduler_startup_delay = Duration::ZERO;
        config.scheduler_interval = Duration::from_secs(3_600);
        let mut runtime = PersistentSyncRuntime::start(config, executor.clone()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while executor.0.load(Ordering::SeqCst) < 3 {
            assert!(
                std::time::Instant::now() < deadline,
                "scheduler did not execute every standard target"
            );
            thread::sleep(Duration::from_millis(10));
        }
        let health = runtime.health();
        assert_eq!(health.scheduler_scans, 1);
        assert_eq!(health.scheduler_errors, 0);
        assert!(runtime.shutdown(Duration::from_secs(1)).graceful());
        assert_eq!(executor.0.load(Ordering::SeqCst), 3);
    }

    struct RecordingExecutor(Mutex<Vec<String>>);

    impl SyncJobExecutor for RecordingExecutor {
        fn execute(
            &self,
            job: &SyncJobReadModel,
            _context: &JobExecutionContext,
        ) -> Result<SyncCompletion, SyncFailure> {
            thread::sleep(Duration::from_millis(10));
            self.0.lock().unwrap().push(job.account_id.clone());
            Ok(SyncCompletion {
                mailbox: "INBOX".into(),
                uid_validity: Some("1".into()),
                highest_modseq: Some("2".into()),
                last_seen_uid: 3,
                synced: 1,
                created: 0,
                updated: 0,
                deleted: 0,
                message_changes: vec![],
            })
        }
    }

    #[test]
    fn equal_priority_jobs_make_progress_across_multiple_accounts() {
        let fixture = Fixture::new();
        let connection = Connection::open(&fixture.path).unwrap();
        for index in 2..=5 {
            connection.execute("INSERT INTO accounts(id,provider,email,display_name,group_name,color,settings_json,encrypted_secret,created_at,status,user_id) VALUES (?1,'custom',?2,?3,'personal','#000','{}','cipher',?4,'connected','user-1')", params![format!("account-{index}"),format!("owner-{index}@example.com"),format!("Owner {index}"),format!("2026-08-10T00:00:0{index}.000Z")]).unwrap();
        }
        drop(connection);
        let mut store = SyncRuntimeStore::open_database(&fixture.path).unwrap();
        let mut job_ids = Vec::new();
        for index in 1..=5 {
            job_ids.push(
                store
                    .enqueue(
                        &SyncEnqueue {
                            account_id: format!("account-{index}"),
                            mailbox: None,
                            mailbox_role: "inbox".into(),
                            reason: "manual".into(),
                            priority: 100,
                            not_before: None,
                        },
                        Utc::now(),
                    )
                    .unwrap()
                    .id,
            );
        }
        drop(store);

        let executor = Arc::new(RecordingExecutor(Mutex::new(Vec::new())));
        let mut config = SyncWorkerConfig::new(&fixture.path);
        config.scheduler_enabled = false;
        config.worker_count = 3;
        config.poll_interval = Duration::from_millis(5);
        config.lease_duration = Duration::from_secs(3);
        let mut runtime = PersistentSyncRuntime::start(config, executor.clone()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let store = SyncRuntimeStore::open_database(&fixture.path).unwrap();
            if job_ids
                .iter()
                .all(|id| store.job(id).unwrap().unwrap().status == "succeeded")
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "an equal-priority account was starved"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(runtime.health().jobs_succeeded, 5);
        assert!(runtime.shutdown(Duration::from_secs(1)).graceful());
        let observed = executor
            .0
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        assert_eq!(observed.len(), 5);
    }

    #[test]
    fn rejects_unsafe_worker_configuration() {
        let fixture = Fixture::new();
        let mut config = SyncWorkerConfig::new(&fixture.path);
        config.scheduler_enabled = false;
        config.worker_count = 0;
        assert!(matches!(
            PersistentSyncRuntime::start(config, Arc::new(SuccessfulExecutor(AtomicUsize::new(0)))),
            Err(SyncWorkerStartError::InvalidConfiguration)
        ));
    }
}

use std::{
    collections::BTreeSet,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

mod account_watchers;
mod embedded_executor;
mod rule_executor;
mod sync_workers;
pub use account_watchers::{AccountWakeContext, AccountWakeExecutor};
pub use embedded_executor::{
    EmbeddedSyncExecutor, EmbeddedSyncExecutorError, NetworkSyncMailTransportFactory, SyncImapPort,
    SyncMailTransportFactory,
};
pub use sync_workers::{
    JobExecutionContext, PersistentSyncRuntime, RuntimeHealthSnapshot, SyncEventSignal,
    SyncJobExecutor, SyncWorkerConfig, SyncWorkerStartError,
};

#[derive(Clone, Default)]
pub struct CancellationToken {
    inner: Arc<CancellationState>,
}

#[derive(Default)]
struct CancellationState {
    cancelled: AtomicBool,
    mutex: Mutex<()>,
    changed: Condvar,
}

impl CancellationToken {
    pub fn cancel(&self) {
        if !self.inner.cancelled.swap(true, Ordering::SeqCst) {
            self.inner.changed.notify_all();
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    pub fn wait_cancelled(&self, maximum_wait: Duration) -> bool {
        if self.is_cancelled() {
            return true;
        }
        let guard = self
            .inner
            .mutex
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.is_cancelled() {
            return true;
        }
        let _ = self
            .inner
            .changed
            .wait_timeout_while(guard, maximum_wait, |_| !self.is_cancelled())
            .unwrap_or_else(|error| error.into_inner());
        self.is_cancelled()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCompletion {
    pub name: String,
    pub panicked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownReport {
    pub completed: Vec<TaskCompletion>,
    pub timed_out: Vec<String>,
}

impl ShutdownReport {
    pub fn graceful(&self) -> bool {
        self.timed_out.is_empty() && self.completed.iter().all(|task| !task.panicked)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("runtime 已进入关闭状态")]
    ShuttingDown,
    #[error("runtime 任务名称无效")]
    InvalidTaskName,
    #[error("runtime 任务名称重复：{0}")]
    DuplicateTask(String),
    #[error("无法创建 runtime 任务线程：{0}")]
    Spawn(String),
}

struct ManagedTask {
    name: String,
    completion: Receiver<TaskCompletion>,
    handle: Option<JoinHandle<()>>,
}

#[derive(Default)]
pub struct RuntimeSupervisor {
    cancellation: CancellationToken,
    tasks: Vec<ManagedTask>,
    names: BTreeSet<String>,
    shutting_down: bool,
}

impl RuntimeSupervisor {
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    pub fn spawn<F>(&mut self, name: impl Into<String>, operation: F) -> Result<(), RuntimeError>
    where
        F: FnOnce(CancellationToken) + Send + 'static,
    {
        if self.shutting_down {
            return Err(RuntimeError::ShuttingDown);
        }
        let name = name.into();
        if name.trim().is_empty() || name.len() > 80 {
            return Err(RuntimeError::InvalidTaskName);
        }
        if !self.names.insert(name.clone()) {
            return Err(RuntimeError::DuplicateTask(name));
        }
        let (sender, completion) = mpsc::sync_channel(1);
        let task_name = name.clone();
        let token = self.cancellation.clone();
        let handle = thread::Builder::new()
            .name(format!("imail-{name}"))
            .spawn(move || {
                let panicked = catch_unwind(AssertUnwindSafe(|| operation(token))).is_err();
                let _ = sender.send(TaskCompletion {
                    name: task_name,
                    panicked,
                });
            })
            .map_err(|error| {
                self.names.remove(&name);
                RuntimeError::Spawn(error.to_string())
            })?;
        self.tasks.push(ManagedTask {
            name,
            completion,
            handle: Some(handle),
        });
        Ok(())
    }

    pub fn task_count(&self) -> usize {
        self.tasks.len()
    }

    pub fn shutdown(&mut self, maximum_wait: Duration) -> ShutdownReport {
        self.shutting_down = true;
        self.cancellation.cancel();
        let deadline = Instant::now() + maximum_wait;
        let mut completed = Vec::new();
        let mut timed_out = Vec::new();
        for task in &mut self.tasks {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let result = if remaining.is_zero() {
                task.completion
                    .try_recv()
                    .map_err(|_| RecvTimeoutError::Timeout)
            } else {
                task.completion.recv_timeout(remaining)
            };
            match result {
                Ok(status) => {
                    if let Some(handle) = task.handle.take() {
                        let _ = handle.join();
                    }
                    completed.push(status);
                }
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {
                    timed_out.push(task.name.clone());
                    task.handle.take();
                }
            }
        }
        self.tasks.clear();
        self.names.clear();
        ShutdownReport {
            completed,
            timed_out,
        }
    }
}

impl Drop for RuntimeSupervisor {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

pub fn run_interval(
    cancellation: &CancellationToken,
    interval: Duration,
    mut operation: impl FnMut(),
) {
    let interval = interval.max(Duration::from_millis(10));
    while !cancellation.is_cancelled() {
        operation();
        if cancellation.wait_cancelled(interval) {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn cancels_recurring_tasks_and_shuts_down_gracefully() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let mut runtime = RuntimeSupervisor::default();
        runtime
            .spawn("scheduler", move |token| {
                run_interval(&token, Duration::from_millis(10), || {
                    observed.fetch_add(1, Ordering::SeqCst);
                });
            })
            .unwrap();
        while calls.load(Ordering::SeqCst) == 0 {
            thread::yield_now();
        }
        let report = runtime.shutdown(Duration::from_secs(1));
        assert!(report.graceful());
        assert_eq!(report.completed[0].name, "scheduler");
        assert_eq!(runtime.task_count(), 0);
    }

    #[test]
    fn isolates_panics_without_stopping_other_tasks() {
        let mut runtime = RuntimeSupervisor::default();
        runtime.spawn("broken", |_| panic!("boom")).unwrap();
        runtime
            .spawn("worker", |token| {
                token.wait_cancelled(Duration::from_secs(1));
            })
            .unwrap();
        let report = runtime.shutdown(Duration::from_secs(1));
        assert!(report.timed_out.is_empty());
        assert_eq!(report.completed.len(), 2);
        assert!(report
            .completed
            .iter()
            .any(|task| task.name == "broken" && task.panicked));
        assert!(report
            .completed
            .iter()
            .any(|task| task.name == "worker" && !task.panicked));
    }

    #[test]
    fn reports_uncooperative_tasks_without_blocking_shutdown() {
        let mut runtime = RuntimeSupervisor::default();
        runtime
            .spawn("stuck", |_| thread::sleep(Duration::from_millis(50)))
            .unwrap();
        let started = Instant::now();
        let report = runtime.shutdown(Duration::from_millis(1));
        assert_eq!(report.timed_out, ["stuck"]);
        assert!(started.elapsed() < Duration::from_millis(40));
        thread::sleep(Duration::from_millis(60));
    }

    #[test]
    fn rejects_duplicate_names_and_new_tasks_after_shutdown() {
        let mut runtime = RuntimeSupervisor::default();
        runtime
            .spawn("worker", |token| {
                token.wait_cancelled(Duration::from_secs(1));
            })
            .unwrap();
        assert!(matches!(
            runtime.spawn("worker", |_| {}),
            Err(RuntimeError::DuplicateTask(_))
        ));
        runtime.shutdown(Duration::from_secs(1));
        assert!(matches!(
            runtime.spawn("later", |_| {}),
            Err(RuntimeError::ShuttingDown)
        ));
    }
}

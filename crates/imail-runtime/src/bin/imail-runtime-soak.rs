use std::{
    env,
    error::Error,
    fs,
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use imail_protocol::SyncJobReadModel;
use imail_runtime::{
    JobExecutionContext, PersistentSyncRuntime, SyncJobExecutor, SyncWorkerConfig,
};
use imail_storage_sqlite::{SyncCompletion, SyncFailure, SyncRuntimeStore};
use serde_json::json;
use sysinfo::{get_current_pid, System};

struct Arguments {
    data_dir: PathBuf,
    duration: Duration,
    maximum_growth_bytes: u64,
    report: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = parse_arguments()?;
    let data_dir = arguments.data_dir.canonicalize()?;
    if data_dir.file_name().and_then(|name| name.to_str()) == Some(".data")
        || !data_dir.join("backup-manifest.json").is_file()
    {
        return Err("soak 只允许使用带 backup-manifest.json 的明确数据副本".into());
    }
    let database_path = data_dir.join("imail.sqlite");
    let initial_store = SyncRuntimeStore::open_database(&database_path)?;
    let initial_queue = initial_store.worker_health(chrono::Utc::now())?.queued_jobs;
    if initial_queue != 0 {
        return Err("soak 副本必须先完成全部 queued job".into());
    }
    drop(initial_store);

    let process_id = get_current_pid()?;
    let mut system = System::new();
    system.refresh_process(process_id);
    let mut config = SyncWorkerConfig::new(&database_path);
    config.scheduler_enabled = false;
    config.worker_count = 2;
    config.poll_interval = Duration::from_millis(250);
    let mut runtime = PersistentSyncRuntime::start(config, Arc::new(UnexpectedJobExecutor))?;
    thread::sleep(Duration::from_millis(500));

    let started = Instant::now();
    let mut samples = Vec::new();
    while started.elapsed() < arguments.duration {
        system.refresh_process(process_id);
        let process = system
            .process(process_id)
            .ok_or("无法读取当前 Rust 进程指标")?;
        samples.push((
            started.elapsed().as_millis() as u64,
            process.memory(),
            process.virtual_memory(),
            process.cpu_usage(),
            process.tasks().map(|tasks| tasks.len()),
        ));
        thread::sleep(Duration::from_secs(1));
    }
    let health_before_shutdown = runtime.health();
    let shutdown = runtime.shutdown(Duration::from_secs(2));
    let final_store = SyncRuntimeStore::open_database(&database_path)?;
    let final_worker_health = final_store.worker_health(chrono::Utc::now())?;
    let first_memory = samples.first().map(|sample| sample.1).unwrap_or(0);
    let last_memory = samples.last().map(|sample| sample.1).unwrap_or(0);
    let peak_memory = samples.iter().map(|sample| sample.1).max().unwrap_or(0);
    let growth = last_memory.saturating_sub(first_memory);
    let no_work_executed = health_before_shutdown.jobs_started == 0
        && health_before_shutdown.jobs_succeeded == 0
        && health_before_shutdown.jobs_failed == 0
        && health_before_shutdown.jobs_cancelled == 0;
    let ok = shutdown.graceful()
        && no_work_executed
        && final_worker_health.workers.is_empty()
        && final_worker_health.queued_jobs == 0
        && growth <= arguments.maximum_growth_bytes;
    let report = json!({
        "ok": ok,
        "durationSeconds": arguments.duration.as_secs(),
        "sampleCount": samples.len(),
        "memory": {
            "firstResidentBytes": first_memory,
            "lastResidentBytes": last_memory,
            "peakResidentBytes": peak_memory,
            "growthBytes": growth,
            "maximumGrowthBytes": arguments.maximum_growth_bytes,
            "withinBudget": growth <= arguments.maximum_growth_bytes,
        },
        "runtime": {
            "workerSlots": 2,
            "jobsStarted": health_before_shutdown.jobs_started,
            "jobsSucceeded": health_before_shutdown.jobs_succeeded,
            "jobsFailed": health_before_shutdown.jobs_failed,
            "jobsCancelled": health_before_shutdown.jobs_cancelled,
            "shutdownGraceful": shutdown.graceful(),
            "remainingWorkers": final_worker_health.workers.len(),
            "remainingQueuedJobs": final_worker_health.queued_jobs,
        },
        "samples": samples.iter().map(|sample| json!({
            "elapsedMs": sample.0,
            "residentBytes": sample.1,
            "virtualBytes": sample.2,
            "cpuPercent": sample.3,
            "threadCount": sample.4,
        })).collect::<Vec<_>>(),
    });
    let output = format!("{}\n", serde_json::to_string_pretty(&report)?);
    if let Some(report_path) = arguments.report {
        if report_path.exists() {
            return Err("soak 报告目标已存在，拒绝覆盖".into());
        }
        fs::write(report_path, output.as_bytes())?;
    }
    print!("{output}");
    if !ok {
        return Err("runtime soak 未通过资源或生命周期门禁".into());
    }
    Ok(())
}

struct UnexpectedJobExecutor;

impl SyncJobExecutor for UnexpectedJobExecutor {
    fn execute(
        &self,
        job: &SyncJobReadModel,
        _context: &JobExecutionContext,
    ) -> Result<SyncCompletion, SyncFailure> {
        Err(SyncFailure {
            mailbox: job
                .mailbox
                .clone()
                .unwrap_or_else(|| format!("@role:{}", job.mailbox_role)),
            code: "UNEXPECTED_SOAK_JOB".into(),
            message: "idle soak must not execute jobs".into(),
            auth_required: false,
            retry_minutes: None,
        })
    }
}

fn parse_arguments() -> Result<Arguments, Box<dyn Error>> {
    let mut values = env::args().skip(1);
    let data_dir = values.next().map(PathBuf::from).ok_or(
        "用法：imail-runtime-soak <数据副本> [--duration-seconds N] [--max-growth-mib N] [--report PATH]",
    )?;
    let mut duration_seconds = 60_u64;
    let mut maximum_growth_mib = 16_u64;
    let mut report = None;
    while let Some(argument) = values.next() {
        match argument.as_str() {
            "--duration-seconds" => {
                duration_seconds = values.next().ok_or("缺少 duration")?.parse()?;
            }
            "--max-growth-mib" => {
                maximum_growth_mib = values.next().ok_or("缺少 memory budget")?.parse()?;
            }
            "--report" => report = Some(PathBuf::from(values.next().ok_or("缺少 report path")?)),
            _ => return Err(format!("未知参数：{argument}").into()),
        }
    }
    if !(5..=86_400).contains(&duration_seconds) || maximum_growth_mib == 0 {
        return Err("duration 必须为 5..86400 秒，memory budget 必须大于 0".into());
    }
    Ok(Arguments {
        data_dir,
        duration: Duration::from_secs(duration_seconds),
        maximum_growth_bytes: maximum_growth_mib.saturating_mul(1024 * 1024),
        report,
    })
}

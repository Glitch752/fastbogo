use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::Parser;
use dotenvy::dotenv;
use futures_util::{SinkExt, StreamExt};
use fastbogo::benchmark::{
    BenchmarkConfig, BenchmarkSummary, BenchmarkSweepSummary, run_kernel_benchmark,
    run_kernel_benchmark_sweep,
};
use fastbogo::kernel::{
    DEFAULT_KERNEL_TUNING, KernelTuning, N, RangeResult, run_range, run_range_with_tuning,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::{MissedTickBehavior, interval};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const EXPECTED_VERSION: i64 = 1_780_587_689_532;
const PROTOCOL_VERSION: u8 = 5;
const VERSION_POLL_SECS: u64 = 30;
const REPORT_MS: u64 = 1_000;
const STATUS_MS: u64 = 1_000;
const MIN_REPORT_DELAY_MS: u64 = 900;
const WORK_CHUNK: u64 = 8_000_000;
const DEFAULT_OFFLINE_SEED: u64 = 1;
const DEFAULT_OFFLINE_COUNT: u64 = 100_000_000;
const KERNEL_TUNING_FILE: &str = "kernel_tuning.json";

#[derive(Parser, Debug, Clone)]
#[command(author, version, about)]
struct Cli {
    #[arg(long, help = "Disable websocket traffic and run a local benchmark lease instead")]
    offline: bool,

    #[arg(long, default_value = "https://bogo.swapjs.dev")]
    base_url: String,

    #[arg(long, default_value = "wss://bogo.swapjs.dev/ws")]
    ws_url: String,

    #[arg(long, help = "Override worker thread count; defaults to logical core count")]
    threads: Option<usize>,

    #[arg(long, help = "Begin prune checks at this Fisher-Yates step. Usually 12-14.")]
    prune_check_start: Option<u8>,

    #[arg(long, help = "Offline seed to process")]
    seed: Option<u64>,

    #[arg(long, help = "Offline lease size to process")]
    count: Option<u64>,

    #[arg(long, help = "Print a sample kernel result and exit")]
    print_sample: bool,

    #[arg(long, help = "Run a local kernel benchmark and exit")]
    benchmark: bool,

    #[arg(long, default_value_t = 1, help = "Number of warmup rounds for --benchmark")]
    benchmark_warmup_rounds: usize,

    #[arg(long, default_value_t = 5, help = "Number of measured rounds for --benchmark")]
    benchmark_rounds: usize,

    #[arg(long, help = "Emit benchmark output as JSON")]
    benchmark_json: bool,

    #[arg(long, help = "Comma-separated thread counts to benchmark, e.g. 26,39,52")]
    benchmark_thread_sweep: Option<String>,

    #[arg(long, help = "Comma-separated prune-check starts to benchmark, e.g. 24,18,16,14,13,12")]
    benchmark_prune_sweep: Option<String>
}

#[derive(Clone, Debug)]
struct AuthConfig {
    uuid: String,
    nickname: String,
    code: String,
}

#[derive(Clone, Debug)]
struct WorkerAssignment {
    lease_id: u64,
    seed: u64,
    lo: u64,
    hi: u64,
    tuning: KernelTuning,
}

#[derive(Clone, Debug)]
struct BestCandidate {
    score: u8,
    arr: [u8; N],
    index: u64,
}

#[derive(Clone, Debug)]
struct WorkerProgress {
    lease_id: u64,
    done: u64,
    best: Option<BestCandidate>,
}

#[derive(Debug)]
struct WorkerState {
    generation: u64,
    shutdown: bool,
    assignment: Option<WorkerAssignment>,
}

#[derive(Debug)]
struct WorkerControl {
    state: Mutex<WorkerState>,
    wake: Condvar,
}

#[derive(Debug)]
struct WorkerPool {
    controls: Vec<Arc<WorkerControl>>,
    handles: Vec<JoinHandle<()>>,
    workers: usize,
    worker_tuning: KernelTuning,
}

#[derive(Clone, Debug)]
struct ActiveLease {
    id: u64,
    seed: u64,
    count: u64,
    assigned_at: Instant,
    machine_total_done: u64,
    last_sent_total: u64,
    window_best: Option<BestCandidate>,
    lease_best: Option<BestCandidate>,
}

#[derive(Debug)]
struct SharedState {
    worker_count: usize,
    mode: &'static str,
    connected: bool,
    received_welcome: bool,
    current_lease: Option<ActiveLease>,
    local_done_total: u64,
    session_credited: u64,
    lifetime_shuffles: u64,
    all_time_best: u8,
    recent_counts: VecDeque<(Instant, u64)>,
}

#[derive(Debug)]
struct StatusSnapshot {
    mode: &'static str,
    connected: bool,
    workers: usize,
    rate: f64,
    received_welcome: bool,
    session_credited: u64,
    lifetime_shuffles: u64,
    all_time_best: u8,
    lease: Option<LeaseSnapshot>,
}

#[derive(Debug)]
struct LeaseSnapshot {
    seed: u64,
    count: u64,
    done: u64,
    best: Option<BestCandidate>,
}

#[derive(Debug)]
enum ControlMessage {
    Abort(String),
}

#[derive(Serialize)]
struct HelloMessage<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    v: u8,
    uuid: &'a str,
    nickname: &'a str,
    code: &'a str,
}

#[derive(Serialize)]
struct ResultMessage {
    #[serde(rename = "type")]
    kind: &'static str,
    seed: String,
    total_done: u64,
    best_correct: u8,
    best_arr: Vec<u8>,
    best_index: u64,
}

#[derive(Deserialize, Debug)]
struct VersionResponse {
    version: i64,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type")]
enum ServerMessage {
    #[serde(rename = "welcome")]
    Welcome {
        lifetime_shuffles: u64,
        all_time_best: Option<u8>,
    },
    #[serde(rename = "job")]
    Job {
        seed: String,
        count: u64,
    },
    #[serde(rename = "credited")]
    Credited {
        credit: Option<u64>,
        lifetime_shuffles: Option<u64>,
        all_time_best: Option<u8>,
        my_session_best: Option<u8>,
        batch_best: Option<u8>,
    },
    #[serde(rename = "rejected")]
    Rejected,
    #[serde(rename = "client_outdated")]
    ClientOutdated {
        message: Option<String>,
    },
    #[serde(rename = "banned")]
    Banned {
        reason: Option<String>,
    },
    #[serde(rename = "contributions_closed")]
    ContributionsClosed,
    #[serde(other)]
    Unknown
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.print_sample {
        let sample = run_range(1_234_567_890_123_456_789, 0, 1_000);
        println!("{}", serde_json::to_string_pretty(&SampleOutput::from(&sample))?);
        return Ok(());
    }

    if cli.benchmark {
        run_benchmark_mode(&cli)?;
        return Ok(());
    }

    dotenv().ok();

    let worker_count = cli
        .threads
        .unwrap_or_else(default_worker_count)
        .max(1);
    let client = Client::builder()
        .use_rustls_tls()
        .build()
        .context("failed to create HTTP client")?;

    check_version(&client, &cli.base_url).await?;

    let state = Arc::new(Mutex::new(SharedState::new(worker_count, if cli.offline { "offline" } else { "online" })));
    let (worker_tx, worker_rx) = mpsc::unbounded_channel();

    let saved_tuning = std::fs::read_to_string(KERNEL_TUNING_FILE)
        .ok()
        .and_then(|s| serde_json::from_str::<KernelTuning>(&s).ok());

    let pool = WorkerPool::new(
        worker_count,
        worker_tx,
        KernelTuning {
            prune_check_start: cli.prune_check_start
                .or_else(|| saved_tuning.map(|t| t.prune_check_start))
                .unwrap_or(DEFAULT_KERNEL_TUNING.prune_check_start)
        },
    );
    let status_handle = spawn_status_reporter(Arc::clone(&state));
    let control_handle = spawn_version_poller(client.clone(), cli.base_url.clone());

    println!("Starting worker pool with {} threads and tuning {:?}", worker_count, pool.tuning());

    let result = if cli.offline {
        run_offline(cli.clone(), Arc::clone(&state), pool, worker_rx, control_handle).await
    } else {
        let auth = load_auth()?;
        run_online(cli.clone(), auth, Arc::clone(&state), pool, worker_rx, control_handle).await
    };

    status_handle.abort();
    result
}

async fn run_online(
    cli: Cli,
    auth: AuthConfig,
    state: Arc<Mutex<SharedState>>,
    mut pool: WorkerPool,
    mut worker_rx: mpsc::UnboundedReceiver<WorkerProgress>,
    mut control_rx: mpsc::UnboundedReceiver<ControlMessage>,
) -> Result<()> {
    let (ws_stream, _) = connect_async(&cli.ws_url)
        .await
        .with_context(|| format!("failed to connect websocket {}", cli.ws_url))?;
    let (mut write, mut read) = ws_stream.split();

    let hello = HelloMessage {
        kind: "hello",
        v: PROTOCOL_VERSION,
        uuid: &auth.uuid,
        nickname: &auth.nickname,
        code: &auth.code,
    };
    write
        .send(Message::Text(serde_json::to_string(&hello)?.into()))
        .await
        .context("failed to send hello message")?;

    {
        let mut guard = state.lock().expect("state poisoned");
        guard.connected = true;
    }

    let mut report_tick = interval(Duration::from_millis(REPORT_MS));
    report_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                pool.shutdown();
                bail!("interrupted");
            }
            maybe_control = control_rx.recv() => {
                if let Some(ControlMessage::Abort(reason)) = maybe_control {
                    pool.shutdown();
                    bail!(reason);
                }
            }
            maybe_progress = worker_rx.recv() => {
                if let Some(progress) = maybe_progress {
                    apply_progress(&state, progress);
                } else {
                    pool.shutdown();
                    bail!("worker progress channel closed unexpectedly");
                }
            }
            _ = report_tick.tick() => {
                if let Some(message) = take_result_message(&state) {
                    write.send(Message::Text(serde_json::to_string(&message)?.into())).await.context("failed to send result message")?;
                }
            }
            maybe_msg = read.next() => {
                match maybe_msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Err(err) = handle_server_message(&state, &pool, &text) {
                            pool.shutdown();
                            return Err(err);
                        }
                    }
                    Some(Ok(Message::Close(_))) => {
                        pool.shutdown();
                        bail!("server closed websocket");
                    }
                    Some(Ok(_)) => {}
                    Some(Err(err)) => {
                        pool.shutdown();
                        return Err(err).context("websocket read failed");
                    }
                    None => {
                        pool.shutdown();
                        bail!("websocket stream ended");
                    }
                }
            }
        }
    }
}

async fn run_offline(
    cli: Cli,
    state: Arc<Mutex<SharedState>>,
    mut pool: WorkerPool,
    mut worker_rx: mpsc::UnboundedReceiver<WorkerProgress>,
    mut control_rx: mpsc::UnboundedReceiver<ControlMessage>,
) -> Result<()> {
    let seed = cli.seed.unwrap_or(DEFAULT_OFFLINE_SEED);
    let count = cli.count.unwrap_or(DEFAULT_OFFLINE_COUNT);
    assign_lease(&state, &pool, seed, count);

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                pool.shutdown();
                bail!("interrupted");
            }
            maybe_control = control_rx.recv() => {
                if let Some(ControlMessage::Abort(reason)) = maybe_control {
                    pool.shutdown();
                    bail!(reason);
                }
            }
            maybe_progress = worker_rx.recv() => {
                if let Some(progress) = maybe_progress {
                    apply_progress(&state, progress);
                    if is_offline_complete(&state) {
                        let snapshot = snapshot_state(&state);
                        pool.shutdown();
                        print_offline_summary(&snapshot);
                        return Ok(());
                    }
                } else {
                    pool.shutdown();
                    bail!("worker progress channel closed unexpectedly");
                }
            }
        }
    }
}

fn handle_server_message(state: &Arc<Mutex<SharedState>>, pool: &WorkerPool, text: &str) -> Result<()> {
    let msg: ServerMessage = serde_json::from_str(text).with_context(|| format!("failed to decode server message: {text}"))?;
    match msg {
        ServerMessage::Welcome {
            lifetime_shuffles,
            all_time_best,
        } => {
            let mut guard = state.lock().expect("state poisoned");
            guard.received_welcome = true;
            guard.lifetime_shuffles = lifetime_shuffles;
            guard.all_time_best = all_time_best.unwrap_or(guard.all_time_best);
        }
        ServerMessage::Job { seed, count } => {
            let parsed_seed = seed.parse::<u64>().with_context(|| format!("invalid job seed: {seed}"))?;
            assign_lease(state, pool, parsed_seed, count);
        }
        ServerMessage::Credited {
            credit,
            lifetime_shuffles,
            all_time_best,
            my_session_best,
            batch_best,
        } => {
            let mut guard = state.lock().expect("state poisoned");
            guard.session_credited = guard.session_credited.saturating_add(credit.unwrap_or(0));
            if let Some(lifetime) = lifetime_shuffles {
                guard.lifetime_shuffles = guard.lifetime_shuffles.max(lifetime);
            }
            if let Some(best) = all_time_best {
                guard.all_time_best = guard.all_time_best.max(best);
            }
            if let Some(best) = my_session_best {
                guard.all_time_best = guard.all_time_best.max(best);
            }
            if let Some(best) = batch_best {
                guard.all_time_best = guard.all_time_best.max(best);
            }
        }
        ServerMessage::Rejected => {
            eprintln!("server rejected a result frame");
        }
        ServerMessage::ClientOutdated { message } => {
            bail!(message.unwrap_or_else(|| "server reported client_outdated".to_owned()));
        }
        ServerMessage::Banned { reason } => {
            bail!(format!("server reported banned: {}", reason.unwrap_or_else(|| "unknown".to_owned())));
        }
        ServerMessage::ContributionsClosed => {
            bail!("server closed contributions");
        }
        ServerMessage::Unknown => {
            // ignore unknown messages
        }
    }
    Ok(())
}

fn assign_lease(state: &Arc<Mutex<SharedState>>, pool: &WorkerPool, seed: u64, count: u64) {
    let lease_id = unique_lease_id();
    {
        let mut guard = state.lock().expect("state poisoned");
        guard.current_lease = Some(ActiveLease {
            id: lease_id,
            seed,
            count,
            assigned_at: Instant::now(),
            machine_total_done: 0,
            last_sent_total: 0,
            window_best: None,
            lease_best: None,
        });
    }
    pool.assign(lease_id, seed, count);
}

fn apply_progress(state: &Arc<Mutex<SharedState>>, progress: WorkerProgress) {
    let mut guard = state.lock().expect("state poisoned");
    let now = Instant::now();
    guard.local_done_total = guard.local_done_total.saturating_add(progress.done);
    if progress.done > 0 {
        guard.recent_counts.push_back((now, progress.done));
        trim_recent_counts(&mut guard.recent_counts, now);
    }
    if let Some(active) = guard.current_lease.as_mut() {
        if active.id != progress.lease_id {
            return;
        }
        active.machine_total_done = active.machine_total_done.saturating_add(progress.done);
        if let Some(best) = progress.best {
            if active.window_best.as_ref().is_none_or(|current| current.score < best.score) {
                active.window_best = Some(best.clone());
            }
            if active.lease_best.as_ref().is_none_or(|current| current.score < best.score) {
                active.lease_best = Some(best.clone());
            }
            guard.all_time_best = guard.all_time_best.max(best.score);
        }
    }
}

fn take_result_message(state: &Arc<Mutex<SharedState>>) -> Option<ResultMessage> {
    let mut guard = state.lock().expect("state poisoned");
    let active = guard.current_lease.as_mut()?;
    if active.assigned_at.elapsed() < Duration::from_millis(MIN_REPORT_DELAY_MS) {
        return None;
    }
    if active.machine_total_done <= active.last_sent_total {
        return None;
    }
    let best = active.window_best.take()?;
    active.last_sent_total = active.machine_total_done;
    Some(ResultMessage {
        kind: "result",
        seed: active.seed.to_string(),
        total_done: active.machine_total_done,
        best_correct: best.score,
        best_arr: best.arr.to_vec(),
        best_index: best.index,
    })
}

fn is_offline_complete(state: &Arc<Mutex<SharedState>>) -> bool {
    let guard = state.lock().expect("state poisoned");
    guard
        .current_lease
        .as_ref()
        .is_some_and(|lease| lease.machine_total_done >= lease.count)
}

fn snapshot_state(state: &Arc<Mutex<SharedState>>) -> StatusSnapshot {
    let guard = state.lock().expect("state poisoned");
    let now = Instant::now();
    let rate = recent_rate(&guard.recent_counts, now);
    StatusSnapshot {
        mode: guard.mode,
        connected: guard.connected,
        workers: guard.worker_count,
        rate,
        received_welcome: guard.received_welcome,
        session_credited: guard.session_credited,
        lifetime_shuffles: guard.lifetime_shuffles,
        all_time_best: guard.all_time_best,
        lease: guard.current_lease.as_ref().map(|lease| LeaseSnapshot {
            seed: lease.seed,
            count: lease.count,
            done: lease.machine_total_done,
            best: lease.lease_best.clone(),
        }),
    }
}

fn print_offline_summary(snapshot: &StatusSnapshot) {
    println!(
        "offline complete workers={} rate(10s)={:.3} M/s best={} done={}",
        snapshot.workers,
        snapshot.rate / 1_000_000.0,
        snapshot.lease.as_ref().and_then(|lease| lease.best.as_ref().map(|best| best.score)).unwrap_or(0),
        snapshot.lease.as_ref().map(|lease| lease.done).unwrap_or(0),
    );
    if let Some(best) = snapshot.lease.as_ref().and_then(|lease| lease.best.as_ref()) {
        println!(
            "best index={} arr={}",
            best.index,
            format_arr(&best.arr),
        );
    }
}

fn spawn_status_reporter(state: Arc<Mutex<SharedState>>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_millis(STATUS_MS));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let snapshot = snapshot_state(&state);
            println!("{}", snapshot);
        }
    })
}

fn spawn_version_poller(client: Client, base_url: String) -> mpsc::UnboundedReceiver<ControlMessage> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(VERSION_POLL_SECS));
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            match check_version(&client, &base_url).await {
                Ok(()) => {}
                Err(err) => {
                    let _ = tx.send(ControlMessage::Abort(format!("version check failed: {err:#}")));
                    return;
                }
            }
        }
    });
    rx
}

async fn check_version(client: &Client, base_url: &str) -> Result<()> {
    let url = format!("{}/api/version", base_url.trim_end_matches('/'));
    let response = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to fetch version from {url}"))?;
    let response = response.error_for_status().context("version endpoint returned an error")?;
    let version = response
        .json::<VersionResponse>()
        .await
        .context("failed to decode version response")?;
    if version.version != EXPECTED_VERSION {
        bail!("expected version {}, got {}", EXPECTED_VERSION, version.version);
    }
    Ok(())
}

fn load_auth() -> Result<AuthConfig> {
    let uuid = std::env::var("BOGO_UUID").context("missing BOGO_UUID in environment or .env")?;
    let nickname = std::env::var("BOGO_NICK").context("missing BOGO_NICK in environment or .env")?;
    let code = std::env::var("BOGO_CODE").unwrap_or_default();
    if uuid.trim().is_empty() {
        bail!("BOGO_UUID is empty");
    }
    if nickname.trim().is_empty() {
        bail!("BOGO_NICK is empty");
    }
    Ok(AuthConfig {
        uuid,
        nickname,
        code,
    })
}

impl WorkerPool {
    fn new(
        workers: usize,
        progress_tx: mpsc::UnboundedSender<WorkerProgress>,
        tuning: KernelTuning,
    ) -> Self {
        let cores = core_affinity::get_core_ids().unwrap();
        // Wrapping allocation of workers to cores (e.g. for 2 cores and 4 workers: 0, 1, 0, 1)
        let core_ids = cores.into_iter().cycle();
        
        let mut controls = Vec::with_capacity(workers);
        let mut handles = Vec::with_capacity(workers);
        for core_id in core_ids.take(workers) {
            let control = Arc::new(WorkerControl::new());
            let thread_control = Arc::clone(&control);
            let thread_tx = progress_tx.clone();
            let handle = thread::spawn(move || {
                core_affinity::set_for_current(core_id);
                worker_loop(thread_control, thread_tx, tuning)
            });
            controls.push(control);
            handles.push(handle);
        }
        Self {
            controls,
            handles,
            workers,
            worker_tuning: tuning,
        }
    }

    fn assign(&self, lease_id: u64, seed: u64, count: u64) {
        for (idx, control) in self.controls.iter().enumerate() {
            let lo = ((idx as u128 * count as u128) / self.workers as u128) as u64;
            let hi = ((((idx + 1) as u128) * count as u128) / self.workers as u128) as u64;
            control.assign(WorkerAssignment {
                lease_id,
                seed,
                lo,
                hi,
                tuning: self.tuning(),
            });
        }
    }

    fn tuning(&self) -> KernelTuning {
        self.worker_tuning
    }

    fn shutdown(&mut self) {
        for control in &self.controls {
            control.shutdown();
        }
        while let Some(handle) = self.handles.pop() {
            let _ = handle.join();
        }
    }
}

impl WorkerControl {
    fn new() -> Self {
        Self {
            state: Mutex::new(WorkerState {
                generation: 0,
                shutdown: false,
                assignment: None,
            }),
            wake: Condvar::new(),
        }
    }

    fn assign(&self, assignment: WorkerAssignment) {
        let mut state = self.state.lock().expect("worker state poisoned");
        state.generation = state.generation.wrapping_add(1);
        state.assignment = Some(assignment);
        self.wake.notify_all();
    }

    fn shutdown(&self) {
        let mut state = self.state.lock().expect("worker state poisoned");
        state.shutdown = true;
        self.wake.notify_all();
    }

    fn wait_for_assignment(&self, last_generation: u64) -> Option<(u64, WorkerAssignment)> {
        let mut state = self.state.lock().expect("worker state poisoned");
        while !state.shutdown && state.generation == last_generation {
            state = self.wake.wait(state).expect("worker state poisoned while waiting");
        }
        if state.shutdown {
            None
        } else {
            Some((state.generation, state.assignment.clone().expect("assignment missing after wake")))
        }
    }

    fn should_abort(&self, generation: u64) -> bool {
        let state = self.state.lock().expect("worker state poisoned");
        state.shutdown || state.generation != generation
    }
}

fn worker_loop(
    control: Arc<WorkerControl>,
    progress_tx: mpsc::UnboundedSender<WorkerProgress>,
    fallback_tuning: KernelTuning,
) {
    let mut generation = 0;
    while let Some((new_generation, assignment)) = control.wait_for_assignment(generation) {
        generation = new_generation;
        let mut cur = assignment.lo;
        let mut pending_done = 0u64;
        let mut pending_best: Option<BestCandidate> = None;
        let mut last_flush = Instant::now();
        let tuning = assignment.tuning;
        while cur < assignment.hi {
            if control.should_abort(generation) {
                break;
            }
            let hi = assignment.hi.min(cur.saturating_add(WORK_CHUNK));
            let chunk = run_range_with_tuning(
                assignment.seed,
                cur,
                hi,
                if tuning.prune_check_start == 0 {
                    fallback_tuning
                } else {
                    tuning
                },
            );
            pending_done = pending_done.saturating_add(hi - cur);
            merge_best(
                &mut pending_best,
                BestCandidate {
                    score: chunk.best_score,
                    arr: chunk.best_arr,
                    index: chunk.best_index,
                },
            );
            cur = hi;
            if last_flush.elapsed() >= Duration::from_millis(REPORT_MS) || cur >= assignment.hi {
                let _ = progress_tx.send(WorkerProgress {
                    lease_id: assignment.lease_id,
                    done: pending_done,
                    best: pending_best.take(),
                });
                pending_done = 0;
                last_flush = Instant::now();
            }
        }
    }
}

#[derive(Serialize)]
struct SampleOutput {
    best: u8,
    best_index: u64,
    best_arr: Vec<u8>,
}

impl From<&RangeResult> for SampleOutput {
    fn from(value: &RangeResult) -> Self {
        Self {
            best: value.best_score,
            best_index: value.best_index,
            best_arr: value.best_arr.to_vec(),
        }
    }
}

fn run_benchmark_mode(cli: &Cli) -> Result<()> {
    let kernel_tuning = if let Ok(tuning) = std::fs::read_to_string(KERNEL_TUNING_FILE) {
        serde_json::from_str(&tuning).unwrap_or(DEFAULT_KERNEL_TUNING)
    } else {
        DEFAULT_KERNEL_TUNING
    };

    let base_config = BenchmarkConfig {
        seed: cli.seed.unwrap_or(DEFAULT_OFFLINE_SEED),
        count: cli.count.unwrap_or(DEFAULT_OFFLINE_COUNT),
        threads: cli.threads.unwrap_or_else(default_worker_count),
        warmup_rounds: cli.benchmark_warmup_rounds,
        measure_rounds: cli.benchmark_rounds,
        tuning: kernel_tuning,
    };

    let thread_sweep = parse_number_list::<usize>(cli.benchmark_thread_sweep.as_deref())?;
    let prune_sweep = parse_number_list::<u8>(cli.benchmark_prune_sweep.as_deref())?;

    if !thread_sweep.is_empty() || !prune_sweep.is_empty() {
        let summary = run_kernel_benchmark_sweep(&base_config, &thread_sweep, &prune_sweep);
        if cli.benchmark_json {
            println!("{}", serde_json::to_string_pretty(&summary)?);
        } else {
            print_benchmark_sweep_summary(&summary);
        }

        // Find the best configuration and save it to a file
        let best_case = summary.cases.iter().max_by(|a, b|
            a.summary.mean_shuffles_per_sec.total_cmp(&b.summary.mean_shuffles_per_sec)
        ).unwrap();
        let best_config = KernelTuning {
            prune_check_start: best_case.prune_check_start,
        };
        std::fs::write(KERNEL_TUNING_FILE, serde_json::to_string_pretty(&best_config)?).unwrap();
        println!("Best configuration saved to {}: {:?} (with {} threads)", KERNEL_TUNING_FILE, best_config, best_case.threads);
        
        return Ok(());
    }

    let summary = run_kernel_benchmark(&base_config);

    if cli.benchmark_json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        print_benchmark_summary(&summary);
    }
    Ok(())
}

fn print_benchmark_summary(summary: &BenchmarkSummary) {
    println!(
        "benchmark seed={} count={} threads={} prune_check_start={} warmup={} rounds={}",
        summary.seed,
        summary.count,
        summary.threads,
        summary.prune_check_start,
        summary.warmup_rounds,
        summary.measure_rounds,
    );
    for (idx, round) in summary.rounds.iter().enumerate() {
        println!(
            "round={} elapsed={:.6}s rate={:.3} M/s best={} index={}",
            idx + 1,
            round.elapsed_secs,
            round.shuffles_per_sec / 1_000_000.0,
            round.best_score,
            round.best_index,
        );
    }
    println!(
        "summary mean={:.3} M/s median={:.3} M/s best={:.3} M/s worst={:.3} M/s",
        summary.mean_shuffles_per_sec / 1_000_000.0,
        summary.median_shuffles_per_sec / 1_000_000.0,
        summary.best_shuffles_per_sec / 1_000_000.0,
        summary.worst_shuffles_per_sec / 1_000_000.0,
    );
}

fn print_benchmark_sweep_summary(summary: &BenchmarkSweepSummary) {
    println!(
        "benchmark sweep seed={} count={} warmup={} rounds={}",
        summary.seed,
        summary.count,
        summary.warmup_rounds,
        summary.measure_rounds,
    );
    for case in &summary.cases {
        println!(
            "threads={} prune_check_start={} mean={:.3} M/s median={:.3} M/s best={:.3} M/s worst={:.3} M/s",
            case.threads,
            case.prune_check_start,
            case.summary.mean_shuffles_per_sec / 1_000_000.0,
            case.summary.median_shuffles_per_sec / 1_000_000.0,
            case.summary.best_shuffles_per_sec / 1_000_000.0,
            case.summary.worst_shuffles_per_sec / 1_000_000.0,
        );
    }
}

fn parse_number_list<T>(value: Option<&str>) -> Result<Vec<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let Some(value) = value else {
        return Ok(Vec::new());
    };

    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| item.parse::<T>().map_err(|err| anyhow::anyhow!("invalid list value `{item}`: {err}")))
        .collect()
}

fn merge_best(slot: &mut Option<BestCandidate>, candidate: BestCandidate) {
    if slot.as_ref().is_none_or(|best| candidate.score > best.score) {
        *slot = Some(candidate);
    }
}

fn trim_recent_counts(recent_counts: &mut VecDeque<(Instant, u64)>, now: Instant) {
    while recent_counts
        .front()
        .is_some_and(|(ts, _)| now.duration_since(*ts) > Duration::from_secs(10))
    {
        recent_counts.pop_front();
    }
}

fn recent_rate(recent_counts: &VecDeque<(Instant, u64)>, now: Instant) -> f64 {
    if recent_counts.len() < 2 {
        return 0.0;
    }
    let earliest = recent_counts.front().expect("recent_counts not empty").0;
    let span = now.duration_since(earliest).as_secs_f64().max(0.001);
    let total: u64 = recent_counts.iter().map(|(_, count)| *count).sum();
    total as f64 / span
}

fn unique_lease_id() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before unix epoch")
        .as_nanos() as u64
}

fn default_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

fn format_arr(arr: &[u8; N]) -> String {
    arr.iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

impl SharedState {
    fn new(worker_count: usize, mode: &'static str) -> Self {
        Self {
            worker_count,
            mode,
            connected: false,
            received_welcome: false,
            current_lease: None,
            local_done_total: 0,
            session_credited: 0,
            lifetime_shuffles: 0,
            all_time_best: 0,
            recent_counts: VecDeque::new(),
        }
    }
}

impl fmt::Display for StatusSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(lease) = &self.lease {
            let progress = if lease.count == 0 {
                0.0
            } else {
                (lease.done as f64 / lease.count as f64) * 100.0
            };
            write!(
                f,
                "mode={} {} {} rate(10s)={:.3} M/s session={} lifetime={} best={} seed={} progress={:.3}% ({}/{})",
                self.mode,
                if self.connected { "connected" } else { "" },
                if self.received_welcome { "welcome" } else { "" },
                self.rate / 1_000_000.0,
                self.session_credited,
                self.lifetime_shuffles,
                self.all_time_best,
                lease.seed,
                progress,
                lease.done,
                lease.count,
            )?;
            if let Some(best) = &lease.best {
                write!(f, " lease_best={} index={}", best.score, best.index)?;
            }
            Ok(())
        } else {
            write!(
                f,
                "mode={} {} {} rate(10s)={:.3} M/s session={} lifetime={} best={} waiting_for_lease=true",
                self.mode,
                if self.connected { "connected" } else { "" },
                if self.received_welcome { "welcome" } else { "" },
                self.rate / 1_000_000.0,
                self.session_credited,
                self.lifetime_shuffles,
                self.all_time_best,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn segment_split_matches_browser_formula() {
        let workers = 6usize;
        let count = 17u64;
        let segments = (0..workers)
            .map(|idx| {
                let lo = ((idx as u128 * count as u128) / workers as u128) as u64;
                let hi = ((((idx + 1) as u128) * count as u128) / workers as u128) as u64;
                (lo, hi)
            })
            .collect::<Vec<_>>();
        assert_eq!(segments, vec![(0, 2), (2, 5), (5, 8), (8, 11), (11, 14), (14, 17)]);
    }
}

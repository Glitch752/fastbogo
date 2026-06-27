use std::sync::{Arc, Barrier, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::kernel::{KernelTuning, N, RangeResult, run_range_with_tuning};

#[derive(Clone, Debug)]
pub struct BenchmarkConfig {
    pub seed: u64,
    pub count: u64,
    pub threads: usize,
    pub warmup_rounds: usize,
    pub measure_rounds: usize,
    pub tuning: KernelTuning,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkRound {
    pub elapsed_secs: f64,
    pub shuffles_per_sec: f64,
    pub best_score: u8,
    pub best_index: u64,
    pub best_arr: Vec<u8>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkSummary {
    pub seed: u64,
    pub count: u64,
    pub threads: usize,
    pub prune_check_start: u8,
    pub warmup_rounds: usize,
    pub measure_rounds: usize,
    pub mean_shuffles_per_sec: f64,
    pub median_shuffles_per_sec: f64,
    pub best_shuffles_per_sec: f64,
    pub worst_shuffles_per_sec: f64,
    pub rounds: Vec<BenchmarkRound>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkSweepCase {
    pub threads: usize,
    pub prune_check_start: u8,
    pub summary: BenchmarkSummary,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkSweepSummary {
    pub seed: u64,
    pub count: u64,
    pub warmup_rounds: usize,
    pub measure_rounds: usize,
    pub cases: Vec<BenchmarkSweepCase>,
}

pub fn run_kernel_benchmark(config: &BenchmarkConfig) -> BenchmarkSummary {
    let threads = config.threads.max(1);
    for _ in 0..config.warmup_rounds {
        let _ = run_benchmark_round(config.seed, config.count, threads, config.tuning);
    }

    let mut rounds = Vec::with_capacity(config.measure_rounds.max(1));
    for _ in 0..config.measure_rounds.max(1) {
        rounds.push(run_benchmark_round(config.seed, config.count, threads, config.tuning));
    }

    let mut rates = rounds
        .iter()
        .map(|round| round.shuffles_per_sec)
        .collect::<Vec<_>>();
    rates.sort_by(|left, right| left.partial_cmp(right).expect("benchmark rate should be finite"));
    let mean = rounds.iter().map(|round| round.shuffles_per_sec).sum::<f64>() / rounds.len() as f64;
    let median = rates[rates.len() / 2];
    let best = *rates.last().expect("benchmark rates not empty");
    let worst = rates[0];

    BenchmarkSummary {
        seed: config.seed,
        count: config.count,
        threads,
        prune_check_start: config.tuning.prune_check_start,
        warmup_rounds: config.warmup_rounds,
        measure_rounds: rounds.len(),
        mean_shuffles_per_sec: mean,
        median_shuffles_per_sec: median,
        best_shuffles_per_sec: best,
        worst_shuffles_per_sec: worst,
        rounds,
    }
}

pub fn run_kernel_benchmark_sweep(
    base: &BenchmarkConfig,
    thread_values: &[usize],
    prune_values: &[u8],
) -> BenchmarkSweepSummary {
    let threads = if thread_values.is_empty() {
        vec![base.threads]
    } else {
        thread_values.to_vec()
    };
    let prune_values = if prune_values.is_empty() {
        vec![base.tuning.prune_check_start]
    } else {
        prune_values.to_vec()
    };

    let mut cases = Vec::with_capacity(threads.len() * prune_values.len());
    for &thread_count in &threads {
        for &prune_check_start in &prune_values {
            let config = BenchmarkConfig {
                seed: base.seed,
                count: base.count,
                threads: thread_count,
                warmup_rounds: base.warmup_rounds,
                measure_rounds: base.measure_rounds,
                tuning: KernelTuning { prune_check_start },
            };
            cases.push(BenchmarkSweepCase {
                threads: thread_count,
                prune_check_start,
                summary: run_kernel_benchmark(&config),
            });
        }
    }

    BenchmarkSweepSummary {
        seed: base.seed,
        count: base.count,
        warmup_rounds: base.warmup_rounds,
        measure_rounds: base.measure_rounds,
        cases,
    }
}

fn run_benchmark_round(seed: u64, count: u64, threads: usize, tuning: KernelTuning) -> BenchmarkRound {
    let ready = Arc::new(Barrier::new(threads + 1));
    let go = Arc::new(AtomicBool::new(false));

    let mut partials = Vec::with_capacity(threads);
    thread::scope(|scope| {
        let mut handles = Vec::with_capacity(threads);
        for idx in 0..threads {
            let lo = ((idx as u128 * count as u128) / threads as u128) as u64;
            let hi = ((((idx + 1) as u128) * count as u128) / threads as u128) as u64;
            let ready = Arc::clone(&ready);
            let go = Arc::clone(&go);
            handles.push(scope.spawn(move || {
                ready.wait();
                while !go.load(Ordering::Acquire) {
                    std::hint::spin_loop();
                }
                run_range_with_tuning(seed, lo, hi, tuning)
            }));
        }

        ready.wait();
        let started = Instant::now();
        go.store(true, Ordering::Release);
        for handle in handles {
            partials.push(handle.join().expect("benchmark worker panicked"));
        }
        build_round(count, started.elapsed(), partials)
    })
}

fn build_round(count: u64, elapsed: Duration, partials: Vec<RangeResult>) -> BenchmarkRound {
    let mut best = partials[0].clone();
    for partial in partials.into_iter().skip(1) {
        if partial.best_score > best.best_score {
            best = partial;
        }
    }
    let elapsed_secs = elapsed.as_secs_f64();
    let shuffles_per_sec = count as f64 / elapsed_secs.max(0.000_001);
    BenchmarkRound {
        elapsed_secs,
        shuffles_per_sec,
        best_score: best.best_score,
        best_index: best.best_index,
        best_arr: best.best_arr[..N].to_vec(),
    }
}
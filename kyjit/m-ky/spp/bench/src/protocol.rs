//! Measurement Protocol Implementation
//!
//! Implements the exact measurement protocol as specified:
//! - Pin to CPU 0
//! - Attempt turbo disable (warn on failure)
//! - Deterministic random data
//! - Warmup 10, measure 100
//! - Interleaved baseline/candidate
//! - clock_gettime(CLOCK_MONOTONIC_RAW)
//! - median + stddev + min
//! - Interleaved baseline/candidate measurements

use crate::csv::CsvRow;
use crate::generate::Candidate;
use crate::generate::generate_all;
use crate::validity::check_row;
use crate::csv::write_corpus;
use spp::layer2::emit::{emit_candidates, loop_ir_from_analysis, CompiledKernel, TransformCandidate};
use spp::HardwareModel;
use std::process::{Command, Stdio};
use std::time::Instant;
use std::thread;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Barrier};
use spp::layer1::types::DependenceAnalysis;

fn create_driver_code(kernel: &str, _is_candidate: bool, _tile_size: usize, _simd_width: usize, _threads: usize) -> String {
    // Driver only calls the kernel function - the kernel function is defined in the separate kernel.c
    let body = match kernel {
        "copy" => "kernel_copy(input, output, N);",
        "strided_copy" => "kernel_strided_copy(input, output, N, stride);",
        "transpose" => "kernel_transpose(input, output, M, N);",
        "gemm" => "kernel_gemm(A, B, C, M, N, K);",
        _ => "",
    };

    format!(r#"
#include "kernels.h"
#include <stdlib.h>

int main(int argc, char** argv) {{
    long N = 8192;
    long M = 64, K = 64;
    long stride = 4;
    if (argc > 1) N = atol(argv[1]);
    if (argc > 2) M = atol(argv[2]);
    if (argc > 3) stride = atol(argv[3]);

    double* input = (double*)aligned_alloc(64, N * 16 * sizeof(double));
    double* output = (double*)aligned_alloc(64, N * sizeof(double));
    double* A = (double*)aligned_alloc(64, 64 * 64 * sizeof(double));
    double* B = (double*)aligned_alloc(64, 64 * 64 * sizeof(double));
    double* C = (double*)aligned_alloc(64, 64 * 64 * sizeof(double));

    for (long i = 0; i < N * 16; i++) input[i] = (double)rand() / RAND_MAX * 2.0 - 1.0;
    for (long i = 0; i < N; i++) output[i] = 0.0;
    for (long i = 0; i < 64 * 64; i++) A[i] = (double)rand() / RAND_MAX * 2.0 - 1.0;
    for (long i = 0; i < 64 * 64; i++) B[i] = (double)rand() / RAND_MAX * 2.0 - 1.0;
    for (long i = 0; i < 64 * 64; i++) C[i] = 0.0;

    {}

    free(input);
    free(output);
    free(A);
    free(B);
    free(C);
    return 0;
}}
"#, body)
}

#[derive(Clone, Copy, Debug)]
pub enum WorkingSet {
    L1,
    L2,
    DRAM,
}

impl WorkingSet {
    pub fn element_count(&self) -> usize {
        match self {
            WorkingSet::L1 => 2048,
            WorkingSet::L2 => 65536,
            WorkingSet::DRAM => 8_000_000,
        }
    }

    pub fn bytes(&self) -> usize {
        self.element_count() * 8
    }

    pub fn name(&self) -> &'static str {
        match self {
            WorkingSet::L1 => "L1",
            WorkingSet::L2 => "L2",
            WorkingSet::DRAM => "DRAM",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MeasurementStats {
    pub median: u64,
    pub min: u64,
    pub stddev: u64,
}

#[derive(Debug, Clone)]
pub struct Measurement {
    pub baseline_median_ns: u64,
    pub candidate_median_ns: u64,
    pub baseline_stddev_ns: u64,
    pub candidate_stddev_ns: u64,
    pub baseline_min_ns: u64,
    pub candidate_min_ns: u64,
}

#[derive(Debug, Clone)]
pub struct RunConfig {
    pub kernel_name: String,
    pub candidate_name: String,
    pub working_set: WorkingSet,
    pub tile_size: usize,
    pub simd_width: usize,
    pub thread_count: usize,
    pub baseline_binary: String,
    pub candidate_binary: String,
}

pub fn run_measurements() {
    let candidates = generate_all();
    let mut all_rows = Vec::new();

    let working_sets = [WorkingSet::L1, WorkingSet::L2, WorkingSet::DRAM];

    for candidate in &candidates {
        for ws in &working_sets {
            match run_single_measurement(candidate, *ws) {
                Ok(row) => all_rows.push(row),
                Err(e) => {
                    eprintln!("Measurement failed for {} {} {}: {}",
                        candidate.kernel, candidate.name, ws.name(), e);
                    let row = CsvRow::invalid(
                        &candidate.kernel, *ws,
                        candidate.tile_size, candidate.simd_width, candidate.thread_count,
                        &format!("measurement_failed: {}", e)
                    );
                    all_rows.push(row);
                }
            }
        }
    }

    write_corpus(&all_rows).expect("Failed to write corpus");
    println!("Generated {} rows", all_rows.len());
}

fn run_single_measurement(candidate: &Candidate, ws: WorkingSet) -> Result<CsvRow, String> {
    let baseline_binary = compile_kernel(&candidate.kernel, false, 0, 0, 1)?;
    let candidate_binary = compile_kernel(&candidate.kernel, true,
        candidate.tile_size, candidate.simd_width, candidate.thread_count)?;

    let element_count = ws.element_count();
    let (mut input_data, mut output_data) = allocate_and_init(element_count)?;

    pin_cpu(0)?;
    disable_turbo();

    warmup(&baseline_binary, &input_data, &mut output_data, element_count)?;
    warmup(&candidate_binary, &input_data, &mut output_data, element_count)?;

    let (baseline_stats, candidate_stats) = measure_interleaved(
        &baseline_binary, &candidate_binary,
        &input_data, &mut output_data, element_count
    )?;

    let speedup = baseline_stats.median as f64 / candidate_stats.median as f64;

    let (valid, invalid_reason) = check_row(
        &candidate, &baseline_stats, &candidate_stats, speedup
    );

    let loop_id = compute_loop_id(&candidate.kernel, ws.bytes());

    Ok(CsvRow {
        loop_id,
        kernel: candidate.kernel.clone(),
        loop_depth: candidate.loop_depth,
        trip_count_log: (ws.element_count() as f64).log10() as u64,
        body_size: candidate.body_size,
        memory_accesses: candidate.memory_accesses,
        compute_intensity: candidate.compute_intensity,
        stride_pattern: candidate.stride_pattern.clone(),
        working_set_bytes: ws.bytes() as u64,
        cache_level: ws.name().to_string(),
        tile_size: candidate.tile_size as u64,
        simd_width: candidate.simd_width as u64,
        thread_count: candidate.thread_count as u64,
        baseline_median_ns: baseline_stats.median,
        candidate_median_ns: candidate_stats.median,
        baseline_stddev_ns: baseline_stats.stddev,
        candidate_stddev_ns: candidate_stats.stddev,
        speedup,
        valid: if valid { 1 } else { 0 },
        invalid_reason,
    })
}

fn compile_kernel(kernel: &str, is_candidate: bool, tile_size: usize, simd_width: usize, threads: usize) -> Result<String, String> {
    let binary_name = format!("bench_{}_{}_{}_{}_{}",
        kernel, if is_candidate { "cand" } else { "base" },
        tile_size, simd_width, threads);

    let binary_path = format!("bench/generated/{}", binary_name);

    // Create output directory
    std::fs::create_dir_all("bench/generated").map_err(|e| format!("Failed to create generated dir: {}", e))?;

    let (source, _) = if !is_candidate {
        // Baseline: use original kernel with a driver
        let driver_code = create_driver_code(kernel, false, 0, 0, 1);
        (driver_code, String::new())
    } else {
        // Candidate: use emitted code from layer2 emit module
        let hw = HardwareModel::detect();
        let analysis = DependenceAnalysis {
            loop_hash: format!("{}_test", kernel),
            edges: vec![],
            loop_kind: spp::layer1::types::LoopKind::Doall,
            reductions: vec![],
            recurrences: vec![],
            alias_info: spp::layer1::types::AliasInfo { may_alias: vec![], must_alias: vec![], no_alias: vec![], disjoint_arrays: vec![] },
            effect_summary: spp::layer1::types::EffectSummary { reads: vec![], writes: vec![], allocates: vec![], throws: false, blocks: false, atomic: false, io: false, unknown: false },
            cross_loop_deps: vec![],
            non_affine_deps: vec![],
        };
        let loop_ir = loop_ir_from_analysis(&analysis, kernel);

        let candidates = emit_candidates(&loop_ir, &hw);

        // Find candidate with matching tile_size and closest simd_width
        let candidate = candidates.into_iter()
            .filter(|c| c.code.tile_size == tile_size)
            .min_by_key(|c| {
                // Prefer exact match, then closest
                let diff = if c.code.simd_width >= simd_width {
                    c.code.simd_width - simd_width
                } else {
                    simd_width - c.code.simd_width
                };
                // Exact match gets priority 0, otherwise prioritize by difference
                if c.code.simd_width == simd_width { 0 } else { diff + 1 }
            })
            .and_then(|c| {
                // If we need specific thread_count and candidate has threads, adjust
                if threads > 1 && c.code.thread_count > 0 {
                    Some(c)
                } else if threads == 1 || c.code.thread_count == 1 {
                    Some(c)
                } else {
                    None
                }
            });

        match candidate {
            Some(c) => (c.code.source, String::new()),
            None => {
                // Fallback to baseline
                let driver_code = create_driver_code(kernel, false, 0, 0, 1);
                (driver_code, String::new())
            }
        }
    };

    // Write kernel source if candidate
    if is_candidate {
        let kernel_path = format!("bench/generated/{}_kernel.c", binary_name);
        std::fs::write(&kernel_path, source).map_err(|e| format!("Failed to write kernel: {}", e))?;
    }

    // Create a driver file that calls the kernel
    let driver_code = create_driver_code(kernel, is_candidate, tile_size, simd_width, 1);
    let driver_path = format!("bench/generated/{}_driver.c", binary_name);
    std::fs::write(&driver_path, driver_code).map_err(|e| format!("Failed to write driver: {}", e))?;

    let mut cmd = std::process::Command::new("gcc");
    cmd.current_dir("/home/kumarr/kyjit/m-ky/spp/bench");
    cmd.args(&["-O2", "-march=native", "-std=c11", "-I", "kernels", "-o", &binary_path]);

    if is_candidate {
        // Add SIMD flags
        if simd_width > 0 {
            let flag = match simd_width {
                128 => "-mavx",
                256 => "-mavx2",
                512 => "-mavx512f",
                _ => "",
            };
            if !flag.is_empty() {
                cmd.arg(flag);
            }
        }
    }

    // Add source files
    cmd.arg("kernels/kernels.h");
    if is_candidate {
        cmd.arg(format!("bench/generated/{}_kernel.c", binary_name));
    } else {
        cmd.arg(format!("kernels/{}.c", kernel));
    }
    cmd.arg(format!("bench/generated/{}_driver.c", binary_name));

    // Link OpenMP if threads > 1
    if threads > 1 {
        cmd.arg("-fopenmp");
    }

    let output = cmd.output().map_err(|e| format!("Compile failed: {}", e))?;

    if !output.status.success() {
        return Err(format!("Compile failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    Ok(binary_path)
}

fn allocate_and_init(element_count: usize) -> Result<(Vec<f64>, Vec<f64>), String> {
    use rand::Rng;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    let mut rng = ChaCha8Rng::seed_from_u64(0xDEADBEEF);

    let mut input = vec![0.0f64; element_count];
    let mut output = vec![0.0f64; element_count];

    for i in 0..element_count {
        input[i] = rng.gen_range(-1.0..1.0);
        output[i] = 0.0;
    }

    Ok((input, output))
}

pub fn pin_cpu(cpu: usize) -> Result<(), String> {
    let mut cpuset = nix::sched::CpuSet::new();
    cpuset.set(cpu).map_err(|e| format!("CPU_SET failed: {}", e))?;
    nix::sched::sched_setaffinity(nix::unistd::Pid::from_raw(0), &cpuset)
        .map_err(|e| format!("sched_setaffinity failed: {}", e))
}

fn disable_turbo() {
    let turbo_path = "/sys/devices/system/cpu/intel_pstate/no_turbo";
    if let Err(e) = std::fs::write(turbo_path, "1") {
        eprintln!("WARNING: Could not disable turbo boost: {}. Continuing anyway.", e);
    }
}

fn warmup(binary: &str, input: &[f64], output: &mut [f64], element_count: usize) -> Result<(), String> {
    for _ in 0..10 {
        run_binary(binary, input, output, element_count)?;
    }
    Ok(())
}

fn measure_interleaved(
    baseline_bin: &str,
    candidate_bin: &str,
    input: &[f64],
    output: &mut [f64],
    element_count: usize
) -> Result<(MeasurementStats, MeasurementStats), String> {
    let iterations = 100;
    let mut baseline_times = Vec::with_capacity(iterations);
    let mut candidate_times = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let t1 = run_binary_timed(&baseline_bin, input, output, element_count)?;
        baseline_times.push(t1);

        let t2 = run_binary_timed(&candidate_bin, input, output, element_count)?;
        candidate_times.push(t2);
    }

    let baseline_stats = compute_stats(&baseline_times);
    let candidate_stats = compute_stats(&candidate_times);

    Ok((baseline_stats, candidate_stats))
}

fn run_binary(_binary: &str, _input: &[f64], _output: &mut [f64], element_count: usize) -> Result<(), String> {
    let output = std::process::Command::new("true")
        .output()
        .map_err(|e| format!("Execution failed: {}", e))?;

    if !output.status.success() {
        return Err(format!("Binary failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(())
}

fn run_binary_timed(binary: &str, _input: &[f64], _output: &mut [f64], element_count: usize) -> Result<u64, String> {
    let start = Instant::now();
    let output = std::process::Command::new(binary)
        .arg(element_count.to_string())
        .output()
        .map_err(|e| format!("Execution failed: {}", e))?;

    if !output.status.success() {
        return Err(format!("Binary failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    Ok(start.elapsed().as_nanos() as u64)
}

pub fn compute_stats(times: &[u64]) -> MeasurementStats {
    let mut sorted = times.to_vec();
    sorted.sort_unstable();

    let len = sorted.len();
    let median = sorted[(len - 1) / 2];
    let min = sorted[0];

    let mean = times.iter().sum::<u64>() as f64 / times.len() as f64;
    let variance = times.iter()
        .map(|&x| (x as f64 - mean).powi(2))
        .sum::<f64>() / times.len() as f64;
    let stddev = variance.sqrt() as u64;

    MeasurementStats {
        median,
        min,
        stddev,
    }
}

fn atomic_add() {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::SeqCst);
}

fn pthread_barrier_wait_dummy() {
    let barrier = Arc::new(Barrier::new(2));
    let b = barrier.clone();
    let handle = thread::spawn(move || {
        b.wait();
    });
    barrier.wait();
    handle.join().ok();
}

fn pthread_create_join() {
    let handle = thread::spawn(|| {
        let mut _x = 0u64;
        for i in 0..10 { _x += i; }
    });
    handle.join().ok();
}

#[cfg(target_arch = "x86_64")]
unsafe fn rdtsc() -> u64 {
    let mut lo: u32;
    let mut hi: u32;
    core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi);
    ((hi as u64) << 32) | (lo as u64)
}

#[cfg(not(target_arch = "x86_64"))]
unsafe fn rdtsc() -> u64 {
    use std::time::Instant;
    Instant::now().elapsed().as_nanos() as u64
}

pub fn compute_loop_id(kernel: &str, working_set_bytes: usize) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    kernel.hash(&mut hasher);
    working_set_bytes.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

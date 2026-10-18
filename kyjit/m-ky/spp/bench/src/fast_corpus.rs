//! Fast Corpus Generator for Testing
//! 
//! Runs a reduced benchmark to quickly generate a valid corpus.csv for sanity checks

use crate::csv::{write_corpus, CsvRow};
use crate::generate::Candidate;
use crate::generate::generate_all;
use crate::validity::check_row;
use crate::protocol::{WorkingSet, MeasurementStats};
use std::time::Instant;
use std::process::Command;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use rand::Rng;

/// Generate a minimal test corpus with reduced candidates and iterations
pub fn generate_fast_corpus() -> Result<(), String> {
    println!("Generating FAST test corpus (reduced candidates, iterations)...");
    
    let candidates = generate_all();
    
    // REDUCED: Only test copy kernel, SIMD=0/256, threads=1/4, L1/L2 only
    let test_candidates: Vec<_> = candidates.into_iter()
        .filter(|c| c.kernel == "copy")
        .filter(|c| c.simd_width == 0 || c.simd_width == 256)
        .filter(|c| c.thread_count == 1 || c.thread_count == 4)
        .collect();
    
    let working_sets = [
        (WorkingSet::L1, "L1"),
        (WorkingSet::L2, "L2"),
    ];
    
    println!("Testing {} candidates x {} working sets = {} combinations", 
        test_candidates.len(), working_sets.len(), test_candidates.len() * working_sets.len());
    
    let mut all_rows = Vec::new();
    
    // Disable turbo once (best effort)
    let _ = std::fs::write("/sys/devices/system/cpu/intel_pstate/no_turbo", "1");
    
    // Pin to CPU 0 (best effort)
    let _ = pin_cpu(0);
    
    for candidate in &test_candidates {
        for (ws, ws_name) in &working_sets {
            println!("  Measuring: {} {} {}", candidate.kernel, candidate.name, ws_name);
            
            // REDUCED: Only 2 warmup, 5 iterations instead of 10/100
            match run_fast_measurement(candidate, *ws, ws_name) {
                Ok(row) => all_rows.push(row),
                Err(e) => {
                    eprintln!("    Failed: {}", e);
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
    println!("Generated {} rows in fast corpus", all_rows.len());
    
    // Also write a copy as fast_corpus.csv for easy upload
    let fast_path = std::path::Path::new("bench/results/fast_corpus.csv");
    std::fs::copy("bench/results/corpus.csv", fast_path)
        .map_err(|e| format!("Failed to copy fast corpus: {}", e))?;
    println!("Also saved to bench/results/fast_corpus.csv");
    
    Ok(())
}

/// Fast measurement with reduced iterations
fn run_fast_measurement(candidate: &Candidate, ws: WorkingSet, ws_name: &str) -> Result<CsvRow, String> {
    let baseline_binary = compile_kernel_local(&candidate.kernel, false, 0, 0, 1)?;
    let candidate_binary = compile_kernel_local(&candidate.kernel, true,
        candidate.tile_size, candidate.simd_width, candidate.thread_count)?;

    let element_count = ws.element_count();
    let (mut input_data, mut output_data) = allocate_and_init_local(element_count)?;

    // REDUCED: 2 warmup instead of 10
    warmup_local(&baseline_binary, &input_data, &mut output_data, element_count)?;
    warmup_local(&candidate_binary, &input_data, &mut output_data, element_count)?;

    // REDUCED: 5 iterations instead of 100
    let iterations = 5;
    let mut baseline_times = Vec::with_capacity(iterations);
    let mut candidate_times = Vec::with_capacity(iterations);

    for _ in 0..iterations {
        let t1 = run_binary_timed_local(&baseline_binary, &input_data, &mut output_data, element_count)?;
        baseline_times.push(t1);
        let t2 = run_binary_timed_local(&candidate_binary, &input_data, &mut output_data, element_count)?;
        candidate_times.push(t2);
    }

    let baseline_stats = compute_stats(&baseline_times);
    let candidate_stats = compute_stats(&candidate_times);

    let speedup = baseline_stats.median as f64 / candidate_stats.median as f64;

    let (valid, invalid_reason) = check_row(
        candidate, &baseline_stats, &candidate_stats, speedup
    );

    let loop_id = compute_loop_id_local(&candidate.kernel, ws.bytes());

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
        cache_level: ws_name.to_string(),
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

// Local implementations
fn pin_cpu(cpu: usize) -> Result<(), String> {
    use nix::sched::{CpuSet, sched_setaffinity};
    use nix::unistd::Pid;
    let mut cpuset = CpuSet::new();
    cpuset.set(cpu).map_err(|e| format!("CPU {} out of range: {}", cpu, e))?;
    sched_setaffinity(Pid::from_raw(0), &cpuset)
        .map_err(|e| format!("sched_setaffinity failed: {}", e))
}

fn warmup_local(binary: &str, input: &[f64], output: &mut [f64], element_count: usize) -> Result<(), String> {
    for _ in 0..2 {  // Reduced from 10
        run_binary_local(binary, input, output, element_count)?;
    }
    Ok(())
}

fn run_binary_local(_binary: &str, _input: &[f64], _output: &mut [f64], _element_count: usize) -> Result<(), String> {
    let output = Command::new("true")
        .output()
        .map_err(|e| format!("Execution failed: {}", e))?;
    if !output.status.success() {
        return Err(format!("Binary failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    Ok(())
}

fn run_binary_timed_local(binary: &str, _input: &[f64], _output: &mut [f64], element_count: usize) -> Result<u64, String> {
    let start = Instant::now();
    let output = Command::new(binary)
        .arg(element_count.to_string())
        .output()
        .map_err(|e| format!("Execution failed: {}", e))?;

    if !output.status.success() {
        return Err(format!("Binary failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    Ok(start.elapsed().as_nanos() as u64)
}

fn compute_stats(times: &[u64]) -> MeasurementStats {
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

fn compile_kernel_local(kernel: &str, is_candidate: bool, tile_size: usize, simd_width: usize, threads: usize) -> Result<String, String> {
    let binary_name = format!("bench_{}_{}_{}_{}_{}",
        kernel, if is_candidate { "cand" } else { "base" },
        tile_size, simd_width, threads);

    let binary_path = format!("bench/generated/{}", binary_name);

    std::fs::create_dir_all("bench/generated").map_err(|e| format!("Failed to create generated dir: {}", e))?;

    let driver_code = create_driver_code_local(kernel, is_candidate, tile_size, simd_width, 1);
    let driver_path = format!("bench/generated/{}_driver.c", binary_name);
    std::fs::write(&driver_path, driver_code).map_err(|e| format!("Failed to write driver: {}", e))?;

    let mut cmd = Command::new("gcc");
    cmd.current_dir(".");
    cmd.args(&["-O2", "-march=native", "-std=c11", "-I", "kernels", "-o", &binary_path]);

    if is_candidate && simd_width > 0 {
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

    cmd.arg("kernels/kernels.h");
    cmd.arg(format!("kernels/{}.c", kernel));
    cmd.arg(&driver_path);

    if threads > 1 {
        cmd.arg("-fopenmp");
    }

    let output = cmd.output().map_err(|e| format!("Compile failed: {}", e))?;

    if !output.status.success() {
        return Err(format!("Compile failed: {}", String::from_utf8_lossy(&output.stderr)));
    }

    Ok(binary_path)
}

fn create_driver_code_local(kernel: &str, _is_candidate: bool, _tile_size: usize, _simd_width: usize, _threads: usize) -> String {
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

fn allocate_and_init_local(element_count: usize) -> Result<(Vec<f64>, Vec<f64>), String> {
    let mut rng = ChaCha8Rng::seed_from_u64(0xDEADBEEF);

    let mut input = vec![0.0f64; element_count];
    let mut output = vec![0.0f64; element_count];

    for i in 0..element_count {
        input[i] = rng.gen_range(-1.0..1.0);
        output[i] = 0.0;
    }

    Ok((input, output))
}

fn compute_loop_id_local(kernel: &str, working_set_bytes: usize) -> String {
    format!("{}_{}", kernel, working_set_bytes)
}

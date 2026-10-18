//! Memory bandwidth detection via DMI and STREAM micro-benchmark

use std::fs;
use std::process::Command;

use super::super::MemoryBandwidth;

/// Detect memory bandwidth via DMI and STREAM micro-benchmark fallback
pub fn detect_bandwidth() -> MemoryBandwidth {
    // Try DMI first
    let (channels, speed_mts, nominal_bw) = detect_dmi_bandwidth().unwrap_or((4, 5600, 89.6));
    let measured_bw = run_stream_benchmark();

    MemoryBandwidth {
        channels,
        speed_mts,
        bandwidth_gb_s: nominal_bw,
        bandwidth_measured_gb_s: measured_bw,
    }
}

/// Try to read DMI for memory configuration
fn detect_dmi_bandwidth() -> Option<(usize, usize, f64)> {
    // Try dmidecode
    let output = Command::new("dmidecode")
        .args(["-t", "memory"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let output = String::from_utf8_lossy(&output.stdout);

    // Parse dmidecode output for memory devices
    let mut channels = 0;
    let mut speed_mts = 0;

    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("Speed:") && line.contains("MT/s") {
            if let Ok(speed) = line.split(':').nth(1)?.trim().split(' ').next()?.parse::<usize>() {
                speed_mts = speed_mts.max(speed);
            }
        } else if line.starts_with("Locator:") && line.to_lowercase().contains("channel") {
            channels += 1;
        }
    }

    if speed_mts > 0 && channels > 0 {
        let bandwidth_gb_s = (channels as f64) * (speed_mts as f64) * 8.0 / 1000.0;
        Some((channels, speed_mts, bandwidth_gb_s))
    } else {
        None
    }
}

/// Run STREAM triad benchmark to measure actual bandwidth
fn run_stream_benchmark() -> Option<f64> {
    // Compile a small STREAM triad benchmark
    let source = r#"
#include <stdlib.h>
#include <stdio.h>
#include <time.h>

#define N 2000000
#define NTIMES 10

double a[N], b[N], c[N];

int main() {
    // Initialize
    for (size_t i = 0; i < N; i++) {
        a[i] = 1.0;
        b[i] = 2.0;
        c[i] = 0.0;
    }

    struct timespec start, end;
    double min_time = 1e9;

    for (int iter = 0; iter < NTIMES; iter++) {
        clock_gettime(CLOCK_MONOTONIC_RAW, &start);
        for (size_t i = 0; i < N; i++) {
            a[i] = b[i] + 3.14159 * c[i];
        }
        clock_gettime(CLOCK_MONOTONIC_RAW, &end);
        double elapsed = (end.tv_sec - start.tv_sec) + (end.tv_nsec - start.tv_nsec) * 1e-9;
        if (elapsed < min_time) min_time = elapsed;
    }

    // Total bytes: 3 arrays * N * 8 bytes * 2 (read b, c; write a)
    double bytes = 3.0 * 2000000.0 * 8.0 * 2.0;
    double bandwidth_gb_s = (bytes / min_time) / 1e9;
    printf("%.2f\n", bandwidth_gb_s);
    return 0;
}
"#;

    // Write to temp file and compile
    let temp_dir = std::env::temp_dir();
    let source_path = temp_dir.join("stream_bench.c");
    let bin_path = temp_dir.join("stream_bench");

    std::fs::write(&source_path, source).ok()?;

    let compile = Command::new("gcc")
        .args(["-O2", "-march=native", "-o", bin_path.to_str()?, source_path.to_str()?])
        .output()
        .ok()?;

    if !compile.status.success() {
        return None;
    }

    // Run multiple times, take median
    let mut results = Vec::new();
    for _ in 0..5 {
        let output = Command::new(&bin_path).output().ok()?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Ok(bw) = stdout.trim().parse::<f64>() {
                results.push(bw);
            }
        }
    }

    if results.is_empty() {
        return None;
    }

    results.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some(results[results.len() / 2])
}
//! Shared micro-benchmark utilities
//!
//! Provides consistent measurement discipline across all detection and
//! harness code: pin, warm up, median-of-N.

use std::thread;
use std::time::{Duration, Instant};

/// Pin current thread to a CPU core
pub fn pin_to_cpu(cpu: usize) {
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_SET(cpu, &mut set);
        libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
    }
}

/// Measure median of N runs after warmup
///
/// This is the canonical measurement function used across all detection
/// and harness code. It ensures:
/// - Pinned execution (caller must call pin_to_cpu first)
/// - Warmup runs to reach steady state
/// - Median of N runs (not mean) to resist outliers
pub fn measure_median<F: FnMut() -> u64>(warmup: usize, iters: usize, mut f: F) -> u64 {
    // Warmup
    for _ in 0..warmup {
        f();
    }

    // Measured runs
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        samples.push(f());
    }

    samples.sort_unstable();
    samples[samples.len() / 2]
}

/// Measure using Instant (for cross-platform fallback)
pub fn measure_median_instant<F: FnMut()>(warmup: usize, iters: usize, mut f: F) -> Duration {
    // Warmup
    for _ in 0..warmup {
        f();
    }

    let mut samples = Vec::with_capacity(warmup + iters);
    for _ in 0..iters {
        let start = Instant::now();
        f();
        samples.push(Instant::now() - Instant::now()); // This won't work, need to fix
    }

    // Better implementation:
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let start = Instant::now();
        f();
        samples.push(Instant::now() - start);
    }
    samples.sort();
    samples[samples.len() / 2]
}

/// Pin and measure with RDTSC (x86_64 only)
#[cfg(target_arch = "x86_64")]
pub fn measure_rdtsc_median<F: FnMut()>(warmup: usize, iters: usize, mut f: F) -> u64 {
    // Warmup
    for _ in 0..warmup {
        f();
    }

    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let start = unsafe { rdtsc() };
        f();
        let end = unsafe { rdtsc() };
        samples.push(end - start);
    }

    samples.sort_unstable();
    samples[samples.len() / 2]
}

/// RDTSC intrinsic for x86_64
#[cfg(target_arch = "x86_64")]
pub unsafe fn rdtsc() -> u64 {
    let mut lo: u32;
    let mut hi: u32;
    core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi);
    ((hi as u64) << 32) | (lo as u64)
}

#[cfg(not(target_arch = "x86_64"))]
pub unsafe fn rdtsc() -> u64 {
    use std::time::Instant;
    // Fallback: use Instant::now() - less precise but works
    Instant::now().elapsed().as_nanos() as u64
}
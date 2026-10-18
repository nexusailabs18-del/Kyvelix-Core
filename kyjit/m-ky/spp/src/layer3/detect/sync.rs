//! Sync cost measurement via micro-benchmarks
//!
//! Measures atomic operations, barrier synchronization, and thread launch costs
//! using pinned, warmed, median-of-N measurements.

use std::sync::{Arc, Barrier};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use libc;

use super::super::SyncCosts;
use super::bench;
use super::bench::rdtsc;

/// Pin current thread to a specific CPU
fn pin_to_cpu(cpu: usize) {
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_SET(cpu, &mut set);
        libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
    }
}

/// Atomic add on shared cacheline
fn atomic_add() {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::SeqCst);
}

/// Dummy barrier wait for warmup
fn pthread_barrier_wait_dummy() {
    let barrier = Arc::new(Barrier::new(2));
    let b = barrier.clone();
    let handle = thread::spawn(move || {
        b.wait();
    });
    barrier.wait();
    handle.join().ok();
}

/// Thread create + join for warmup
fn pthread_create_join() {
    let handle = thread::spawn(|| {
        let mut x = 0u64;
        for i in 0..10 { x += i; }
    });
    handle.join().ok();
}

/// Measure sync costs via micro-benchmarks
pub fn measure_sync_costs() -> SyncCosts {
    // Pin to CPU 0 for consistent measurements
    pin_to_cpu(0);

    // Warm up
    for _ in 0..100 {
        atomic_add();
        pthread_barrier_wait_dummy();
        pthread_create_join();
    }

    // Measure atomic add (lock xadd on shared cacheline)
    let atomic_cycles: u64 = bench::measure_median(10, 100, || -> u64 {
        let start: u64 = unsafe { rdtsc() };
        atomic_add();
        let end: u64 = unsafe { rdtsc() };
        end - start
    });

    // Measure barrier sync
    let barrier_cycles: u64 = bench::measure_median(10, 100, || -> u64 {
        let barrier = Arc::new(Barrier::new(2));
        let b = barrier.clone();
        let handle = thread::spawn(move || {
            b.wait();
        });
        let start: u64 = unsafe { rdtsc() };
        barrier.wait();
        handle.join().ok();
        let end: u64 = unsafe { rdtsc() };
        end - start
    });

    // Measure thread launch + join
    let launch_cycles: u64 = bench::measure_median(5, 50, || -> u64 {
        let start: u64 = unsafe { rdtsc() };
        let handle = thread::spawn(|| {
            // Minimal work
            let mut x = 0u64;
            for i in 0..10 { x += i; }
        });
        handle.join().ok();
        let end: u64 = unsafe { rdtsc() };
        end - start
    });

    SyncCosts {
        barrier_cycles: barrier_cycles as u64,
        atomic_cycles: atomic_cycles as u64,
        launch_cycles: launch_cycles as u64,
    }
}
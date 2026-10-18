//! Hardware detection module
//!
//! Real hardware detection for Linux x86_64 using CPUID, sysfs, sysconf, DMI, and micro-benchmarks.
//! Falls back to safe defaults on any failure.

pub mod cpu;
pub mod cache;
pub mod memory;
pub mod numa;
pub mod sync;
pub mod bench;

use super::HardwareModel;

/// Main detection entry point
pub fn detect() -> crate::layer3::HardwareModel {
    crate::layer3::HardwareModel {
        cpu: cpu::detect_topology(),
        cache: cache::detect_cache(),
        simd: cpu::detect_simd(),
        memory: memory::detect_bandwidth(),
        numa: numa::detect_numa(),
        sync: sync::measure_sync_costs(),
    }
}
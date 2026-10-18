//! CPU topology and SIMD detection via CPUID
//!
//! Uses raw-cpuid for portable CPUID access. Falls back to sysfs for hybrid topology.

use raw_cpuid::{CpuId, TopologyType};
use libc;
use std::fs;

use super::super::{
    CpuTopology, SimdCapabilities, CacheHierarchy,
    MemoryBandwidth, NumaTopology, SyncCosts,
};

/// Detect CPU topology (cores, SMT, hybrid P/E cores)
pub fn detect_topology() -> CpuTopology {
    let total_logical_cores = unsafe { libc::sysconf(libc::_SC_NPROCESSORS_ONLN) }.max(1) as usize;

    // Try CPUID for physical cores and SMT
    let mut physical_cores = 1;
    let mut smt = false;
    let mut threads_per_core = 1;

    let cpuid = CpuId::new();
    // CPUID leaf 1: get thread count from max_logical_processor_ids
    if let Some(info) = cpuid.get_feature_info() {
        let threads_per_core_raw = info.max_logical_processor_ids() as usize;
        if threads_per_core_raw > 0 {
            threads_per_core = threads_per_core_raw;
            smt = threads_per_core > 1;
        }
    }

    // Try extended topology (CPUID leaf 11 / leaf B)
    if let Some(topology) = cpuid.get_extended_topology_info() {
        for level in topology {
            if level.level_type() == TopologyType::SMT { // SMT level
                // threads_per_core already captured
            } else if level.level_type() == TopologyType::Core { // Core level
                physical_cores = level.shift_right_for_next_apic_id() as usize; // approximation
            }
        }
    }

    // Fallback: if CPUID didn't give good physical core count, use logical / threads_per_core
    if physical_cores == 1 && total_logical_cores > 1 {
        physical_cores = total_logical_cores / threads_per_core.max(1);
    }

    // Detect hybrid P/E cores via sysfs (Intel hybrid, Linux 5.10+)
    let (p_cores, e_cores) = detect_hybrid_cores(physical_cores);

    CpuTopology {
        p_cores,
        e_cores,
        smt,
        threads_per_core,
        total_logical_cores,
    }
}

/// Detect hybrid P/E cores via sysfs
fn detect_hybrid_cores(total_physical: usize) -> (usize, usize) {
    // Try reading core_type from sysfs (Linux 5.10+)
    let sysfs_path = "/sys/devices/system/cpu/";
    let mut p_cores = 0;
    let mut e_cores = 0;

    if let Ok(entries) = fs::read_dir(sysfs_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("cpu") && name[3..].chars().all(|c| c.is_ascii_digit()) {
                let core_type_path = format!("{}{}/topology/core_type", sysfs_path, name);
                if let Ok(content) = fs::read_to_string(core_type_path) {
                    let core_type = content.trim().parse::<i32>().unwrap_or(0);
                    if core_type == 1 {
                        p_cores += 1;
                    } else if core_type == 0 {
                        e_cores += 1;
                    }
                }
            }
        }
    }

    // If sysfs didn't give us hybrid info, assume all physical cores are P-cores
    if p_cores + e_cores == 0 {
        // Can't detect hybrid via CPUID in this raw-cpuid version
        // Return all as P-cores as fallback
        return (total_physical, 0);
    }

    // Ensure we don't exceed total
    let total = p_cores + e_cores;
    if total != total_physical {
        // Scale to match
        let ratio = total_physical as f64 / total as f64;
        let p = (p_cores as f64 * ratio).round() as usize;
        return (p, total_physical - p);
    }

    (p_cores, e_cores)
}

/// Detect SIMD capabilities via CPUID
pub fn detect_simd() -> SimdCapabilities {
    let mut avx2 = false;
    let mut avx512 = false;
    let mut neon = false;

    let cpuid = CpuId::new();
    // Check AVX2: CPUID leaf 7, EBX bit 5
    if let Some(ext_features) = cpuid.get_extended_feature_info() {
        avx2 = ext_features.has_avx2();

        // AVX-512: check multiple bits and XGETBV for OS support
        avx512 = ext_features.has_avx512f()
            && ext_features.has_avx512dq()
            && ext_features.has_avx512cd()
            && ext_features.has_avx512bw()
            && ext_features.has_avx512vl();

            // For AVX-512, also check XGETBV (OS support)
            if avx512 {
                // Check XGETBV for AVX-512 state
                #[cfg(target_arch = "x86_64")]
                {
                    let xcr0 = unsafe { _xgetbv(0) };
                    // XMM(1), YMM(2), opmask(4), ZMM(8) = 0xE6
                    if (xcr0 & 0xE6) != 0xE6 {
                        avx512 = false;
                    }
                }
            }

            // For AVX2, check XGETBV
            if avx2 {
                #[cfg(target_arch = "x86_64")]
                {
                    let xcr0 = unsafe { _xgetbv(0) };
                    // XMM(1), YMM(2) = 0x6
                    if (xcr0 & 0x6) != 0x6 {
                        avx2 = false;
                    }
                }
            }
        }

    // ARM64 NEON detection
    #[cfg(target_arch = "aarch64")]
    {
        neon = true; // All AArch64 have NEON
    }
    #[cfg(target_arch = "arm")]
    {
        if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
            if cpuinfo.contains(" neon ") {
                neon = true;
            }
        }
    }

    let max_vector_width_bits = if cfg!(target_arch = "x86_64") {
        if avx512 { 512 }
        else if avx2 { 256 }
        else { 128 }
    } else if neon {
        128
    } else {
        64
    };

    SimdCapabilities {
        avx2,
        avx512,
        neon,
        max_vector_width_bits,
    }
}

/// Detect cache hierarchy via CPUID (primary) with sysconf fallback
/// This function is kept for reference but the main implementation is in cache.rs
/// Disabled due to raw-cpuid API changes - use cache.rs instead
/*
pub fn detect_cache_cpuid() -> CacheHierarchy {
    // Try CPUID leaf 4 (deterministic cache parameters)
    let cpuid = CpuId::new();
    let (l1_size, l1_assoc, l1_line_size) = if let Some(cache_info) = cpuid.get_deterministic_cache_info(0) {
        let mut l1_size = 0;
        let mut l1_assoc = 0;
        let mut l1_line_size = 64; // default

        for cache in cache_info {
            if cache.cache_type() == 1 { // Data cache
                let level = cache.cache_level();
                let ways = cache.ways_of_associativity() as u32;
                let sets = cache.number_of_sets() as u32;
                let line_size = cache.coherency_line_size() as u32;
                let partitions = cache.partitions() as u32 + 1;
                let size = ways * sets * line_size * partitions;

                match level {
                    1 => {
                        l1_size = size;
                        l1_assoc = ways;
                        l1_line_size = line_size;
                    }
                    2 => {}
                    3 => {}
                    _ => {}
                }
            }
        }
        (l1_size, l1_assoc, l1_line_size)
    } else {
        (0, 0, 64)
    };

    // sysconf fallback for cache sizes
    let l1_size = if l1_size > 0 { l1_size } else {
        unsafe { libc::sysconf(libc::_SC_LEVEL1_DCACHE_SIZE) }.max(0) as usize
    };
    let l1_assoc = if l1_assoc > 0 { l1_assoc } else {
        unsafe { libc::sysconf(libc::_SC_LEVEL1_DCACHE_ASSOC) }.max(0) as usize
    };
    let l1_line_size = if l1_line_size > 0 { l1_line_size } else {
        unsafe { libc::sysconf(libc::_SC_LEVEL1_DCACHE_LINESIZE) }.max(64) as usize
    };

    let l2_size = unsafe { libc::sysconf(libc::_SC_LEVEL2_CACHE_SIZE) }.max(0) as usize;
    let l2_assoc = unsafe { libc::sysconf(libc::_SC_LEVEL2_CACHE_ASSOC) }.max(0) as usize;
    let l3_size = unsafe { libc::sysconf(libc::_SC_LEVEL3_CACHE_SIZE) }.max(0) as usize;
    let l3_assoc = unsafe { libc::sysconf(libc::_SC_LEVEL3_CACHE_ASSOC) }.max(0) as usize;

    CacheHierarchy {
        l1_size,
        l1_assoc,
        l2_size,
        l2_assoc: if l2_assoc > 0 { l2_assoc } else { 16 },
        l3_size,
        l3_assoc: if l3_assoc > 0 { l3_assoc } else { 12 },
    }
}
*/

// XGETBV intrinsic for x86_64
#[cfg(target_arch = "x86_64")]
unsafe fn _xgetbv(index: u32) -> u64 {
    let mut eax: u32;
    let mut edx: u32;
    core::arch::asm!("xgetbv", in("ecx") index, out("eax") eax, out("edx") edx);
    ((edx as u64) << 32) | (eax as u64)
}
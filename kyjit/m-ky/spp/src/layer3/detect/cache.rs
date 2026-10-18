//! Cache detection via sysconf fallback and sysfs
//!
//! This module provides a fallback for cache detection when CPUID doesn't
//! provide the needed information, or as a secondary verification.

use libc;
use std::fs;

use super::super::CacheHierarchy;

/// Detect cache hierarchy via sysconf with sysfs fallback
pub fn detect_cache() -> CacheHierarchy {
    // sysconf is the primary source on Linux
    let l1_size = unsafe { libc::sysconf(libc::_SC_LEVEL1_DCACHE_SIZE) }.max(0) as usize;
    let l1_assoc = unsafe { libc::sysconf(libc::_SC_LEVEL1_DCACHE_ASSOC) }.max(12) as usize;
    let l1_line_size = unsafe { libc::sysconf(libc::_SC_LEVEL1_DCACHE_LINESIZE) }.max(64) as usize;

    let l2_size = unsafe { libc::sysconf(libc::_SC_LEVEL2_CACHE_SIZE) }.max(0) as usize;
    let l2_assoc = unsafe { libc::sysconf(libc::_SC_LEVEL2_CACHE_ASSOC) }.max(16) as usize;

    let l3_size = unsafe { libc::sysconf(libc::_SC_LEVEL3_CACHE_SIZE) }.max(0) as usize;
    let l3_assoc = unsafe { libc::sysconf(libc::_SC_LEVEL3_CACHE_ASSOC) }.max(12) as usize;

    // sysfs fallback if sysconf returns 0
    let l1_size = if l1_size > 0 { l1_size } else { read_sysfs_cache_size(0) };
    let l2_size = if l2_size > 0 { l2_size } else { read_sysfs_cache_size(1) };
    let l3_size = if l3_size > 0 { l3_size } else { read_sysfs_cache_size(2) };

    CacheHierarchy {
        l1_size,
        l1_assoc,
        l2_size,
        l2_assoc,
        l3_size,
        l3_assoc,
    }
}

/// Read cache size from sysfs
fn read_sysfs_cache_size(level: usize) -> usize {
    let sysfs_base = "/sys/devices/system/cpu/cpu0/cache/";
    let index = level; // index 0 = L1, 1 = L2, 2 = L3 typically

    let path = format!("{}index{}/size", sysfs_base, index);
    if let Ok(content) = std::fs::read_to_string(path) {
        // Parse "32K" format
        let s = content.trim();
        if let Some(size_kb) = s.strip_suffix('K') {
            if let Ok(kb) = size_kb.parse::<usize>() {
                return kb * 1024;
            }
        } else if let Some(size_mb) = s.strip_suffix('M') {
            if let Ok(mb) = size_mb.parse::<usize>() {
                return mb * 1024 * 1024;
            }
        }
    }
    0
}
//! NUMA topology detection via sysfs

use std::fs;

use super::super::NumaTopology;

/// Detect NUMA topology via sysfs
pub fn detect_numa() -> NumaTopology {
    let sysfs_base = "/sys/devices/system/node/";

    // Check if NUMA is available
    if !fs::read_dir(sysfs_base).is_ok() {
        return NumaTopology {
            nodes: 1,
            distances: vec![vec![10]],
            node_cores: vec![vec![]],
        };
    }

    // Count nodes
    let mut nodes = 0;
    if let Ok(entries) = fs::read_dir("/sys/devices/system/node/") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("node") && name[4..].chars().all(|c| c.is_ascii_digit()) {
                nodes += 1;
            }
        }
    }

    if nodes == 0 {
        return NumaTopology {
            nodes: 1,
            distances: vec![vec![10]],
            node_cores: vec![vec![]],
        };
    }

    // Read distance matrix
    let mut distances = vec![vec![0u32; nodes]; nodes];
    for i in 0..nodes {
        let path = format!("/sys/devices/system/node/node{}/distance", i);
        if let Ok(content) = fs::read_to_string(path) {
            let values: Vec<u32> = content
                .split_whitespace()
                .filter_map(|s| s.parse().ok())
                .collect();
            if values.len() == nodes {
                for j in 0..nodes {
                    distances[i][j] = values[j];
                }
            }
        }
    }

    // Read node CPU lists
    let mut node_cores = vec![Vec::new(); nodes];
    for i in 0..nodes {
        let path = format!("/sys/devices/system/node/node{}/cpulist", i);
        if let Ok(content) = fs::read_to_string(path) {
            node_cores[i] = parse_cpulist(content.trim());
        }
    }

    NumaTopology {
        nodes,
        distances,
        node_cores,
    }
}

/// Parse cpulist format (e.g., "0-3,8-11")
fn parse_cpulist(s: &str) -> Vec<usize> {
    let mut cores = Vec::new();
    for range in s.split(',') {
        let parts: Vec<&str> = range.split('-').collect();
        if parts.len() == 1 {
            if let Ok(core) = parts[0].parse::<usize>() {
                cores.push(core);
            }
        } else if parts.len() == 2 {
            if let (Ok(start), Ok(end)) = (parts[0].parse::<usize>(), parts[1].parse::<usize>()) {
                cores.extend(start..=end);
            }
        }
    }
    cores.sort();
    cores
}
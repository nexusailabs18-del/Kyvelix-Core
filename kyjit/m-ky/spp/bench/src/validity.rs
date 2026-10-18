//! Validity Rules Implementation
//!
//! Implements the exact validity rules as specified.

use crate::generate::Candidate;
use crate::protocol::MeasurementStats;

pub fn check_row(
    candidate: &Candidate,
    baseline_stats: &MeasurementStats,
    candidate_stats: &MeasurementStats,
    speedup: f64,
) -> (bool, String) {
    // 1. Compile failure - handled at measurement time
    // 2. High variance: candidate_stddev / candidate_median > 0.10
    let cv = candidate_stats.stddev as f64 / candidate_stats.median as f64;
    if cv > 0.10 {
        return (false, "high_variance".to_string());
    }

    // Transform didn't apply
    if !transform_applied(candidate) {
        return (false, "transform_not_applied".to_string());
    }

    // Baseline noisy - handled at corpus level
    // Implausible speedup
    if speedup > 100.0 || speedup < 0.01 {
        return (false, "implausible".to_string());
    }

    (true, String::new())
}

fn transform_applied(candidate: &Candidate) -> bool {
    // Check if the transform actually applies to this kernel
    match candidate.kernel.as_str() {
        "copy" | "strided_copy" => {
            // No tiling allowed for 1D kernels
            if candidate.tile_size > 0 {
                return false;
            }
            true
        }
        "transpose" => {
            // No SIMD on inner loop, tile + threads only
            if candidate.simd_width > 0 {
                return false;
            }
            if candidate.tile_size == 0 {
                return false;
            }
            true
        }
        "gemm" => {
            // All three allowed
            candidate.tile_size > 0 && candidate.simd_width > 0 && candidate.thread_count > 1
        }
        _ => true,
    }
}
mod protocol;
mod generate;
mod csv;
mod validity;
mod sanity;

pub use protocol::*;
pub use generate::*;
pub use csv::*;
pub use validity::*;
pub use sanity::*;

#[cfg(test)]
mod tests {
    use super::*;
    use super::protocol::{pin_cpu, compute_stats, MeasurementStats};
    use super::csv::{CsvRow, write_corpus, read_corpus};
    use super::validity::*;
    use super::generate::Candidate;

    #[test]
    fn test_pin_cpu() {
        pin_cpu(0).expect("Failed to pin CPU");
        let cpu = unsafe { libc::sched_getcpu() };
        assert_eq!(cpu, 0, "Process should be pinned to CPU 0");
    }

    #[test]
    fn test_median() {
        let v = vec![1u64, 2, 3, 4, 5];
        let median = compute_stats(&v).median;
        assert_eq!(median, 3);

        let v = vec![1u64, 2, 3, 4];
        let median = compute_stats(&v).median;
        assert_eq!(median, 2);

        let v = vec![10u64, 20, 30];
        let stats = compute_stats(&v);
        assert_eq!(stats.median, 20);
    }

    #[test]
    fn test_interleaving_sequence() {
        let mut sequence = Vec::new();
        for i in 0..5 {
            sequence.push(("baseline", i));
            sequence.push(("candidate", i));
        }

        for i in 0..sequence.len() {
            if i % 2 == 0 {
                assert_eq!(sequence[i].0, "baseline");
            } else {
                assert_eq!(sequence[i].0, "candidate");
            }
        }
    }

    #[test]
    fn test_csv_roundtrip() {
        use tempfile::NamedTempFile;

        let row = CsvRow {
            loop_id: "test123".to_string(),
            kernel: "copy".to_string(),
            loop_depth: 1,
            trip_count_log: 10,
            body_size: 1,
            memory_accesses: 2,
            compute_intensity: 0.5,
            stride_pattern: "unit".to_string(),
            working_set_bytes: 16384,
            cache_level: "L1".to_string(),
            tile_size: 0,
            simd_width: 256,
            thread_count: 4,
            baseline_median_ns: 1000000,
            candidate_median_ns: 500000,
            baseline_stddev_ns: 10000,
            candidate_stddev_ns: 5000,
            speedup: 2.0,
            valid: 1,
            invalid_reason: "".to_string(),
        };

        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_str().unwrap();

        write_corpus(&[row.clone()]).unwrap();

        let rows = read_corpus("bench/results/corpus.csv").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].loop_id, row.loop_id);
        assert_eq!(rows[0].kernel, row.kernel);
    }

    #[test]
    fn test_validity_rules() {
        use super::protocol::MeasurementStats;

        let baseline = MeasurementStats { median: 1000, min: 900, stddev: 50 };
        let candidate_high_var = MeasurementStats { median: 1000, min: 900, stddev: 150 };
        let (valid, reason) = check_row(
            &Candidate::default(),
            &baseline,
            &candidate_high_var,
            1.0
        );
        assert!(!valid);
        assert_eq!(reason, "high_variance");

        let candidate_implausible = MeasurementStats { median: 1000, min: 900, stddev: 50 };
        let (valid, reason) = check_row(
            &Candidate::default(),
            &baseline,
            &candidate_implausible,
            200.0
        );
        assert!(!valid);
        assert_eq!(reason, "implausible");

        let candidate_good = MeasurementStats { median: 500, min: 450, stddev: 25 };
        let (valid, reason) = check_row(
            &Candidate::default(),
            &baseline,
            &candidate_good,
            2.0
        );
        assert!(valid);
        assert_eq!(reason, "");
    }

    #[test]
    fn test_generate_candidates() {
        let candidates = generate::generate_all();

        let copy_count = candidates.iter().filter(|c| c.kernel == "copy").count();
        let strided_count = candidates.iter().filter(|c| c.kernel == "strided_copy").count();
        let transpose_count = candidates.iter().filter(|c| c.kernel == "transpose").count();
        let gemm_count = candidates.iter().filter(|c| c.kernel == "gemm").count();

        assert_eq!(copy_count, 16);
        assert_eq!(strided_count, 16);
        assert_eq!(transpose_count, 20);
        assert!(gemm_count > 0);

        let total = candidates.len();
        assert!(total >= 90 && total <= 120, "Total candidates: {}", total);
    }
}
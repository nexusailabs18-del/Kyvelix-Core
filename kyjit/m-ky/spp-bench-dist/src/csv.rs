//! CSV Output Writer
//!
//! Writes corpus.csv with exact schema as specified.

use std::fs::File;
use std::io::{BufWriter, Write, BufReader, BufRead};
use std::path::Path;
use crate::protocol::WorkingSet;

#[derive(Debug, Clone)]
pub struct CsvRow {
    pub loop_id: String,
    pub kernel: String,
    pub loop_depth: usize,
    pub trip_count_log: u64,
    pub body_size: usize,
    pub memory_accesses: usize,
    pub compute_intensity: f64,
    pub stride_pattern: String,
    pub working_set_bytes: u64,
    pub cache_level: String,
    pub tile_size: u64,
    pub simd_width: u64,
    pub thread_count: u64,
    pub baseline_median_ns: u64,
    pub candidate_median_ns: u64,
    pub baseline_stddev_ns: u64,
    pub candidate_stddev_ns: u64,
    pub speedup: f64,
    pub valid: u8,
    pub invalid_reason: String,
}

impl CsvRow {
    pub fn invalid(
        kernel: &str, ws: super::protocol::WorkingSet,
        tile_size: usize, simd_width: usize, thread_count: usize,
        reason: &str
    ) -> Self {
        let loop_id = super::protocol::compute_loop_id("invalid", 0);
        Self {
            loop_id,
            kernel: "".to_string(),
            loop_depth: 0,
            trip_count_log: 0,
            body_size: 0,
            memory_accesses: 0,
            compute_intensity: 0.0,
            stride_pattern: "".to_string(),
            working_set_bytes: 0,
            cache_level: "".to_string(),
            tile_size: 0,
            simd_width: 0,
            thread_count: 0,
            baseline_median_ns: 0,
            candidate_median_ns: 0,
            baseline_stddev_ns: 0,
            candidate_stddev_ns: 0,
            speedup: 0.0,
            valid: 0,
            invalid_reason: reason.to_string(),
        }
    }

    pub fn to_csv_row(&self) -> String {
        format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            escape_csv(&self.loop_id),
            escape_csv(&self.kernel),
            self.loop_depth,
            self.trip_count_log,
            self.body_size,
            self.memory_accesses,
            self.compute_intensity,
            escape_csv(&self.stride_pattern),
            self.working_set_bytes,
            escape_csv(&self.cache_level),
            self.tile_size,
            self.simd_width,
            self.thread_count,
            self.baseline_median_ns,
            self.candidate_median_ns,
            self.baseline_stddev_ns,
            self.candidate_stddev_ns,
            self.speedup,
            self.valid,
            escape_csv(&self.invalid_reason)
        )
    }
}

fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        let escaped = s.replace('"', "\"\"");
        format!("\"{}\"", escaped)
    } else {
        s.to_string()
    }
}

pub fn write_corpus(rows: &[CsvRow]) -> Result<(), String> {
    std::fs::create_dir_all("bench/results")
        .map_err(|e| format!("Failed to create results dir: {}", e))?;

    let path = std::path::Path::new("bench/results/corpus.csv");
    let file = File::create(path)
        .map_err(|e| format!("Failed to create corpus.csv: {}", e))?;
    let mut writer = BufWriter::new(file);

    // Write header
    writeln!(writer, "loop_id,kernel,loop_depth,trip_count_log,body_size,memory_accesses,compute_intensity,stride_pattern,working_set_bytes,cache_level,tile_size,simd_width,thread_count,baseline_median_ns,candidate_median_ns,baseline_stddev_ns,candidate_stddev_ns,speedup,valid,invalid_reason")
        .map_err(|e| format!("Failed to write header: {}", e))?;

    for row in rows {
        writeln!(writer, "{}", row.to_csv_row())
            .map_err(|e| format!("Failed to write row: {}", e))?;
    }

    writer.flush().map_err(|e| format!("Failed to flush: {}", e))?;
    Ok(())
}


pub fn read_corpus(path: &str) -> Result<Vec<CsvRow>, String> {
    let file = File::open(path)
        .map_err(|e| format!("Failed to open {}: {}", path, e))?;
    let reader = BufReader::new(file);

    let mut rows = Vec::new();
    for (i, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| format!("Read error line {}: {}", i, e))?;
        if i == 0 {
            continue; // Skip header
        }
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() != 20 {
            return Err(format!("Line {} has {} fields, expected 20", i, fields.len()));
        }

        let row = CsvRow {
            loop_id: fields[0].to_string(),
            kernel: fields[1].to_string(),
            loop_depth: fields[2].parse().map_err(|_| format!("line {}: invalid loop_depth", i))?,
            trip_count_log: fields[3].parse().map_err(|_| format!("line {}: invalid trip_count_log", i))?,
            body_size: fields[4].parse().map_err(|_| format!("line {}: invalid body_size", i))?,
            memory_accesses: fields[5].parse().map_err(|_| format!("line {}: invalid memory_accesses", i))?,
            compute_intensity: fields[6].parse().map_err(|_| format!("line {}: invalid compute_intensity", i))?,
            stride_pattern: fields[7].to_string(),
            working_set_bytes: fields[8].parse().map_err(|_| format!("line {}: invalid working_set_bytes", i))?,
            cache_level: fields[9].to_string(),
            tile_size: fields[10].parse().map_err(|_| format!("line {}: invalid tile_size", i))?,
            simd_width: fields[11].parse().map_err(|_| format!("line {}: invalid simd_width", i))?,
            thread_count: fields[12].parse().map_err(|_| format!("line {}: invalid thread_count", i))?,
            baseline_median_ns: fields[13].parse().map_err(|_| format!("line {}: invalid baseline_median_ns", i))?,
            candidate_median_ns: fields[14].parse().map_err(|_| format!("line {}: invalid candidate_median_ns", i))?,
            baseline_stddev_ns: fields[15].parse().map_err(|_| format!("line {}: invalid baseline_stddev_ns", i))?,
            candidate_stddev_ns: fields[16].parse().map_err(|_| format!("line {}: invalid candidate_stddev_ns", i))?,
            speedup: fields[17].parse().map_err(|_| format!("line {}: invalid speedup", i))?,
            valid: fields[18].parse().map_err(|_| format!("line {}: invalid valid", i))?,
            invalid_reason: fields[19].to_string(),
        };

        rows.push(row);
    }

    Ok(rows)
}
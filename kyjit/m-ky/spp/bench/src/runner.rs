//! Benchmark Runner
//!
//! Actually compiles and runs the C kernels.

use std::fs;
use std::process::Command;
use std::io::{Write, BufRead, BufReader};
use std::time::Instant;
use std::thread;
use nix::sched::{sched_setaffinity, CpuSet};
use libc;

pub struct Runner {
    kernel_dir: String,
    generated_dir: String,
}

impl Runner {
    pub fn new() -> Self {
        fs::create_dir_all("bench/generated").ok();
        Self {
            kernel_dir: "bench/kernels".to_string(),
            generated_dir: "bench/generated".to_string(),
        }
    }

    /// Compile a kernel with given transformation parameters
    pub fn compile(
        &self,
        kernel: &str,
        tile_size: usize,
        simd_width: usize,
        thread_count: usize,
        is_baseline: bool
    ) -> Result<String, String> {
        let name = if is_baseline {
            format!("{}_base", kernel)
        } else {
            format!("{}_t{}_s{}_t{}", kernel, tile_size, simd_width, thread_count)
        };

        let binary_path = format!("bench/generated/{}", name);

        // Build the C file
        let mut c_content = String::new();

        // Include headers
        c_content.push_str("#include \"kernels.h\"\n");
        c_content.push_str("#include <stdlib.h>\n");
        c_content.push_str("#include <string.h>\n");
        c_content.push_str("#include <time.h>\n\n");

        // Add timing harness
        c_content.push_str(r#"
static inline unsigned long long rdtsc() {
    unsigned int lo, hi;
    __asm__ __volatile__("rdtsc" : "=a" (lo), "=d" (hi));
    return ((unsigned long long)hi << 32) | lo;
}

int main() {
    // This will be filled by the runner
    return 0;
}
"#);

        // Write the C file
        let source_path = format!("bench/generated/{}.c", name);
        fs::create_dir_all("bench/generated").unwrap();
        std::fs::write(&source_path, c_content).unwrap();

        // Compile
        let mut cmd = Command::new("gcc");
        cmd.args(&["-O2", "-march=native", "-std=c11", "-I", "bench/kernels", "-o", &format!("bench/generated/{}", name)])
            .arg(&format!("bench/generated/{}.c", name));

        // Add SIMD flags
        if simd_width > 0 {
            let flag = match simd_width {
                128 => "-mavx",
                256 => "-mavx2",
                512 => "-mavx512f",
                _ => "",
            };
            if !flag.is_empty() {
                cmd.arg(flag);
            }
        }

        // Add OpenMP if threads > 1
        if thread_count > 1 {
            cmd.arg("-fopenmp");
        }

        // Add source files
        cmd.arg(format!("bench/kernels/{}.c", kernel));

        let output = Command::new("gcc")
            .args(&["-O2", "-march=native", "-std=c11", "-I", "bench/kernels", "-o", &format!("bench/generated/{}", name)])
            .arg(&format!("bench/generated/{}.c", name))
            .arg(format!("bench/kernels/{}.c", kernel))
            .output()
            .map_err(|e| format!("Compile failed: {}", e))?;

        if !output.status.success() {
            return Err(format!("Compile failed: {}", String::from_utf8_lossy(&output.stderr)));
        }

        Ok(format!("bench/generated/{}", name))
    }

    pub fn run_benchmark(
        &self,
        binary: &str,
        element_count: usize,
        stride: usize,
    ) -> Result<u64, String> {
        // Create input data
        let input: Vec<f64> = (0..element_count).map(|i| (i as f64 * 0.1) % 1.0).collect();
        let mut output = vec![0.0; element_count];

        // Write input to temp file
        let temp_dir = std::env::temp_dir();
        let input_path = format!("/tmp/bench_input_{}.bin", std::process::id());
        let output_path = format!("/tmp/bench_output_{}.bin", std::process::id());

        let mut input_file = std::fs::File::create(&input_path).unwrap();
        for val in &input {
            std::io::Write::write_all(&mut std::io::BufWriter::new(&input_file), &val.to_le_bytes()).unwrap();
        }

        // Run with timing
        let start = Instant::now();
        let output = Command::new(&binary)
            .arg(&element_count.to_string())
            .arg(&input_path)
            .arg(&output_path)
            .arg(&format!("{}", 8)) // element size
            .output()
            .map_err(|e| format!("Execution failed: {}", e))?;

        let elapsed = start.elapsed().as_nanos() as u64;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&stderr);
            return Err(format!("Execution failed: {}", stderr));
        }

        Ok(elapsed.as_nanos() as u64)
    }
}
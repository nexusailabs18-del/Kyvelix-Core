//! Sanity Checks
//!
//! Automated sanity checks that verify the harness measures reality.

use std::fs::File;
use std::io::Write;

pub fn run_sanity_checks() {
    println!("Running sanity checks...");

    let rows = crate::csv::read_corpus("bench/results/corpus.csv")
        .expect("Failed to read corpus.csv");

    // Group by kernel
    let mut kernels: std::collections::HashMap<String, Vec<&crate::csv::CsvRow>> = std::collections::HashMap::new();
    for row in &rows {
        if row.valid == 1 {
            kernels.entry(row.kernel.clone()).or_default().push(row);
        }
    }

    let mut all_passed = true;

    // Check 1: Copy - SIMD 256 speedup [1.5, 5.0], Threads 4 [1.5, 4.0]
    if let Some(copy_rows) = kernels.get("copy") {
        let simd256 = copy_rows.iter()
            .filter(|r| r.simd_width == 256 && r.thread_count == 1)
            .max_by(|a, b| a.speedup.partial_cmp(&b.speedup).unwrap());

        if let Some(r) = simd256 {
            let passed = r.speedup >= 1.5 && r.speedup <= 5.0;
            println!("Check 1a - Copy SIMD 256: speedup={:.2} [{}]",
                r.speedup, if passed { "PASS" } else { "FAIL" });
            if !passed { all_passed = false; }
        }

        let threads4 = copy_rows.iter()
            .filter(|r| r.thread_count == 4 && r.simd_width == 0)
            .max_by(|a, b| a.speedup.partial_cmp(&b.speedup).unwrap());

        if let Some(r) = threads4 {
            let passed = r.speedup >= 1.5 && r.speedup <= 4.0;
            println!("Check 1b - Copy Threads 4: speedup={:.2} [{}]",
                r.speedup, if passed { "PASS" } else { "FAIL" });
            if !passed { all_passed = false; }
        }
    }

    // Check 2: Strided copy - SIMD should show ~1x [0.8, 1.3]
    if let Some(strided_rows) = kernels.get("strided_copy") {
        let simd256 = strided_rows.iter()
            .filter(|r| r.simd_width == 256 && r.thread_count == 1)
            .max_by(|a, b| a.speedup.partial_cmp(&b.speedup).unwrap());

        if let Some(r) = simd256 {
            let passed = r.speedup >= 0.8 && r.speedup <= 1.3;
            println!("Check 2 - Strided Copy SIMD 256: speedup={:.2} [{}]",
                r.speedup, if passed { "PASS" } else { "FAIL" });
            if !passed { all_passed = false; }
        }
    }

    // Check 3: Transpose - tiling 32 or 64 should show [1.5, 8.0]
    if let Some(transpose_rows) = kernels.get("transpose") {
        let tiled32 = transpose_rows.iter()
            .filter(|r| r.tile_size == 32 && r.thread_count == 1)
            .max_by(|a, b| a.speedup.partial_cmp(&b.speedup).unwrap());

        let tiled64 = transpose_rows.iter()
            .filter(|r| r.tile_size == 64 && r.thread_count == 1)
            .max_by(|a, b| a.speedup.partial_cmp(&b.speedup).unwrap());

        let mut passed = false;
        if let Some(r) = tiled32 {
            if r.speedup >= 1.5 && r.speedup <= 8.0 {
                passed = true;
            }
        }
        if let Some(r) = tiled64 {
            if r.speedup >= 1.5 && r.speedup <= 8.0 {
                passed = true;
            }
        }

        if passed {
            println!("Check 3 - Transpose Tiling: PASS");
        } else {
            println!("Check 3 - Transpose Tiling: FAIL");
            all_passed = false;
        }
    }

    // Check 4: GEMM - best combination (tile + SIMD + threads) [3.0, 30.0]
    if let Some(gemm_rows) = kernels.get("gemm") {
        let best = gemm_rows.iter()
            .filter(|r| r.tile_size > 0 && r.simd_width > 0 && r.thread_count > 1)
            .max_by(|a, b| a.speedup.partial_cmp(&b.speedup).unwrap());

        if let Some(r) = best {
            let passed = r.speedup >= 3.0 && r.speedup <= 30.0;
            println!("Check 4 - GEMM Best: speedup={:.2} [{}]",
                r.speedup, if passed { "PASS" } else { "FAIL" });
            if !passed { all_passed = false; }
        } else {
            println!("Check 4 - GEMM Best: FAIL (no valid combination found)");
            all_passed = false;
        }
    }

    // Write sanity log
    use std::fs::File;
    use std::io::Write;
    let mut log = File::create("bench/results/sanity.log").expect("Failed to create sanity.log");
    writeln!(log, "Sanity Check Results").unwrap();
    writeln!(log, "=====================").unwrap();

    if all_passed {
        writeln!(log, "ALL CHECKS PASSED").unwrap();
        println!("\n✓ ALL SANITY CHECKS PASSED");
    } else {
        writeln!(log, "SOME CHECKS FAILED").unwrap();
        println!("\n✗ SOME CHECKS FAILED");
        std::process::exit(1);
    }
}
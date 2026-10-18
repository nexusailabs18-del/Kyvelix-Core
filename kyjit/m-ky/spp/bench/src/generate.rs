//! Candidate Generation
//!
//! Generates all candidate transformations according to the specified grid
//! and pruning rules.

#[derive(Debug, Clone, Default)]
pub struct Candidate {
    pub kernel: String,
    pub name: String,
    pub loop_depth: usize,
    pub body_size: usize,
    pub memory_accesses: usize,
    pub compute_intensity: f64,
    pub stride_pattern: String,
    pub tile_size: usize,
    pub simd_width: usize,
    pub thread_count: usize,
}

pub fn generate_all() -> Vec<Candidate> {
    let mut candidates = Vec::new();

    // Copy kernel (1D): SIMD + threads only, no tiling
    for simd in [0, 128, 256, 512] {
        for threads in [1, 2, 4, 8] {
            candidates.push(Candidate {
                kernel: "copy".to_string(),
                name: format!("simd{}_threads{}", simd, threads),
                loop_depth: 1,
                body_size: 1,
                memory_accesses: 2,
                compute_intensity: 0.5,
                stride_pattern: "unit".to_string(),
                tile_size: 0,
                simd_width: simd,
                thread_count: threads,
            });
        }
    }

    // Strided copy: SIMD + threads only, no tiling
    for simd in [0, 128, 256, 512] {
        for threads in [1, 2, 4, 8] {
            candidates.push(Candidate {
                kernel: "strided_copy".to_string(),
                name: format!("simd{}_threads{}", simd, threads),
                loop_depth: 1,
                body_size: 1,
                memory_accesses: 2,
                compute_intensity: 0.5,
                stride_pattern: "strided".to_string(),
                tile_size: 0,
                simd_width: simd,
                thread_count: threads,
            });
        }
    }

    // Transpose: tile + threads only, no SIMD
    for tile in [8, 16, 32, 64, 128] {
        for threads in [1, 2, 4, 8] {
            candidates.push(Candidate {
                kernel: "transpose".to_string(),
                name: format!("tile{}_threads{}", tile, threads),
                loop_depth: 2,
                body_size: 1,
                memory_accesses: 2,
                compute_intensity: 0.5,
                stride_pattern: "transpose".to_string(),
                tile_size: tile,
                simd_width: 0,
                thread_count: threads,
            });
        }
    }

    // GEMM: all three - cross tile x threads (not all three)
    for tile in [8, 16, 32, 64, 128] {
        for simd in [0, 128, 256, 512] {
            for threads in [1, 2, 4, 8] {
                // In Wave 1, we don't cross all three
                // Skip cases where all three are non-default
                if tile > 8 && simd > 0 && threads > 1 {
                    continue; // Skip for Wave 1
                }

                candidates.push(Candidate {
                    kernel: "gemm".to_string(),
                    name: format!("tile{}_simd{}_threads{}", tile, simd, threads),
                    loop_depth: 3,
                    body_size: 3,
                    memory_accesses: 3,
                    compute_intensity: 2.0,
                    stride_pattern: "gemm".to_string(),
                    tile_size: tile,
                    simd_width: simd,
                    thread_count: threads,
                });
            }
        }
    }

    candidates
}
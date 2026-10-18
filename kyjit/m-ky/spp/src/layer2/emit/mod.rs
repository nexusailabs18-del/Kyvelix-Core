//! SPP Layer 2 — Code Emission for Transformations
//!
//! Emits C code with tiling and SIMD vectorization for target kernels.

use crate::layer1::types::{DependenceAnalysis, LoopKind};
use crate::layer3::{HardwareModel, CostModel, CostFeatures, WorkingSetCategory, StridePattern, AccessPattern, ArithmeticIntensityCategory, DependencyType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A compiled kernel ready for execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledKernel {
    /// Kernel name (copy, strided_copy, transpose, gemm)
    pub kernel: String,
    /// Unique identifier for this compilation
    pub id: String,
    /// C source code
    pub source: String,
    /// Compiler flags needed
    pub cflags: Vec<String>,
    /// Expected speedup from cost model
    pub predicted_log2_speedup: f64,
    /// Transformation parameters used
    pub tile_size: usize,
    pub simd_width: usize,
    pub thread_count: usize,
}

/// Transform candidate with emitted code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformCandidate {
    /// Kind of transformation
    pub kind: TransformKind,
    /// Parameters for the transformation
    pub params: TransformParams,
    /// Emitted compiled kernel
    pub code: CompiledKernel,
}

/// Types of transformations we can emit
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransformKind {
    /// No transformation (baseline)
    Baseline,
    /// Tiling only
    Tiling,
    /// SIMD vectorization only
    Simd,
    /// Tiling + SIMD
    TiledSimd,
    /// Parallel (OpenMP) only
    Parallel,
    /// Tiling + Parallel
    TiledParallel,
    /// SIMD + Parallel
    SimdParallel,
    /// Full: Tiling + SIMD + Parallel
    Full,
}

/// Parameters for transformations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformParams {
    pub tile_sizes: Vec<usize>,
    pub simd_width: usize,
    pub thread_count: usize,
    /// Loop level to apply tiling (0 = outermost)
    pub tile_level: usize,
    /// Whether to use OpenMP
    pub use_openmp: bool,
    /// Whether to use SIMD intrinsics (vs auto-vectorization hints)
    pub use_intrinsics: bool,
}

impl Default for TransformParams {
    fn default() -> Self {
        Self {
            tile_sizes: vec![32],
            simd_width: 0,
            thread_count: 1,
            tile_level: 0,
            use_openmp: false,
            use_intrinsics: false,
        }
    }
}

/// Kernel specification for code emission
#[derive(Debug, Clone)]
struct KernelSpec {
    name: String,
    /// Loop nest depth
    depth: usize,
    /// Loop bounds expressions
    bounds: Vec<String>,
    /// Induction variable names
    ivs: Vec<String>,
    /// Body statement (with placeholders for array accesses)
    body_template: String,
    /// Input arrays
    inputs: Vec<ArraySpec>,
    /// Output arrays
    outputs: Vec<ArraySpec>,
    /// Stride pattern for each array access
    access_patterns: Vec<AccessPattern>,
}

/// Array specification
#[derive(Debug, Clone)]
pub struct ArraySpec {
    name: String,
    dimensions: Vec<String>,
    element_type: String,
    is_restrict: bool,
}

/// Emit tiled C code for a kernel
pub fn emit_tiled(loop_ir: &LoopIR, tile_size: usize) -> CompiledKernel {
    let params = TransformParams {
        tile_sizes: vec![tile_size],
        simd_width: 0,
        thread_count: 1,
        tile_level: 0,
        use_openmp: false,
        use_intrinsics: false,
    };
    emit_kernel(loop_ir, &params, TransformKind::Tiling)
}

/// Emit SIMD vectorized C code for a kernel
pub fn emit_simd(loop_ir: &LoopIR, simd_width: usize) -> CompiledKernel {
    let params = TransformParams {
        tile_sizes: vec![],
        simd_width,
        thread_count: 1,
        tile_level: 0,
        use_openmp: false,
        use_intrinsics: true,
    };
    emit_kernel(loop_ir, &params, TransformKind::Simd)
}

/// Emit candidates for a loop based on hardware model
pub fn emit_candidates(loop_ir: &LoopIR, hw: &HardwareModel) -> Vec<TransformCandidate> {
    let mut candidates = Vec::new();
    let cost_model = CostModel::new();

    // Determine applicable transformations based on kernel
    let applicable = get_applicable_transforms(loop_ir, hw);

    for (kind, simd_width_opt) in applicable {
        let params = params_for_kind(kind, loop_ir, hw, simd_width_opt);
        let code = emit_kernel(loop_ir, &params, kind);

        candidates.push(TransformCandidate {
            kind,
            params: params.clone(),
            code,
        });
    }

    candidates
}

/// Internal LoopIR for code emission (simplified from m0::Loop)
#[derive(Debug, Clone)]
pub struct LoopIR {
    pub name: String,
    pub depth: usize,
    pub ivs: Vec<IVInfo>,
    pub body: String, // C statement body
    pub arrays: Vec<ArraySpec>,
}

#[derive(Debug, Clone)]
pub struct IVInfo {
    pub name: String,
    pub lower: i64,
    pub upper: i64,
    pub step: i64,
}

/// Main kernel emission function
fn emit_kernel(loop_ir: &LoopIR, params: &TransformParams, kind: TransformKind) -> CompiledKernel {
    let spec = kernel_spec(loop_ir);
    let source = match kind {
        TransformKind::Baseline => emit_baseline(&spec),
        TransformKind::Tiling => emit_tiled_kernel(&spec, params),
        TransformKind::Simd => emit_simd_kernel(&spec, params),
        TransformKind::TiledSimd => emit_tiled_simd_kernel(&spec, params),
        TransformKind::Parallel => emit_parallel_kernel(&spec, params),
        TransformKind::TiledParallel => emit_tiled_parallel_kernel(&spec, params),
        TransformKind::SimdParallel => emit_simd_parallel_kernel(&spec, params),
        TransformKind::Full => emit_full_kernel(&spec, params),
    };

    let cflags = compiler_flags(params, hw_for_flags(&spec));
    let predicted_log2_speedup = predict_speedup(&spec, params, kind);

    CompiledKernel {
        kernel: spec.name.clone(),
        id: format!("{}_{}_{}_{}",
            spec.name,
            params.tile_sizes.first().unwrap_or(&0),
            params.simd_width,
            params.thread_count
        ),
        source,
        cflags,
        predicted_log2_speedup,
        tile_size: params.tile_sizes.first().copied().unwrap_or(0),
        simd_width: params.simd_width,
        thread_count: params.thread_count,
    }
}

/// Get kernel specification from LoopIR
fn kernel_spec(loop_ir: &LoopIR) -> KernelSpec {
    match loop_ir.name.as_str() {
        "copy" => KernelSpec {
            name: "copy".into(),
            depth: 1,
            bounds: vec!["n".into()],
            ivs: vec!["i".into()],
            body_template: "b[i] = a[i];".into(),
            inputs: vec![ArraySpec {
                name: "a".into(),
                dimensions: vec!["n".into()],
                element_type: "double".into(),
                is_restrict: true,
            }],
            outputs: vec![ArraySpec {
                name: "b".into(),
                dimensions: vec!["n".into()],
                element_type: "double".into(),
                is_restrict: true,
            }],
            access_patterns: vec![AccessPattern::UnitStride, AccessPattern::UnitStride],
        },
        "strided_copy" => KernelSpec {
            name: "strided_copy".into(),
            depth: 1,
            bounds: vec!["n".into()],
            ivs: vec!["i".into()],
            body_template: "b[i] = a[i * stride];".into(),
            inputs: vec![ArraySpec {
                name: "a".into(),
                dimensions: vec!["n * stride".into()],
                element_type: "double".into(),
                is_restrict: true,
            }],
            outputs: vec![ArraySpec {
                name: "b".into(),
                dimensions: vec!["n".into()],
                element_type: "double".into(),
                is_restrict: true,
            }],
            access_patterns: vec![AccessPattern::Strided, AccessPattern::UnitStride],
        },
        "transpose" => KernelSpec {
            name: "transpose".into(),
            depth: 2,
            bounds: vec!["rows".into(), "cols".into()],
            ivs: vec!["i".into(), "j".into()],
            body_template: "b[j * rows + i] = a[i * cols + j];".into(),
            inputs: vec![ArraySpec {
                name: "a".into(),
                dimensions: vec!["rows * cols".into()],
                element_type: "double".into(),
                is_restrict: true,
            }],
            outputs: vec![ArraySpec {
                name: "b".into(),
                dimensions: vec!["rows * cols".into()],
                element_type: "double".into(),
                is_restrict: true,
            }],
            access_patterns: vec![AccessPattern::Strided, AccessPattern::Strided],
        },
        "gemm" => KernelSpec {
            name: "gemm".into(),
            depth: 3,
            bounds: vec!["M".into(), "N".into(), "K".into()],
            ivs: vec!["i".into(), "j".into(), "k".into()],
            body_template: "sum += A[i*K+k] * B[k*N+j]; C[i*N+j] = sum;".into(),
            inputs: vec![
                ArraySpec { name: "A".into(), dimensions: vec!["M * K".into()], element_type: "double".into(), is_restrict: true },
                ArraySpec { name: "B".into(), dimensions: vec!["K * N".into()], element_type: "double".into(), is_restrict: true },
            ],
            outputs: vec![ArraySpec { name: "C".into(), dimensions: vec!["M * N".into()], element_type: "double".into(), is_restrict: true }],
            access_patterns: vec![AccessPattern::Strided, AccessPattern::Strided, AccessPattern::UnitStride],
        },
        _ => panic!("Unknown kernel: {}", loop_ir.name),
    }
}

/// Emit baseline kernel (no transformations) - kernel function only, no main()
fn emit_baseline(spec: &KernelSpec) -> String {
    let mut code = String::new();

    code.push_str("#include <stddef.h>\n\n");

    // Function signature
    code.push_str(&function_signature(spec, false, false, false));
    code.push_str("\n{\n");

    // Loop nest
    emit_loop_nest(&mut code, spec, 0, &[], &[]);

    code.push_str("}\n");
    code
}

/// Emit tiled kernel
fn emit_tiled_kernel(spec: &KernelSpec, params: &TransformParams) -> String {
    let tile_size = params.tile_sizes.first().copied().unwrap_or(32);
    let mut code = String::new();

    code.push_str("#include <stddef.h>\n\n");
    code.push_str(&function_signature(spec, false, false, false));
    code.push_str("\n{\n");

    match spec.name.as_str() {
        "copy" => {
            let n = spec.bounds[0].clone();
            code.push_str(&format!(
                r#"    for (long ii = 0; ii < {n}; ii += {tile_size}) {{
        long end = ii + {tile_size};
        if (end > {n}) end = {n};
        for (long i = ii; i < end; i++) {{
            b[i] = a[i];
        }}
    }}
"#
            ));
        }
        "strided_copy" => {
            let n = spec.bounds[0].clone();
            code.push_str(&format!(
                r#"    for (long ii = 0; ii < {n}; ii += {tile_size}) {{
        long end = ii + {tile_size};
        if (end > {n}) end = {n};
        for (long i = ii; i < end; i++) {{
            b[i] = a[i * stride];
        }}
    }}
"#
            ));
        }
        "transpose" => {
            let rows = spec.bounds[0].clone();
            let cols = spec.bounds[1].clone();
            code.push_str(&format!(
                r#"    for (long ii = 0; ii < {rows}; ii += {tile_size}) {{
        long i_end = ii + {tile_size};
        if (i_end > {rows}) i_end = {rows};
        for (long jj = 0; jj < {cols}; jj += {tile_size}) {{
            long j_end = jj + {tile_size};
            if (j_end > {cols}) j_end = {cols};
            for (long i = ii; i < i_end; i++) {{
                for (long j = jj; j < j_end; j++) {{
                    b[j * {rows} + i] = a[i * {cols} + j];
                }}
            }}
        }}
    }}
"#
            ));
        }
        "gemm" => {
            let M = spec.bounds[0].clone();
            let N = spec.bounds[1].clone();
            let K = spec.bounds[2].clone();
            code.push_str(&format!(
                r#"    for (long ii = 0; ii < {M}; ii += {tile_size}) {{
        long i_end = ii + {tile_size};
        if (i_end > {M}) i_end = {M};
        for (long jj = 0; jj < {N}; jj += {tile_size}) {{
            long j_end = jj + {tile_size};
            if (j_end > {N}) j_end = {N};
            for (long kk = 0; kk < {K}; kk += {tile_size}) {{
                long k_end = kk + {tile_size};
                if (k_end > {K}) k_end = {K};
                for (long i = ii; i < i_end; i++) {{
                    for (long j = jj; j < j_end; j++) {{
                        double sum = 0.0;
                        for (long k = kk; k < k_end; k++) {{
                            sum += A[i*{K}+k] * B[k*{N}+j];
                        }}
                        C[i*{N}+j] += sum;
                    }}
                }}
            }}
        }}
    }}
"#
            ));
        }
        _ => {}
    }

    code.push_str("}\n");
    code
}

/// Emit SIMD kernel
fn emit_simd_kernel(spec: &KernelSpec, params: &TransformParams) -> String {
    let simd_width = params.simd_width;
    let simd_factor = simd_width / 64; // double = 64 bits
    let mut code = String::new();

    // Map width to correct intrinsic prefix and type
    let (intrin_prefix, type_name) = match simd_width {
        128 => ("mm", "m128d"),      // SSE2: _mm_loadu_pd, __m128d
        256 => ("mm256", "m256d"),   // AVX: _mm256_loadu_pd, __m256d
        512 => ("mm512", "m512d"),   // AVX-512: _mm512_loadu_pd, __m512d
        _ => return emit_baseline(spec), // fallback
    };

    code.push_str("#include <stddef.h>\n");

    // Include appropriate SIMD header
    if simd_width >= 512 {
        code.push_str("#include <immintrin.h>\n");
        code.push_str("#ifndef __AVX512F__\n");
        code.push_str("#error \"AVX-512 not supported\"\n");
        code.push_str("#endif\n\n");
    } else if simd_width >= 256 {
        code.push_str("#include <immintrin.h>\n");
        code.push_str("#ifndef __AVX2__\n");
        code.push_str("#error \"AVX2 not supported\"\n");
        code.push_str("#endif\n\n");
    } else if simd_width >= 128 {
        code.push_str("#include <immintrin.h>\n");
        code.push_str("#ifndef __SSE2__\n");
        code.push_str("#error \"SSE2 not supported\"\n");
        code.push_str("#endif\n\n");
    } else {
        code.push_str("\n");
    }

    code.push_str(&function_signature(spec, false, false, false));
    code.push_str("\n{\n");

    match spec.name.as_str() {
        "copy" => {
            let n = spec.bounds[0].clone();
            code.push_str(&format!(
                r#"    long i = 0;
    for (; i + {simd_factor} - 1 < {n}; i += {simd_factor}) {{
        __{type_name} va = _{prefix}_loadu_pd(&a[i]);
        _{prefix}_storeu_pd(&b[i], va);
    }}
    for (; i < {n}; i++) {{
        b[i] = a[i];
    }}
"#, prefix = intrin_prefix, type_name = type_name)
            );
        }
        "strided_copy" => {
            // Strided copy doesn't benefit from SIMD - emit baseline with pragma
            let n = spec.bounds[0].clone();
            code.push_str(&format!(
                r#"    for (long i = 0; i < {n}; i++) {{
        b[i] = a[i * stride];
    }}
"#
            ));
        }
        "transpose" => {
            // Transpose - SIMD not directly applicable, use pragma for inner loop
            let rows = spec.bounds[0].clone();
            let cols = spec.bounds[1].clone();
            code.push_str(&format!(
                r#"    for (long i = 0; i < {rows}; i++) {{
        #pragma GCC ivdep
        for (long j = 0; j < {cols}; j++) {{
            b[j * {rows} + i] = a[i * {cols} + j];
        }}
    }}
"#
            ));
        }
        "gemm" => {
            let M = spec.bounds[0].clone();
            let N = spec.bounds[1].clone();
            let K = spec.bounds[2].clone();
            let (intrin_prefix, type_name) = match simd_width {
                128 => ("mm", "m128d"),
                256 => ("mm256", "m256d"),
                512 => ("mm512", "m512d"),
                _ => return emit_baseline(spec),
            };
            let hsum_fn = match simd_width {
                512 => "_mm512_reduce_add_pd",
                256 => "hsum_pd256",
                128 => "hsum_pd128",
                _ => "",
            };
            code.push_str(&format!(
                r#"    for (long i = 0; i < {M}; i++) {{
        for (long j = 0; j < {N}; j++) {{
            double sum = 0.0;
            long k = 0;
            for (; k + {simd_factor} - 1 < {K}; k += {simd_factor}) {{
                __{type_name} va = _{prefix}_loadu_pd(&A[i*{K}+k]);
                __{type_name} vb = _{prefix}_loadu_pd(&B[k*{N}+j]);
                __{type_name} vmul = _{prefix}_mul_pd(va, vb);
                sum += {hsum}(vmul);
            }}
            for (; k < {K}; k++) {{
                sum += A[i*{K}+k] * B[k*{N}+j];
            }}
            C[i*{N}+j] = sum;
        }}
    }}
"#, prefix = intrin_prefix, type_name = type_name, simd_factor = simd_factor, hsum = hsum_fn)
            );
            // Add horizontal sum helper
            if simd_width <= 256 {
                code.push_str(&simd_hsum_helpers(simd_width));
            }
        }
        _ => {}
    }

    code.push_str("}\n");
    code
}

/// SIMD horizontal sum helpers
fn simd_hsum_helpers(width: usize) -> String {
    match width {
        256 => r#"
static inline double hsum_pd256(__m256d v) {
    __m128d vlow = _mm256_castpd256_pd128(v);
    __m128d vhigh = _mm256_extractf128_pd(v, 1);
    vlow = _mm_add_pd(vlow, vhigh);
    vhigh = _mm_shuffle_pd(vlow, vlow, 1);
    vlow = _mm_add_pd(vlow, vhigh);
    return _mm_cvtsd_f64(vlow);
}
"#.into(),
        128 => r#"
static inline double hsum_pd128(__m128d v) {
    __m128d vhigh = _mm_shuffle_pd(v, v, 1);
    v = _mm_add_pd(v, vhigh);
    return _mm_cvtsd_f64(v);
}
"#.into(),
        _ => "".into(),
    }
}

/// Emit tiled + SIMD kernel
fn emit_tiled_simd_kernel(spec: &KernelSpec, params: &TransformParams) -> String {
    let tile_size = params.tile_sizes.first().copied().unwrap_or(32);
    let simd_width = params.simd_width;
    let simd_factor = simd_width / 64;
    let (intrin_prefix, type_name) = match simd_width {
        128 => ("mm", "m128d"),
        256 => ("mm256", "m256d"),
        512 => ("mm512", "m512d"),
        _ => return emit_tiled_kernel(spec, params),
    };
    let mut code = String::new();

    code.push_str("#include <stddef.h>\n#include <immintrin.h>\n\n");
    code.push_str(&function_signature(spec, false, false, false));
    code.push_str("\n{\n");

    match spec.name.as_str() {
        "copy" => {
            let n = spec.bounds[0].clone();
            code.push_str(&format!(
                r#"    for (long ii = 0; ii < {n}; ii += {tile_size}) {{
        long end = ii + {tile_size};
        if (end > {n}) end = {n};
        long i = ii;
        for (; i + {simd_factor} - 1 < end; i += {simd_factor}) {{
            __{type_name} va = _{prefix}_loadu_pd(&a[i]);
            _{prefix}_storeu_pd(&b[i], va);
        }}
        for (; i < end; i++) {{
            b[i] = a[i];
        }}
    }}
"#, prefix = intrin_prefix, type_name = type_name, simd_factor = simd_factor)
            );
        }
        "transpose" => {
            let rows = spec.bounds[0].clone();
            let cols = spec.bounds[1].clone();
            code.push_str(&format!(
                r#"    for (long ii = 0; ii < {rows}; ii += {tile_size}) {{
        long i_end = ii + {tile_size};
        if (i_end > {rows}) i_end = {rows};
        for (long jj = 0; jj < {cols}; jj += {tile_size}) {{
            long j_end = jj + {tile_size};
            if (j_end > {cols}) j_end = {cols};
            for (long i = ii; i < i_end; i++) {{
                for (long j = jj; j < j_end; j++) {{
                    b[j * {rows} + i] = a[i * {cols} + j];
                }}
            }}
        }}
    }}
"#
            ));
        }
        "gemm" => {
            let M = spec.bounds[0].clone();
            let N = spec.bounds[1].clone();
            let K = spec.bounds[2].clone();
            let (intrin_prefix, type_name) = match simd_width {
                128 => ("mm", "m128d"),
                256 => ("mm256", "m256d"),
                512 => ("mm512", "m512d"),
                _ => return emit_tiled_kernel(spec, params),
            };
            let hsum_fn = match simd_width {
                512 => "_mm512_reduce_add_pd",
                256 => "hsum_pd256",
                128 => "hsum_pd128",
                _ => "",
            };
            code.push_str(&format!(
                r#"    for (long ii = 0; ii < {M}; ii += {tile_size}) {{
        long i_end = ii + {tile_size};
        if (i_end > {M}) i_end = {M};
        for (long jj = 0; jj < {N}; jj += {tile_size}) {{
            long j_end = jj + {tile_size};
            if (j_end > {N}) j_end = {N};
            for (long kk = 0; kk < {K}; kk += {tile_size}) {{
                long k_end = kk + {tile_size};
                if (k_end > {K}) k_end = {K};
                for (long i = ii; i < i_end; i++) {{
                    for (long j = jj; j < j_end; j++) {{
                        double sum = 0.0;
                        long k = kk;
                        for (; k + {simd_factor} - 1 < k_end; k += {simd_factor}) {{
                            __{type_name} va = _{prefix}_loadu_pd(&A[i*{K}+k]);
                            __{type_name} vb = _{prefix}_loadu_pd(&B[k*{N}+j]);
                            __{type_name} vmul = _{prefix}_mul_pd(va, vb);
                            sum += {hsum}(vmul);
                        }}
                        for (; k < k_end; k++) {{
                            sum += A[i*{K}+k] * B[k*{N}+j];
                        }}
                        C[i*{N}+j] += sum;
                    }}
                }}
            }}
        }}
    }}
"#, prefix = intrin_prefix, type_name = type_name, simd_factor = simd_factor, hsum = hsum_fn)
            );
            if simd_width <= 256 {
                code.push_str(&simd_hsum_helpers(simd_width));
            }
        }
        _ => {}
    }

    code.push_str("}\n");
    code
}

/// Emit parallel kernel (OpenMP)
fn emit_parallel_kernel(spec: &KernelSpec, params: &TransformParams) -> String {
    let mut code = String::new();
    let threads = params.thread_count;

    code.push_str("#include <stddef.h>\n");
    code.push_str("#include <omp.h>\n\n");
    code.push_str(&function_signature(spec, true, false, false));
    code.push_str("\n{\n");

    match spec.name.as_str() {
        "copy" => {
            let n = spec.bounds[0].clone();
            code.push_str(&format!(
                r#"    #pragma omp parallel for num_threads({threads}) schedule(static)
    for (long i = 0; i < {n}; i++) {{
        b[i] = a[i];
    }}
"#
            ));
        }
        "strided_copy" => {
            let n = spec.bounds[0].clone();
            code.push_str(&format!(
                r#"    #pragma omp parallel for num_threads({threads}) schedule(static)
    for (long i = 0; i < {n}; i++) {{
        b[i] = a[i * stride];
    }}
"#
            ));
        }
        "transpose" => {
            let rows = spec.bounds[0].clone();
            let cols = spec.bounds[1].clone();
            code.push_str(&format!(
                r#"    #pragma omp parallel for num_threads({threads}) schedule(static) collapse(2)
    for (long i = 0; i < {rows}; i++) {{
        for (long j = 0; j < {cols}; j++) {{
            b[j * {rows} + i] = a[i * {cols} + j];
        }}
    }}
"#
            ));
        }
        "gemm" => {
            let M = spec.bounds[0].clone();
            let N = spec.bounds[1].clone();
            let K = spec.bounds[2].clone();
            code.push_str(&format!(
                r#"    #pragma omp parallel for num_threads({threads}) schedule(static) collapse(2)
    for (long i = 0; i < {M}; i++) {{
        for (long j = 0; j < {N}; j++) {{
            double sum = 0.0;
            for (long k = 0; k < {K}; k++) {{
                sum += A[i*{K}+k] * B[k*{N}+j];
            }}
            C[i*{N}+j] = sum;
        }}
    }}
"#
            ));
        }
        _ => {}
    }

    code.push_str("}\n");
    code
}

/// Emit tiled + parallel kernel
fn emit_tiled_parallel_kernel(spec: &KernelSpec, params: &TransformParams) -> String {
    let tile_size = params.tile_sizes.first().copied().unwrap_or(32);
    let threads = params.thread_count;
    let mut code = String::new();

    code.push_str("#include <stddef.h>\n");
    code.push_str("#include <omp.h>\n\n");
    code.push_str(&function_signature(spec, true, false, false));
    code.push_str("\n{\n");

    match spec.name.as_str() {
        "transpose" => {
            let rows = spec.bounds[0].clone();
            let cols = spec.bounds[1].clone();
            code.push_str(&format!(
                r#"    #pragma omp parallel for num_threads({threads}) schedule(static) collapse(2)
    for (long ii = 0; ii < {rows}; ii += {tile_size}) {{
        long i_end = ii + {tile_size};
        if (i_end > {rows}) i_end = {rows};
        for (long jj = 0; jj < {cols}; jj += {tile_size}) {{
            long j_end = jj + {tile_size};
            if (j_end > {cols}) j_end = {cols};
            for (long i = ii; i < i_end; i++) {{
                for (long j = jj; j < j_end; j++) {{
                    b[j * {rows} + i] = a[i * {cols} + j];
                }}
            }}
        }}
    }}
"#
            ));
        }
        "gemm" => {
            let M = spec.bounds[0].clone();
            let N = spec.bounds[1].clone();
            let K = spec.bounds[2].clone();
            code.push_str(&format!(
                r#"    #pragma omp parallel for num_threads({threads}) schedule(static) collapse(2)
    for (long ii = 0; ii < {M}; ii += {tile_size}) {{
        long i_end = ii + {tile_size};
        if (i_end > {M}) i_end = {M};
        for (long jj = 0; jj < {N}; jj += {tile_size}) {{
            long j_end = jj + {tile_size};
            if (j_end > {N}) j_end = {N};
            for (long kk = 0; kk < {K}; kk += {tile_size}) {{
                long k_end = kk + {tile_size};
                if (k_end > {K}) k_end = {K};
                for (long i = ii; i < i_end; i++) {{
                    for (long j = jj; j < j_end; j++) {{
                        double sum = 0.0;
                        for (long k = kk; k < k_end; k++) {{
                            sum += A[i*{K}+k] * B[k*{N}+j];
                        }}
                        C[i*{N}+j] += sum;
                    }}
                }}
            }}
        }}
    }}
"#
            ));
        }
        _ => {}
    }

    code.push_str("}\n");
    code
}

/// Emit SIMD + parallel kernel
fn emit_simd_parallel_kernel(spec: &KernelSpec, params: &TransformParams) -> String {
    let simd_width = params.simd_width;
    let simd_factor = simd_width / 64;
    let threads = params.thread_count;
    let mut code = String::new();

    code.push_str("#include <stddef.h>\n");
    code.push_str("#include <immintrin.h>\n");
    code.push_str("#include <omp.h>\n\n");
    code.push_str(&function_signature(spec, true, false, true));
    code.push_str("\n{\n");

    match spec.name.as_str() {
        "copy" => {
            let n = spec.bounds[0].clone();
            code.push_str(&format!(
                r#"    #pragma omp parallel for num_threads({threads}) schedule(static)
    for (long i = 0; i < {n}; i += {simd_factor}) {{
        if (i + {simd_factor} <= {n}) {{
            __m{width}d va = _mm{width}_loadu_pd(&a[i]);
            _mm{width}_storeu_pd(&b[i], va);
        }} else {{
            for (long k = i; k < {n} && k < i + {simd_factor}; k++) {{
                b[k] = a[k];
            }}
        }}
    }}
"#, width = simd_width)
            );
        }
        "gemm" => {
            let M = spec.bounds[0].clone();
            let N = spec.bounds[1].clone();
            let K = spec.bounds[2].clone();
            code.push_str(&format!(
                r#"    #pragma omp parallel for num_threads({threads}) schedule(static) collapse(2)
    for (long i = 0; i < {M}; i++) {{
        for (long j = 0; j < {N}; j++) {{
            double sum = 0.0;
            long k = 0;
            for (; k + {simd_factor} - 1 < {K}; k += {simd_factor}) {{
                __m{width}d va = _mm{width}_loadu_pd(&A[i*{K}+k]);
                __m{width}d vb = _mm{width}_loadu_pd(&B[k*{N}+j]);
                __m{width}d vmul = _mm{width}_mul_pd(va, vb);
                sum += hsum_pd{width}(vmul);
            }}
            for (; k < {K}; k++) {{
                sum += A[i*{K}+k] * B[k*{N}+j];
            }}
            C[i*{N}+j] = sum;
        }}
    }}
"#, width = simd_width)
            );
            code = code.replace("hsum_pd512", "_mm512_reduce_add_pd")
                      .replace("hsum_pd256", "hsum_pd256")
                      .replace("hsum_pd128", "hsum_pd128");
            code.push_str(&simd_hsum_helpers(simd_width));
        }
        _ => {}
    }

    code.push_str("}\n");
    code
}

/// Emit full kernel (tiled + SIMD + parallel)
fn emit_full_kernel(spec: &KernelSpec, params: &TransformParams) -> String {
    let tile_size = params.tile_sizes.first().copied().unwrap_or(32);
    let simd_width = params.simd_width;
    let simd_factor = simd_width / 64;
    let threads = params.thread_count;
    let mut code = String::new();

    code.push_str("#include <stddef.h>\n");
    code.push_str("#include <immintrin.h>\n");
    code.push_str("#include <omp.h>\n\n");
    code.push_str(&function_signature(spec, true, true, true));
    code.push_str("\n{\n");

    match spec.name.as_str() {
        "gemm" => {
            let M = spec.bounds[0].clone();
            let N = spec.bounds[1].clone();
            let K = spec.bounds[2].clone();
            code.push_str(&format!(
                r#"    #pragma omp parallel for num_threads({threads}) schedule(static) collapse(2)
    for (long ii = 0; ii < {M}; ii += {tile_size}) {{
        long i_end = ii + {tile_size};
        if (i_end > {M}) i_end = {M};
        for (long jj = 0; jj < {N}; jj += {tile_size}) {{
            long j_end = jj + {tile_size};
            if (j_end > {N}) j_end = {N};
            for (long kk = 0; kk < {K}; kk += {tile_size}) {{
                long k_end = kk + {tile_size};
                if (k_end > {K}) k_end = {K};
                for (long i = ii; i < i_end; i++) {{
                    for (long j = jj; j < j_end; j++) {{
                        double sum = 0.0;
                        long k = kk;
                        for (; k + {simd_factor} - 1 < k_end; k += {simd_factor}) {{
                            __m{width}d va = _mm{width}_loadu_pd(&A[i*{K}+k]);
                            __m{width}d vb = _mm{width}_loadu_pd(&B[k*{N}+j]);
                            __m{width}d vmul = _mm{width}_mul_pd(va, vb);
                            sum += hsum_pd{width}(vmul);
                        }}
                        for (; k < k_end; k++) {{
                            sum += A[i*{K}+k] * B[k*{N}+j];
                        }}
                        C[i*{N}+j] += sum;
                    }}
                }}
            }}
        }}
    }}
"#, width = simd_width)
            );
            code = code.replace("hsum_pd512", "_mm512_reduce_add_pd")
                      .replace("hsum_pd256", "hsum_pd256")
                      .replace("hsum_pd128", "hsum_pd128");
            code.push_str(&simd_hsum_helpers(simd_width));
        }
        "transpose" => {
            let rows = spec.bounds[0].clone();
            let cols = spec.bounds[1].clone();
            code.push_str(&format!(
                r#"    #pragma omp parallel for num_threads({threads}) schedule(static) collapse(2)
    for (long ii = 0; ii < {rows}; ii += {tile_size}) {{
        long i_end = ii + {tile_size};
        if (i_end > {rows}) i_end = {rows};
        for (long jj = 0; jj < {cols}; jj += {tile_size}) {{
            long j_end = jj + {tile_size};
            if (j_end > {cols}) j_end = {cols};
            for (long i = ii; i < i_end; i++) {{
                for (long j = jj; j < j_end; j++) {{
                    b[j * {rows} + i] = a[i * {cols} + j];
                }}
            }}
        }}
    }}
"#
            ));
        }
        _ => {}
    }

    code.push_str("}\n");
    code
}

/// Generate function signature
fn function_signature(spec: &KernelSpec, with_openmp: bool, with_simd: bool, with_intrinsics: bool) -> String {
    let mut params = Vec::new();

    for input in &spec.inputs {
        let restrict = if input.is_restrict { "restrict " } else { "" };
        params.push(format!("const {} *{} {}", input.element_type, restrict, input.name));
    }
    for output in &spec.outputs {
        let restrict = if output.is_restrict { "restrict " } else { "" };
        params.push(format!("{} *{} {}", output.element_type, restrict, output.name));
    }

    // Add loop bound parameters
    match spec.name.as_str() {
        "copy" | "strided_copy" => {
            params.push("long n".into());
            if spec.name == "strided_copy" {
                params.push("long stride".into());
            }
        }
        "transpose" => {
            params.push("long rows".into());
            params.push("long cols".into());
        }
        "gemm" => {
            params.push("long M".into());
            params.push("long N".into());
            params.push("long K".into());
        }
        _ => {}
    }

    format!("void kernel_{}({})", spec.name, params.join(", "))
}

/// Emit loop nest recursively
fn emit_loop_nest(code: &mut String, spec: &KernelSpec, level: usize, ivs_outer: &[String], bounds_outer: &[String]) {
    if level >= spec.depth {
        // Emit body
        code.push_str(&format!("{} {}", "    ".repeat(level + 1), spec.body_template));
        code.push_str("\n");
        return;
    }

    let iv = &spec.ivs[level];
    let bound = &spec.bounds[level];

    code.push_str(&format!("{}for (long {} = 0; {} < {}; {}++) {{\n", "    ".repeat(level + 1), iv, iv, bound, iv));
    emit_loop_nest(code, spec, level + 1, ivs_outer, bounds_outer);
    code.push_str(&format!("{}}}\n", "    ".repeat(level + 1)));
}

/// Compiler flags for given parameters
fn compiler_flags(params: &TransformParams, hw: &HardwareModel) -> Vec<String> {
    let mut flags = vec!["-O2".into(), "-march=native".into(), "-std=c11".into()];

    if params.thread_count > 1 {
        flags.push("-fopenmp".into());
    }

    if params.simd_width >= 512 && hw.simd.avx512 {
        flags.push("-mavx512f".into());
    } else if params.simd_width >= 256 && hw.simd.avx2 {
        flags.push("-mavx2".into());
    } else if params.simd_width >= 128 && hw.simd.max_vector_width_bits >= 128 {
        flags.push("-mavx".into());
    }

    flags
}

/// Predict speedup using cost model
fn predict_speedup(spec: &KernelSpec, params: &TransformParams, kind: TransformKind) -> f64 {
    let cost_model = CostModel::new();

    let features = CostFeatures {
        loop_depth: spec.depth,
        trip_count: 1000, // default
        element_size: 8,
        working_set_bytes: 1024 * 1024,
        working_set_category: WorkingSetCategory::L2,
        memory_accesses: spec.inputs.len() + spec.outputs.len(),
        compute_intensity: match spec.name.as_str() {
            "gemm" => 4.0,
            "transpose" => 0.5,
            _ => 1.0,
        },
        stride_pattern: StridePattern::Unit,
        stride_value: None,
        access_pattern: AccessPattern::UnitStride,
        arithmetic_intensity_category: match spec.name.as_str() {
            "gemm" => ArithmeticIntensityCategory::MatMul,
            "transpose" => ArithmeticIntensityCategory::Copy,
            _ => ArithmeticIntensityCategory::Axpy,
        },
        dependency_type: DependencyType::None,
        is_parallelizable: params.thread_count > 1,
        tile_size: params.tile_sizes.first().copied().unwrap_or(0),
        simd_width: params.simd_width,
        thread_count: params.thread_count,
        numa_locality: 0.9,
        reuse_distance: if params.tile_sizes.first().copied().unwrap_or(0) > 0 { 1.0 } else { 100.0 },
    };

    cost_model.predict(&features)
}

/// Hardware model for flags
fn hw_for_flags(spec: &KernelSpec) -> &HardwareModel {
    // Return a reference to a static HardwareModel
    static HW: std::sync::OnceLock<HardwareModel> = std::sync::OnceLock::new();
    HW.get_or_init(HardwareModel::detect)
}

/// Get applicable transformations for a kernel - returns (kind, simd_width) pairs
fn get_applicable_transforms(loop_ir: &LoopIR, hw: &HardwareModel) -> Vec<(TransformKind, Option<usize>)> {
    let mut transforms = vec![(TransformKind::Baseline, None)];
    let max_simd = hw.simd.max_vector_width_bits;
    let simd_widths: Vec<usize> = [128, 256, 512].into_iter()
        .filter(|&w| w <= max_simd)
        .collect();

    match loop_ir.name.as_str() {
        "copy" => {
            // SIMD variants for each supported width
            for w in &simd_widths {
                transforms.push((TransformKind::Simd, Some(*w)));
            }
            if hw.cpu.total_logical_cores > 1 {
                transforms.push((TransformKind::Parallel, None));
                for w in &simd_widths {
                    transforms.push((TransformKind::SimdParallel, Some(*w)));
                }
            }
        }
        "strided_copy" => {
            if hw.cpu.total_logical_cores > 1 {
                transforms.push((TransformKind::Parallel, None));
            }
            // No SIMD benefit for strided
        }
        "transpose" => {
            transforms.push((TransformKind::Tiling, None));
            if hw.cpu.total_logical_cores > 1 {
                transforms.push((TransformKind::Parallel, None));
                transforms.push((TransformKind::TiledParallel, None));
            }
            // Transpose doesn't benefit from SIMD directly
        }
        "gemm" => {
            transforms.push((TransformKind::Tiling, None));
            for w in &simd_widths {
                transforms.push((TransformKind::Simd, Some(*w)));
                transforms.push((TransformKind::TiledSimd, Some(*w)));
            }
            if hw.cpu.total_logical_cores > 1 {
                transforms.push((TransformKind::Parallel, None));
                transforms.push((TransformKind::TiledParallel, None));
                for w in &simd_widths {
                    transforms.push((TransformKind::SimdParallel, Some(*w)));
                    transforms.push((TransformKind::Full, Some(*w)));
                }
            }
        }
        _ => {}
    }

    transforms
}

/// Parameters for transform kind
fn params_for_kind(kind: TransformKind, loop_ir: &LoopIR, hw: &HardwareModel, simd_width_opt: Option<usize>) -> TransformParams {
    let max_simd = hw.simd.max_vector_width_bits;
    let max_threads = hw.cpu.total_logical_cores;
    let simd_width = simd_width_opt.unwrap_or(max_simd.min(256));

    match kind {
        TransformKind::Baseline => TransformParams::default(),
        TransformKind::Tiling => TransformParams {
            tile_sizes: vec![32],
            simd_width: 0,
            thread_count: 1,
            tile_level: 0,
            use_openmp: false,
            use_intrinsics: false,
        },
        TransformKind::Simd => TransformParams {
            tile_sizes: vec![],
            simd_width,
            thread_count: 1,
            tile_level: 0,
            use_openmp: false,
            use_intrinsics: true,
        },
        TransformKind::TiledSimd => TransformParams {
            tile_sizes: vec![32],
            simd_width,
            thread_count: 1,
            tile_level: 0,
            use_openmp: false,
            use_intrinsics: true,
        },
        TransformKind::Parallel => TransformParams {
            tile_sizes: vec![],
            simd_width: 0,
            thread_count: max_threads.min(4),
            tile_level: 0,
            use_openmp: true,
            use_intrinsics: false,
        },
        TransformKind::TiledParallel => TransformParams {
            tile_sizes: vec![32],
            simd_width: 0,
            thread_count: max_threads.min(4),
            tile_level: 0,
            use_openmp: true,
            use_intrinsics: false,
        },
        TransformKind::SimdParallel => TransformParams {
            tile_sizes: vec![],
            simd_width,
            thread_count: max_threads.min(4),
            tile_level: 0,
            use_openmp: true,
            use_intrinsics: true,
        },
        TransformKind::Full => TransformParams {
            tile_sizes: vec![32],
            simd_width,
            thread_count: max_threads.min(4),
            tile_level: 0,
            use_openmp: true,
            use_intrinsics: true,
        },
    }
}

/// Create LoopIR from DependenceAnalysis for emission
pub fn loop_ir_from_analysis(analysis: &DependenceAnalysis, kernel_name: &str) -> LoopIR {
    // Create a basic LoopIR based on the kernel name
    let (depth, ivs, body, arrays) = match kernel_name {
        "copy" => (
            1,
            vec![IVInfo { name: "i".into(), lower: 0, upper: 1000, step: 1 }],
            "b[i] = a[i];".into(),
            vec![
                ArraySpec { name: "a".into(), dimensions: vec!["n".into()], element_type: "double".into(), is_restrict: true },
                ArraySpec { name: "b".into(), dimensions: vec!["n".into()], element_type: "double".into(), is_restrict: true },
            ],
        ),
        "strided_copy" => (
            1,
            vec![IVInfo { name: "i".into(), lower: 0, upper: 1000, step: 1 }],
            "b[i] = a[i * stride];".into(),
            vec![
                ArraySpec { name: "a".into(), dimensions: vec!["n * stride".into()], element_type: "double".into(), is_restrict: true },
                ArraySpec { name: "b".into(), dimensions: vec!["n".into()], element_type: "double".into(), is_restrict: true },
            ],
        ),
        "transpose" => (
            2,
            vec![
                IVInfo { name: "i".into(), lower: 0, upper: 64, step: 1 },
                IVInfo { name: "j".into(), lower: 0, upper: 64, step: 1 },
            ],
            "b[j * rows + i] = a[i * cols + j];".into(),
            vec![
                ArraySpec { name: "a".into(), dimensions: vec!["rows * cols".into()], element_type: "double".into(), is_restrict: true },
                ArraySpec { name: "b".into(), dimensions: vec!["rows * cols".into()], element_type: "double".into(), is_restrict: true },
            ],
        ),
        "gemm" => (
            3,
            vec![
                IVInfo { name: "i".into(), lower: 0, upper: 32, step: 1 },
                IVInfo { name: "j".into(), lower: 0, upper: 32, step: 1 },
                IVInfo { name: "k".into(), lower: 0, upper: 32, step: 1 },
            ],
            "C[i*N+j] += A[i*K+k] * B[k*N+j];".into(),
            vec![
                ArraySpec { name: "A".into(), dimensions: vec!["M * K".into()], element_type: "double".into(), is_restrict: true },
                ArraySpec { name: "B".into(), dimensions: vec!["K * N".into()], element_type: "double".into(), is_restrict: true },
                ArraySpec { name: "C".into(), dimensions: vec!["M * N".into()], element_type: "double".into(), is_restrict: true },
            ],
        ),
        _ => panic!("Unknown kernel: {}", kernel_name),
    };

    LoopIR {
        name: kernel_name.into(),
        depth,
        ivs,
        body,
        arrays,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_baseline_copy() {
        let loop_ir = LoopIR {
            name: "copy".into(),
            depth: 1,
            ivs: vec![IVInfo { name: "i".into(), lower: 0, upper: 100, step: 1 }],
            body: "b[i] = a[i];".into(),
            arrays: vec![
                ArraySpec { name: "a".into(), dimensions: vec!["n".into()], element_type: "double".into(), is_restrict: true },
                ArraySpec { name: "b".into(), dimensions: vec!["n".into()], element_type: "double".into(), is_restrict: true },
            ],
        };

        let code = emit_tiled(&loop_ir, 32);
        assert!(code.source.contains("b[i] = a[i]"));
        assert!(code.source.contains("ii"));
        assert_eq!(code.tile_size, 32);
    }

    #[test]
    fn test_emit_simd_copy() {
        let loop_ir = LoopIR {
            name: "copy".into(),
            depth: 1,
            ivs: vec![IVInfo { name: "i".into(), lower: 0, upper: 100, step: 1 }],
            body: "b[i] = a[i];".into(),
            arrays: vec![
                ArraySpec { name: "a".into(), dimensions: vec!["n".into()], element_type: "double".into(), is_restrict: true },
                ArraySpec { name: "b".into(), dimensions: vec!["n".into()], element_type: "double".into(), is_restrict: true },
            ],
        };

        let code = emit_simd(&loop_ir, 256);
        assert!(code.source.contains("_mm256_loadu_pd"));
        assert!(code.source.contains("_mm256_storeu_pd"));
        assert_eq!(code.simd_width, 256);
    }

    #[test]
    fn test_emit_candidates() {
        let hw = HardwareModel::detect();
        let loop_ir = LoopIR {
            name: "gemm".into(),
            depth: 3,
            ivs: vec![
                IVInfo { name: "i".into(), lower: 0, upper: 32, step: 1 },
                IVInfo { name: "j".into(), lower: 0, upper: 32, step: 1 },
                IVInfo { name: "k".into(), lower: 0, upper: 32, step: 1 },
            ],
            body: "C[i*N+j] += A[i*K+k] * B[k*N+j];".into(),
            arrays: vec![],
        };

        let candidates = emit_candidates(&loop_ir, &hw);
        assert!(!candidates.is_empty());
        assert!(candidates.iter().any(|c| c.kind == TransformKind::Baseline));
    }
}

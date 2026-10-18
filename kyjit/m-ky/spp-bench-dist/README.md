# SPP Benchmark Distribution Package

Self-contained benchmark suite for SPP (Speculative Polyhedral Parallelization) Layer 2 measurement harness.

## Contents

```
spp-bench-dist/
├── Cargo.toml              # Rust project manifest
├── run_bench.sh            # Main runner script (use this)
├── README.md               # This file
├── kernels/                # C kernel implementations
│   ├── copy.c
│   ├── strided_copy.c
│   ├── transpose.c
│   ├── gemm.c
│   └── kernels.h
├── src/                    # Rust source
│   ├── main.rs
│   ├── protocol.rs         # Measurement protocol
│   ├── generate.rs         # Candidate generation
│   ├── csv.rs              # CSV output
│   ├── validity.rs         # Validity rules
│   ├── sanity.rs           # Sanity checks
│   └── fast_corpus.rs      # Quick test mode
└── bench/
    ├── generated/          # Compiled binaries (created at runtime)
    └── results/            # CSV output (created at runtime)
```

## Quick Start (Remote Server)

```bash
# 1. Extract package
tar -xzf spp-bench-dist.tar.gz
cd spp-bench-dist

# 2. Install dependencies (Ubuntu/Debian)
sudo apt-get update && sudo apt-get install -y \
    build-essential \
    pkg-config \
    libnuma-dev

# Install Rust (if not present)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source ~/.cargo/env

# 3. Run QUICK test first (5-10 min) to validate setup
./run_bench.sh quick

# 4. If quick test passes, run FULL benchmark (17-35 hours)
./run_bench.sh full
```

## Commands

| Command | Description | Time |
|---------|-------------|------|
| `./run_bench.sh quick` | Reduced test: copy kernel, SIMD=0/256, threads=1/4, L1/L2 only | ~5-10 min |
| `./run_bench.sh generate` | Full benchmark: all ~300 combinations | ~17-35 hours |
| `./run_bench.sh sanity` | Run sanity checks on existing corpus.csv | ~1 min |
| `./run_bench.sh full` | Full benchmark + sanity checks | ~17-35 hours |

## Output Files

After completion, results are in `bench/results/`:

- `corpus.csv` - Full measurement data (~300 rows)
- `fast_corpus.csv` - Quick test data (from `quick` command)
- `sanity.log` - Sanity check results

## Corpus CSV Schema

```
loop_id,kernel,loop_depth,trip_count_log,body_size,memory_accesses,
compute_intensity,stride_pattern,working_set_bytes,cache_level,
tile_size,simd_width,thread_count,baseline_median_ns,candidate_median_ns,
baseline_stddev_ns,candidate_stddev_ns,speedup,valid,invalid_reason
```

## Sanity Checks (Must Pass)

1. **Copy SIMD 256**: speedup 1.5x - 5.0x
2. **Copy Threads 4**: speedup 1.5x - 4.0x  
3. **Strided Copy SIMD 256**: speedup 0.8x - 1.3x (negative result - should NOT speed up)
4. **Transpose Tiling 32/64**: speedup 1.5x - 8.0x
5. **GEMM Best (tile+SIMD+threads)**: speedup 3.0x - 30.0x

## Hardware Requirements

- x86-64 with AVX2 support (for SIMD 256)
- At least 4 cores (for thread scaling tests)
- 8GB+ RAM (for DRAM working set)
- Linux with `taskset` for CPU pinning
- Root access optional (for turbo boost disable)

## Transferring Results Back

```bash
# After benchmark completes, copy results back
scp user@remote:~/spp-bench-dist/bench/results/*.csv ./local_results/
```

## Troubleshooting

**"Turbo boost warning"** - Normal, just a warning. Needs root to disable.

**"taskset not found"** - Install `util-linux` package. Benchmark runs without pinning but less accurate.

**"AVX2 not supported"** - CPU doesn't support AVX2. SIMD 256 tests will fail or fall back.

**Build fails** - Ensure Rust 1.70+ and GCC 9+ are installed.

**Sanity checks fail** - Verify hardware isn't throttling, check thermal/power limits.

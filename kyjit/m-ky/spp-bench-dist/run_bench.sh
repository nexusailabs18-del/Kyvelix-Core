#!/bin/bash
# SPP Benchmark Runner - Self-contained script for remote server
# Usage: ./run_bench.sh [generate|sanity|full]

set -e

echo "=== SPP Benchmark Runner ==="
echo "Host: $(hostname)"
echo "CPU: $(lscpu | grep 'Model name' | cut -d: -f2 | xargs)"
echo "Cores: $(nproc)"
echo "Date: $(date)"
echo ""

# Check for required tools
echo "Checking dependencies..."
for tool in rustc cargo gcc; do
    if ! command -v $tool &> /dev/null; then
        echo "ERROR: $tool not found. Please install Rust and GCC."
        exit 1
    fi
done

# Try to disable turbo boost (requires root)
if [ -w /sys/devices/system/cpu/intel_pstate/no_turbo ]; then
    echo "1" > /sys/devices/system/cpu/intel_pstate/no_turbo
    echo "Turbo boost disabled"
else
    echo "WARNING: Cannot disable turbo boost (need root). Continuing anyway."
fi

# Pin to CPU 0 if possible
if command -v taskset &> /dev/null; then
    echo "Using taskset for CPU pinning"
    TASKSET="taskset -c 0"
else
    echo "WARNING: taskset not available. No CPU pinning."
    TASKSET=""
fi

# Build
echo ""
echo "=== Building (release) ==="
cargo build --release

# Run benchmark
case "${1:-full}" in
    generate)
        echo ""
        echo "=== Running full benchmark (generate) ==="
        echo "This will take ~17-35 hours for ~300 combinations"
        echo "Output: bench/results/corpus.csv"
        $TASKSET ./target/release/bench generate
        ;;
    sanity)
        echo ""
        echo "=== Running sanity checks ==="
        $TASKSET ./target/release/bench sanity
        ;;
    full)
        echo ""
        echo "=== Running full benchmark + sanity ==="
        echo "This will take ~17-35 hours"
        $TASKSET ./target/release/bench generate
        echo ""
        echo "=== Running sanity checks ==="
        $TASKSET ./target/release/bench sanity
        ;;
    quick)
        echo ""
        echo "=== Running QUICK test (reduced) ==="
        echo "Testing copy kernel only, SIMD=0/256, threads=1/4, L1/L2 only"
        echo "Estimated time: ~5-10 minutes"
        $TASKSET ./target/release/bench quick
        ;;
    *)
        echo "Usage: $0 [generate|sanity|full|quick]"
        exit 1
        ;;
esac

echo ""
echo "=== Done ==="
echo "Results in: bench/results/"
ls -la bench/results/

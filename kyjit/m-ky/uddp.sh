#!/usr/bin/env bash
# UDDP Control Panel — run test suites, build, clean, etc.
# Usage: ./uddp.sh <command> [args...]

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
M0_DIR="$ROOT/m0"
CARGO="$M0_DIR/Cargo.toml"

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

info() { echo -e "${BLUE}[INFO]${NC} $*"; }
ok() { echo -e "${GREEN}[OK]${NC} $*"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
err() { echo -e "${RED}[ERR]${NC} $*"; }

usage() {
    cat <<'EOF'
UDDP Control Panel

Commands:
  all              Full regression (lib + integration + negative)
  lib              cargo test --lib --release
  integration      All E2E test suites (phase4 through phase9)
  negative         Soundness corpus (checker MUST reject)
  level <NAME>     Single level suite (m2g, m2a, m2a5, m2b, m3, m4, m5, m6, m7, m8, m9, m10, m11, m12, m13)
  build            cargo build --release
  clean            cargo clean
  bench            Run benchmarks
  check            Verify all certificates in tests/loops/
  help             Show this help

Examples:
  ./uddp.sh all
  ./uddp.sh level m4
  ./uddp.sh lib
  ./uddp.sh negative
EOF
}

run_lib() {
    info "Running library tests..."
    (cd "$M0_DIR" && cargo test --lib --release 2>&1 | tail -20)
}

run_integration() {
    info "Running integration tests..."
    local suites=(
        "phase4_e2e"
        "phase5_e2e"
        "phase6_e2e"
        "phase7_e2e"
        "phase8_e2e"
        "phase9_e2e"
        "phase9_kernels"
        "phase9_lower"
        "phase9_parser"
        "phase9a_evidence"
    )
    for suite in "${suites[@]}"; do
        info "Running $suite..."
        (cd "$M0_DIR" && cargo test --test "$suite" --release 2>&1 | tail -10)
    done
}

run_negative() {
    info "Running negative (soundness) corpus..."
    (cd "$M0_DIR/tests" && ./run_negative.sh)
}

run_level() {
    local level="${1:-}"
    if [[ -z "$level" ]]; then
        err "Usage: uddp.sh level <NAME>"
        exit 1
    fi
    local script="$M0_DIR/tests/run_${level}_tests.sh"
    if [[ -x "$script" ]]; then
        info "Running $level test suite..."
        "$script"
    else
        err "No test runner found for level: $level"
        err "Available: m2g, m2a, m2a5, m2b, m3, m4, m5, m6, m7, m8, m9, m10, m11, m12, m13"
        exit 1
    fi
}

run_build() {
    info "Building release..."
    (cd "$M0_DIR" && cargo build --release)
}

run_clean() {
    info "Cleaning..."
    (cd "$M0_DIR" && cargo clean)
}

run_bench() {
    info "Running benchmarks..."
    (cd "$M0_DIR" && cargo bench 2>&1 | tail -30)
}

run_check() {
    info "Verifying certificates in tests/loops/..."
    (cd "$M0_DIR" && cargo build --release --bin m0-decide --bin m0-check 2>&1 | tail -5)
    (cd "$M0_DIR/tests" && ./run_all.sh)
}

run_all() {
    info "=== FULL REGRESSION ==="
    run_build
    echo ""
    run_lib
    echo ""
    run_integration
    echo ""
    run_negative
    echo ""
    ok "Full regression complete"
}

# Main dispatch
cmd="${1:-help}"
shift || true

case "$cmd" in
    all)        run_all ;;
    lib)        run_lib ;;
    integration) run_integration ;;
    negative)   run_negative ;;
    level)      run_level "${1:-}" ;;
    build)      run_build ;;
    clean)      run_clean ;;
    bench)      run_bench ;;
    check)      run_check ;;
    help|*)     usage ;;
esac
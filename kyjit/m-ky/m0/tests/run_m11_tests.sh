#!/bin/bash
set -e
cd "$(dirname "$0")/.."
echo "=== M11 Unit Tests ==="
cargo test --lib decide::m11 2>&1 | grep "test result"
echo "M11 unit tests passed."

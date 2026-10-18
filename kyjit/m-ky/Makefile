# UDDP Makefile — convenience targets
# Usage: make <target>

.PHONY: all build clean test lib integration negative check bench fmt clippy doc help

# Default target
all: build test

# Build release binaries
build:
	cd m0 && cargo build --release

# Clean build artifacts
clean:
	cd m0 && cargo clean

# Run all tests
test: lib integration negative

# Library tests only
lib:
	cd m0 && cargo test --lib --release

# Integration test suites
integration:
	cd m0 && cargo test --test phase4_e2e --release
	cd m0 && cargo test --test phase5_e2e --release
	cd m0 && cargo test --test phase6_e2e --release
	cd m0 && cargo test --test phase7_e2e --release
	cd m0 && cargo test --test phase8_e2e --release
	cd m0 && cargo test --test phase9_e2e --release
	cd m0 && cargo test --test phase9_kernels --release
	cd m0 && cargo test --test phase9_lower --release
	cd m0 && cargo test --test phase9_parser --release
	cd m0 && cargo test --test phase9a_evidence --release

# Negative (soundness) corpus
negative:
	cd m0/tests && ./run_negative.sh

# Single level test (usage: make level LEVEL=m4)
level:
	@if [ -z "$(LEVEL)" ]; then \
		echo "Usage: make level LEVEL=<name>"; \
		echo "Available: m2g m2a m2a5 m2b m3 m4 m5 m6 m7 m8 m9 m10 m11 m12 m13"; \
		exit 1; \
	fi
	cd m0/tests && ./run_$(LEVEL)_tests.sh

# Verify all certificates in tests/loops/
check: build
	cd m0/tests && ./run_all.sh

# Run benchmarks
bench:
	cd m0 && cargo bench

# Format code
fmt:
	cd m0 && cargo fmt --all

# Lint
clippy:
	cd m0 && cargo clippy --all-targets -- -D warnings

# Generate documentation
doc:
	cd m0 && cargo doc --no-deps --open

# Run uddp.sh commands via make
uddp-all:
	./uddp.sh all

uddp-lib:
	./uddp.sh lib

uddp-integration:
	./uddp.sh integration

uddp-negative:
	./uddp.sh negative

uddp-level:
	@if [ -z "$(LEVEL)" ]; then \
		echo "Usage: make uddp-level LEVEL=<name>"; \
		exit 1; \
	fi
	./uddp.sh level $(LEVEL)

# Show help
help:
	@echo "UDDP Makefile Targets:"
	@echo "  make              - Build and run all tests (default)"
	@echo "  make build        - Build release binaries"
	@echo "  make clean        - Clean build artifacts"
	@echo "  make test         - Run all tests (lib + integration + negative)"
	@echo "  make lib          - Run library tests only"
	@echo "  make integration  - Run integration test suites"
	@echo "  make negative     - Run soundness corpus"
	@echo "  make level LEVEL=<name>  - Run single level test suite"
	@echo "  make check        - Verify all certificates in tests/loops/"
	@echo "  make bench        - Run benchmarks"
	@echo "  make fmt          - Format code"
	@echo "  make clippy       - Lint with clippy"
	@echo "  make doc          - Generate and open documentation"
	@echo ""
	@echo "Via uddp.sh:"
	@echo "  make uddp-all"
	@echo "  make uddp-lib"
	@echo "  make uddp-integration"
	@echo "  make uddp-negative"
	@echo "  make uddp-level LEVEL=<name>"
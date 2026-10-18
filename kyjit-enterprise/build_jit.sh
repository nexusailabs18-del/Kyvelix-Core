#!/bin/bash
set -e

echo "Building KYJIT LLVM 21 JIT"
echo "=========================="

cd ~/kyjit-enterprise
mkdir -p build

# Get LLVM flags
LLVM_CFLAGS=$(llvm-config-21 --cflags)
LLVM_LDFLAGS="-L/usr/lib/llvm-21/lib -lLLVM-21 $(llvm-config-21 --system-libs) -lstdc++ -lm -lpthread -ldl -lz -ltinfo"

echo "Compiling JIT implementation..."
gcc -c -O2 -fPIC -Wall -Wextra $LLVM_CFLAGS \
    -Isrc/jit src/jit/llvm_jit_v21.c -o build/llvm_jit_v21.o

echo "Compiling test program..."
gcc -c -O2 -Wall -Wextra $LLVM_CFLAGS \
    -Isrc/jit src/jit/test_jit.c -o build/test_jit.o

echo "Linking..."
gcc -o build/test_jit build/llvm_jit_v21.o build/test_jit.o \
    $LLVM_LDFLAGS -Wl,-rpath,/usr/lib/llvm-21/lib

echo "✓ Build successful!"
echo ""
echo "Running tests..."
./build/test_jit

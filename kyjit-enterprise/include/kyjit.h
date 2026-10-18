#ifndef KYJIT_H
#define KYJIT_H

#include <llvm-c/Core.h>
#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct KyJITCompiler KyJITCompiler;

typedef enum {
    KYJIT_TIER1_BASELINE = 0,   // O1, no vectorization
    KYJIT_TIER2_OPTIMIZED = 1,  // O2 + vectorization
    KYJIT_TIER3_MAXIMUM = 2     // O3 + aggressive vectorization + unroll
} KyJITTier;

// Initialize LLVM and detect CPU (call once)
void ky_jit_init(void);

// Create JIT compiler for a tier
KyJITCompiler* ky_jit_create(KyJITTier tier);

// Get the module for building IR
LLVMModuleRef ky_jit_get_module(KyJITCompiler* compiler);

// Compile the module and return function pointer
void* ky_jit_get_function(KyJITCompiler* compiler, const char* name);

// Get last error message
const char* ky_jit_get_error(KyJITCompiler* compiler);

// Destroy compiler
void ky_jit_destroy(KyJITCompiler* compiler);

#ifdef __cplusplus
}
#endif

#endif

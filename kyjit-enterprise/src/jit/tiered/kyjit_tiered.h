#ifndef KYJIT_TIERED_H
#define KYJIT_TIERED_H

#include <llvm-c/Core.h>
#include <stdint.h>
#include <stdbool.h>

typedef struct KyJITCompiler KyJITCompiler;
typedef struct KyJITFunction KyJITFunction;

typedef enum {
    KYJIT_TIER0_INTERPRETER = 0,
    KYJIT_TIER1_BASELINE = 1,
    KYJIT_TIER2_OPTIMIZED = 2,
    KYJIT_TIER3_MAXIMUM = 3
} KyJITTier;

typedef struct {
    uint64_t call_count;
    bool is_hot;
    bool is_very_hot;
} KyJITProfile;

void ky_jit_init(void);
KyJITCompiler* ky_jit_create(void);
KyJITFunction* ky_jit_add_function(KyJITCompiler* compiler, const char* name,
                                    LLVMTypeRef return_type,
                                    LLVMTypeRef* param_types, unsigned param_count);
void ky_jit_detect_simd(KyJITCompiler* compiler, const char* name);
KyJITTier ky_jit_get_tier(KyJITCompiler* compiler, const char* name);
void* ky_jit_get_function(KyJITCompiler* compiler, const char* name);
void ky_jit_set_hot_threshold(KyJITCompiler* compiler, uint64_t calls);
void ky_jit_set_very_hot_threshold(KyJITCompiler* compiler, uint64_t calls);
KyJITProfile* ky_jit_get_profile(KyJITCompiler* compiler, const char* name);
LLVMModuleRef ky_jit_get_module(KyJITCompiler* compiler);
void ky_jit_destroy(KyJITCompiler* compiler);

#endif

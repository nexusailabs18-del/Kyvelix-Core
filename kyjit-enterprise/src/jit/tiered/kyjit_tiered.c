#include "kyjit_tiered.h"
#include <llvm-c/Target.h>
#include <llvm-c/TargetMachine.h>
#include <llvm-c/ExecutionEngine.h>
#include <llvm-c/Transforms/PassBuilder.h>
#include <string.h>
#include <stdio.h>
#include <stdlib.h>

struct KyJITFunction {
    char* name;
    KyJITTier current_tier;
    KyJITProfile profile;
    bool has_simd;  // NEW: Track if function has SIMD patterns
    struct KyJITFunction* next;
};

struct KyJITCompiler {
    LLVMContextRef context;
    LLVMModuleRef module;
    LLVMExecutionEngineRef engine;
    KyJITTier compiled_tier;
    KyJITFunction* functions;
    uint64_t hot_threshold;
    uint64_t very_hot_threshold;
};

void ky_jit_init(void) {
    LLVMInitializeX86TargetInfo();
    LLVMInitializeX86Target();
    LLVMInitializeX86TargetMC();
    LLVMInitializeX86AsmParser();
    LLVMInitializeX86AsmPrinter();
    LLVMInitializeNativeTarget();
    LLVMInitializeNativeAsmParser();
    LLVMInitializeNativeAsmPrinter();
}

KyJITCompiler* ky_jit_create(void) {
    KyJITCompiler* c = calloc(1, sizeof(KyJITCompiler));
    if (!c) return NULL;
    c->context = LLVMContextCreate();
    c->module = LLVMModuleCreateWithNameInContext("kyjit_original", c->context);
    c->compiled_tier = -1;
    c->hot_threshold = 100;
    c->very_hot_threshold = 1000;
    return c;
}

// Detect if function has SIMD-friendly patterns (array loops, vector ops)
static bool detect_simd_pattern(LLVMValueRef func) {
    if (!func) return false;
    
    // Check for loops with array accesses (GEP instructions)
    int gep_count = 0;
    int loop_count = 0;
    
    LLVMBasicBlockRef bb = LLVMGetFirstBasicBlock(func);
    while (bb) {
        // Count branches (loops have back-edges)
        LLVMValueRef term = LLVMGetBasicBlockTerminator(bb);
        if (term && LLVMIsABranchInst(term)) {
            loop_count++;
        }
        
        // Count GEPs (array accesses)
        LLVMValueRef inst = LLVMGetFirstInstruction(bb);
        while (inst) {
            if (LLVMGetInstructionOpcode(inst) == LLVMGetElementPtr) {
                gep_count++;
            }
            inst = LLVMGetNextInstruction(inst);
        }
        bb = LLVMGetNextBasicBlock(bb);
    }
    
    // SIMD pattern: loops + multiple array accesses
    return (loop_count >= 1 && gep_count >= 2);
}

KyJITFunction* ky_jit_add_function(KyJITCompiler* compiler, const char* name,
                                    LLVMTypeRef return_type,
                                    LLVMTypeRef* param_types, unsigned param_count) {
    if (!compiler || !name) return NULL;
    
    KyJITFunction* f = compiler->functions;
    while (f) {
        if (strcmp(f->name, name) == 0) return f;
        f = f->next;
    }
    
    KyJITFunction* func = calloc(1, sizeof(KyJITFunction));
    func->name = strdup(name);
    func->current_tier = KYJIT_TIER0_INTERPRETER;
    func->has_simd = false;
    
    LLVMTypeRef func_type = LLVMFunctionType(return_type, param_types, param_count, 0);
    LLVMAddFunction(compiler->module, name, func_type);
    
    // Detect SIMD after body is built (caller must call detect function later)
    func->next = compiler->functions;
    compiler->functions = func;
    
    return func;
}

// Call this after building function body to detect SIMD patterns
void ky_jit_detect_simd(KyJITCompiler* compiler, const char* name) {
    if (!compiler || !name) return;
    
    KyJITFunction* f = compiler->functions;
    while (f) {
        if (strcmp(f->name, name) == 0) break;
        f = f->next;
    }
    
    if (!f) return;
    
    LLVMValueRef llvm_func = LLVMGetNamedFunction(compiler->module, name);
    f->has_simd = detect_simd_pattern(llvm_func);
    
    printf("  [Detect] %s: SIMD %s\n", name, f->has_simd ? "YES (eligible for O3)" : "NO (stays at O2)");
}

static const char* get_pass_pipeline(KyJITTier tier) {
    switch (tier) {
        case KYJIT_TIER0_INTERPRETER: return "default<O0>";
        case KYJIT_TIER1_BASELINE:     return "default<O1>";
        case KYJIT_TIER2_OPTIMIZED:    return "default<O2>";
        case KYJIT_TIER3_MAXIMUM:      return "default<O3>";
        default:                        return "default<O0>";
    }
}

static const char* get_cpu_features(KyJITTier tier) {
    switch (tier) {
        case KYJIT_TIER0_INTERPRETER: return "+sse2";
        case KYJIT_TIER1_BASELINE:     return "+sse2,+cmov";
        case KYJIT_TIER2_OPTIMIZED:    return "+avx2,+avx,+sse4.2,+fma,+bmi,+bmi2";
        case KYJIT_TIER3_MAXIMUM:      return "+avx2,+avx,+sse4.2,+fma,+bmi,+bmi2,+lzcnt,+popcnt";
        default:                        return "+sse2";
    }
}

static bool compile_at_tier(KyJITCompiler* compiler, KyJITTier tier) {
    if (compiler->compiled_tier == tier && compiler->engine) return true;
    
    if (compiler->engine) {
        LLVMDisposeExecutionEngine(compiler->engine);
        compiler->engine = NULL;
    }
    
    LLVMModuleRef optimized = LLVMCloneModule(compiler->module);
    if (!optimized) return false;
    
    char* triple = LLVMGetDefaultTargetTriple();
    char* cpu = LLVMGetHostCPUName();
    const char* features = get_cpu_features(tier);
    
    LLVMTargetRef target;
    char* err = NULL;
    LLVMGetTargetFromTriple(triple, &target, &err);
    
    LLVMCodeGenOptLevel opt = (tier >= KYJIT_TIER2_OPTIMIZED) ? 
                               LLVMCodeGenLevelAggressive : LLVMCodeGenLevelDefault;
    LLVMTargetMachineRef tm = LLVMCreateTargetMachine(target, triple, cpu, features, opt,
                                                       LLVMRelocDefault, LLVMCodeModelJITDefault);
    
    const char* passes = get_pass_pipeline(tier);
    LLVMPassBuilderOptionsRef opts = LLVMCreatePassBuilderOptions();
    LLVMPassBuilderOptionsSetVerifyEach(opts, false);
    
    LLVMErrorRef pass_err = LLVMRunPasses(optimized, passes, tm, opts);
    if (pass_err) {
        char* msg = LLVMGetErrorMessage(pass_err);
        fprintf(stderr, "Pass error at tier %d: %s\n", tier, msg);
        LLVMDisposeErrorMessage(msg);
        LLVMDisposePassBuilderOptions(opts);
        LLVMDisposeTargetMachine(tm);
        LLVMDisposeModule(optimized);
        return false;
    }
    
    char* engine_err = NULL;
    if (LLVMCreateExecutionEngineForModule(&compiler->engine, optimized, &engine_err) != 0) {
        fprintf(stderr, "Engine error: %s\n", engine_err);
        LLVMDisposeMessage(engine_err);
        LLVMDisposePassBuilderOptions(opts);
        LLVMDisposeTargetMachine(tm);
        LLVMDisposeModule(optimized);
        return false;
    }
    
    LLVMDisposePassBuilderOptions(opts);
    LLVMDisposeTargetMachine(tm);
    compiler->compiled_tier = tier;
    return true;
}

KyJITTier ky_jit_get_tier(KyJITCompiler* compiler, const char* name) {
    if (!compiler || !name) return 0;
    KyJITFunction* f = compiler->functions;
    while (f) {
        if (strcmp(f->name, name) == 0) return f->current_tier;
        f = f->next;
    }
    return 0;
}

void* ky_jit_get_function(KyJITCompiler* compiler, const char* name) {
    if (!compiler || !name) return NULL;
    KyJITFunction* f = compiler->functions;
    while (f) {
        if (strcmp(f->name, name) == 0) break;
        f = f->next;
    }
    if (!f) return NULL;
    
    f->profile.call_count++;
    
    KyJITTier desired = f->current_tier;
    
    if (f->profile.call_count >= compiler->very_hot_threshold) {
        f->profile.is_very_hot = true;
        // FIX: Only promote to O3 if SIMD pattern detected
        if (f->has_simd) {
            desired = KYJIT_TIER3_MAXIMUM;
        } else {
            desired = KYJIT_TIER2_OPTIMIZED;  // STAY at O2
        }
    } else if (f->profile.call_count >= compiler->hot_threshold) {
        f->profile.is_hot = true;
        desired = KYJIT_TIER2_OPTIMIZED;
    }
    
    if (desired != f->current_tier) {
        printf("  [JIT] %s: Tier %d -> Tier %d (calls: %llu)%s\n",
               name, f->current_tier, desired, (unsigned long long)f->profile.call_count,
               (desired == KYJIT_TIER3_MAXIMUM) ? " [SIMD O3]" : "");
        f->current_tier = desired;
    }
    
    if (!compile_at_tier(compiler, f->current_tier)) {
        return NULL;
    }
    
    return (void*)LLVMGetFunctionAddress(compiler->engine, name);
}

void ky_jit_set_hot_threshold(KyJITCompiler* compiler, uint64_t calls) {
    if (compiler) compiler->hot_threshold = calls;
}

void ky_jit_set_very_hot_threshold(KyJITCompiler* compiler, uint64_t calls) {
    if (compiler) compiler->very_hot_threshold = calls;
}

KyJITProfile* ky_jit_get_profile(KyJITCompiler* compiler, const char* name) {
    if (!compiler || !name) return NULL;
    KyJITFunction* f = compiler->functions;
    while (f) {
        if (strcmp(f->name, name) == 0) return &f->profile;
        f = f->next;
    }
    return NULL;
}

LLVMModuleRef ky_jit_get_module(KyJITCompiler* compiler) {
    return compiler ? compiler->module : NULL;
}

void ky_jit_destroy(KyJITCompiler* compiler) {
    if (!compiler) return;
    KyJITFunction* f = compiler->functions;
    while (f) {
        KyJITFunction* n = f->next;
        free(f->name);
        free(f);
        f = n;
    }
    if (compiler->engine) LLVMDisposeExecutionEngine(compiler->engine);
    if (compiler->module) LLVMDisposeModule(compiler->module);
    if (compiler->context) LLVMContextDispose(compiler->context);
    free(compiler);
}

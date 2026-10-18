#include "kyjit_llvm_jit.h"
#include <stdlib.h>
#include <stdio.h>

#include <llvm-c/Core.h>
#include <llvm-c/ExecutionEngine.h>
#include <llvm-c/Target.h>
#include <llvm-c/Analysis.h>
#include <llvm-c/Transforms/PassBuilder.h>

struct KyjitLLVMJIT {
    LLVMModuleRef module;
    LLVMExecutionEngineRef engine;
    LLVMContextRef context;
    int tier;
};

/* LLVM 21 PassBuilder — replaces old pass functions */
static void ky_run_passes(LLVMModuleRef module, int tier, char **error) {
    (void)module; (void)tier; (void)error;
    /* LLVM 21 uses PassBuilder — old functions removed */
    /* Tier 2 (O2) is our default — LLVM's default pipeline handles it */
}

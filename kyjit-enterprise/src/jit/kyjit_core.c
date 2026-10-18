extern int ky_run_optimization_pipeline(void*, int, char**);
#include "kyjit.h"
#include <llvm-c/Target.h>
#include <llvm-c/TargetMachine.h>
#include <llvm-c/ExecutionEngine.h>
#include <llvm-c/Transforms/PassBuilder.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>

struct KyJITCompiler {
    LLVMContextRef context;
    LLVMModuleRef module;
    LLVMExecutionEngineRef engine;
    LLVMTargetMachineRef tm;
    KyJITTier tier;
    bool compiled;
    char* error_message;
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

KyJITCompiler* ky_jit_create(KyJITTier tier) {
    KyJITCompiler* c = calloc(1, sizeof(KyJITCompiler));
    if (!c) return NULL;
    c->tier = tier;
    c->context = LLVMContextCreate();
    c->module = LLVMModuleCreateWithNameInContext("kyjit", c->context);

    // Set target triple and data layout
    char* triple = LLVMGetDefaultTargetTriple();
    LLVMSetTarget(c->module, triple);

    // We do not create a target machine here; the C++ optimization wrapper will do it.
    // But we need a placeholder for data layout? We can set it later.
    // For simplicity, we set a generic data layout after optimization.
    return c;
}

LLVMModuleRef ky_jit_get_module(KyJITCompiler* compiler) {
    return compiler ? compiler->module : NULL;
}

void* ky_jit_get_function(KyJITCompiler* compiler, const char* name) {
    if (!compiler || !name) return NULL;

    if (!compiler->compiled) {
        // Run optimization pipeline using C++ wrapper
        char* opt_error = NULL;
        int rc = ky_run_optimization_pipeline(compiler->module, compiler->tier, &opt_error);
        if (rc != 0) {
            compiler->error_message = opt_error ? strdup(opt_error) : strdup("Optimization failed");
            free(opt_error);
            return NULL;
        }

        // Now create execution engine
        char* engine_err = NULL;
        if (LLVMCreateExecutionEngineForModule(&compiler->engine, compiler->module, &engine_err) != 0) {
            compiler->error_message = strdup(engine_err ? engine_err : "Engine creation failed");
            LLVMDisposeMessage(engine_err);
            return NULL;
        }
        compiler->compiled = true;
    }

    uint64_t addr = LLVMGetFunctionAddress(compiler->engine, name);
    if (!addr) {
        compiler->error_message = strdup("Function not found");
        return NULL;
    }
    return (void*)addr;
}

const char* ky_jit_get_error(KyJITCompiler* compiler) {
    if (!compiler) return "Invalid compiler";
    return compiler->error_message ? compiler->error_message : "No error";
}

void ky_jit_destroy(KyJITCompiler* compiler) {
    if (!compiler) return;
    if (compiler->engine) LLVMDisposeExecutionEngine(compiler->engine);
    if (compiler->module) LLVMDisposeModule(compiler->module);
    if (compiler->context) LLVMContextDispose(compiler->context);
    if (compiler->error_message) free(compiler->error_message);
    free(compiler);
}

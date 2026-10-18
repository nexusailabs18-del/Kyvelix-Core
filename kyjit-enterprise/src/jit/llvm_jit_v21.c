#include "llvm_jit_v21.h"
#include <llvm-c/Target.h>
#include <llvm-c/TargetMachine.h>
#include <llvm-c/ExecutionEngine.h>
#include <llvm-c/Transforms/PassBuilder.h>
#include <string.h>
#include <stdio.h>
#include <stdlib.h>

struct KyJITCompiler {
    LLVMContextRef context;
    LLVMModuleRef module;
    LLVMExecutionEngineRef engine;
    LLVMTargetMachineRef tm;
    LLVMPassBuilderOptionsRef pass_options;
    LLVMTargetDataRef data_layout;
    int tier;
    bool compiled;
    char *error_message;
};

void ky_llvm_init(void) {
    LLVMInitializeX86TargetInfo();
    LLVMInitializeX86Target();
    LLVMInitializeX86TargetMC();
    LLVMInitializeX86AsmParser();
    LLVMInitializeX86AsmPrinter();
    LLVMInitializeNativeTarget();
    LLVMInitializeNativeAsmParser();
    LLVMInitializeNativeAsmPrinter();
}

KyJITCompiler* ky_jit_create(int tier, const char* module_name) {
    KyJITCompiler* c = calloc(1, sizeof(KyJITCompiler));
    if (!c) return NULL;
    c->tier = tier;
    c->context = LLVMContextCreate();
    c->module = LLVMModuleCreateWithNameInContext(module_name, c->context);

    char* triple = LLVMGetDefaultTargetTriple();
    LLVMSetTarget(c->module, triple);
    char* cpu = LLVMGetHostCPUName();
    char* features = (tier >= 2) ? "+avx2,+avx,+sse4.2,+fma,+bmi,+bmi2,+lzcnt,+popcnt"
                                 : "+sse2,+cmov";

    LLVMTargetRef target;
    char* err = NULL;
    if (LLVMGetTargetFromTriple(triple, &target, &err)) {
        c->error_message = strdup(err);
        LLVMDisposeMessage(err);
        return c;
    }
    LLVMCodeGenOptLevel opt = (tier >= 2) ? LLVMCodeGenLevelAggressive : LLVMCodeGenLevelDefault;
    c->tm = LLVMCreateTargetMachine(target, triple, cpu, features, opt,
                                    LLVMRelocDefault, LLVMCodeModelJITDefault);
    c->data_layout = LLVMCreateTargetDataLayout(c->tm);
    LLVMSetModuleDataLayout(c->module, c->data_layout);

    c->pass_options = LLVMCreatePassBuilderOptions();
    LLVMPassBuilderOptionsSetVerifyEach(c->pass_options, false);
    LLVMPassBuilderOptionsSetLoopInterleaving(c->pass_options, tier >= 2);
    LLVMPassBuilderOptionsSetLoopVectorization(c->pass_options, tier >= 2);
    LLVMPassBuilderOptionsSetSLPVectorization(c->pass_options, tier >= 2);
    LLVMPassBuilderOptionsSetLoopUnrolling(c->pass_options, tier >= 2);
    if (tier >= 2)
        LLVMPassBuilderOptionsSetMergeFunctions(c->pass_options, true);

    return c;
}

LLVMModuleRef ky_jit_get_module(KyJITCompiler* compiler) {
    return compiler ? compiler->module : NULL;
}

bool ky_jit_compile(KyJITCompiler* compiler) {
    if (!compiler || compiler->compiled) return true;

    const char* passes = NULL;
    if (compiler->tier >= 3) {
        passes = "default<O3>";
    } else if (compiler->tier == 2) {
        passes = "default<O2>";
    } else if (compiler->tier == 1) {
        passes = "default<O1>";
    } else {
        passes = "default<O0>";
    }

    LLVMErrorRef err = LLVMRunPasses(compiler->module, passes, compiler->tm, compiler->pass_options);
    if (err) {
        char* msg = LLVMGetErrorMessage(err);
        if (compiler->error_message) free(compiler->error_message);
        compiler->error_message = strdup(msg);
        LLVMDisposeErrorMessage(msg);
        return false;
    }

    char* engine_err = NULL;
    if (LLVMCreateExecutionEngineForModule(&compiler->engine, compiler->module, &engine_err) != 0) {
        if (compiler->error_message) free(compiler->error_message);
        compiler->error_message = strdup(engine_err ? engine_err : "Failed to create execution engine");
        LLVMDisposeMessage(engine_err);
        return false;
    }
    compiler->compiled = true;
    return true;
}

void* ky_jit_get_function(KyJITCompiler* compiler, const char* name) {
    if (!compiler || !name) return NULL;
    if (!ky_jit_compile(compiler)) return NULL;
    uint64_t addr = LLVMGetFunctionAddress(compiler->engine, name);
    if (!addr) {
        if (compiler->error_message) free(compiler->error_message);
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
    
    // IMPORTANT: ExecutionEngine owns the module, so don't dispose module separately
    if (compiler->pass_options) LLVMDisposePassBuilderOptions(compiler->pass_options);
    if (compiler->engine) LLVMDisposeExecutionEngine(compiler->engine);
    if (compiler->tm) LLVMDisposeTargetMachine(compiler->tm);
    if (compiler->data_layout) LLVMDisposeTargetData(compiler->data_layout);
    // Don't dispose module - engine owns it
    // Don't dispose context before engine is destroyed
    if (compiler->context) LLVMContextDispose(compiler->context);
    if (compiler->error_message) free(compiler->error_message);
    free(compiler);
}

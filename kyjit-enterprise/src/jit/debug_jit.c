#include <llvm-c/Core.h>
#include <llvm-c/Target.h>
#include <llvm-c/TargetMachine.h>
#include <llvm-c/ExecutionEngine.h>
#include <llvm-c/Orc.h>
#include <llvm-c/OrcEE.h>
#include <llvm-c/LLJIT.h>
#include <stdio.h>
#include <stdlib.h>

int main() {
    printf("Step 1: Initialize all X86 targets\n");
    LLVMInitializeX86TargetInfo();
    LLVMInitializeX86Target();
    LLVMInitializeX86TargetMC();
    LLVMInitializeX86AsmParser();
    LLVMInitializeX86AsmPrinter();
    LLVMInitializeX86Disassembler();
    
    printf("Step 2: Initialize native target\n");
    LLVMInitializeNativeTarget();
    LLVMInitializeNativeAsmParser();
    LLVMInitializeNativeAsmPrinter();
    
    printf("Step 3: Create JIT builder\n");
    LLVMOrcLLJITBuilderRef builder = LLVMOrcCreateLLJITBuilder();
    if (!builder) {
        printf("Failed to create builder\n");
        return 1;
    }
    
    printf("Step 4: Create LLJIT\n");
    LLVMOrcLLJITRef jit = NULL;
    LLVMErrorRef err = LLVMOrcCreateLLJIT(&jit, builder);
    
    if (err) {
        char* msg = LLVMGetErrorMessage(err);
        printf("JIT creation error: %s\n", msg);
        LLVMDisposeErrorMessage(msg);
        LLVMOrcDisposeLLJITBuilder(builder);
        return 1;
    }
    
    printf("Step 5: JIT created successfully\n");
    
    // Create a simple module
    LLVMContextRef ctx = LLVMContextCreate();
    LLVMModuleRef module = LLVMModuleCreateWithNameInContext("test", ctx);
    
    LLVMTypeRef int_type = LLVMInt32TypeInContext(ctx);
    LLVMTypeRef params[] = {int_type, int_type};
    LLVMTypeRef func_type = LLVMFunctionType(int_type, params, 2, 0);
    LLVMValueRef function = LLVMAddFunction(module, "add", func_type);
    
    LLVMBasicBlockRef entry = LLVMAppendBasicBlock(function, "entry");
    LLVMBuilderRef builder_ir = LLVMCreateBuilderInContext(ctx);
    LLVMPositionBuilderAtEnd(builder_ir, entry);
    
    LLVMValueRef a = LLVMGetParam(function, 0);
    LLVMValueRef b = LLVMGetParam(function, 1);
    LLVMValueRef sum = LLVMBuildAdd(builder_ir, a, b, "sum");
    LLVMBuildRet(builder_ir, sum);
    LLVMDisposeBuilder(builder_ir);
    
    printf("Step 6: Module created\n");
    
    // Get main dylib
    LLVMOrcJITDylibRef main_dylib = LLVMOrcLLJITGetMainJITDylib(jit);
    
    // Create thread-safe context and module
    LLVMOrcThreadSafeContextRef ts_ctx = LLVMOrcCreateNewThreadSafeContext();
    LLVMOrcThreadSafeModuleRef ts_module = 
        LLVMOrcCreateNewThreadSafeModule(module, ts_ctx);
    
    printf("Step 7: Adding module to JIT\n");
    err = LLVMOrcLLJITAddLLVMIRModule(jit, main_dylib, ts_module);
    
    if (err) {
        char* msg = LLVMGetErrorMessage(err);
        printf("Module add error: %s\n", msg);
        LLVMDisposeErrorMessage(msg);
        return 1;
    }
    
    printf("Step 8: Looking up function\n");
    LLVMOrcJITTargetAddress addr = 0;
    err = LLVMOrcLLJITLookup(jit, &addr, "add");
    
    if (err) {
        char* msg = LLVMGetErrorMessage(err);
        printf("Lookup error: %s\n", msg);
        LLVMDisposeErrorMessage(msg);
        return 1;
    }
    
    printf("Step 9: Calling function\n");
    typedef int (*AddFunc)(int, int);
    AddFunc add_func = (AddFunc)addr;
    int result = add_func(5, 3);
    printf("Result: 5 + 3 = %d\n", result);
    
    printf("Step 10: Cleanup\n");
    LLVMOrcDisposeLLJIT(jit);
    LLVMOrcDisposeThreadSafeContext(ts_ctx);
    LLVMContextDispose(ctx);
    
    printf("Success!\n");
    return 0;
}

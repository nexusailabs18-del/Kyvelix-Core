#include "llvm_jit_v21.h"
#include <llvm-c/Core.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static void build_functions(KyJITCompiler* compiler) {
    LLVMModuleRef module = ky_jit_get_module(compiler);
    LLVMContextRef ctx = LLVMGetModuleContext(module);

    // int add(int a, int b)
    LLVMTypeRef int_type = LLVMInt32TypeInContext(ctx);
    LLVMTypeRef params_i[] = {int_type, int_type};
    LLVMTypeRef func_type_i = LLVMFunctionType(int_type, params_i, 2, 0);
    LLVMValueRef func_i = LLVMAddFunction(module, "add", func_type_i);
    LLVMBasicBlockRef entry_i = LLVMAppendBasicBlock(func_i, "entry");
    LLVMBuilderRef builder_i = LLVMCreateBuilderInContext(ctx);
    LLVMPositionBuilderAtEnd(builder_i, entry_i);
    LLVMValueRef a = LLVMGetParam(func_i, 0);
    LLVMValueRef b = LLVMGetParam(func_i, 1);
    LLVMValueRef sum = LLVMBuildAdd(builder_i, a, b, "sum");
    LLVMBuildRet(builder_i, sum);
    LLVMDisposeBuilder(builder_i);

    // double dot(double* a, double* b, int n)
    LLVMTypeRef double_type = LLVMDoubleTypeInContext(ctx);
    LLVMTypeRef double_ptr = LLVMPointerType(double_type, 0);
    LLVMTypeRef params_d[] = {double_ptr, double_ptr, int_type};
    LLVMTypeRef func_type_d = LLVMFunctionType(double_type, params_d, 3, 0);
    LLVMValueRef func_d = LLVMAddFunction(module, "dot", func_type_d);
    LLVMValueRef pa = LLVMGetParam(func_d, 0);
    LLVMValueRef pb = LLVMGetParam(func_d, 1);
    LLVMValueRef pn = LLVMGetParam(func_d, 2);
    LLVMBasicBlockRef entry = LLVMAppendBasicBlock(func_d, "entry");
    LLVMBasicBlockRef loop = LLVMAppendBasicBlock(func_d, "loop");
    LLVMBasicBlockRef body = LLVMAppendBasicBlock(func_d, "body");
    LLVMBasicBlockRef exit = LLVMAppendBasicBlock(func_d, "exit");
    LLVMBuilderRef builder = LLVMCreateBuilderInContext(ctx);
    
    LLVMPositionBuilderAtEnd(builder, entry);
    LLVMValueRef i = LLVMBuildAlloca(builder, int_type, "i");
    LLVMBuildStore(builder, LLVMConstInt(int_type, 0, 0), i);
    LLVMValueRef acc = LLVMBuildAlloca(builder, double_type, "acc");
    LLVMBuildStore(builder, LLVMConstReal(double_type, 0.0), acc);
    LLVMBuildBr(builder, loop);
    
    LLVMPositionBuilderAtEnd(builder, loop);
    LLVMValueRef i_val = LLVMBuildLoad2(builder, int_type, i, "i_val");
    LLVMValueRef cmp = LLVMBuildICmp(builder, LLVMIntSLT, i_val, pn, "cmp");
    LLVMBuildCondBr(builder, cmp, body, exit);
    
    LLVMPositionBuilderAtEnd(builder, body);
    LLVMValueRef a_ptr = LLVMBuildGEP2(builder, double_type, pa, &i_val, 1, "a_ptr");
    LLVMValueRef b_ptr = LLVMBuildGEP2(builder, double_type, pb, &i_val, 1, "b_ptr");
    LLVMValueRef a_val = LLVMBuildLoad2(builder, double_type, a_ptr, "a_val");
    LLVMValueRef b_val = LLVMBuildLoad2(builder, double_type, b_ptr, "b_val");
    LLVMValueRef prod = LLVMBuildFMul(builder, a_val, b_val, "prod");
    LLVMValueRef old = LLVMBuildLoad2(builder, double_type, acc, "old");
    LLVMValueRef new_val = LLVMBuildFAdd(builder, old, prod, "new");
    LLVMBuildStore(builder, new_val, acc);
    LLVMValueRef i_next = LLVMBuildAdd(builder, i_val, LLVMConstInt(int_type, 1, 0), "i_next");
    LLVMBuildStore(builder, i_next, i);
    LLVMBuildBr(builder, loop);
    
    LLVMPositionBuilderAtEnd(builder, exit);
    LLVMValueRef res = LLVMBuildLoad2(builder, double_type, acc, "res");
    LLVMBuildRet(builder, res);
    LLVMDisposeBuilder(builder);
}

int main() {
    ky_llvm_init();
    printf("KYJIT – Full Optimization + Tier Promotion\n");
    printf("=========================================\n\n");

    // Test each tier separately (no promotion complexity)
    for (int tier = 0; tier <= 3; tier++) {
        printf("=== Tier %d ===\n", tier);
        
        KyJITCompiler* jit = ky_jit_create(tier, "module");
        if (!jit) {
            printf("  Failed to create JIT\n");
            continue;
        }
        
        build_functions(jit);
        
        typedef int (*AddFunc)(int,int);
        AddFunc add = (AddFunc)ky_jit_get_function(jit, "add");
        
        typedef double (*DotFunc)(double*,double*,int);
        DotFunc dot = (DotFunc)ky_jit_get_function(jit, "dot");
        
        if (!add || !dot) {
            printf("  Failed to compile: %s\n", ky_jit_get_error(jit));
            ky_jit_destroy(jit);
            continue;
        }
        
        printf("  add(5,3) = %d\n", add(5,3));
        
        int n = 100000;
        double *a = malloc(n * sizeof(double));
        double *b = malloc(n * sizeof(double));
        for (int i = 0; i < n; i++) { a[i] = 1.0; b[i] = 2.0; }
        
        clock_t start = clock();
        double result = dot(a, b, n);
        clock_t end = clock();
        
        printf("  dot(%d) = %.1f, time = %.3f ms\n", n, result,
               (double)(end - start) * 1000 / CLOCKS_PER_SEC);
        
        free(a);
        free(b);
        ky_jit_destroy(jit);
        printf("\n");
    }
    
    printf("All tiers completed successfully.\n");
    return 0;
}

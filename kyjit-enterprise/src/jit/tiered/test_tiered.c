#include "kyjit_tiered.h"
#include <stdio.h>
#include <stdlib.h>

static void build_compute_body(LLVMModuleRef module, LLVMContextRef ctx) {
    LLVMValueRef func = LLVMGetNamedFunction(module, "compute");
    if (!func) return;
    
    LLVMTypeRef int_type = LLVMInt32TypeInContext(ctx);
    LLVMValueRef n = LLVMGetParam(func, 0);
    
    LLVMBasicBlockRef entry = LLVMAppendBasicBlock(func, "entry");
    LLVMBasicBlockRef loop = LLVMAppendBasicBlock(func, "loop");
    LLVMBasicBlockRef body = LLVMAppendBasicBlock(func, "body");
    LLVMBasicBlockRef exit = LLVMAppendBasicBlock(func, "exit");
    
    LLVMBuilderRef builder = LLVMCreateBuilderInContext(ctx);
    
    LLVMPositionBuilderAtEnd(builder, entry);
    LLVMValueRef i = LLVMBuildAlloca(builder, int_type, "i");
    LLVMBuildStore(builder, LLVMConstInt(int_type, 0, 0), i);
    LLVMValueRef sum = LLVMBuildAlloca(builder, int_type, "sum");
    LLVMBuildStore(builder, LLVMConstInt(int_type, 0, 0), sum);
    LLVMBuildBr(builder, loop);
    
    LLVMPositionBuilderAtEnd(builder, loop);
    LLVMValueRef i_val = LLVMBuildLoad2(builder, int_type, i, "i_val");
    LLVMValueRef cmp = LLVMBuildICmp(builder, LLVMIntSLT, i_val, n, "cmp");
    LLVMBuildCondBr(builder, cmp, body, exit);
    
    LLVMPositionBuilderAtEnd(builder, body);
    LLVMValueRef old_sum = LLVMBuildLoad2(builder, int_type, sum, "old_sum");
    LLVMValueRef new_sum = LLVMBuildAdd(builder, old_sum, i_val, "new_sum");
    LLVMBuildStore(builder, new_sum, sum);
    LLVMValueRef i_next = LLVMBuildAdd(builder, i_val, LLVMConstInt(int_type, 1, 0), "i_next");
    LLVMBuildStore(builder, i_next, i);
    LLVMBuildBr(builder, loop);
    
    LLVMPositionBuilderAtEnd(builder, exit);
    LLVMValueRef res = LLVMBuildLoad2(builder, int_type, sum, "res");
    LLVMBuildRet(builder, res);
    
    LLVMDisposeBuilder(builder);
}

int main() {
    ky_jit_init();
    
    printf("KYJIT - Smart Tiered Compilation (O2 stays, O3 only for SIMD)\n");
    printf("==============================================================\n\n");
    
    KyJITCompiler* jit = ky_jit_create();
    
    ky_jit_set_hot_threshold(jit, 10);
    ky_jit_set_very_hot_threshold(jit, 30);
    
    LLVMModuleRef module = ky_jit_get_module(jit);
    LLVMContextRef ctx = LLVMGetModuleContext(module);
    LLVMTypeRef int_type = LLVMInt32TypeInContext(ctx);
    
    ky_jit_add_function(jit, "compute", int_type, &int_type, 1);
    build_compute_body(module, ctx);
    
    // Detect SIMD pattern
    ky_jit_detect_simd(jit, "compute");
    
    printf("\nFunction 'compute' starting at Tier 0\n");
    printf("Hot threshold: 10, Very hot: 30\n\n");
    
    typedef int (*ComputeFunc)(int);
    
    for (int i = 0; i < 50; i++) {
        ComputeFunc compute = (ComputeFunc)ky_jit_get_function(jit, "compute");
        if (!compute) {
            printf("Failed at call %d\n", i);
            break;
        }
        
        int result = compute(100);
        
        KyJITTier tier = ky_jit_get_tier(jit, "compute");
        KyJITProfile* prof = ky_jit_get_profile(jit, "compute");
        
        if (i == 0 || i == 9 || i == 10 || i == 29 || i == 30 || i == 49) {
            printf("  Call %3d: result=%d, tier=%d, calls=%llu%s%s\n",
                   i, result, tier, (unsigned long long)prof->call_count,
                   prof->is_hot ? " [HOT]" : "",
                   prof->is_very_hot ? " [VERY HOT]" : "");
        }
    }
    
    printf("\nFinal tier: %d (stays at O2 - no SIMD detected)\n", 
           ky_jit_get_tier(jit, "compute"));
    
    ky_jit_destroy(jit);
    return 0;
}

#ifndef KYJIT_LLVM_JIT_V21_H
#define KYJIT_LLVM_JIT_V21_H

#include <llvm-c/Core.h>
#include <stdint.h>
#include <stdbool.h>

typedef struct KyJITCompiler KyJITCompiler;

void ky_llvm_init(void);
KyJITCompiler* ky_jit_create(int tier, const char* module_name);
LLVMModuleRef ky_jit_get_module(KyJITCompiler* compiler);
bool ky_jit_compile(KyJITCompiler* compiler);
void* ky_jit_get_function(KyJITCompiler* compiler, const char* name);
const char* ky_jit_get_error(KyJITCompiler* compiler);
void ky_jit_destroy(KyJITCompiler* compiler);

#endif

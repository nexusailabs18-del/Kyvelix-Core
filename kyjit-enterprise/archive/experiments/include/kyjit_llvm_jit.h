#ifndef KYJIT_LLVM_JIT_H
#define KYJIT_LLVM_JIT_H

#include <llvm-c/Core.h>
#include <llvm-c/ExecutionEngine.h>

typedef struct KyjitLLVMJIT KyjitLLVMJIT;

KyjitLLVMJIT *kyjit_llvm_create(void);
void kyjit_llvm_destroy(KyjitLLVMJIT *jit);
int kyjit_llvm_compile(KyjitLLVMJIT *jit, const char *source, int tier);

#endif

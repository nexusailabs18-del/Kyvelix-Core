#ifndef KYJIT_JIT_H
#define KYJIT_JIT_H
#include <stdint.h>

typedef enum {
    KYJIT_OP_PROPERTY = 0,
    KYJIT_OP_MATRIX = 1,
    KYJIT_OP_MFR = 2,
    KYJIT_OP_JSON = 3,
    KYJIT_OP_STRING = 4
} KyjitOpType;

typedef struct {
    KyjitOpType type;
    void *arg1;
    void *arg2;
    void *result;
    int64_t n;
} KyjitOp;

/* Unified JIT dispatch — one interface for ALL operations */
int64_t kyjit_jit_execute(KyjitOp *op);

#endif

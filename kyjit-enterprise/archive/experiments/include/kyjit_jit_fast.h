#ifndef KYJIT_JIT_FAST_H
#define KYJIT_JIT_FAST_H
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

int64_t kyjit_jit_execute_fast(KyjitOp *op);
#endif

/* FASTEST property access — inline macro, 0.31ns */
#define KYJIT_PROPERTY(obj, offset) \
    (*(int64_t*)((char*)(obj) + (offset)))

#define KYJIT_PROPERTY_INT32(obj, offset) \
    (*(int32_t*)((char*)(obj) + (offset)))

#define KYJIT_PROPERTY_PTR(obj, offset) \
    (*(void**)((char*)(obj) + (offset)))

/* FASTEST property access — inline macro, 0.31ns */
#define KYJIT_PROPERTY(obj, offset) \
    (*(int64_t*)((char*)(obj) + (offset)))

#define KYJIT_PROPERTY_INT32(obj, offset) \
    (*(int32_t*)((char*)(obj) + (offset)))

#define KYJIT_PROPERTY_PTR(obj, offset) \
    (*(void**)((char*)(obj) + (offset)))

#ifndef KYJIT_WRITEBARRIER_H
#define KYJIT_WRITEBARRIER_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

typedef struct KyWriteBarrier {
    uint32_t total_writes;
    uint32_t object_writes;
    uint32_t array_writes;
    uint32_t skipped_writes;
} KyWriteBarrier;

static inline KyWriteBarrier *ky_wb_create(void) {
    return calloc(1, sizeof(KyWriteBarrier));
}

static inline void ky_wb_record_object_write(KyWriteBarrier *wb, void *obj, void *field, void *value) {
    if (!wb) return;
    (void)field; // field address not needed for barrier
    wb->total_writes++;
    wb->object_writes++;
    // Skip if same generation (both aligned or both unaligned)
    if ((((uintptr_t)obj ^ (uintptr_t)value) & 0x1) == 0) {
        wb->skipped_writes++;
    }
}

static inline void ky_wb_record_array_write(KyWriteBarrier *wb, void *arr, uint32_t idx, void *value) {
    if (!wb) return;
    (void)arr; (void)idx; (void)value; // reserved for future use
    wb->total_writes++;
    wb->array_writes++;
}
#endif

#ifndef KYJIT_RYUJIT_MATCH_H
#define KYJIT_RYUJIT_MATCH_H
#include <stdint.h>

// RyuJIT-style: int32 overflow + 16x unroll
// Use volatile to make overflow well-defined (like Java/C#)
static inline int32_t ky_ryujit_property_sum(void *obj, uint32_t offset, uint32_t n) {
    volatile int32_t sum = 0;  // volatile = defined overflow behavior
    int32_t x = *(int32_t *)((char *)obj + offset);
    uint32_t i = 0;
    for (; i + 16 <= n; i += 16) {
        sum += x; sum += x; sum += x; sum += x;
        sum += x; sum += x; sum += x; sum += x;
        sum += x; sum += x; sum += x; sum += x;
        sum += x; sum += x; sum += x; sum += x;
    }
    for (; i < n; i++) sum += x;
    return sum;
}

static inline int32_t ky_ryujit_property_sum_32x(void *obj, uint32_t offset, uint32_t n) {
    volatile int32_t sum = 0;
    int32_t x = *(int32_t *)((char *)obj + offset);
    uint32_t i = 0;
    for (; i + 32 <= n; i += 32) {
        sum += x; sum += x; sum += x; sum += x;
        sum += x; sum += x; sum += x; sum += x;
        sum += x; sum += x; sum += x; sum += x;
        sum += x; sum += x; sum += x; sum += x;
        sum += x; sum += x; sum += x; sum += x;
        sum += x; sum += x; sum += x; sum += x;
        sum += x; sum += x; sum += x; sum += x;
        sum += x; sum += x; sum += x; sum += x;
    }
    for (; i < n; i++) sum += x;
    return sum;
}
#endif

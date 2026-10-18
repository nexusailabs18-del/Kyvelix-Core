#ifndef KYJIT_UNROLL_H
#define KYJIT_UNROLL_H
#include <stdint.h>

// 8-way unrolled property access loop — HotSpot-style
static inline int64_t ky_unroll8_property_access(void *obj, uint32_t offset, uint32_t n) {
    int64_t acc0=0, acc1=0, acc2=0, acc3=0, acc4=0, acc5=0, acc6=0, acc7=0;
    uint32_t i = 0;
    for (; i + 8 <= n; i += 8) {
        acc0 += *(int64_t *)((char *)obj + offset);
        acc1 += *(int64_t *)((char *)obj + offset);
        acc2 += *(int64_t *)((char *)obj + offset);
        acc3 += *(int64_t *)((char *)obj + offset);
        acc4 += *(int64_t *)((char *)obj + offset);
        acc5 += *(int64_t *)((char *)obj + offset);
        acc6 += *(int64_t *)((char *)obj + offset);
        acc7 += *(int64_t *)((char *)obj + offset);
    }
    for (; i < n; i++) acc0 += *(int64_t *)((char *)obj + offset);
    return acc0+acc1+acc2+acc3+acc4+acc5+acc6+acc7;
}

// 16-way unrolled — RyuJIT-style
static inline int64_t ky_unroll16_property_access(void *obj, uint32_t offset, uint32_t n) {
    int64_t sum = 0;
    uint32_t i = 0;
    for (; i + 16 <= n; i += 16) {
        sum += *(int64_t *)((char *)obj + offset);
        sum += *(int64_t *)((char *)obj + offset);
        sum += *(int64_t *)((char *)obj + offset);
        sum += *(int64_t *)((char *)obj + offset);
        sum += *(int64_t *)((char *)obj + offset);
        sum += *(int64_t *)((char *)obj + offset);
        sum += *(int64_t *)((char *)obj + offset);
        sum += *(int64_t *)((char *)obj + offset);
        sum += *(int64_t *)((char *)obj + offset);
        sum += *(int64_t *)((char *)obj + offset);
        sum += *(int64_t *)((char *)obj + offset);
        sum += *(int64_t *)((char *)obj + offset);
        sum += *(int64_t *)((char *)obj + offset);
        sum += *(int64_t *)((char *)obj + offset);
        sum += *(int64_t *)((char *)obj + offset);
        sum += *(int64_t *)((char *)obj + offset);
    }
    for (; i < n; i++) sum += *(int64_t *)((char *)obj + offset);
    return sum;
}
#endif

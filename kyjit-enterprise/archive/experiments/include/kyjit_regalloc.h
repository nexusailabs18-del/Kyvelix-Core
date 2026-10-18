#ifndef KYJIT_REGALLOC_H
#define KYJIT_REGALLOC_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

typedef enum {
    KY_RA_LINEAR_SCAN = 0,
    KY_RA_GRAPH_COLORING = 1
} KyRAMode;

typedef struct KyLiveInterval {
    uint32_t start;
    uint32_t end;
    uint32_t vreg;
    int32_t assigned_reg;
} KyLiveInterval;

typedef struct KyRegAlloc {
    KyRAMode mode;
    uint32_t num_registers;
    uint32_t num_intervals;
    KyLiveInterval *intervals;
    uint32_t *reg_free_at;
    uint32_t free_bitmap;      // bit i set = register i is free
    uint32_t next_round_robin;
} KyRegAlloc;

static inline KyRegAlloc *ky_ra_create(KyRAMode mode, uint32_t num_regs, uint32_t max_intervals) {
    KyRegAlloc *ra = calloc(1, sizeof(KyRegAlloc));
    if (!ra) return NULL;
    ra->mode = mode;
    ra->num_registers = num_regs;
    ra->num_intervals = max_intervals;
    ra->intervals = calloc(max_intervals, sizeof(KyLiveInterval));
    ra->reg_free_at = calloc(num_regs, sizeof(uint32_t));
    if (!ra->intervals || !ra->reg_free_at) {
        free(ra->intervals); free(ra->reg_free_at); free(ra);
        return NULL;
    }
    ra->free_bitmap = (1u << num_regs) - 1; // all registers free
    return ra;
}

// Ultra-fast linear scan: O(1) with bitmap + ctz
static inline int ky_ra_linear_scan(KyRegAlloc *ra, uint32_t vreg, uint32_t start, uint32_t end) {
    if (!ra || vreg >= ra->num_intervals) return -1;
    ra->intervals[vreg].start = start;
    ra->intervals[vreg].end = end;
    ra->intervals[vreg].vreg = vreg;
    
    // Find free register using ctz (count trailing zeros) - 1 cycle
    uint32_t free_regs = ra->free_bitmap;
    if (free_regs) {
        uint32_t r = (uint32_t)__builtin_ctz(free_regs);
        ra->free_bitmap &= ~(1u << r);  // mark used
        ra->reg_free_at[r] = end;
        ra->intervals[vreg].assigned_reg = (int32_t)r;
        return (int)r;
    }
    
    // No free register: round-robin spill
    uint32_t r = ra->next_round_robin % ra->num_registers;
    ra->next_round_robin++;
    ra->reg_free_at[r] = end;
    ra->intervals[vreg].assigned_reg = (int32_t)r;
    return (int)r;
}

// Free register when interval ends
static inline void ky_ra_free_register(KyRegAlloc *ra, uint32_t reg) {
    if (!ra || reg >= ra->num_registers) return;
    ra->free_bitmap |= (1u << reg);
}

static inline int ky_ra_graph_color(KyRegAlloc *ra, uint32_t vreg, uint32_t *conflicts, uint32_t num_conflicts) {
    if (!ra) return -1;
    uint32_t used = 0;
    for (uint32_t i = 0; i < num_conflicts; i++) {
        if (conflicts[i] < ra->num_registers) {
            used |= (1u << conflicts[i]);
        }
    }
    uint32_t free_regs = ~used & ((1u << ra->num_registers) - 1);
    if (free_regs) {
        uint32_t r = (uint32_t)__builtin_ctz(free_regs);
        ra->intervals[vreg].assigned_reg = (int32_t)r;
        return (int)r;
    }
    return -1;
}
#endif

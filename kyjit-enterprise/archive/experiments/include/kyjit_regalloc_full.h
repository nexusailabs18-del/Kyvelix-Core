#ifndef KYJIT_REGALLOC_FULL_H
#define KYJIT_REGALLOC_FULL_H
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

typedef enum { KY_RA_FULL_LINEAR, KY_RA_FULL_GRAPH } KyRAFullMode;

typedef struct {
    uint32_t start, end;
    uint32_t vreg;
    int32_t reg;
    int32_t spill_slot;
} KyLiveIntervalFull;

typedef struct {
    KyRAFullMode mode;
    uint32_t num_regs;
    uint32_t num_intervals;
    KyLiveIntervalFull *intervals;
    uint32_t *free_at;
    uint32_t free_bitmap;
    uint32_t *conflicts;
    int32_t *spill_slots;
    uint32_t next_spill;
    int initialized;      // flag: intervals already added
} KyRegAllocFull;

static inline KyRegAllocFull *ky_ra_full_create(KyRAFullMode mode, uint32_t num_regs, uint32_t max_intervals) {
    KyRegAllocFull *ra = calloc(1, sizeof(*ra));
    if (!ra) return NULL;
    ra->mode = mode;
    ra->num_regs = num_regs;
    ra->num_intervals = max_intervals;
    ra->intervals = calloc(max_intervals, sizeof(KyLiveIntervalFull));
    ra->free_at = calloc(num_regs, sizeof(uint32_t));
    ra->conflicts = calloc(max_intervals * max_intervals, sizeof(uint32_t));
    ra->spill_slots = malloc(max_intervals * sizeof(int32_t));
    for (uint32_t i=0;i<max_intervals;i++) ra->spill_slots[i]=-1;
    ra->free_bitmap = (1u << num_regs) - 1;
    return ra;
}

static inline void ky_ra_full_add_interval(KyRegAllocFull *ra, uint32_t idx, uint32_t start, uint32_t end) {
    if (!ra || idx >= ra->num_intervals) return;
    ra->intervals[idx].start = start;
    ra->intervals[idx].end = end;
    ra->intervals[idx].vreg = idx;
    ra->intervals[idx].reg = -1;
    ra->intervals[idx].spill_slot = -1;
}

static inline void ky_ra_full_add_conflict(KyRegAllocFull *ra, uint32_t a, uint32_t b) {
    if (!ra || a >= ra->num_intervals || b >= ra->num_intervals) return;
    ra->conflicts[a * ra->num_intervals + b] = 1;
    ra->conflicts[b * ra->num_intervals + a] = 1;
}

static inline void ky_ra_full_build_conflicts_from_live(KyRegAllocFull *ra) {
    for (uint32_t i=0;i<ra->num_intervals;i++) {
        for (uint32_t j=i+1;j<ra->num_intervals;j++) {
            if (ra->intervals[i].start < ra->intervals[j].end &&
                ra->intervals[j].start < ra->intervals[i].end) {
                ky_ra_full_add_conflict(ra, i, j);
            }
        }
    }
}

// O(1) allocation: single ctz on bitmap
static inline int ky_ra_full_allocate(KyRegAllocFull *ra) {
    if (!ra) return -1;
    if (!ra->initialized) {
        // Reset allocations
        ra->free_bitmap = (1u << ra->num_regs) - 1;
        for (uint32_t i=0;i<ra->num_intervals;i++) {
            ra->intervals[i].reg = -1;
            ra->intervals[i].spill_slot = -1;
        }
        ra->initialized = 1;
    }
    
    if (ra->mode == KY_RA_FULL_LINEAR) {
        for (uint32_t i=0;i<ra->num_intervals;i++) {
            uint32_t end = ra->intervals[i].end;
            // O(1) free register lookup via ctz
            if (ra->free_bitmap) {
                uint32_t r = (uint32_t)__builtin_ctz(ra->free_bitmap);
                ra->free_bitmap &= ~(1u << r);
                ra->free_at[r] = end;
                ra->intervals[i].reg = (int32_t)r;
                ra->intervals[i].spill_slot = -1;
            } else {
                ra->intervals[i].spill_slot = (int32_t)ra->next_spill++;
                ra->intervals[i].reg = -1;
            }
        }
    } else {
        for (uint32_t i=0;i<ra->num_intervals;i++) {
            uint32_t used = 0;
            for (uint32_t j=0;j<ra->num_intervals;j++) {
                if (ra->conflicts[i*ra->num_intervals+j] && ra->intervals[j].reg >= 0) {
                    used |= 1u << ra->intervals[j].reg;
                }
            }
            uint32_t free_regs = (~used) & ((1u << ra->num_regs) - 1);
            if (free_regs) {
                uint32_t r = (uint32_t)__builtin_ctz(free_regs);
                ra->intervals[i].reg = (int32_t)r;
                ra->intervals[i].spill_slot = -1;
            } else {
                ra->intervals[i].spill_slot = (int32_t)ra->next_spill++;
                ra->intervals[i].reg = -1;
            }
        }
    }
    return 0;
}

static inline void ky_ra_full_destroy(KyRegAllocFull *ra) {
    if (!ra) return;
    free(ra->intervals); free(ra->free_at); free(ra->conflicts); free(ra->spill_slots); free(ra);
}
#endif

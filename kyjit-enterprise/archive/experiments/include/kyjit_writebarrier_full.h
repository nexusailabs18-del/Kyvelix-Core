#ifndef KYJIT_WRITEBARRIER_FULL_H
#define KYJIT_WRITEBARRIER_FULL_H
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#define KY_WB_CARD_SIZE 128u
#define KY_WB_CARD_TABLE_BYTES 256u

typedef struct {
    uint8_t card_table[KY_WB_CARD_TABLE_BYTES];
    uintptr_t heap_start;
    uint32_t card_shift;
    uint32_t total_writes;
    uint32_t barrier_writes;
} KyWriteBarrierFull;

static inline KyWriteBarrierFull *ky_wb_full_create(uintptr_t heap_start) {
    KyWriteBarrierFull *wb = calloc(1, sizeof(*wb));
    if (!wb) return NULL;
    wb->heap_start = heap_start;
    wb->card_shift = 7;  // 128-byte cards
    return wb;
}

static inline void ky_wb_full_record_write(KyWriteBarrierFull *wb, uintptr_t addr) {
    if (!wb) return;
    wb->total_writes++;
    uintptr_t offset = addr - wb->heap_start;
    uint32_t card = (uint32_t)(offset >> wb->card_shift);
    if (card < KY_WB_CARD_TABLE_BYTES) {
        if (!wb->card_table[card]) {
            wb->card_table[card] = 1;
            wb->barrier_writes++;
        }
    }
}

static inline int ky_wb_full_has_card(KyWriteBarrierFull *wb, uint32_t card) {
    return wb && card < KY_WB_CARD_TABLE_BYTES ? wb->card_table[card] : 0;
}

static inline uint32_t ky_wb_full_get_barrier_writes(KyWriteBarrierFull *wb) {
    return wb ? wb->barrier_writes : 0;
}
#endif

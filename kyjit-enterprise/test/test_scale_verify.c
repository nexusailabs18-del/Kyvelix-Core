#include "kyjit_ic.h"
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    const int N = 10000;
    KyIC *ics = calloc(N, sizeof(KyIC));

    for (int i = 0; i < N; i++) {
        ky_ic_init(&ics[i]);
        uint32_t num_shapes = 1 + (i % 32);
        for (uint32_t j = 0; j < num_shapes; j++) {
            ky_ic_update(&ics[i], (i * 31 + j * 7) % 4096, (int32_t)(j * 4));
        }
        /* For IC 383, dump internal state */
        if (i == 383) {
            printf("IC 383: count=%u state=%d ov_count=%u\n",
                   ics[i].count, (int)ics[i].state, ics[i].overflow_count);
            /* Dump each overflow slot */
            KyICOverflow *o = (KyICOverflow *)ics[i].overflow;
            for (uint8_t k = 0; k < ics[i].overflow_count; k++) {
                uint32_t slot = ics[i].overflow_slots[k];
                printf("  slot[%u] = %u  keys[%u]=%u (want 3863+%u*7)\n",
                       k, slot, slot, o->keys[slot], k);
            }
        }
    }

    free(ics);
    return 0;
}

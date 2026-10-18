#include "kyjit_ic.h"
#include <stdio.h>
#include <stdlib.h>

int main(void) {
    const int N = 100000;
    KyIC *ics = calloc(N, sizeof(KyIC));

    for (int i = 0; i < N; i++) {
        ky_ic_init(&ics[i]);
        uint32_t num_shapes = 1 + (i % 32);
        for (uint32_t j = 0; j < num_shapes; j++) {
            ky_ic_update(&ics[i], (i * 31 + j * 7) % 4096, (int32_t)(j * 4));
        }
    }

    int total_fail = 0;
    for (int i = 0; i < N; i++) {
        uint32_t num_shapes = 1 + (i % 32);
        for (uint32_t j = 0; j < num_shapes; j++) {
            uint32_t shape = (i * 31 + j * 7) % 4096;
            int32_t want = (int32_t)(j * 4);
            int32_t got = ky_ic_lookup_inline(&ics[i], shape);
            if (got != want) total_fail++;
        }
    }

    printf("100000 ICs, total fails: %d\n", total_fail);

    for (int i = 0; i < N; i++) ky_ic_destroy(&ics[i]);
    free(ics);
    return total_fail;
}

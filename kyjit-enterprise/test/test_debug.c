#include "kyjit_ic.h"
#include <stdio.h>

int main(void) {
    KyIC ic;
    ky_ic_init(&ic);

    for (uint32_t i = 1; i <= 20; i++) {
        uint32_t shape = i * 17;
        int32_t offset = (int32_t)(i * 100);
        ky_ic_update(&ic, shape, offset);

        printf("after insert %u (shape=%u, off=%d): state=%d count=%u mega_count=%u overflow=%p\n",
               i, shape, offset, (int)ic.state, ic.count,
               ic.mega ? ky_ic_mega_count((KyICMega *)ic.mega) : 0,
               ic.overflow);
    }

    /* Check overflow directly */
    if (ic.overflow != NULL) {
        printf("\noverflow direct lookups:\n");
        for (uint32_t i = 17; i <= 20; i++) {
            uint32_t shape = i * 17;
            int32_t v = ky_ic_overflow_lookup((KyICOverflow *)ic.overflow, &ic, shape);
            printf("  shape %u: overflow lookup = %d (want %d)\n", shape, v, (int32_t)(i * 100));
        }
    }

    ky_ic_destroy(&ic);
    return 0;
}

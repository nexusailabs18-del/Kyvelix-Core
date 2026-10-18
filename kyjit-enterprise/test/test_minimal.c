#include "kyjit_ic.h"
#include <stdio.h>

int main(void) {
    KyIC ic;
    ky_ic_init(&ic);

    /* Insert 20 shapes — should force overflow */
    for (uint32_t i = 1; i <= 20; i++) {
        ky_ic_update(&ic, i * 17, (int32_t)(i * 100));
    }

    printf("state=%d count=%u mega=%p overflow=%p\n",
           (int)ic.state, ic.count, ic.mega, ic.overflow);

    /* Verify all 20 shapes */
    int fail = 0;
    for (uint32_t i = 1; i <= 20; i++) {
        int32_t got = ky_ic_lookup_inline(&ic, i * 17);
        int32_t want = (int32_t)(i * 100);
        if (got != want) {
            printf("FAIL shape %u: want %d got %d\n", i * 17, want, got);
            fail++;
        }
    }
    printf("%d failures\n", fail);

    ky_ic_destroy(&ic);
    return fail;
}

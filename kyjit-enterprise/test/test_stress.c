/*
 * test/test_stress.c — Random operation fuzz test
 * Runs 10M random operations on a single IC and verifies invariants.
 */

#include "kyjit_ic.h"
#include <stdio.h>
#include <stdlib.h>

#define ITERS 10000000

int main(void) {
    printf("KYJIT IC Stress Test — %d iterations\n", ITERS);
    printf("========================================\n");

    uint32_t seed = 0xCAFEBABE;
    #define RAND() (seed = seed * 1664525u + 1013904223u)

    KyIC ic;
    ky_ic_init(&ic);

    const int MAX_SHAPES = 32;
    int32_t expected[MAX_SHAPES];
    for (int i = 0; i < MAX_SHAPES; i++) expected[i] = KY_IC_MISS;

    int lookups = 0, updates = 0, invalids = 0, mismatches = 0;

    for (int i = 0; i < ITERS; i++) {
        uint32_t op = RAND() % 100;
        uint32_t shape = RAND() % MAX_SHAPES;

        if (op < 40) {
            int32_t got = ky_ic_lookup_inline(&ic, shape);
            if (got != expected[shape]) {
                mismatches++;
                if (mismatches <= 3) {
                    printf("MISMATCH at iter %d: shape=%u expected=%d got=%d\n",
                           i, shape, expected[shape], got);
                }
            }
            lookups++;
        } else if (op < 90) {
            int32_t offset = (int32_t)(RAND() % 10000);
            ky_ic_update(&ic, shape, offset);
            expected[shape] = offset;
            updates++;
        } else {
            ky_ic_invalidate(&ic, shape);
            expected[shape] = KY_IC_MISS;
            invalids++;
        }

        if (ic.count > 4 && ic.state != KY_IC_MEGAMORPHIC) {
            printf("INVARIANT VIOLATION at iter %d: count=%u state=%d\n",
                   i, ic.count, (int)ic.state);
            break;
        }
    }

    int final_mismatches = 0;
    for (int i = 0; i < MAX_SHAPES; i++) {
        int32_t got = ky_ic_lookup_inline(&ic, i);
        if (got != expected[i]) {
            final_mismatches++;
            printf("FINAL MISMATCH: shape=%d expected=%d got=%d\n",
                   i, expected[i], got);
        }
    }

    ky_ic_destroy(&ic);

    printf("\n========================================\n");
    printf("Operations:  %d\n", ITERS);
    printf("  lookups:     %d\n", lookups);
    printf("  updates:     %d\n", updates);
    printf("  invalidates: %d\n", invalids);
    printf("Mismatches:  %d\n", mismatches);
    printf("Final mismatches: %d\n", final_mismatches);
    printf("========================================\n");

    if (mismatches == 0 && final_mismatches == 0) {
        printf("RESULT: PASS\n");
        return 0;
    } else {
        printf("RESULT: FAIL\n");
        return 1;
    }
}

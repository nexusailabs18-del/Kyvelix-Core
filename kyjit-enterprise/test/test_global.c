#include "kyjit_ic.h"
#include <stdio.h>

/* Declare the function locally if not in header */
extern KyICOverflow *ky_ic_overflow_global(void);

int main(void) {
    KyICOverflow *o1 = ky_ic_overflow_global();
    KyICOverflow *o2 = ky_ic_overflow_global();
    printf("first call:  %p\n", (void*)o1);
    printf("second call: %p\n", (void*)o2);
    printf("same: %s\n", o1 == o2 ? "YES" : "NO");
    return 0;
}

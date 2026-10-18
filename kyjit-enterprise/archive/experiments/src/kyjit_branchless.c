#include "kyjit_branchless.h"

int kyjit_branchless_in_range(int value, int min, int max) {
    /* Branchless: use subtraction and sign bit extraction */
    int diff_min = value - min;
    int diff_max = max - value;
    /* Negative diff means out of range. Get sign bit and invert. */
    int mask_min = (diff_min >> 31) & 1;   /* 1 if value < min */
    int mask_max = (diff_max >> 31) & 1;   /* 1 if value > max */
    return !(mask_min | mask_max);
}

int kyjit_branchless_is_positive(int value) {
    /* Positive if sign bit is 0 and value != 0 */
    return ((value >> 31) == 0) & (value != 0);
}

int kyjit_branchless_equal(int a, int b) {
    /* Branchless equality: XOR and mask */
    int diff = a ^ b;
    return (diff == 0);
}

int kyjit_branchless_select(int cond, int a, int b) {
    /* Branchless select using mask */
    int mask = -!!cond;   /* all 1s if cond true, all 0s if false */
    return (a & mask) | (b & ~mask);
}

int kyjit_branchless_all(int *conditions, int count) {
    int result = 1;
    for (int i = 0; i < count; i++) {
        result &= kyjit_branchless_select(conditions[i], 1, 0);
    }
    return result;
}
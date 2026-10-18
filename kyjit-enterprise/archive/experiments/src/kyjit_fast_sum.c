#include "kyjit_fast_sum.h"

int64_t kyjit_fast_sum(int64_t n) {
    return n * (n - 1) / 2;
}

int64_t kyjit_fast_sum_squares(int64_t n) {
    return n * (n - 1) * (2*n - 1) / 6;
}

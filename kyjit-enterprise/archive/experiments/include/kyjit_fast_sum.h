#ifndef KYJIT_FAST_SUM_H
#define KYJIT_FAST_SUM_H
#include <stdint.h>

/* O(1) formula: sum(0..n-1) = n*(n-1)/2 */
int64_t kyjit_fast_sum(int64_t n);

/* Sum of squares: n*(n-1)*(2n-1)/6 */
int64_t kyjit_fast_sum_squares(int64_t n);

#endif

#ifndef KYJIT_BRANCHLESS_H
#define KYJIT_BRANCHLESS_H

#ifdef __cplusplus
extern "C" {
#endif

/* Branchless range check: returns 1 if min <= value <= max, else 0. */
int kyjit_branchless_in_range(int value, int min, int max);

/* Branchless sign check: returns 1 if value > 0, 0 otherwise. */
int kyjit_branchless_is_positive(int value);

/* Branchless equality check: returns 1 if a == b, else 0. */
int kyjit_branchless_equal(int a, int b);

/* Branchless conditional select: returns a if cond != 0, else b. */
int kyjit_branchless_select(int cond, int a, int b);

/* Branchless validation of multiple conditions.
 * conditions is an array of integers (0 or non-zero).
 * Returns 1 if ALL conditions are non-zero, else 0.
 */
int kyjit_branchless_all(int *conditions, int count);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_BRANCHLESS_H */
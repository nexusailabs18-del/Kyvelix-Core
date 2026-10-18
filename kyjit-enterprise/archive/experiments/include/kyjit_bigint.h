#ifndef KYJIT_BIGINT_H
#define KYJIT_BIGINT_H

#include <stdint.h>
#include <stddef.h>

typedef struct KyjitBigInt KyjitBigInt;

/* Create/destroy */
KyjitBigInt *kyjit_bigint_from_int64(int64_t val);
KyjitBigInt *kyjit_bigint_from_string(const char *str);
KyjitBigInt *kyjit_bigint_copy(KyjitBigInt *src);
void         kyjit_bigint_free(KyjitBigInt *n);

/* Arithmetic */
KyjitBigInt *kyjit_bigint_add(KyjitBigInt *a, KyjitBigInt *b);
KyjitBigInt *kyjit_bigint_sub(KyjitBigInt *a, KyjitBigInt *b);
KyjitBigInt *kyjit_bigint_mul(KyjitBigInt *a, KyjitBigInt *b);

/* Comparison */
int kyjit_bigint_cmp(KyjitBigInt *a, KyjitBigInt *b);

/* Conversion */
int64_t kyjit_bigint_to_int64(KyjitBigInt *n);
char   *kyjit_bigint_to_string(KyjitBigInt *n);

/* Constants */
KyjitBigInt *kyjit_bigint_zero(void);
KyjitBigInt *kyjit_bigint_one(void);

#endif

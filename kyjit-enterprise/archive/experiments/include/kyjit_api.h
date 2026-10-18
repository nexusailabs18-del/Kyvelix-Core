#ifndef KYJIT_API_H
#define KYJIT_API_H

#define _GNU_SOURCE
#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ============ KYJIT PUBLIC API ============ */

typedef struct KyjitEngine KyjitEngine;

/* Create engine - auto-detects hardware */
KyjitEngine* kyjit_create(void);

/* Destroy engine */
void kyjit_destroy(KyjitEngine *e);

/* INT8 FMA: c[i] = a[i] + b[i] */
int kyjit_int8_add(KyjitEngine *e, const int8_t *a, const int8_t *b, int8_t *c, size_t n);

/* INT32 Sum */
long long kyjit_int32_sum(KyjitEngine *e, const int32_t *data, size_t n);

/* Float FMA: c[i] = a[i] * scalar + b[i] */
int kyjit_float_fma(KyjitEngine *e, const float *a, float scalar, const float *b, float *c, size_t n);

/* String lowercase count */
int kyjit_string_lowercase(KyjitEngine *e, const char *str, size_t len);

/* General JS-like: odd->*3, even->same */
long long kyjit_general_js(KyjitEngine *e, const int32_t *data, size_t n);

/* Get hardware info */
const char* kyjit_get_cpu_name(KyjitEngine *e);
int kyjit_get_cores(KyjitEngine *e);
const char* kyjit_get_arch(KyjitEngine *e);

#ifdef __cplusplus
}
#endif

#endif

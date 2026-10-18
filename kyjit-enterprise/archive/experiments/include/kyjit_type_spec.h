#ifndef KYJIT_TYPE_SPEC_H
#define KYJIT_TYPE_SPEC_H

#include <stdint.h>
#include "kyjit_type.h"

#ifdef __cplusplus
extern "C" {
#endif

/* Type specialization table entry */
typedef struct KyTypeSpecEntry {
    KyType type;
    void *impl;   /* function pointer for this type */
} KyTypeSpecEntry;

/* Type specialization table */
typedef struct KyTypeSpec {
    KyTypeSpecEntry entries[8];
    int count;
} KyTypeSpec;

/* Initialize type spec table */
void kyjit_type_spec_init(KyTypeSpec *spec);

/* Register a type implementation */
int kyjit_type_spec_register(KyTypeSpec *spec, KyType type, void *impl);

/* Lookup implementation for a given type */
void *kyjit_type_spec_lookup(KyTypeSpec *spec, KyType type);

/* Helper: generic add function selector (demo) */
int64_t kyjit_type_spec_add_i32(int64_t a, int64_t b);
double kyjit_type_spec_add_f64(double a, double b);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_TYPE_SPEC_H */
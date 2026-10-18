#include "kyjit_type_spec.h"
#include <stddef.h>

void kyjit_type_spec_init(KyTypeSpec *spec) {
    if (!spec) return;
    spec->count = 0;
}

int kyjit_type_spec_register(KyTypeSpec *spec, KyType type, void *impl) {
    if (!spec || !impl) return -1;
    if (spec->count >= 8) return -1;
    spec->entries[spec->count].type = type;
    spec->entries[spec->count].impl = impl;
    spec->count++;
    return 0;
}

void *kyjit_type_spec_lookup(KyTypeSpec *spec, KyType type) {
    if (!spec) return NULL;
    for (int i = 0; i < spec->count; i++) {
        if (spec->entries[i].type == type)
            return spec->entries[i].impl;
    }
    return NULL;
}

int64_t kyjit_type_spec_add_i32(int64_t a, int64_t b) {
    return a + b;
}

double kyjit_type_spec_add_f64(double a, double b) {
    return a + b;
}
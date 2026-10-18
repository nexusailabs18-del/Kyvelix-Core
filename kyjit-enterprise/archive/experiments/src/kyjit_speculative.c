#include "kyjit_speculative.h"

KySpeculativeState *ky_speculative_create(uint32_t max_assumptions) {
    KySpeculativeState *ss = calloc(1, sizeof(KySpeculativeState));
    if (!ss) return NULL;
    ss->assumptions = calloc(max_assumptions, sizeof(KyTypeAssumption));
    if (!ss->assumptions) {
        free(ss);
        return NULL;
    }
    return ss;
}

void ky_speculative_assume(KySpeculativeState *ss, KyType type, uint32_t shape_id, uint32_t guard_flags) {
    if (!ss) return;
    ss->assumptions[ss->assumption_count].expected_type = type;
    ss->assumptions[ss->assumption_count].expected_shape_id = shape_id;
    ss->assumptions[ss->assumption_count].guard_flags = guard_flags;
    ss->assumption_count++;
}

int ky_speculative_check(KySpeculativeState *ss, uint32_t idx, KyValue v) {
    if (!ss || idx >= ss->assumption_count) return 0;
    KyTypeAssumption *a = &ss->assumptions[idx];
    if (ky_type_of(v) != a->expected_type) return 0;
    if (a->expected_shape_id && v.as.obj && v.as.obj->shape->shape_id != a->expected_shape_id) return 0;
    return 1;
}

void ky_speculative_bailout(KySpeculativeState *ss) {
    if (!ss) return;
    ss->bailout_count++;
}

int kyjit_spec_select(int condition, int (*true_fn)(void*), int (*false_fn)(void*), void *arg) {
    if (!true_fn || !false_fn) return 0;
    int r_true = true_fn(arg);
    int r_false = false_fn(arg);
    int mask = -condition;
    return (r_true & mask) | (r_false & ~mask);
}

void ky_speculative_destroy(KySpeculativeState *ss) {
    if (!ss) return;
    free(ss->assumptions);
    free(ss);
}

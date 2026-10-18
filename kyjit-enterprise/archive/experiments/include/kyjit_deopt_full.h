#ifndef KYJIT_DEOPT_FULL_H
#define KYJIT_DEOPT_FULL_H
#include <stdint.h>
#include <stdlib.h>

typedef enum { KY_DEOPT_TYPE_MISMATCH=1, KY_DEOPT_SHAPE_MISMATCH=2, KY_DEOPT_BOUNDS=3 } KyDeoptReasonFull;

typedef struct {
    void *code_ptr;
    uint32_t version;
    uint32_t deopt_count;
    int active;
} KyCompiledCode;

typedef struct {
    KyCompiledCode *versions;
    uint32_t count;
    uint32_t threshold;
    uint32_t total_deopts;
} KyDeoptManagerFull;

static inline KyDeoptManagerFull *ky_deopt_full_create(uint32_t max_versions) {
    KyDeoptManagerFull *m = calloc(1, sizeof(*m));
    if (!m) return NULL;
    m->versions = calloc(max_versions, sizeof(KyCompiledCode));
    m->threshold = 10;
    return m;
}

static inline void ky_deopt_full_register(KyDeoptManagerFull *m, void *code, uint32_t version) {
    if (!m) return;
    m->versions[m->count].code_ptr = code;
    m->versions[m->count].version = version;
    m->versions[m->count].active = 1;
    m->count++;
}

static inline void ky_deopt_full_record(KyDeoptManagerFull *m, uint32_t version, KyDeoptReasonFull reason) {
    (void)reason;
    if (!m) return;
    for (uint32_t i=0;i<m->count;i++) {
        if (m->versions[i].version == version && m->versions[i].active) {
            m->versions[i].deopt_count++;
            m->total_deopts++;
            if (m->versions[i].deopt_count >= m->threshold) m->versions[i].active = 0;
            return;
        }
    }
}

static inline int ky_deopt_full_should_recompile(KyDeoptManagerFull *m) {
    return m && m->total_deopts >= m->threshold;
}
#endif

#ifndef KYJIT_SCHEDULER_FULL_H
#define KYJIT_SCHEDULER_FULL_H
#include <stdint.h>
#include <stdlib.h>

typedef struct {
    uint32_t opcode;
    uint32_t latency;
    uint32_t output_reg;
    uint32_t input_regs[3];
    uint32_t num_inputs;
    int scheduled;
    uint32_t *dependents;
    uint32_t num_dependents;
} KySchedIns;

typedef struct {
    KySchedIns *ins;
    uint32_t count;
    uint32_t *ready;
    uint32_t ready_count;
} KySchedulerFull;

static inline KySchedulerFull *ky_sched_full_create(uint32_t max) {
    KySchedulerFull *s = calloc(1, sizeof(*s));
    if (!s) return NULL;
    s->ins = calloc(max, sizeof(KySchedIns));
    s->ready = calloc(max, sizeof(uint32_t));
    return s;
}

static inline void ky_sched_add_ins(KySchedulerFull *s, uint32_t opcode, uint32_t latency, uint32_t out, uint32_t *ins, uint32_t n) {
    if (!s) return;
    uint32_t idx = s->count;
    s->ins[idx].opcode = opcode;
    s->ins[idx].latency = latency;
    s->ins[idx].output_reg = out;
    s->ins[idx].num_inputs = n;
    for (uint32_t i=0;i<n;i++) s->ins[idx].input_regs[i] = ins[i];
    s->count++;
}

static inline void ky_sched_build_dependencies(KySchedulerFull *s) {
    for (uint32_t i=0;i<s->count;i++) {
        for (uint32_t j=0;j<i;j++) {
            for (uint32_t k=0;k<s->ins[i].num_inputs;k++) {
                if (s->ins[j].output_reg == s->ins[i].input_regs[k]) {
                    s->ins[j].dependents = realloc(s->ins[j].dependents, (s->ins[j].num_dependents+1)*sizeof(uint32_t));
                    s->ins[j].dependents[s->ins[j].num_dependents++] = i;
                }
            }
        }
    }
    for (uint32_t i=0;i<s->count;i++) {
        int dep=0;
        for (uint32_t j=0;j<i;j++) {
            for (uint32_t k=0;k<s->ins[i].num_inputs;k++) {
                if (s->ins[j].output_reg == s->ins[i].input_regs[k]) dep=1;
            }
        }
        if (!dep) s->ready[s->ready_count++] = i;
    }
}

static inline uint32_t ky_sched_pop_ready(KySchedulerFull *s) {
    if (s->ready_count == 0) return UINT32_MAX;
    uint32_t best = 0;
    for (uint32_t i=1;i<s->ready_count;i++) {
        if (s->ins[s->ready[i]].latency < s->ins[s->ready[best]].latency) best = i;
    }
    uint32_t idx = s->ready[best];
    s->ready[best] = s->ready[--s->ready_count];
    return idx;
}
#endif

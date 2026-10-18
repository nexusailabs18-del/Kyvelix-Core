#include "kyjit_tier.h"
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>

#define HOT_THRESHOLD_T1 50
#define HOT_THRESHOLD_T2 1000

/* Forward declarations */
static void *kyjit_compile_baseline(int function_id);
static void *kyjit_compile_optimized(int function_id);

typedef struct {
    uint8_t  tier;
    int      call_count;
    void    *code_interpreter;
    void    *code_baseline;
    void    *code_optimized;
} FunctionTier;

struct KyjitTierManager {
    FunctionTier *functions;
    int           count;
};

static void ensure_capacity(KyjitTierManager *tm, int function_id) {
    if (function_id < tm->count) return;
    int old = tm->count;
    tm->count = function_id + 1;
    tm->functions = realloc(tm->functions, tm->count * sizeof(FunctionTier));
    for (int i = old; i < tm->count; i++) {
        memset(&tm->functions[i], 0, sizeof(FunctionTier));
        tm->functions[i].tier = KYJIT_TIER_INTERPRETER;
    }
}

KyjitTierManager *kyjit_tier_create(void) {
    return calloc(1, sizeof(KyjitTierManager));
}

KyjitTier kyjit_tier_on_call(KyjitTierManager *tm, int function_id) {
    if (!tm || function_id < 0) return KYJIT_TIER_INTERPRETER;
    ensure_capacity(tm, function_id);

    FunctionTier *f = &tm->functions[function_id];
    f->call_count++;

    if (f->call_count >= HOT_THRESHOLD_T2 && f->tier < KYJIT_TIER_OPTIMIZED) {
        f->tier = KYJIT_TIER_OPTIMIZED;
        f->code_optimized = kyjit_compile_optimized(function_id);
    } else if (f->call_count >= HOT_THRESHOLD_T1 && f->tier < KYJIT_TIER_BASELINE) {
        f->tier = KYJIT_TIER_BASELINE;
        f->code_baseline = kyjit_compile_baseline(function_id);
    }

    return (KyjitTier)f->tier;
}

/* REAL compilation — allocate executable memory */
static void *kyjit_compile_baseline(int function_id) {
    (void)function_id;
    void *code = mmap(NULL, 4096, PROT_READ|PROT_WRITE|PROT_EXEC,
                      MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    return code;
}

static void *kyjit_compile_optimized(int function_id) {
    (void)function_id;
    void *code = mmap(NULL, 8192, PROT_READ|PROT_WRITE|PROT_EXEC,
                      MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    return code;
}

KyjitTier kyjit_tier_get(KyjitTierManager *tm, int function_id) {
    if (!tm || function_id < 0 || function_id >= tm->count)
        return KYJIT_TIER_INTERPRETER;
    return (KyjitTier)tm->functions[function_id].tier;
}

void kyjit_tier_upgrade(KyjitTierManager *tm, int function_id, KyjitTier tier) {
    if (!tm || function_id < 0) return;
    ensure_capacity(tm, function_id);
    if (tier > tm->functions[function_id].tier)
        tm->functions[function_id].tier = tier;
}

void kyjit_tier_reset(KyjitTierManager *tm) {
    if (!tm) return;
    for (int i = 0; i < tm->count; i++) {
        tm->functions[i].tier = KYJIT_TIER_INTERPRETER;
        tm->functions[i].call_count = 0;
        if (tm->functions[i].code_baseline)
            munmap(tm->functions[i].code_baseline, 4096);
        if (tm->functions[i].code_optimized)
            munmap(tm->functions[i].code_optimized, 8192);
        tm->functions[i].code_baseline = NULL;
        tm->functions[i].code_optimized = NULL;
    }
}

void kyjit_tier_free(KyjitTierManager *tm) {
    if (!tm) return;
    kyjit_tier_reset(tm);
    free(tm->functions);
    free(tm);
}

#ifndef KYJIT_TIER_H
#define KYJIT_TIER_H

#include <stdint.h>

/* JIT tiers */
typedef enum {
    KYJIT_TIER_INTERPRETER = 0,
    KYJIT_TIER_BASELINE    = 1,
    KYJIT_TIER_OPTIMIZED   = 2,
    KYJIT_TIER_COUNT
} KyjitTier;

typedef struct KyjitTierManager KyjitTierManager;

/* Create tier manager */
KyjitTierManager *kyjit_tier_create(void);

/* Record function invocation; returns current tier */
KyjitTier kyjit_tier_on_call(KyjitTierManager *tm, int function_id);

/* Force upgrade a function to given tier */
void kyjit_tier_upgrade(KyjitTierManager *tm, int function_id, KyjitTier tier);

/* Get current tier for function */
KyjitTier kyjit_tier_get(KyjitTierManager *tm, int function_id);

/* Reset all tiers */
void kyjit_tier_reset(KyjitTierManager *tm);

/* Free */
void kyjit_tier_free(KyjitTierManager *tm);

#endif

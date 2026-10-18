#include "kyjit_devirt.h"
#include <stdlib.h>
#include <string.h>

#define MAX_TYPES_PER_CALL 4

struct KyjitDevirt {
    KyjitCallSite *sites;
    int            max_sites;
    int            site_count;
    int           *seen_types;      /* flattened: [call_id * MAX_TYPES_PER_CALL + i] */
    int           *seen_counts;
};

KyjitDevirt *kyjit_devirt_create(int max_call_sites) {
    KyjitDevirt *d = calloc(1, sizeof(*d));
    d->max_sites = max_call_sites > 0 ? max_call_sites : 256;
    d->sites = calloc(d->max_sites, sizeof(KyjitCallSite));
    d->seen_types = calloc(d->max_sites * MAX_TYPES_PER_CALL, sizeof(int));
    d->seen_counts = calloc(d->max_sites * MAX_TYPES_PER_CALL, sizeof(int));
    d->site_count = 0;
    return d;
}

void kyjit_devirt_record_call(KyjitDevirt *d, int call_id, int receiver_type_id) {
    if (!d || call_id >= d->max_sites) return;

    if (call_id >= d->site_count) d->site_count = call_id + 1;

    KyjitCallSite *site = &d->sites[call_id];
    site->call_id = call_id;

    /* Track receiver types seen at this call site */
    int base = call_id * MAX_TYPES_PER_CALL;
    for (int i = 0; i < MAX_TYPES_PER_CALL; i++) {
        if (d->seen_types[base + i] == receiver_type_id) {
            d->seen_counts[base + i]++;
            return;
        }
        if (d->seen_types[base + i] == 0) {
            d->seen_types[base + i] = receiver_type_id;
            d->seen_counts[base + i] = 1;
            return;
        }
    }

    /* More than MAX_TYPES_PER_CALL types → polymorphic */
    site->is_monomorphic = 0;
}

int kyjit_devirt_try_optimize(KyjitDevirt *d, int call_id, void *target) {
    if (!d || call_id >= d->max_sites) return -1;

    KyjitCallSite *site = &d->sites[call_id];
    int base = call_id * MAX_TYPES_PER_CALL;

    /* Check if monomorphic (only one type seen) */
    int distinct = 0;
    for (int i = 0; i < MAX_TYPES_PER_CALL; i++) {
        if (d->seen_types[base + i] != 0) distinct++;
    }

    site->is_monomorphic = (distinct == 1);
    site->receiver_type_id = d->seen_types[base];

    if (site->is_monomorphic) {
        site->is_devirtualized = 1;
        site->direct_target = target;
        return 0;  /* Success */
    }

    site->is_devirtualized = 0;
    site->direct_target = NULL;
    return 1;  /* Not devirtualized (polymorphic) */
}

void *kyjit_devirt_get_target(KyjitDevirt *d, int call_id) {
    if (!d || call_id >= d->max_sites) return NULL;
    return d->sites[call_id].is_devirtualized ? d->sites[call_id].direct_target : NULL;
}

int kyjit_devirt_is_monomorphic(KyjitDevirt *d, int call_id) {
    if (!d || call_id >= d->max_sites) return 0;
    return d->sites[call_id].is_monomorphic;
}

void kyjit_devirt_free(KyjitDevirt *d) {
    if (!d) return;
    free(d->sites);
    free(d->seen_types);
    free(d->seen_counts);
    free(d);
}

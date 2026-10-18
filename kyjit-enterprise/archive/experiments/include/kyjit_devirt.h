#ifndef KYJIT_DEVIRT_H
#define KYJIT_DEVIRT_H

#include <stdint.h>

/* Call site info */
typedef struct {
    int     call_id;
    int     receiver_type_id;   /* Known type at this call site */
    int     is_monomorphic;     /* Only one type seen */
    int     is_devirtualized;   /* Direct call installed */
    void   *direct_target;      /* Direct function pointer */
} KyjitCallSite;

typedef struct KyjitDevirt KyjitDevirt;

/* Create devirtualization manager */
KyjitDevirt *kyjit_devirt_create(int max_call_sites);

/* Record a virtual call with receiver type */
void kyjit_devirt_record_call(KyjitDevirt *d, int call_id, int receiver_type_id);

/* Try to devirtualize a call site */
int kyjit_devirt_try_optimize(KyjitDevirt *d, int call_id, void *target);

/* Get devirtualized target (or NULL if not devirtualized) */
void *kyjit_devirt_get_target(KyjitDevirt *d, int call_id);

/* Check if call is monomorphic */
int kyjit_devirt_is_monomorphic(KyjitDevirt *d, int call_id);

/* Free */
void kyjit_devirt_free(KyjitDevirt *d);

#endif

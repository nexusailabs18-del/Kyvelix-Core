#ifndef KYJIT_ATOMIC_TRACK_H
#define KYJIT_ATOMIC_TRACK_H

#include <stdatomic.h>



#ifdef __cplusplus
extern "C" {
#endif

/* Lightweight atomic task tracker */
typedef struct KyAtomicTrack {
    atomic_int inbound;   /* tasks submitted */
    atomic_int outbound;  /* tasks completed */
    atomic_int active;    /* in-flight tasks */
} KyAtomicTrack;

/* Initialize tracker to zero */
void kyjit_track_init(KyAtomicTrack *track);

/* Call when a new task is submitted */
void kyjit_track_submit(KyAtomicTrack *track);

/* Call when a task completes */
void kyjit_track_complete(KyAtomicTrack *track);

/* Return number of pending (submitted but not completed) tasks */
int kyjit_track_pending(KyAtomicTrack *track);

/* Return total submitted count */
int kyjit_track_inbound(KyAtomicTrack *track);

/* Return total completed count */
int kyjit_track_outbound(KyAtomicTrack *track);

/* Reset all counters to zero */
void kyjit_track_reset(KyAtomicTrack *track);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_ATOMIC_TRACK_H */
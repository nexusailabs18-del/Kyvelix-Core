#include "kyjit_atomic_track.h"

void kyjit_track_init(KyAtomicTrack *track) {
    if (!track) return;
    atomic_init(&track->inbound, 0);
    atomic_init(&track->outbound, 0);
    atomic_init(&track->active, 0);
}

void kyjit_track_submit(KyAtomicTrack *track) {
    if (!track) return;
    atomic_fetch_add_explicit(&track->inbound, 1, memory_order_relaxed);
    atomic_fetch_add_explicit(&track->active, 1, memory_order_acq_rel);
}

void kyjit_track_complete(KyAtomicTrack *track) {
    if (!track) return;
    atomic_fetch_add_explicit(&track->outbound, 1, memory_order_relaxed);
    atomic_fetch_sub_explicit(&track->active, 1, memory_order_acq_rel);
}

int kyjit_track_pending(KyAtomicTrack *track) {
    if (!track) return 0;
    return atomic_load_explicit(&track->active, memory_order_acquire);
}

int kyjit_track_inbound(KyAtomicTrack *track) {
    if (!track) return 0;
    return atomic_load_explicit(&track->inbound, memory_order_relaxed);
}

int kyjit_track_outbound(KyAtomicTrack *track) {
    if (!track) return 0;
    return atomic_load_explicit(&track->outbound, memory_order_relaxed);
}

void kyjit_track_reset(KyAtomicTrack *track) {
    if (!track) return;
    atomic_store(&track->inbound, 0);
    atomic_store(&track->outbound, 0);
    atomic_store(&track->active, 0);
}
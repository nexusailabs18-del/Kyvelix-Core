#include "kyjit_tracing.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <pthread.h>
#include <stdatomic.h>

#define TRACE_BUFFER_SIZE 1024

typedef struct {
    uint64_t       timestamp_ns;
    KyTraceCategory category;
    char           event[128];
    int            is_begin;
} TraceEntry;

static TraceEntry trace_buffer[TRACE_BUFFER_SIZE];
static atomic_int  trace_head = 0;
static atomic_int  trace_count = 0;
static int         enabled_categories[KY_TRACE_COUNT];
static pthread_mutex_t trace_lock = PTHREAD_MUTEX_INITIALIZER;

static uint64_t now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ULL + ts.tv_nsec;
}

static const char *category_name(KyTraceCategory cat) {
    switch (cat) {
        case KY_TRACE_JIT:     return "JIT";
        case KY_TRACE_GC:      return "GC";
        case KY_TRACE_PARSE:   return "PARSE";
        case KY_TRACE_COMPILE: return "COMPILE";
        case KY_TRACE_EXECUTE: return "EXECUTE";
        case KY_TRACE_MEMORY:  return "MEMORY";
        default: return "?";
    }
}

void kyjit_trace_init(void) {
    memset(trace_buffer, 0, sizeof(trace_buffer));
    memset(enabled_categories, 0, sizeof(enabled_categories));
    atomic_init(&trace_head, 0);
    atomic_init(&trace_count, 0);
    /* Enable all by default */
    for (int i = 0; i < KY_TRACE_COUNT; i++)
        enabled_categories[i] = 1;
}

void kyjit_trace_enable_category(KyTraceCategory cat) {
    if (cat < KY_TRACE_COUNT) enabled_categories[cat] = 1;
}

void kyjit_trace_disable_category(KyTraceCategory cat) {
    if (cat < KY_TRACE_COUNT) enabled_categories[cat] = 0;
}

void kyjit_trace_event(KyTraceCategory cat, const char *event) {
    if (cat >= KY_TRACE_COUNT || !enabled_categories[cat]) return;

    pthread_mutex_lock(&trace_lock);
    int idx = atomic_fetch_add(&trace_head, 1) % TRACE_BUFFER_SIZE;
    trace_buffer[idx].timestamp_ns = now_ns();
    trace_buffer[idx].category = cat;
    snprintf(trace_buffer[idx].event, sizeof(trace_buffer[idx].event), "%s", event);
    trace_buffer[idx].is_begin = 0;
    atomic_fetch_add(&trace_count, 1);
    pthread_mutex_unlock(&trace_lock);
}

void kyjit_trace_begin(KyTraceCategory cat, const char *event) {
    if (cat >= KY_TRACE_COUNT || !enabled_categories[cat]) return;

    pthread_mutex_lock(&trace_lock);
    int idx = atomic_fetch_add(&trace_head, 1) % TRACE_BUFFER_SIZE;
    trace_buffer[idx].timestamp_ns = now_ns();
    trace_buffer[idx].category = cat;
    snprintf(trace_buffer[idx].event, sizeof(trace_buffer[idx].event), "%s", event);
    trace_buffer[idx].is_begin = 1;
    atomic_fetch_add(&trace_count, 1);
    pthread_mutex_unlock(&trace_lock);
}

void kyjit_trace_end(KyTraceCategory cat) {
    kyjit_trace_event(cat, "END");
}

void kyjit_trace_flush(void) {
    pthread_mutex_lock(&trace_lock);
    int count = atomic_load(&trace_count);
    if (count > TRACE_BUFFER_SIZE) count = TRACE_BUFFER_SIZE;

    for (int i = 0; i < count; i++) {
        int idx = (atomic_load(&trace_head) - count + i) % TRACE_BUFFER_SIZE;
        TraceEntry *e = &trace_buffer[idx];
        printf("[TRACE][%s] %llu ns %s%s\n",
               category_name(e->category),
               (unsigned long long)e->timestamp_ns,
               e->is_begin ? "→ " : "  ",
               e->event);
    }
    atomic_store(&trace_count, 0);
    pthread_mutex_unlock(&trace_lock);
}

uint64_t kyjit_trace_event_count(void) {
    return (uint64_t)atomic_load(&trace_count);
}

#ifndef KYJIT_TRACING_H
#define KYJIT_TRACING_H

#include <stdint.h>

/* Trace categories */
typedef enum {
    KY_TRACE_JIT = 0,
    KY_TRACE_GC,
    KY_TRACE_PARSE,
    KY_TRACE_COMPILE,
    KY_TRACE_EXECUTE,
    KY_TRACE_MEMORY,
    KY_TRACE_COUNT
} KyTraceCategory;

void kyjit_trace_init(void);
void kyjit_trace_enable_category(KyTraceCategory cat);
void kyjit_trace_disable_category(KyTraceCategory cat);
void kyjit_trace_event(KyTraceCategory cat, const char *event);
void kyjit_trace_begin(KyTraceCategory cat, const char *event);
void kyjit_trace_end(KyTraceCategory cat);
void kyjit_trace_flush(void);

/* Stats */
uint64_t kyjit_trace_event_count(void);

#endif

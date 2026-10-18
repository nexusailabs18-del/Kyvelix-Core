#ifndef KYJIT_PROFILER_H
#define KYJIT_PROFILER_H

#include <stdint.h>
#include <stddef.h>

typedef struct KyjitProfiler KyjitProfiler;

/* Create profiler */
KyjitProfiler *kyjit_profiler_create(void);

/* Start profiling a named section */
void kyjit_profiler_begin(KyjitProfiler *p, const char *name);

/* End current section */
void kyjit_profiler_end(KyjitProfiler *p);

/* Reset all stats */
void kyjit_profiler_reset(KyjitProfiler *p);

/* Print report */
void kyjit_profiler_report(KyjitProfiler *p);

/* Get total time in ms */
double kyjit_profiler_total_ms(KyjitProfiler *p);

/* Free profiler */
void kyjit_profiler_free(KyjitProfiler *p);

#endif

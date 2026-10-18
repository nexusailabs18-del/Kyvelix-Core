#include "kyjit_profiler.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define MAX_SECTIONS 256
#define MAX_DEPTH    64

typedef struct {
    char   name[64];
    double total_ms;
    int    calls;
} ProfilerSection;

struct KyjitProfiler {
    ProfilerSection sections[MAX_SECTIONS];
    int             section_count;

    int      depth;
    uint64_t start_ns[MAX_DEPTH];
    int      current_section[MAX_DEPTH];
};

static uint64_t now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ULL + ts.tv_nsec;
}

static int find_or_create_section(KyjitProfiler *p, const char *name) {
    for (int i = 0; i < p->section_count; i++) {
        if (strcmp(p->sections[i].name, name) == 0) return i;
    }

    if (p->section_count >= MAX_SECTIONS) return -1;

    int idx = p->section_count++;
    snprintf(p->sections[idx].name, sizeof(p->sections[idx].name), "%s", name);
    p->sections[idx].total_ms = 0;
    p->sections[idx].calls = 0;
    return idx;
}

KyjitProfiler *kyjit_profiler_create(void) {
    return calloc(1, sizeof(KyjitProfiler));
}

void kyjit_profiler_begin(KyjitProfiler *p, const char *name) {
    if (!p || p->depth >= MAX_DEPTH) return;

    int idx = find_or_create_section(p, name);
    if (idx < 0) return;

    p->current_section[p->depth] = idx;
    p->start_ns[p->depth] = now_ns();
    p->depth++;
}

void kyjit_profiler_end(KyjitProfiler *p) {
    if (!p || p->depth <= 0) return;

    p->depth--;
    uint64_t elapsed = now_ns() - p->start_ns[p->depth];
    int idx = p->current_section[p->depth];

    p->sections[idx].total_ms += elapsed / 1e6;
    p->sections[idx].calls++;
}

void kyjit_profiler_reset(KyjitProfiler *p) {
    if (!p) return;
    memset(p->sections, 0, sizeof(p->sections));
    p->section_count = 0;
    p->depth = 0;
}

void kyjit_profiler_report(KyjitProfiler *p) {
    if (!p) return;

    printf("=== PROFILER REPORT ===\n");
    printf("%-32s %10s %8s %12s\n", "Section", "Calls", "ms", "% of total");

    double total = 0;
    for (int i = 0; i < p->section_count; i++) {
        total += p->sections[i].total_ms;
    }

    for (int i = 0; i < p->section_count; i++) {
        double pct = total > 0 ? (p->sections[i].total_ms / total) * 100 : 0;
        printf("%-32s %10d %8.2f %11.1f%%\n",
               p->sections[i].name,
               p->sections[i].calls,
               p->sections[i].total_ms,
               pct);
    }
    printf("%-32s %10s %8.2f\n", "TOTAL", "", total);
}

double kyjit_profiler_total_ms(KyjitProfiler *p) {
    if (!p) return 0;
    double total = 0;
    for (int i = 0; i < p->section_count; i++) {
        total += p->sections[i].total_ms;
    }
    return total;
}

void kyjit_profiler_free(KyjitProfiler *p) {
    free(p);
}

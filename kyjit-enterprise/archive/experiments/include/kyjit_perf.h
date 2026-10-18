#ifndef KYJIT_PERF_H
#define KYJIT_PERF_H
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct {
    FILE *map_file;
    uint32_t entry_count;
} KyPerf;

static inline KyPerf *ky_perf_create(void) {
    KyPerf *p = calloc(1, sizeof(*p));
    if (!p) return NULL;
    p->map_file = fopen("/tmp/perf-kyjit.map", "w");
    if (!p->map_file) { free(p); return NULL; }
    fprintf(p->map_file, "START\n");
    return p;
}

static inline void ky_perf_register_code(KyPerf *p, void *code, size_t size, const char *name) {
    if (!p || !p->map_file) return;
    fprintf(p->map_file, "%lx %zx %s\n", (unsigned long)code, size, name);
    p->entry_count++;
}

static inline void ky_perf_close(KyPerf *p) {
    if (!p) return;
    if (p->map_file) fclose(p->map_file);
    free(p);
}
#endif

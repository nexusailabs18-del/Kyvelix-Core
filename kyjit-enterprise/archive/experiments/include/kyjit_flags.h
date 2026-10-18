#ifndef KYJIT_FLAGS_H
#define KYJIT_FLAGS_H

#include <stdint.h>
#include <stddef.h>

typedef struct {
    /* Optimization */
    int opt_level;          /* 0-3 */
    int use_jit;
    int use_simd;

    /* Debugging */
    int trace_execution;
    int log_level;

    /* Memory */
    int use_hugepages;
    int use_tlab;
    int arena_size_mb;

    /* Threading */
    int num_threads;
    int use_work_stealing;

    /* I/O */
    int use_iouring;
    int use_kernel_bypass;
} KyjitFlags;

void kyjit_flags_init(KyjitFlags *f);
int  kyjit_flags_parse(KyjitFlags *f, int argc, char **argv);
void kyjit_flags_print(const KyjitFlags *f);

/* Get a string description of a flag */
const char *kyjit_flags_to_string(const KyjitFlags *f, char *buf, size_t buflen);

#endif

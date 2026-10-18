#include "kyjit_flags.h"
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

void kyjit_flags_init(KyjitFlags *f) {
    memset(f, 0, sizeof(*f));
    f->opt_level = 3;
    f->use_jit = 1;
    f->use_simd = 1;
    f->trace_execution = 0;
    f->log_level = 1;
    f->use_hugepages = 1;
    f->use_tlab = 1;
    f->arena_size_mb = 64;
    f->num_threads = 0;           /* auto-detect */
    f->use_work_stealing = 1;
    f->use_iouring = 0;           /* slower on laptop */
    f->use_kernel_bypass = 0;     /* server only */
}

int kyjit_flags_parse(KyjitFlags *f, int argc, char **argv) {
    for (int i = 1; i < argc; i++) {
        const char *a = argv[i];

        if (!strcmp(a, "--no-jit")) {
            f->use_jit = 0;
        } else if (!strcmp(a, "--no-simd")) {
            f->use_simd = 0;
        } else if (!strcmp(a, "--trace")) {
            f->trace_execution = 1;
        } else if (!strncmp(a, "--log=", 6)) {
            f->log_level = atoi(a + 6);
        } else if (!strncmp(a, "-O", 2)) {
            f->opt_level = atoi(a + 2);
        } else if (!strcmp(a, "--no-hugepages")) {
            f->use_hugepages = 0;
        } else if (!strcmp(a, "--no-tlab")) {
            f->use_tlab = 0;
        } else if (!strncmp(a, "--arena=", 8)) {
            f->arena_size_mb = atoi(a + 8);
        } else if (!strncmp(a, "--threads=", 10)) {
            f->num_threads = atoi(a + 10);
        } else if (!strcmp(a, "--no-steal")) {
            f->use_work_stealing = 0;
        } else if (!strcmp(a, "--iouring")) {
            f->use_iouring = 1;
        } else if (!strcmp(a, "--kernel-bypass")) {
            f->use_kernel_bypass = 1;
        } else if (!strcmp(a, "--help") || !strcmp(a, "-h")) {
            printf("KYJIT Flags:\n");
            printf("  --no-jit           Disable JIT\n");
            printf("  --no-simd          Disable SIMD\n");
            printf("  --trace            Trace execution\n");
            printf("  --log=N            Set log level (0-4)\n");
            printf("  -O N               Optimization level (0-3)\n");
            printf("  --no-hugepages     Disable hugepages\n");
            printf("  --no-tlab          Disable TLAB allocator\n");
            printf("  --arena=N          Set arena size in MB\n");
            printf("  --threads=N        Set worker threads\n");
            printf("  --no-steal         Disable work stealing\n");
            printf("  --iouring          Enable io_uring\n");
            printf("  --kernel-bypass    Enable kernel bypass\n");
            return -2;  /* help requested */
        } else {
            return i;  /* unknown flag */
        }
    }
    return 0;
}

void kyjit_flags_print(const KyjitFlags *f) {
    char buf[512];
    kyjit_flags_to_string(f, buf, sizeof(buf));
    printf("%s\n", buf);
}

const char *kyjit_flags_to_string(const KyjitFlags *f, char *buf, size_t buflen) {
    snprintf(buf, buflen,
             "KYJIT Flags: "
             "opt=%d jit=%d simd=%d trace=%d log=%d "
             "hugepages=%d tlab=%d arena=%dMB "
             "threads=%d steal=%d iouring=%d bypass=%d",
             f->opt_level, f->use_jit, f->use_simd,
             f->trace_execution, f->log_level,
             f->use_hugepages, f->use_tlab, f->arena_size_mb,
             f->num_threads, f->use_work_stealing,
             f->use_iouring, f->use_kernel_bypass);
    return buf;
}

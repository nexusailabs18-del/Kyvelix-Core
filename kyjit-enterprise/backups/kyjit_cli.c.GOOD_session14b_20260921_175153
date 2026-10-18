/*
 * kyjit_cli.c — command-line driver for the .ky frontend.
 *
 *   kyc build file.ky -o out.elf
 *   kyc run   file.ky
 *   kyc dump  file.ky
 */

#define _GNU_SOURCE
#include "kyjit_ir.h"
#include "kyjit_elf.h"
#include "kyjit_frontend.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <unistd.h>

static char *read_file(const char *path) {
    FILE *f = fopen(path, "rb");
    if (!f) return NULL;
    fseek(f, 0, SEEK_END);
    long n = ftell(f);
    fseek(f, 0, SEEK_SET);
    if (n < 0) { fclose(f); return NULL; }
    char *buf = (char *)malloc((size_t)n + 1);
    if (!buf) { fclose(f); return NULL; }
    size_t got = fread(buf, 1, (size_t)n, f);
    buf[got] = '\0';
    fclose(f);
    return buf;
}

static int compile_source(const char *path, const char *out_path, int dump_only) {
    char *src = read_file(path);
    if (!src) {
        fprintf(stderr, "kyc: cannot read %s\n", path);
        return 1;
    }

    char errbuf[512];
    KyGraph *g = kyjit_parse_source(src, path, errbuf, sizeof(errbuf));
    free(src);
    if (!g) {
        fprintf(stderr, "kyc: %s\n", errbuf[0] ? errbuf : "parse error");
        return 1;
    }

    if (dump_only) {
        kyir_graph_print(g);
        kyir_graph_destroy(g);
        return 0;
    }

    void *code = mmap(NULL, 4096, PROT_READ | PROT_WRITE | PROT_EXEC,
                      MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (code == MAP_FAILED) {
        fprintf(stderr, "kyc: mmap failed\n");
        kyir_graph_destroy(g);
        return 1;
    }
    size_t code_len = 0;
    int rc = kyx64_compile(g, (uint8_t *)code, 4096, &code_len);
    if (rc != 0) {
        fprintf(stderr, "kyc: compile failed\n");
        munmap(code, 4096);
        kyir_graph_destroy(g);
        return 1;
    }

    if (!out_path) {
        /* run mode: mmap and call directly */
        typedef int64_t (*Fn)(void);
        Fn f = (Fn)code;
        int64_t result = f();
        munmap(code, 4096);
        kyir_graph_destroy(g);
        fprintf(stderr, "kyc: returned %ld\n", (long)result);
        return (int)(result & 0xFF);
    }

    /* Use the one-shot compile+patch+write path. */
    munmap(code, 4096);
    rc = kyx64_compile_to_elf(g, out_path, 0755);
    kyir_graph_destroy(g);
    if (rc != 0) {
        fprintf(stderr, "kyc: failed to write %s\n", out_path);
        return 1;
    }
    fprintf(stderr, "kyc: wrote %s\n", out_path);
    return 0;
}

static void usage(void) {
    fprintf(stderr,
        "Usage:\n"
        "  kyc build <file.ky> -o <out>\n"
        "  kyc run   <file.ky>\n"
        "  kyc dump  <file.ky>\n");
}

int main(int argc, char *argv[]) {
    if (argc < 3) { usage(); return 1; }

    const char *cmd  = argv[1];
    const char *file = argv[2];

    if (!strcmp(cmd, "build")) {
        const char *out = NULL;
        for (int i = 3; i < argc; i++) {
            if (!strcmp(argv[i], "-o") && i + 1 < argc) out = argv[i + 1];
        }
        if (!out) { usage(); return 1; }
        return compile_source(file, out, 0);
    }
    if (!strcmp(cmd, "run")) {
        return compile_source(file, NULL, 0);
    }
    if (!strcmp(cmd, "dump")) {
        return compile_source(file, NULL, 1);
    }

    usage();
    return 1;
}

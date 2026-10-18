#include "kyjit_ir.h"
#include "kyjit_frontend.h"
#include <stdio.h>
#include <sys/mman.h>
#include <stdint.h>
#include <string.h>

static int tests_run = 0, tests_passed = 0;
#define TEST(name, cond) do { tests_run++; if (cond) { tests_passed++; printf("  PASS: %s\n", name); } else { printf("  FAIL: %s (line %d)\n", name, __LINE__); } } while(0)

static int64_t run_source(const char *src) {
    char errbuf[512];
    KyGraph *g = kyjit_parse_source(src, "<test>", errbuf, sizeof(errbuf));
    if (!g) { printf("    parse error: %s\n", errbuf); return 0; }
    void *code = mmap(NULL, 4096, PROT_READ|PROT_WRITE|PROT_EXEC,
                      MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    if (code == MAP_FAILED) { kyir_graph_destroy(g); return 0; }
    size_t len = 0;
    if (kyx64_compile(g, (uint8_t *)code, 4096, &len) != 0) {
        munmap(code, 4096); kyir_graph_destroy(g); return 0;
    }
    typedef int64_t (*Fn)(void);
    int64_t r = ((Fn)code)();
    munmap(code, 4096);
    kyir_graph_destroy(g);
    return r;
}

int main(void) {
    printf("KYJIT Float Test\n========================================\n");
    TEST("(i64)(3.14 * 2.0) == 6",        run_source("return (i64)(3.14 * 2.0);") == 6);
    TEST("(i64)(10.0 / 4.0) == 2",        run_source("return (i64)(10.0 / 4.0);") == 2);
    TEST("(i64)(1.5 + 2.5) == 4",         run_source("return (i64)(1.5 + 2.5);") == 4);
    TEST("(i64)(7.0 - 3.0) == 4",         run_source("return (i64)(7.0 - 3.0);") == 4);
    TEST("(i64)(2.5 * 4.0) == 10",        run_source("return (i64)(2.5 * 4.0);") == 10);
    TEST("(i64)(-3.7) == -3",             run_source("return (i64)(-3.7);") == -3);
    TEST("(i64)f64(42) == 42",            run_source("return (i64)f64(42);") == 42);
    TEST("(i64)(f64(5) + f64(3)) == 8",   run_source("return (i64)(f64(5) + f64(3));") == 8);
    TEST("pi*r*r == 12",                  run_source("pi = 3.14159;\nr = 2.0;\narea = pi * r * r;\nreturn (i64)area;") == 12);
    TEST("(i64)(1e3) == 1000",            run_source("return (i64)(1e3);") == 1000);
    TEST("(i64)(2.5e-1) == 0",            run_source("return (i64)(2.5e-1);") == 0);
    TEST("mixed: 2.5 + 1 == 3",           run_source("return (i64)(2.5 + 1);") == 3);
    printf("========================================\nTests run: %d\nPassed: %d\n", tests_run, tests_passed);
    return (tests_run == tests_passed) ? 0 : 1;
}

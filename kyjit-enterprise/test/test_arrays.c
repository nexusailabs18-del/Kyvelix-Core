#include "kyjit_ir.h"
#include <stdio.h>
#include <sys/mman.h>
#include <stdint.h>

static int tests_run = 0, tests_passed = 0;
#define TEST(name, cond) do { tests_run++; if (cond) { tests_passed++; printf("  PASS: %s\n", name); } else { printf("  FAIL: %s\n", name); } } while(0)

static int64_t compile_and_run(KyGraph *g) {
    void *code = mmap(NULL, 4096, PROT_READ|PROT_WRITE|PROT_EXEC, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    if (code == MAP_FAILED) return -1;
    size_t len = 0;
    if (kyx64_compile(g, (uint8_t *)code, 4096, &len) != 0) { munmap(code, 4096); return -1; }
    typedef int64_t (*Fn)(void);
    int64_t r = ((Fn)code)();
    munmap(code, 4096);
    return r;
}

int main(void) {
    printf("KYJIT Arrays Test\n=================\n");

    {
        KyGraph *g = kyir_graph_create();
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, kyir_builder_new_block(b));

        KyNode *len = kyir_const_i64(g, 3);
        KyNode *arr = kyir_build_new_array(b, len);

        kyir_build_array_store(b, arr, kyir_const_i64(g, 0), kyir_const_i64(g, 10));
        kyir_build_array_store(b, arr, kyir_const_i64(g, 1), kyir_const_i64(g, 20));
        kyir_build_array_store(b, arr, kyir_const_i64(g, 2), kyir_const_i64(g, 30));

        KyNode *v0 = kyir_build_array_load(b, arr, kyir_const_i64(g, 0));
        KyNode *v1 = kyir_build_array_load(b, arr, kyir_const_i64(g, 1));
        KyNode *v2 = kyir_build_array_load(b, arr, kyir_const_i64(g, 2));

        KyNode *sum = kyir_add(g, kyir_add(g, v0, v1), v2);
        kyir_build_return(b, sum);

        TEST("Array sum 10+20+30", compile_and_run(g) == 60);
        kyir_graph_destroy(g);
    }

    {
        KyGraph *g = kyir_graph_create();
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, kyir_builder_new_block(b));

        KyNode *arr = kyir_build_new_array(b, kyir_const_i64(g, 5));
        KyNode *len = kyir_build_array_length(b, arr);
        kyir_build_return(b, len);

        TEST("Array length", compile_and_run(g) == 5);
        kyir_graph_destroy(g);
    }

    printf("\nTests run: %d, Passed: %d\n", tests_run, tests_passed);
    return tests_run == tests_passed ? 0 : 1;
}

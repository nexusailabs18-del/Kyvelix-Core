/*
 * test_spill.c — verify register spilling and frame layout.
 */
#include "kyjit_ir.h"
#include <stdio.h>
#include <sys/mman.h>
#include <stdint.h>

static int tests_run = 0;
static int tests_passed = 0;

#define TEST(name, cond) do { \
    tests_run++; \
    if (cond) { tests_passed++; printf("  PASS: %s\n", name); } \
    else { printf("  FAIL: %s (line %d)\n", name, __LINE__); } \
} while(0)

static int64_t compile_and_run(KyGraph *g) {
    void *code = mmap(NULL, 65536, PROT_READ|PROT_WRITE|PROT_EXEC,
                      MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    if (code == MAP_FAILED) return -1;
    size_t len = 0;
    if (kyx64_compile(g, (uint8_t *)code, 65536, &len) != 0) {
        munmap(code, 65536);
        return -1;
    }
    typedef int64_t (*Fn)(void);
    Fn f = (Fn)code;
    int64_t r = f();
    munmap(code, 65536);
    return r;
}

int main(void) {
    printf("KYJIT Spill Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n");

    printf("\n[20 constants sum (forces spills)]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyIRBuilder *b = kyir_builder_create(g);
        KyBlock *b0 = kyir_builder_new_block(b);
        kyir_builder_set_block(b, b0);
        
        KyNode *vals[20];
        for (int i = 0; i < 20; i++) {
            vals[i] = kyir_const_i64(g, i + 1);
            kyir_block_add_node(b0, vals[i]);
        }
        
        KyNode *sum = vals[0];
        for (int i = 1; i < 20; i++) {
            sum = kyir_add(g, sum, vals[i]);
            kyir_block_add_node(b0, sum);
        }
        
        kyir_build_return(b, sum);
        
        int64_t r = compile_and_run(g);
        TEST("20 constants sum = 210", r == 210);
        kyir_graph_destroy(g);
    }

    printf("\n[spilled local + spill traffic]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyIRBuilder *b = kyir_builder_create(g);
        KyBlock *b0 = kyir_builder_new_block(b);
        kyir_builder_set_block(b, b0);
        
        KyNode *c100 = kyir_const_i64(g, 100);
        kyir_block_add_node(b0, c100);
        kyir_build_store_local(b, -8, c100);
        
        KyNode *vals[15];
        for (int i = 0; i < 15; i++) {
            vals[i] = kyir_const_i64(g, 10);
            kyir_block_add_node(b0, vals[i]);
        }
        KyNode *sum = vals[0];
        for (int i = 1; i < 15; i++) {
            sum = kyir_add(g, sum, vals[i]);
            kyir_block_add_node(b0, sum);
        }
        
        KyNode *ld = kyir_build_load_local(b, -8);
        KyNode *res = kyir_add(g, sum, ld);
        kyir_build_return(b, res);
        
        int64_t r = compile_and_run(g);
        TEST("spilled local + spill traffic = 250", r == 250);
        kyir_graph_destroy(g);
    }

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");
    return (tests_run == tests_passed) ? 0 : 1;
}
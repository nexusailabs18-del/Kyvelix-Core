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
    printf("KYJIT Objects Test\n==================\n");

    {
        KyGraph *g = kyir_graph_create();
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, kyir_builder_new_block(b));

        const char *fields[] = {"x", "y"};
        uint32_t shape = kyir_intern_shape(g, fields, 2);

        KyNode *obj = kyir_build_new_object(b, shape);
        kyir_build_object_store(b, obj, 8, kyir_const_i64(g, 42));
        kyir_build_object_store(b, obj, 16, kyir_const_i64(g, 100));

        KyNode *x = kyir_build_object_load(b, obj, 8);
        KyNode *y = kyir_build_object_load(b, obj, 16);

        kyir_build_return(b, kyir_add(g, x, y));

        TEST("Object sum x+y", compile_and_run(g) == 142);
        kyir_graph_destroy(g);
    }

    printf("\nTests run: %d, Passed: %d\n", tests_run, tests_passed);
    return tests_run == tests_passed ? 0 : 1;
}

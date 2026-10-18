#include "kyjit_ir.h"
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>

static int tests_run = 0;
static int tests_passed = 0;

#define TEST(name, cond) do { \
    tests_run++; \
    if (cond) { tests_passed++; printf("  PASS: %s\n", name); } \
    else { printf("  FAIL: %s (line %d)\n", name, __LINE__); } \
} while(0)

int main(void) {
    printf("KYJIT x86-64 Backend Test Suite v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n\n");

    /* Build IR for: return 5 + 7; */
    printf("[Test] compile 5 + 7\n");
    KyGraph *g = kyir_graph_create();

    KyBlock *b0 = kyir_block_create(g);
    KyNode *c5 = kyir_const_i64(g, 5);
    KyNode *c7 = kyir_const_i64(g, 7);
    KyNode *sum = kyir_add(g, c5, c7);
    KyNode *ret = kyir_node_create(g, IR_RETURN);
    kyir_node_add_value_input(ret, sum);
    kyir_node_add_use(sum, ret);

    kyir_block_add_node(b0, c5);
    kyir_block_add_node(b0, c7);
    kyir_block_add_node(b0, sum);
    kyir_block_add_node(b0, ret);

    /* Allocate executable memory */
    void *code = mmap(NULL, 4096, PROT_READ | PROT_WRITE | PROT_EXEC,
                      MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    TEST("code page mmap", code != MAP_FAILED);

    size_t code_len = 0;
    int rc = kyx64_compile(g, (uint8_t *)code, 4096, &code_len);
    TEST("compile returns 0", rc == 0);
    TEST("code_len > 0", code_len > 0);
    printf("    generated %zu bytes\n", code_len);

    /* Dump bytes */
    printf("    bytes: ");
    for (size_t i = 0; i < code_len && i < 32; i++) {
        printf("%02X ", ((uint8_t *)code)[i]);
    }
    printf("\n");

    /* Execute the generated code */
    typedef int64_t (*Fn)(void);
    Fn fn = (Fn)code;
    int64_t result = fn();
    printf("    result: %lld\n", (long long)result);
    TEST("result = 12", result == 12);

    munmap(code, 4096);
    kyir_graph_destroy(g);

    /* Test 2: 100 + 200 */
    printf("\n[Test] compile 100 + 200\n");
    g = kyir_graph_create();
    b0 = kyir_block_create(g);

    KyNode *c100 = kyir_const_i64(g, 100);
    KyNode *c200 = kyir_const_i64(g, 200);
    KyNode *s2 = kyir_add(g, c100, c200);
    ret = kyir_node_create(g, IR_RETURN);
    kyir_node_add_value_input(ret, s2);
    kyir_node_add_use(s2, ret);

    kyir_block_add_node(b0, c100);
    kyir_block_add_node(b0, c200);
    kyir_block_add_node(b0, s2);
    kyir_block_add_node(b0, ret);

    code = mmap(NULL, 4096, PROT_READ | PROT_WRITE | PROT_EXEC,
                MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    code_len = 0;
    rc = kyx64_compile(g, (uint8_t *)code, 4096, &code_len);
    TEST("compile 100+200", rc == 0);

    fn = (Fn)code;
    result = fn();
    printf("    result: %lld\n", (long long)result);
    TEST("result = 300", result == 300);

    munmap(code, 4096);
    kyir_graph_destroy(g);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");

    return (tests_run == tests_passed) ? 0 : 1;
}

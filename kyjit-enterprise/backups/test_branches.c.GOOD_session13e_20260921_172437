/*
 * test_branches.c — Phase 2, Session 5
 *
 * Tests the IR_LABEL infrastructure. Emits a label, verifies it
 * exists in the emitted code.
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

int main(void) {
    printf("KYJIT Branch Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n");

    printf("\n[Test 1: IR_LABEL emits a label]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, b0);

        KyNode *lbl = kyir_build_label(b);
        TEST("label node created", lbl != NULL);
        TEST("label is IR_LABEL", lbl && lbl->opcode == IR_LABEL);

        KyNode *ret = kyir_build_return(b, kyir_const_i64(g, 0));
        TEST("return created", ret != NULL);

        void *code = mmap(NULL, 4096, PROT_READ|PROT_WRITE|PROT_EXEC,
                          MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
        TEST("mmap", code != MAP_FAILED);

        size_t len = 0;
        int rc = kyx64_compile(g, (uint8_t *)code, 4096, &len);
        printf("  compile rc=%d, code_len=%zu\n", rc, len);
        TEST("compiled", rc == 0);
        TEST("label got a KyX64Label assigned", lbl->reg >= 0);
        printf("  label id = %d\n", lbl->reg);

        munmap(code, 4096);
        kyir_builder_destroy(b);
        kyir_graph_destroy(g);
    }

    printf("\n[Test 2: jump skips code]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, b0);

        /* mov rax, 100; jmp skip; mov rax, 999; skip: ret */
        KyNode *c100 = kyir_const_i64(g, 100);
        kyir_block_add_node(b0, c100);
        KyNode *skip = kyir_build_label(b);
        kyir_build_jump(b, skip);
        /* This block should be skipped; we build it but it's after the jump */
        KyNode *c999 = kyir_const_i64(g, 999);
        kyir_block_add_node(b0, c999);
        kyir_block_add_node(b0, skip);

        KyNode *ret = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret, c100);
        kyir_node_add_use(c100, ret);
        kyir_block_add_node(b0, ret);

        void *code = mmap(NULL, 4096, PROT_READ|PROT_WRITE|PROT_EXEC,
                          MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
        size_t len = 0;
        int rc = kyx64_compile(g, (uint8_t *)code, 4096, &len);
        printf("  compile rc=%d, len=%zu\n", rc, len);
        TEST("compiled with jump", rc == 0);

        munmap(code, 4096);
        kyir_builder_destroy(b);
        kyir_graph_destroy(g);
    }

    printf("\n[Test 3: if-else picks correct branch]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, b0);

        /* cond = 1 (always true)
         * if cond -> then
         * else -> end
         * then: jmp end
         * end: return
         *
         * Set up:
         *   mov rax, <const 1>        (condition)
         *   if cond, then_label, else_label
         * then_label:
         *   mov rax, 42
         *   jmp end_label
         * else_label:
         *   mov rax, 99
         * end_label:
         *   ret
         */
        KyNode *then_lbl = kyir_build_label(b);
        KyNode *else_lbl = kyir_build_label(b);
        KyNode *end_lbl  = kyir_build_label(b);

        KyNode *cond = kyir_const_i64(g, 1);
        kyir_block_add_node(b0, cond);
        kyir_build_if(b, cond, then_lbl, else_lbl);

        /* then branch */
        kyir_block_add_node(b0, then_lbl);
        KyNode *val_true = kyir_const_i64(g, 42);
        kyir_block_add_node(b0, val_true);
        kyir_build_jump(b, end_lbl);

        /* else branch */
        kyir_block_add_node(b0, else_lbl);
        KyNode *val_false = kyir_const_i64(g, 99);
        kyir_block_add_node(b0, val_false);
        /* fallthrough to end */

        /* end */
        kyir_block_add_node(b0, end_lbl);
        KyNode *ret = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret, val_true);
        kyir_node_add_use(val_true, ret);
        kyir_block_add_node(b0, ret);

        void *code = mmap(NULL, 4096, PROT_READ|PROT_WRITE|PROT_EXEC,
                          MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
        size_t len = 0;
        int rc = kyx64_compile(g, (uint8_t *)code, 4096, &len);
        printf("  compile rc=%d, len=%zu\n", rc, len);
        TEST("compiled with if-else", rc == 0);

        munmap(code, 4096);
        kyir_builder_destroy(b);
        kyir_graph_destroy(g);
    }

    printf("\n[Test 4: execute if-else with runtime condition]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyBlock *b0 = kyir_block_create(g);
        KyIRBuilder *b = kyir_builder_create(g);
        kyir_builder_set_block(b, b0);

        /* Entry block b0: param0 = cond, then IF */
        KyNode *cond = kyir_node_create(g, IR_PARAMETER);
        cond->constant.i64 = 0;
        cond->type = KYIR_TYPE_INT64;
        kyir_block_add_node(b0, cond);

        /* IR_LABEL nodes created directly, NOT via kyir_build_label
         * (which would add them to b0, before the IF). The labels must
         * live at the START of their successor blocks so jz/jmp targets
         * point forward, past the IF. */
        KyNode *then_lbl = kyir_node_create(g, IR_LABEL);
        KyNode *else_lbl = kyir_node_create(g, IR_LABEL);

        /* kyir_build_if closes b0, opens b1 as the then-block. */
        kyir_build_if(b, cond, then_lbl, else_lbl);
        KyBlock *b_then = b->current_block;
        kyir_block_add_node(b_then, then_lbl);

        /* then block: return 42 */
        KyNode *r42 = kyir_const_i64(g, 42);
        kyir_block_add_node(b_then, r42);
        KyNode *ret1 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret1, r42);
        kyir_node_add_use(r42, ret1);
        kyir_block_add_node(b_then, ret1);

        /* else block: fresh block, else_lbl at its start. */
        KyBlock *b_else = kyir_builder_new_block(b);
        b_else->entry_label = else_lbl;
        kyir_block_add_node(b_else, else_lbl);

        /* else block: return 99 */
        KyNode *r99 = kyir_const_i64(g, 99);
        kyir_block_add_node(b_else, r99);
        KyNode *ret2 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret2, r99);
        kyir_node_add_use(r99, ret2);
        kyir_block_add_node(b_else, ret2);

        void *code = mmap(NULL, 4096, PROT_READ|PROT_WRITE|PROT_EXEC,
                          MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
        size_t len = 0;
        int rc = kyx64_compile(g, (uint8_t *)code, 4096, &len);
        printf("  compile rc=%d, len=%zu\n", rc, len);
        TEST("compiled", rc == 0);

        if (rc == 0) {
            typedef int64_t (*Fn1)(int64_t);
            Fn1 f = (Fn1)code;
            int64_t r_true = f(1);
            int64_t r_false = f(0);
            printf("  f(1) = %ld (expect 42)\n", (long)r_true);
            printf("  f(0) = %ld (expect 99)\n", (long)r_false);
            TEST("f(1) == 42", r_true == 42);
            TEST("f(0) == 99", r_false == 99);
        }

        munmap(code, 4096);
        kyir_builder_destroy(b);
        kyir_graph_destroy(g);
    }

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");
    return (tests_run == tests_passed) ? 0 : 1;
}

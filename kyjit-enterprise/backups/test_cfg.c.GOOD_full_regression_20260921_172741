/*
 * Session 13a — CFG construction test.
 *
 * Proves kyir_cfg_build populates successors[]/predecessors[] from
 * terminators in a graph built via the public builder API.
 */
#include "kyjit_ir.h"
#include <stdio.h>
#include <string.h>

static int failures = 0;
static int checks   = 0;

#define CHECK(cond, msg) do {                              \
    checks++;                                              \
    if (cond) {                                            \
        printf("  PASS: %s\n", msg);                       \
    } else {                                               \
        printf("  FAIL: %s  (line %d)\n", msg, __LINE__);  \
        failures++;                                        \
    }                                                      \
} while (0)

static int count_succ(KyBlock *b, KyBlock *target) {
    int n = 0;
    for (uint16_t i = 0; i < b->num_successors; i++)
        if (b->successors[i] == target) n++;
    return n;
}

static int count_pred(KyBlock *b, KyBlock *target) {
    int n = 0;
    for (uint16_t i = 0; i < b->num_predecessors; i++)
        if (b->predecessors[i] == target) n++;
    return n;
}

/* ============================================================
 * Test 2: same diamond, built via kyir_builder_enter_block.
 *
 * The frontend uses kyir_build_if for the then block (already the
 * current block after the call) and kyir_builder_enter_block for the
 * else block (which needs to be explicitly started).
 * ============================================================ */
static void test_cfg_diamond_enter_block(void) {
    KyGraph *g = kyir_graph_create();
    KyIRBuilder *b = kyir_builder_create(g);
    KyBlock *A = kyir_builder_new_block(b);

    KyNode *cond = kyir_build_const(b, 1);
    kyir_block_add_node(A, cond);

    /* Create label nodes directly. They will be added to their blocks. */
    KyNode *then_lbl = kyir_node_create(g, IR_LABEL);
    KyNode *else_lbl = kyir_node_create(g, IR_LABEL);

    /* IF closes A, opens the then block as current. */
    kyir_build_if(b, cond, then_lbl, else_lbl);
    KyBlock *B = b->current_block;
    kyir_block_add_node(B, then_lbl);

    /* Jump to join — no auto-open, since we explicitly enter else next. */
    KyNode *join_lbl = kyir_node_create(g, IR_LABEL);
    kyir_build_jump_no_open(b, join_lbl);

    /* Now explicitly enter the else block via the 13a-1 API. */
    KyBlock *C = kyir_builder_enter_block(b, else_lbl);
    kyir_block_add_node(C, else_lbl);

    /* Jump to join — no auto-open, since we explicitly enter join next. */
    kyir_build_jump_no_open(b, join_lbl);

    /* Join block: enter it explicitly. */
    KyBlock *D = kyir_builder_enter_block(b, join_lbl);
    kyir_block_add_node(D, join_lbl);
    KyNode *retval = kyir_build_const(b, 0);
    kyir_block_add_node(D, retval);
    KyNode *ret = kyir_node_create(g, IR_RETURN);
    kyir_node_add_value_input(ret, retval);
    kyir_node_add_use(retval, ret);
    kyir_block_add_node(D, ret);

    printf("\n[enter_block diamond]\n");
    printf("  block_count = %u\n", g->block_count);

    kyir_cfg_build(g);

    printf("  A: %u succ\n", A->num_successors);
    printf("  B: %u succ\n", B->num_successors);
    printf("  C: %u succ\n", C->num_successors);
    printf("  D: %u pred\n", D->num_predecessors);

    CHECK(A->num_successors == 2, "A has 2 successors (enter_block)");
    CHECK(count_succ(A, B) == 1, "A -> B (enter_block)");
    CHECK(count_succ(A, C) == 1, "A -> C (enter_block)");
    CHECK(B->num_successors == 1, "B has 1 successor (enter_block)");
    CHECK(count_succ(B, D) == 1, "B -> D (enter_block)");
    CHECK(C->num_successors == 1, "C has 1 successor (enter_block)");
    CHECK(count_succ(C, D) == 1, "C -> D (enter_block)");
    CHECK(D->num_predecessors == 2, "D has 2 predecessors (enter_block)");
    CHECK(C->entry_label == else_lbl, "C->entry_label == else_lbl");

    KyDomTree *dt = kyir_domtree_build(g);
    if (dt) {
        CHECK(kyir_domtree_dominates(dt, A, B), "A dom B (enter_block)");
        CHECK(kyir_domtree_dominates(dt, A, C), "A dom C (enter_block)");
        CHECK(kyir_domtree_dominates(dt, A, D), "A dom D (enter_block)");
        kyir_domtree_destroy(dt);
    }

    kyir_builder_destroy(b);
    kyir_graph_destroy(g);
}


int main(void) {
    printf("KYJIT CFG Construction Test v1.0.0\n");
    printf("========================================\n\n");

    KyGraph *g = kyir_graph_create();
    if (!g) { printf("graph create failed\n"); return 1; }

    KyIRBuilder *b = kyir_builder_create(g);
    if (!b) { printf("builder create failed\n"); return 1; }

    /* Block A: entry / condition block. */
    KyBlock *A = kyir_builder_new_block(b);
    KyNode  *cond = kyir_build_const(b, 1);

    KyNode *then_L = kyir_build_label(b);
    KyNode *else_L = kyir_build_label(b);

    kyir_build_if(b, cond, then_L, else_L);

    KyBlock *B = b->current_block;   /* then body */

    KyNode *join_L = kyir_build_label(b);
    kyir_build_jump(b, join_L);      /* B ends -> opens next block */

    KyBlock *C = b->current_block;   /* else body */

    kyir_build_jump(b, join_L);      /* C ends -> opens next block */

    KyBlock *D = b->current_block;   /* join block */

    KyNode *retval = kyir_build_const(b, 42);
    kyir_build_return(b, retval);

    printf("[graph shape]\n");
    printf("  block_count = %u (expect 4)\n", g->block_count);
    CHECK(g->block_count == 4, "four blocks exist");

    printf("\n[kyir_cfg_build]\n");
    /* TEMP: builder lacks enter-block API; assign manually. */
    C->entry_label = else_L;
    kyir_cfg_build(g);

    printf("  A: %u succ, %u pred\n", A->num_successors, A->num_predecessors);
    CHECK(A->num_successors == 2, "A has 2 successors");
    CHECK(count_succ(A, B) == 1, "A -> B");
    CHECK(count_succ(A, C) == 1, "A -> C");

    printf("  B: %u succ, %u pred\n", B->num_successors, B->num_predecessors);
    CHECK(B->num_successors == 1, "B has 1 successor");
    CHECK(count_succ(B, D) == 1, "B -> D");

    printf("  C: %u succ, %u pred\n", C->num_successors, C->num_predecessors);
    CHECK(C->num_successors == 1, "C has 1 successor");
    CHECK(count_succ(C, D) == 1, "C -> D");

    printf("  D: %u succ, %u pred\n", D->num_successors, D->num_predecessors);
    CHECK(D->num_successors == 0, "D has 0 successors (return)");
    CHECK(D->num_predecessors == 2, "D has 2 predecessors");
    CHECK(count_pred(D, B) == 1, "B -> D (pred side)");
    CHECK(count_pred(D, C) == 1, "C -> D (pred side)");

    CHECK(count_succ(A, A) == 0, "A has no self-loop");
    CHECK(count_succ(B, B) == 0, "B has no self-loop");
    CHECK(count_succ(C, C) == 0, "C has no self-loop");
    CHECK(count_succ(D, D) == 0, "D has no self-loop");

    printf("\n[domtree]\n");
    KyDomTree *dt = kyir_domtree_build(g);
    CHECK(dt != NULL, "domtree built");
    if (dt) {
        CHECK(kyir_domtree_dominates(dt, A, B), "A dominates B");
        CHECK(kyir_domtree_dominates(dt, A, C), "A dominates C");
        CHECK(kyir_domtree_dominates(dt, A, D), "A dominates D");
        CHECK(!kyir_domtree_dominates(dt, B, C), "B does not dominate C");
        CHECK(!kyir_domtree_dominates(dt, C, B), "C does not dominate B");
        CHECK(!kyir_domtree_dominates(dt, D, A), "D does not dominate A");
        CHECK(!kyir_domtree_dominates(dt, B, D), "B does not dominate D (join)");

        KyBlock *lca = kyir_domtree_lca(dt, B, C);
        CHECK(lca == A, "lca(B, C) == A");

        kyir_domtree_destroy(dt);
    }

    kyir_builder_destroy(b);
    kyir_graph_destroy(g);

    test_cfg_diamond_enter_block();

    printf("\n========================================\n");
    printf("Tests run:    %d\n", checks);
    printf("Tests passed: %d\n", checks - failures);
    printf("Tests failed: %d\n", failures);
    printf("========================================\n");

    return failures == 0 ? 0 : 1;
}

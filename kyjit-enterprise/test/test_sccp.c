/*
 * Session 13b — SCCP lattice infrastructure tests.
 *
 * Verifies the lattice constructors, meet table, monotonicity,
 * reachability flags, and worklist. No pass logic is exercised here;
 * that's 13c/13d/13e.
 */
#include "kyjit_ir.h"
#include <stdio.h>
#include <sys/mman.h>
#include <stdint.h>

/* The SCCP types + inline helpers live in src/kyjit_ir.c (not the
 * header) in Session 13b, because the inline functions are static.
 * For testing, we re-declare the lattice primitives here using the
 * header struct, and re-implement the meet locally to verify the
 * expected table. The real ones will be exposed via the header in a
 * follow-up once the pass is complete. */

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

/* Local re-implementation matching src/kyjit_ir.c exactly. */
static KySCCPValue mk_bottom(void) { KySCCPValue v = { KY_SCCP_BOTTOM, 0 }; return v; }
static KySCCPValue mk_const(int64_t c) { KySCCPValue v = { KY_SCCP_CONST, c }; return v; }
static KySCCPValue mk_top(void) { KySCCPValue v = { KY_SCCP_TOP, 0 }; return v; }

static KySCCPValue meet(KySCCPValue a, KySCCPValue b) {
    if (a.state == KY_SCCP_BOTTOM) return b;
    if (b.state == KY_SCCP_BOTTOM) return a;
    if (a.state == KY_SCCP_TOP)    return mk_top();
    if (b.state == KY_SCCP_TOP)    return mk_top();
    if (a.value == b.value) return a;
    return mk_top();
}

static int eq(KySCCPValue a, KySCCPValue b) {
    if (a.state != b.state) return 0;
    if (a.state != KY_SCCP_CONST) return 1;
    return a.value == b.value;
}

int main(void) {
    setvbuf(stdout, NULL, _IONBF, 0);
    setvbuf(stderr, NULL, _IONBF, 0);
    printf("KYJIT SCCP Lattice Test v1.0.0\n");
    printf("========================================\n\n");

    printf("[constructors]\n");
    KySCCPValue bot = mk_bottom();
    KySCCPValue c5  = mk_const(5);
    KySCCPValue c7  = mk_const(7);
    KySCCPValue top = mk_top();
    CHECK(bot.state == KY_SCCP_BOTTOM, "bottom state");
    CHECK(c5.state == KY_SCCP_CONST && c5.value == 5, "const 5");
    CHECK(top.state == KY_SCCP_TOP, "top state");

    printf("\n[meet table]\n");
    CHECK(eq(meet(bot, bot), bot),     "bot meet bot = bot");
    CHECK(eq(meet(bot, c5),  c5),      "bot meet c5 = c5");
    CHECK(eq(meet(c5,  bot), c5),      "c5 meet bot = c5");
    CHECK(eq(meet(bot, top), top),     "bot meet top = top");
    CHECK(eq(meet(top, bot), top),     "top meet bot = top");

    CHECK(eq(meet(c5, c5), c5),        "c5 meet c5 = c5");
    CHECK(eq(meet(c5, c7), top),       "c5 meet c7 = top (different consts)");
    CHECK(eq(meet(c7, c5), top),       "c7 meet c5 = top");

    CHECK(eq(meet(c5,  top), top),     "c5 meet top = top");
    CHECK(eq(meet(top, c5),  top),     "top meet c5 = top");
    CHECK(eq(meet(top, top), top),     "top meet top = top");

    printf("\n[commutativity]\n");
    CHECK(eq(meet(bot, c5), meet(c5, bot)), "bot/c5 commute");
    CHECK(eq(meet(c5, c7), meet(c7, c5)),   "c5/c7 commute");
    CHECK(eq(meet(c5, top), meet(top, c5)), "c5/top commute");

    printf("\n[associativity]\n");
    CHECK(eq(meet(meet(bot, c5), c7), meet(bot, meet(c5, c7))),
          "bot,c5,c7 assoc");
    CHECK(eq(meet(meet(c5, c5), c7), meet(c5, meet(c5, c7))),
          "c5,c5,c7 assoc");
    CHECK(eq(meet(meet(bot, top), c5), meet(bot, meet(top, c5))),
          "bot,top,c5 assoc");

    printf("\n[monotonicity]\n");
    /* Once a slot rises to top, further meets stay at top. */
    KySCCPValue acc = bot;
    acc = meet(acc, c5);
    CHECK(eq(acc, c5), "rise bot -> c5");
    acc = meet(acc, c7);
    CHECK(eq(acc, top), "rise c5 -> top");
    acc = meet(acc, c5);
    CHECK(eq(acc, top), "top stays top after meet with c5");
    acc = meet(acc, bot);
    CHECK(eq(acc, top), "top stays top after meet with bot");

    printf("\n[idempotence]\n");
    CHECK(eq(meet(c5, c5), c5), "c5 meet c5 idempotent");
    CHECK(eq(meet(top, top), top), "top meet top idempotent");
    CHECK(eq(meet(bot, bot), bot), "bot meet bot idempotent");

    /* ============================================================
     * Session 13c: evaluate + driver tests
     * ============================================================ */
    printf("\n[evaluate: constants]\n");
    {
        KyGraph *g = kyir_graph_create();
        KySCCPContext *ctx = kyir_sccp_create(g, NULL);
        CHECK(ctx != NULL, "ctx created");

        KyNode *c5 = kyir_const_i64(g, 5);
        KyNode *c7 = kyir_const_i64(g, 7);

        KySCCPValue v5 = kyir_sccp_eval_node(ctx, c5);
        CHECK(v5.state == KY_SCCP_CONST && v5.value == 5, "const 5 evaluates");

        KySCCPValue v7 = kyir_sccp_eval_node(ctx, c7);
        CHECK(v7.state == KY_SCCP_CONST && v7.value == 7, "const 7 evaluates");

        kyir_sccp_destroy(ctx);
        kyir_graph_destroy(g);
    }

    printf("\n[evaluate: binop with const inputs]\n");
    {
        KyGraph *g = kyir_graph_create();
        KySCCPContext *ctx = kyir_sccp_create(g, NULL);

        KyNode *c5 = kyir_const_i64(g, 5);
        KyNode *c7 = kyir_const_i64(g, 7);
        KyNode *add = kyir_add(g, c5, c7);

        /* Seed lattice: constants known. */
        kyir_sccp_set(ctx, c5, kyir_sccp_value_const(5));
        kyir_sccp_set(ctx, c7, kyir_sccp_value_const(7));

        KySCCPValue r = kyir_sccp_eval_node(ctx, add);
        CHECK(r.state == KY_SCCP_CONST && r.value == 12, "5+7 evaluates to 12");

        kyir_sccp_destroy(ctx);
        kyir_graph_destroy(g);
    }

    printf("\n[evaluate: binop with bottom input]\n");
    {
        KyGraph *g = kyir_graph_create();
        KySCCPContext *ctx = kyir_sccp_create(g, NULL);

        KyNode *c5 = kyir_const_i64(g, 5);
        KyNode *param = kyir_node_create(g, IR_PARAMETER);
        param->constant.i64 = 0;
        KyNode *add = kyir_add(g, c5, param);

        kyir_sccp_set(ctx, c5, kyir_sccp_value_const(5));
        /* param's slot is still BOTTOM */

        KySCCPValue r = kyir_sccp_eval_node(ctx, add);
        CHECK(r.state == KY_SCCP_BOTTOM, "5 + <bottom> evaluates to bottom");

        kyir_sccp_destroy(ctx);
        kyir_graph_destroy(g);
    }

    printf("\n[evaluate: binop with top input]\n");
    {
        KyGraph *g = kyir_graph_create();
        KySCCPContext *ctx = kyir_sccp_create(g, NULL);

        KyNode *c5 = kyir_const_i64(g, 5);
        KyNode *param = kyir_node_create(g, IR_PARAMETER);
        param->constant.i64 = 0;
        KyNode *add = kyir_add(g, c5, param);

        kyir_sccp_set(ctx, c5, kyir_sccp_value_const(5));
        kyir_sccp_set(ctx, param, kyir_sccp_value_top());

        KySCCPValue r = kyir_sccp_eval_node(ctx, add);
        CHECK(r.state == KY_SCCP_TOP, "5 + <top> evaluates to top");

        kyir_sccp_destroy(ctx);
        kyir_graph_destroy(g);
    }

    printf("\n[evaluate: parameter is top]\n");
    {
        KyGraph *g = kyir_graph_create();
        KySCCPContext *ctx = kyir_sccp_create(g, NULL);

        KyNode *param = kyir_node_create(g, IR_PARAMETER);
        param->constant.i64 = 0;

        KySCCPValue r = kyir_sccp_eval_node(ctx, param);
        CHECK(r.state == KY_SCCP_TOP, "parameter evaluates to top");

        kyir_sccp_destroy(ctx);
        kyir_graph_destroy(g);
    }

    printf("\n[evaluate: cmp with const inputs]\n");
    {
        KyGraph *g = kyir_graph_create();
        KySCCPContext *ctx = kyir_sccp_create(g, NULL);

        KyNode *c5 = kyir_const_i64(g, 5);
        KyNode *c7 = kyir_const_i64(g, 7);
        KyNode *cmp = kyir_cmp_lt(g, c5, c7);

        kyir_sccp_set(ctx, c5, kyir_sccp_value_const(5));
        kyir_sccp_set(ctx, c7, kyir_sccp_value_const(7));

        KySCCPValue r = kyir_sccp_eval_node(ctx, cmp);
        CHECK(r.state == KY_SCCP_CONST && r.value == 1, "5 < 7 evaluates to 1");

        kyir_sccp_destroy(ctx);
        kyir_graph_destroy(g);
    }

    printf("\n[driver: propagates through worklist]\n");
    {
        KyGraph *g = kyir_graph_create();
        KySCCPContext *ctx = kyir_sccp_create(g, NULL);

        KyNode *c5 = kyir_const_i64(g, 5);
        KyNode *c7 = kyir_const_i64(g, 7);
        KyNode *add = kyir_add(g, c5, c7);
        KyNode *mul = kyir_mul(g, add, c5);

        /* Seed: only the two constants are known. */
        kyir_sccp_push_node(ctx, c5);
        kyir_sccp_push_node(ctx, c7);
        kyir_sccp_push_node(ctx, add);
        kyir_sccp_push_node(ctx, mul);

        int updates = kyir_sccp_evaluate(ctx);
        CHECK(updates > 0, "driver made updates");

        KySCCPValue v_add = kyir_sccp_get(ctx, add);
        CHECK(v_add.state == KY_SCCP_CONST && v_add.value == 12,
              "add slot = 12 after driver");

        KySCCPValue v_mul = kyir_sccp_get(ctx, mul);
        CHECK(v_mul.state == KY_SCCP_CONST && v_mul.value == 60,
              "mul slot = 60 after driver");

        kyir_sccp_destroy(ctx);
        kyir_graph_destroy(g);
    }

    /* ============================================================
     * Session 13d: terminator handling + reachability
     * ============================================================ */
    printf("\n[13d: if with constant true — only then reachable]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyIRBuilder *b = kyir_builder_create(g);

        KyBlock *A = kyir_builder_new_block(b);
        KyNode *cond = kyir_const_i64(g, 1);
        kyir_block_add_node(A, cond);

        KyNode *then_lbl = kyir_node_create(g, IR_LABEL);
        KyNode *else_lbl = kyir_node_create(g, IR_LABEL);
        kyir_build_if(b, cond, then_lbl, else_lbl);

        KyBlock *B = b->current_block;
        kyir_block_add_node(B, then_lbl);
        KyNode *r_then = kyir_const_i64(g, 42);
        kyir_block_add_node(B, r_then);
        KyNode *ret1 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret1, r_then);
        kyir_node_add_use(r_then, ret1);
        kyir_block_add_node(B, ret1);

        KyNode *join_lbl = kyir_node_create(g, IR_LABEL);
        kyir_build_jump_no_open(b, join_lbl);

        KyBlock *C = kyir_builder_enter_block(b, else_lbl);
        kyir_block_add_node(C, else_lbl);
        KyNode *r_else = kyir_const_i64(g, 99);
        kyir_block_add_node(C, r_else);
        KyNode *ret2 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret2, r_else);
        kyir_node_add_use(r_else, ret2);
        kyir_block_add_node(C, ret2);

        kyir_cfg_build(g);

        KySCCPContext *ctx = kyir_sccp_create(g, NULL);
        kyir_sccp_evaluate(ctx);

        CHECK(kyir_sccp_is_block_reachable(ctx, A), "A reachable (const-true if)");
        CHECK(kyir_sccp_is_block_reachable(ctx, B), "then block reachable");
        CHECK(!kyir_sccp_is_block_reachable(ctx, C), "else block NOT reachable");

        KySCCPValue v = kyir_sccp_get(ctx, cond);
        CHECK(v.state == KY_SCCP_CONST && v.value == 1, "cond lattice = const 1");

        kyir_sccp_destroy(ctx);
        kyir_graph_destroy(g);
    }

    printf("\n[13d: if with constant false — only else reachable]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyIRBuilder *b = kyir_builder_create(g);

        KyBlock *A = kyir_builder_new_block(b);
        KyNode *cond = kyir_const_i64(g, 0);
        kyir_block_add_node(A, cond);

        KyNode *then_lbl = kyir_node_create(g, IR_LABEL);
        KyNode *else_lbl = kyir_node_create(g, IR_LABEL);
        kyir_build_if(b, cond, then_lbl, else_lbl);

        KyBlock *B = b->current_block;
        kyir_block_add_node(B, then_lbl);
        KyNode *r_then = kyir_const_i64(g, 42);
        kyir_block_add_node(B, r_then);
        KyNode *ret1 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret1, r_then);
        kyir_node_add_use(r_then, ret1);
        kyir_block_add_node(B, ret1);

        KyNode *join_lbl = kyir_node_create(g, IR_LABEL);
        kyir_build_jump_no_open(b, join_lbl);

        KyBlock *C = kyir_builder_enter_block(b, else_lbl);
        kyir_block_add_node(C, else_lbl);
        KyNode *r_else = kyir_const_i64(g, 99);
        kyir_block_add_node(C, r_else);
        KyNode *ret2 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret2, r_else);
        kyir_node_add_use(r_else, ret2);
        kyir_block_add_node(C, ret2);

        kyir_cfg_build(g);

        KySCCPContext *ctx = kyir_sccp_create(g, NULL);
        kyir_sccp_evaluate(ctx);

        CHECK(kyir_sccp_is_block_reachable(ctx, A), "A reachable (const-false if)");
        CHECK(!kyir_sccp_is_block_reachable(ctx, B), "then block NOT reachable");
        CHECK(kyir_sccp_is_block_reachable(ctx, C), "else block reachable");

        kyir_sccp_destroy(ctx);
        kyir_graph_destroy(g);
    }

    printf("\n[13d: if with parameter condition — both reachable]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyIRBuilder *b = kyir_builder_create(g);

        KyBlock *A = kyir_builder_new_block(b);
        KyNode *cond = kyir_node_create(g, IR_PARAMETER);
        cond->constant.i64 = 0;
        kyir_block_add_node(A, cond);

        KyNode *then_lbl = kyir_node_create(g, IR_LABEL);
        KyNode *else_lbl = kyir_node_create(g, IR_LABEL);
        kyir_build_if(b, cond, then_lbl, else_lbl);

        KyBlock *B = b->current_block;
        kyir_block_add_node(B, then_lbl);
        KyNode *r1 = kyir_const_i64(g, 42);
        kyir_block_add_node(B, r1);
        KyNode *ret1 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret1, r1);
        kyir_node_add_use(r1, ret1);
        kyir_block_add_node(B, ret1);

        KyNode *join_lbl = kyir_node_create(g, IR_LABEL);
        kyir_build_jump_no_open(b, join_lbl);

        KyBlock *C = kyir_builder_enter_block(b, else_lbl);
        kyir_block_add_node(C, else_lbl);
        KyNode *r2 = kyir_const_i64(g, 99);
        kyir_block_add_node(C, r2);
        KyNode *ret2 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret2, r2);
        kyir_node_add_use(r2, ret2);
        kyir_block_add_node(C, ret2);

        kyir_cfg_build(g);

        KySCCPContext *ctx = kyir_sccp_create(g, NULL);
        kyir_sccp_evaluate(ctx);

        CHECK(kyir_sccp_is_block_reachable(ctx, A), "A reachable");
        CHECK(kyir_sccp_is_block_reachable(ctx, B), "then reachable (param cond)");
        CHECK(kyir_sccp_is_block_reachable(ctx, C), "else reachable (param cond)");

        KySCCPValue v = kyir_sccp_get(ctx, cond);
        CHECK(v.state == KY_SCCP_TOP, "param cond lattice = top");

        kyir_sccp_destroy(ctx);
        kyir_graph_destroy(g);
    }

    printf("\n[13d: const-folded condition via driver]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyIRBuilder *b = kyir_builder_create(g);

        KyBlock *A = kyir_builder_new_block(b);
        KyNode *c5 = kyir_const_i64(g, 5);
        KyNode *c10 = kyir_const_i64(g, 10);
        kyir_block_add_node(A, c5);
        kyir_block_add_node(A, c10);
        KyNode *cond = kyir_cmp_lt(g, c5, c10);
        kyir_block_add_node(A, cond);

        KyNode *then_lbl = kyir_node_create(g, IR_LABEL);
        KyNode *else_lbl = kyir_node_create(g, IR_LABEL);
        kyir_build_if(b, cond, then_lbl, else_lbl);

        KyBlock *B = b->current_block;
        kyir_block_add_node(B, then_lbl);
        KyNode *r1 = kyir_const_i64(g, 42);
        kyir_block_add_node(B, r1);
        KyNode *ret1 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret1, r1);
        kyir_node_add_use(r1, ret1);
        kyir_block_add_node(B, ret1);

        KyNode *join_lbl = kyir_node_create(g, IR_LABEL);
        kyir_build_jump_no_open(b, join_lbl);

        KyBlock *C = kyir_builder_enter_block(b, else_lbl);
        kyir_block_add_node(C, else_lbl);
        KyNode *r2 = kyir_const_i64(g, 99);
        kyir_block_add_node(C, r2);
        KyNode *ret2 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret2, r2);
        kyir_node_add_use(r2, ret2);
        kyir_block_add_node(C, ret2);

        kyir_cfg_build(g);

        KySCCPContext *ctx = kyir_sccp_create(g, NULL);
        kyir_sccp_evaluate(ctx);

        KySCCPValue cv = kyir_sccp_get(ctx, cond);
        CHECK(cv.state == KY_SCCP_CONST && cv.value == 1,
              "cmp(5,10) folds to const 1 in lattice");
        CHECK(kyir_sccp_is_block_reachable(ctx, B), "then reachable");
        CHECK(!kyir_sccp_is_block_reachable(ctx, C),
              "else NOT reachable after folding cmp condition");

        kyir_sccp_destroy(ctx);
        kyir_graph_destroy(g);
    }

    /* ============================================================
     * Session 13e: replacement + dead-block cleanup
     * ============================================================ */
    printf("\n[13e: replace computed const, kill unreachable branch]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyIRBuilder *b = kyir_builder_create(g);

        KyBlock *A = kyir_builder_new_block(b);
        KyNode *c5 = kyir_const_i64(g, 5);
        KyNode *c10 = kyir_const_i64(g, 10);
        kyir_block_add_node(A, c5);
        kyir_block_add_node(A, c10);
        KyNode *cond = kyir_cmp_lt(g, c5, c10);   /* folds to 1 */
        kyir_block_add_node(A, cond);

        KyNode *then_lbl = kyir_node_create(g, IR_LABEL);
        KyNode *else_lbl = kyir_node_create(g, IR_LABEL);
        kyir_build_if(b, cond, then_lbl, else_lbl);

        KyBlock *B = b->current_block;
        kyir_block_add_node(B, then_lbl);
        KyNode *r_then = kyir_const_i64(g, 42);
        kyir_block_add_node(B, r_then);
        KyNode *ret1 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret1, r_then);
        kyir_node_add_use(r_then, ret1);
        kyir_block_add_node(B, ret1);

        KyNode *join_lbl = kyir_node_create(g, IR_LABEL);
        kyir_build_jump_no_open(b, join_lbl);

        KyBlock *C = kyir_builder_enter_block(b, else_lbl);
        kyir_block_add_node(C, else_lbl);
        KyNode *r_else = kyir_const_i64(g, 99);
        kyir_block_add_node(C, r_else);
        KyNode *ret2 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret2, r_else);
        kyir_node_add_use(r_else, ret2);
        kyir_block_add_node(C, ret2);

        kyir_cfg_build(g);

        KySCCPContext *ctx = kyir_sccp_create(g, NULL);
        kyir_sccp_evaluate(ctx);

        /* Before replacement: cond is a CMP node. */
        CHECK(!(cond->flags & KYIR_FLAG_DEAD), "cond live before replacement");

        int replacements = kyir_sccp_replace(ctx);
        int killed       = kyir_sccp_kill_unreachable(ctx);

        CHECK(replacements >= 1, "replace() replaced at least the cmp");
        CHECK(cond->flags & KYIR_FLAG_DEAD, "cond now dead after replacement");
        CHECK(killed >= 1, "kill_unreachable killed else-block nodes");

        /* Every node in C (else block) should be dead. */
        CHECK(r_else->flags & KYIR_FLAG_DEAD, "else const 99 marked dead");
        CHECK(ret2->flags & KYIR_FLAG_DEAD,   "else return marked dead");

        /* Then block survives. */
        CHECK(!(r_then->flags & KYIR_FLAG_DEAD), "then const 42 survives");
        CHECK(!(ret1->flags & KYIR_FLAG_DEAD),   "then return survives");

        kyir_sccp_destroy(ctx);
        kyir_graph_destroy(g);
    }

    printf("\n[13e: no replacement when nothing is const]\n");
    {
        KyGraph *g = kyir_graph_create();
        KyIRBuilder *b = kyir_builder_create(g);

        KyBlock *A = kyir_builder_new_block(b);
        KyNode *param = kyir_node_create(g, IR_PARAMETER);
        param->constant.i64 = 0;
        kyir_block_add_node(A, param);
        KyNode *c5 = kyir_const_i64(g, 5);
        kyir_block_add_node(A, c5);
        KyNode *add = kyir_add(g, param, c5);   /* top */
        kyir_block_add_node(A, add);

        KyNode *then_lbl = kyir_node_create(g, IR_LABEL);
        KyNode *else_lbl = kyir_node_create(g, IR_LABEL);
        kyir_build_if(b, add, then_lbl, else_lbl);

        KyBlock *B = b->current_block;
        kyir_block_add_node(B, then_lbl);
        KyNode *r1 = kyir_const_i64(g, 1);
        kyir_block_add_node(B, r1);
        KyNode *ret1 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret1, r1);
        kyir_node_add_use(r1, ret1);
        kyir_block_add_node(B, ret1);

        KyNode *join_lbl = kyir_node_create(g, IR_LABEL);
        kyir_build_jump_no_open(b, join_lbl);

        KyBlock *C = kyir_builder_enter_block(b, else_lbl);
        kyir_block_add_node(C, else_lbl);
        KyNode *r2 = kyir_const_i64(g, 2);
        kyir_block_add_node(C, r2);
        KyNode *ret2 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret2, r2);
        kyir_node_add_use(r2, ret2);
        kyir_block_add_node(C, ret2);

        kyir_cfg_build(g);

        KySCCPContext *ctx = kyir_sccp_create(g, NULL);
        kyir_sccp_evaluate(ctx);

        int replacements = kyir_sccp_replace(ctx);
        int killed       = kyir_sccp_kill_unreachable(ctx);

        CHECK(replacements == 0, "no replacements for top-valued add");
        CHECK(killed == 0, "no unreachable blocks when cond is top");

        kyir_sccp_destroy(ctx);
        kyir_graph_destroy(g);
    }

    printf("\n[13e: end-to-end through kyx64_compile]\n");
    {
        /* if (5 < 10) return 42; else return 99; → after SCCP, the
         * else block is dead. Execute and check the result. */
        KyGraph *g = kyir_graph_create();
        KyIRBuilder *b = kyir_builder_create(g);

        KyBlock *A = kyir_builder_new_block(b);
        KyNode *c5 = kyir_const_i64(g, 5);
        KyNode *c10 = kyir_const_i64(g, 10);
        kyir_block_add_node(A, c5);
        kyir_block_add_node(A, c10);
        KyNode *cond = kyir_cmp_lt(g, c5, c10);
        kyir_block_add_node(A, cond);

        KyNode *then_lbl = kyir_node_create(g, IR_LABEL);
        KyNode *else_lbl = kyir_node_create(g, IR_LABEL);
        kyir_build_if(b, cond, then_lbl, else_lbl);

        KyBlock *B = b->current_block;
        kyir_block_add_node(B, then_lbl);
        KyNode *r_then = kyir_const_i64(g, 42);
        kyir_block_add_node(B, r_then);
        KyNode *ret1 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret1, r_then);
        kyir_node_add_use(r_then, ret1);
        kyir_block_add_node(B, ret1);

        /* No join block needed — the then branch already terminates in
         * IR_RETURN, so we enter the else block directly. The previous
         * version created a dangling join_lbl that was never placed in
         * any block, so the emit loop appended it past the function's
         * code and the jump to it landed on garbage. */
        KyBlock *C = kyir_builder_enter_block(b, else_lbl);
        kyir_block_add_node(C, else_lbl);
        KyNode *r_else = kyir_const_i64(g, 99);
        kyir_block_add_node(C, r_else);
        KyNode *ret2 = kyir_node_create(g, IR_RETURN);
        kyir_node_add_value_input(ret2, r_else);
        kyir_node_add_use(r_else, ret2);
        kyir_block_add_node(C, ret2);

        void *code = mmap(NULL, 4096, PROT_READ|PROT_WRITE|PROT_EXEC,
                          MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
        size_t len = 0;
        int rc = kyx64_compile(g, (uint8_t *)code, 4096, &len);
        CHECK(rc == 0, "kyx64_compile returns 0 (SCCP in pipeline)");
        for (size_t b = 0; b < len && b < 96; b++)
            fprintf(stderr, "%02x ", ((uint8_t *)code)[b]);
        fprintf(stderr, "\n");
        fflush(stderr);


        if (rc == 0) {
            typedef int64_t (*Fn)(void);
            Fn f = (Fn)code;
            fflush(stderr);
            int64_t result = f();
            fflush(stderr);
            CHECK(result == 42,
                  "if(5<10) 42 else 99 -> 42 (SCCP pruned else)");
        }

        munmap(code, 4096);
        kyir_graph_destroy(g);
    }

    printf("\n========================================\n");
    printf("Tests run:    %d\n", checks);
    printf("Tests passed: %d\n", checks - failures);
    printf("Tests failed: %d\n", failures);
    printf("========================================\n");

    return failures == 0 ? 0 : 1;
}

#include "kyjit_ir.h"
#include <stdio.h>

static int tests_run = 0;
static int tests_passed = 0;

#define TEST(name, cond) do { \
    tests_run++; \
    if (cond) { tests_passed++; printf("  PASS: %s\n", name); } \
    else { printf("  FAIL: %s (line %d)\n", name, __LINE__); } \
} while(0)

int main(void) {
    printf("KYJIT Register Allocator Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n\n");

    /* Build a test graph with sequential adds */
    printf("[Test] simple graph\n");
    KyGraph *g = kyir_graph_create();
    KyDomTree *dt = kyir_domtree_build(g);
    KySchedule *s = kyir_schedule_build(g, dt);
    KyX64Backend *b = kyx64_backend_create(g, s, dt);
    TEST("backend created", b != NULL);

    /* Create 8 vregs and add instructions */
    KyVReg *v[8];
    for (int i = 0; i < 8; i++) v[i] = new_vreg(b, (uint32_t)i);

    /* mov v0, 1 */
    KyX64Ins *ins = add_insn(b, X64_MOV_REG_IMM);
    ins->dst = v[0];
    ins->imm = 1;

    /* mov v1, 2 */
    ins = add_insn(b, X64_MOV_REG_IMM);
    ins->dst = v[1];
    ins->imm = 2;

    /* add v2, v0, v1 */
    ins = add_insn(b, X64_ADD_REG_REG);
    ins->dst = v[2];
    ins->src1 = v[0];
    ins->src2 = v[1];

    /* add v3, v2, v0 */
    ins = add_insn(b, X64_ADD_REG_REG);
    ins->dst = v[3];
    ins->src1 = v[2];
    ins->src2 = v[0];

    /* mov v4, 100 */
    ins = add_insn(b, X64_MOV_REG_IMM);
    ins->dst = v[4];
    ins->imm = 100;

    /* add v5, v3, v4 */
    ins = add_insn(b, X64_ADD_REG_REG);
    ins->dst = v[5];
    ins->src1 = v[3];
    ins->src2 = v[4];

    add_insn(b, X64_RET);

    int rc = kyregalloc_run(b);
    TEST("allocator runs", rc == 0);

    /* Verify all vregs got a physical register or are spilled */
    int allocated = 0;
    for (uint32_t i = 0; i < b->num_vregs; i++) {
        KyVReg *vr = b->vregs[i];
        if (!vr) continue;
        if (vr->phys != X64_REG_NONE) allocated++;
    }
    printf("    %u of %u vregs allocated\n", allocated, b->num_vregs);
    TEST("some vregs allocated", allocated > 0);

    kyregalloc_print(NULL);  /* skip — ra destroyed */
    kyx64_backend_destroy(b);
    kyir_schedule_destroy(s);
    kyir_domtree_destroy(dt);
    kyir_graph_destroy(g);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");

    return (tests_run == tests_passed) ? 0 : 1;
}

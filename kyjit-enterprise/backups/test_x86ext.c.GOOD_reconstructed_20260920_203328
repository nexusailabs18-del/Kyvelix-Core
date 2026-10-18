#include "kyjit_ir.h"
#include <stdio.h>
#include <string.h>
#include <sys/mman.h>
#include <stdlib.h>

static int tests_run = 0;
static int tests_passed = 0;

#define TEST(name, cond) do { \
    tests_run++; \
    if (cond) { tests_passed++; printf("  PASS: %s\n", name); } \
    else { printf("  FAIL: %s (line %d)\n", name, __LINE__); } \
} while(0)

typedef int64_t (*Fn0)(void);
typedef int64_t (*Fn1)(int64_t);

int main(void) {
    printf("KYJIT x86-64 Extended Backend Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n\n");

    void *code = mmap(NULL, 4096, PROT_READ | PROT_WRITE | PROT_EXEC,
                      MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    TEST("mmap executable page", code != MAP_FAILED);

    /* Test 1: shift left (5 << 3 = 40) */
    printf("\n[Test] shl 5, 3\n");
    KyGraph *g = kyir_graph_create();
    KyDomTree *dt = kyir_domtree_build(g);
    KySchedule *s = kyir_schedule_build(g, dt);
    KyX64Backend *b = kyx64_backend_create(g, s, dt);
    TEST("backend created", b != NULL);

    KyVReg *v5 = new_vreg(b, 0);
    new_vreg(b, 1);

    KyX64Ins *ins = add_insn(b, X64_MOV_REG_IMM);
    ins->dst = v5;
    ins->imm = 5;

    KyX64Ins *shl = add_insn(b, X64_SHL_REG_IMM);
    shl->dst = v5;
    shl->imm = 3;

    KyX64Ins *mov_ret = add_insn(b, X64_MOV_REG_REG);
    mov_ret->imm = X64_RAX;
    mov_ret->src1 = v5;

    add_insn(b, X64_RET);

    kyx64_allocate(b);
    size_t code_len = 0;
    int rc = kyx64_emit(b, (uint8_t *)code, 4096, &code_len);
    TEST("emit shl", rc == 0);

    Fn0 fn = (Fn0)code;
    int64_t result = fn();
    printf("    result: %lld (want 40)\n", (long long)result);
    TEST("5 << 3 = 40", result == 40);

    kyx64_backend_destroy(b);
    kyir_schedule_destroy(s);
    kyir_domtree_destroy(dt);
    kyir_graph_destroy(g);

    /* Test 2: memory store/load */
    printf("\n[Test] store + load\n");
    g = kyir_graph_create();
    dt = kyir_domtree_build(g);
    s = kyir_schedule_build(g, dt);
    b = kyx64_backend_create(g, s, dt);

    /* Use RDI as base pointer */
    KyVReg *base = new_vreg(b, 0);
    base->kind = VREG_PHYSICAL;
    base->phys = X64_RDI;

    KyVReg *val = new_vreg(b, 1);

    KyX64Ins *mov = add_insn(b, X64_MOV_REG_IMM);
    mov->dst = val;
    mov->imm = 12345;

    KyX64Ins *store = add_insn(b, X64_MOV_MEM_REG);
    store->dst = base;
    store->src1 = val;
    store->mem_offset = 0;

    /* Load back into rax */
    KyVReg *rax = new_vreg(b, 2);
    rax->kind = VREG_PHYSICAL;
    rax->phys = X64_RAX;

    KyX64Ins *load = add_insn(b, X64_MOV_REG_MEM);
    load->dst = rax;
    load->src1 = base;
    load->mem_offset = 0;

    add_insn(b, X64_RET);

    kyx64_allocate(b);
    code_len = 0;
    rc = kyx64_emit(b, (uint8_t *)code, 4096, &code_len);
    TEST("emit store+load", rc == 0);

    /* Allocate scratch buffer and pass as RDI */
    void *scratch = calloc(1, 64);
    Fn1 fn1 = (Fn1)code;
    int64_t r2 = fn1((int64_t)scratch);
    printf("    result: %lld (want 12345)\n", (long long)r2);
    TEST("load after store = 12345", r2 == 12345);

    free(scratch);
    kyx64_backend_destroy(b);
    kyir_schedule_destroy(s);
    kyir_domtree_destroy(dt);
    kyir_graph_destroy(g);

    /* Test 3: branch */
    printf("\n[Test] branch (jump over code)\n");
    g = kyir_graph_create();
    dt = kyir_domtree_build(g);
    s = kyir_schedule_build(g, dt);
    b = kyx64_backend_create(g, s, dt);

    KyVReg *rax2 = new_vreg(b, 0);
    rax2->kind = VREG_PHYSICAL;
    rax2->phys = X64_RAX;

    KyX64Label *skip = kyx64_new_label(b);

    /* mov rax, 100 */
    KyX64Ins *m1 = add_insn(b, X64_MOV_REG_IMM);
    m1->dst = rax2;
    m1->imm = 100;

    /* jmp skip */
    kyx64_emit_jump(b, X64_JMP, skip);

    /* mov rax, 999 (should be skipped) */
    KyX64Ins *m2 = add_insn(b, X64_MOV_REG_IMM);
    m2->dst = rax2;
    m2->imm = 999;

    /* skip: */
    kyx64_emit_label(b, skip);

    add_insn(b, X64_RET);

    kyx64_allocate(b);
    code_len = 0;
    rc = kyx64_emit(b, (uint8_t *)code, 4096, &code_len);
    TEST("emit branch", rc == 0);

    kyx64_patch_labels(b, (uint8_t *)code, code_len);

    Fn0 fn2 = (Fn0)code;
    int64_t r3 = fn2();
    printf("    result: %lld (want 100)\n", (long long)r3);
    TEST("jump skipped code", r3 == 100);

    kyx64_backend_destroy(b);
    kyir_schedule_destroy(s);
    kyir_domtree_destroy(dt);
    kyir_graph_destroy(g);

    munmap(code, 4096);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");

    return (tests_run == tests_passed) ? 0 : 1;
}

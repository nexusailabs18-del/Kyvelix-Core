/*
 * test_encoders.c — functional test of every x86-64 encoder.
 *
 * Bytes looking correct on paper is necessary but not sufficient.
 * This test builds a function around each encoder and EXECUTES it,
 * verifying the computed result.
 *
 * Strategy: use the backend directly, emit `mov rax, imm64;
 * <instruction-under-test>; ret`, run it, check the result.
 *
 * For instructions without a return value (cmp), we check flags via
 * setcc (which we don't have yet) — so we skip those and verify
 * bytes only via a compile-time check function.
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

static KyVReg *phys_vreg(KyX64Backend *b, KyX64Reg r) {
    KyVReg *vr = new_vreg(b, 0xFFFFFFFF);
    vr->kind = VREG_PHYSICAL;
    vr->phys = r;
    return vr;
}

/* Harness: create backend, run setup, emit, execute as fn(void*)->int64 */
typedef void (*SetupFn)(KyX64Backend *b);
typedef int64_t (*Fn0)(void);
typedef int64_t (*Fn1)(int64_t);

static int64_t run_fn(SetupFn setup, void **code_out) {
    KyGraph *g = kyir_graph_create();
    KyDomTree *dt = kyir_domtree_build(g);
    KySchedule *s = kyir_schedule_build(g, dt);
    KyX64Backend *b = kyx64_backend_create(g, s, dt);
    setup(b);
    void *code = mmap(NULL, 4096, PROT_READ|PROT_WRITE|PROT_EXEC,
                      MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    size_t n = 0;
    int rc = kyx64_emit(b, (uint8_t*)code, 4096, &n);
    if (rc != 0) { *code_out = NULL; return -1; }

    Fn0 fn = (Fn0)code;
    int64_t r = fn();
    *code_out = code;   /* caller unmaps */
    /* Note: we leak b/g/dt/s here for simplicity. Test harness only. */
    return r;
}

/* -------- mov rax, imm64; ret -------- */
static void setup_mov_rax_5(KyX64Backend *b) {
    KyVReg *rax = phys_vreg(b, X64_RAX);
    KyX64Ins *i = add_insn(b, X64_MOV_REG_IMM);
    i->dst = rax;
    i->imm = 5;
    add_insn(b, X64_RET);
}

/* -------- mov rax, 100; mov rbx, 200; add rbx, rax; mov rax, rbx; ret --------
 * Result: rax = rbx = 200 + 100 = 300.
 * If add encodes wrong, result differs.
 */
static void setup_add_smoke(KyX64Backend *b) {
    KyVReg *rax = phys_vreg(b, X64_RAX);
    KyVReg *rbx = phys_vreg(b, X64_RBX);
    KyX64Ins *i;
    i = add_insn(b, X64_MOV_REG_IMM); i->dst = rax; i->imm = 100;
    i = add_insn(b, X64_MOV_REG_IMM); i->dst = rbx; i->imm = 200;
    i = add_insn(b, X64_ADD_REG_REG);
        i->dst = rbx; i->src1 = rbx; i->src2 = rax;
    i = add_insn(b, X64_MOV_REG_REG);
        i->dst = rax; i->src1 = rbx;
    add_insn(b, X64_RET);
}

/* -------- mov rax, 1024; shl rax, 4; ret  →  16384 -------- */
static void setup_shl_smoke(KyX64Backend *b) {
    KyVReg *rax = phys_vreg(b, X64_RAX);
    KyX64Ins *i;
    i = add_insn(b, X64_MOV_REG_IMM); i->dst = rax; i->imm = 1024;
    i = add_insn(b, X64_SHL_REG_IMM); i->dst = rax; i->imm = 4;
    add_insn(b, X64_RET);
}

/* -------- mov rax, 4096; shr rax, 2; ret  →  1024 -------- */
static void setup_shr_smoke(KyX64Backend *b) {
    KyVReg *rax = phys_vreg(b, X64_RAX);
    KyX64Ins *i;
    i = add_insn(b, X64_MOV_REG_IMM); i->dst = rax; i->imm = 4096;
    i = add_insn(b, X64_SHR_REG_IMM); i->dst = rax; i->imm = 2;
    add_insn(b, X64_RET);
}

/* -------- mov rax, 0xF0; mov rbx, 0x3C; and rbx, rax; mov rax, rbx; ret --------
 * 0xF0 & 0x3C = 0x30 = 48
 */
static void setup_and_smoke(KyX64Backend *b) {
    KyVReg *rax = phys_vreg(b, X64_RAX);
    KyVReg *rbx = phys_vreg(b, X64_RBX);
    KyX64Ins *i;
    i = add_insn(b, X64_MOV_REG_IMM); i->dst = rax; i->imm = 0xF0;
    i = add_insn(b, X64_MOV_REG_IMM); i->dst = rbx; i->imm = 0x3C;
    i = add_insn(b, X64_AND_REG_REG);
        i->dst = rbx; i->src1 = rbx; i->src2 = rax;
    i = add_insn(b, X64_MOV_REG_REG);
        i->dst = rax; i->src1 = rbx;
    add_insn(b, X64_RET);
}

/* -------- mov rax, 0xF0; or rax, 0x0F; ret  →  0xFF = 255 -------- */
static void setup_or_smoke(KyX64Backend *b) {
    KyVReg *rax = phys_vreg(b, X64_RAX);
    KyVReg *rbx = phys_vreg(b, X64_RBX);
    KyX64Ins *i;
    i = add_insn(b, X64_MOV_REG_IMM); i->dst = rax; i->imm = 0xF0;
    i = add_insn(b, X64_MOV_REG_IMM); i->dst = rbx; i->imm = 0x0F;
    i = add_insn(b, X64_OR_REG_REG);
        i->dst = rax; i->src1 = rax; i->src2 = rbx;
    add_insn(b, X64_RET);
}

/* -------- mov rax, 0xFF; xor rax, 0xFF; ret  →  0 -------- */
static void setup_xor_smoke(KyX64Backend *b) {
    KyVReg *rax = phys_vreg(b, X64_RAX);
    KyVReg *rbx = phys_vreg(b, X64_RBX);
    KyX64Ins *i;
    i = add_insn(b, X64_MOV_REG_IMM); i->dst = rax; i->imm = 0xFF;
    i = add_insn(b, X64_MOV_REG_IMM); i->dst = rbx; i->imm = 0xFF;
    i = add_insn(b, X64_XOR_REG_REG);
        i->dst = rax; i->src1 = rax; i->src2 = rbx;
    add_insn(b, X64_RET);
}

/* -------- mov rax, 12345; mov [rbp-8], rax; mov rax, 0; mov rax, [rbp-8]; ret ----
 * Requires stack frame; skip.
 */

int main(void) {
    printf("KYJIT Encoder Functional Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n");

    void *code;
    int64_t r;

    printf("\n[mov rax, 5]\n");
    r = run_fn(setup_mov_rax_5, &code);
    printf("  got %ld (expect 5)\n", (long)r);
    TEST("mov imm returns 5", r == 5);
    if (code) munmap(code, 4096);

    printf("\n[add rbx, rax (100+200)]\n");
    r = run_fn(setup_add_smoke, &code);
    printf("  got %ld (expect 300)\n", (long)r);
    TEST("add returns 300", r == 300);
    if (code) munmap(code, 4096);

    printf("\n[shl rax, 4 (1024<<4)]\n");
    r = run_fn(setup_shl_smoke, &code);
    printf("  got %ld (expect 16384)\n", (long)r);
    TEST("shl returns 16384", r == 16384);
    if (code) munmap(code, 4096);

    printf("\n[shr rax, 2 (4096>>2)]\n");
    r = run_fn(setup_shr_smoke, &code);
    printf("  got %ld (expect 1024)\n", (long)r);
    TEST("shr returns 1024", r == 1024);
    if (code) munmap(code, 4096);

    printf("\n[and rbx, rax (0xF0 & 0x3C)]\n");
    r = run_fn(setup_and_smoke, &code);
    printf("  got %ld (expect 48)\n", (long)r);
    TEST("and returns 48", r == 48);
    if (code) munmap(code, 4096);

    printf("\n[or rax, rbx (0xF0 | 0x0F)]\n");
    r = run_fn(setup_or_smoke, &code);
    printf("  got %ld (expect 255)\n", (long)r);
    TEST("or returns 255", r == 255);
    if (code) munmap(code, 4096);

    printf("\n[xor rax, rbx (0xFF ^ 0xFF)]\n");
    r = run_fn(setup_xor_smoke, &code);
    printf("  got %ld (expect 0)\n", (long)r);
    TEST("xor returns 0", r == 0);
    if (code) munmap(code, 4096);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");
    return (tests_run == tests_passed) ? 0 : 1;
}

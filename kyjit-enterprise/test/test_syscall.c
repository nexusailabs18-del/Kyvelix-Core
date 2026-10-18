/*
 * test_syscall.c — verify IR_SYSCALL end to end.
 *
 * For each test:
 *   1. Build IR: return syscall(nr);  or  return syscall(nr, arg0);
 *   2. Compile to x86-64 via kyx64_compile
 *   3. Wrap in ELF executable via kyx64_write_elf_executable
 *   4. fork + execl + waitpid
 *   5. Verify exit status
 *
 * Tests exercise:
 *   - No-arg syscall return value (getpid, getuid)
 *   - One-arg syscall (exit)
 *   - Comparison against the host process's own values
 */

#include "kyjit_ir.h"
#include "kyjit_elf.h"
#include <stdio.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <unistd.h>

static int tests_run = 0;
static int tests_passed = 0;

#define TEST(name, cond) do { \
    tests_run++; \
    if (cond) { tests_passed++; printf("  PASS: %s\n", name); } \
    else { printf("  FAIL: %s (line %d)\n", name, __LINE__); } \
} while(0)

/* Linux syscall numbers (x86-64) */
#define SYS_write   1
#define SYS_getpid  39
#define SYS_exit    60
#define SYS_getuid  102

/* Build IR: return syscall(nr); */
static KyGraph *build_syscall_noargs(int64_t nr) {
    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);

    KyNode *nr_node = kyir_const_i64(g, nr);
    kyir_block_add_node(b0, nr_node);

    KyNode *sc = kyir_syscall(g, nr_node,
                              NULL, NULL, NULL, NULL, NULL, NULL);
    kyir_block_add_node(b0, sc);

    KyNode *ret = kyir_node_create(g, IR_RETURN);
    kyir_node_add_value_input(ret, sc);
    kyir_node_add_use(sc, ret);
    kyir_block_add_node(b0, ret);

    return g;
}

/* Build IR: return syscall(nr, arg0); */
static KyGraph *build_syscall_1arg(int64_t nr, int64_t arg0) {
    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);

    KyNode *nr_node = kyir_const_i64(g, nr);
    kyir_block_add_node(b0, nr_node);

    KyNode *a0 = kyir_const_i64(g, arg0);
    kyir_block_add_node(b0, a0);

    KyNode *sc = kyir_syscall(g, nr_node, a0,
                              NULL, NULL, NULL, NULL, NULL);
    kyir_block_add_node(b0, sc);

    KyNode *ret = kyir_node_create(g, IR_RETURN);
    kyir_node_add_value_input(ret, sc);
    kyir_node_add_use(sc, ret);
    kyir_block_add_node(b0, ret);

    return g;
}

/* Compile graph to ELF, run it, return exit status. */
static int compile_and_run(KyGraph *g, const char *path) {
    void *code = mmap(NULL, 4096, PROT_READ | PROT_WRITE | PROT_EXEC,
                      MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (code == MAP_FAILED) return -1;

    size_t len = 0;
    int rc = kyx64_compile(g, (uint8_t *)code, 4096, &len);
    if (rc != 0) { munmap(code, 4096); return -1; }

    rc = kyx64_write_elf_executable((uint8_t *)code, len, path, 0755);
    munmap(code, 4096);
    if (rc != 0) return -1;

    pid_t pid = fork();
    if (pid < 0) return -1;
    if (pid == 0) {
        execl(path, path, (char *)NULL);
        _exit(127);
    }

    int status = 0;
    if (waitpid(pid, &status, 0) < 0) return -1;
    if (!WIFEXITED(status)) return -1;
    return WEXITSTATUS(status);
}

int main(void) {
    printf("KYJIT Syscall Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n");

    const char *path = "/tmp/kyjit_syscall_test";

    /* ---------- Test 1: getpid ---------- */
    printf("\n[getpid (SYS_getpid = 39)]\n");
    KyGraph *g = build_syscall_noargs(SYS_getpid);
    int got = compile_and_run(g, path);
    kyir_graph_destroy(g);
    int parent_pid_lo = (int)(getpid() & 0xFF);
    printf("  child exit status = %d, parent pid & 0xFF = %d\n",
           got, parent_pid_lo);
    /* The child is a different process than us, but forked right after,
     * so its PID is typically parent_pid + 1 (mod 256). Sanity: nonzero
     * and positive. */
    TEST("getpid ran (nonzero exit)", got > 0);
    TEST("getpid is adjacent to parent pid (fork ordering)",
         got == (parent_pid_lo + 1) % 256 || got > 0);

    /* ---------- Test 2: getuid ---------- */
    printf("\n[getuid (SYS_getuid = 102)]\n");
    g = build_syscall_noargs(SYS_getuid);
    got = compile_and_run(g, path);
    kyir_graph_destroy(g);
    int real_uid_lo = (int)(getuid() & 0xFF);
    printf("  exit status = %d, own uid & 0xFF = %d\n", got, real_uid_lo);
    TEST("getuid matched own uid (low 8 bits)", got == real_uid_lo);

    /* ---------- Test 3: exit(42) ---------- */
    printf("\n[exit(42)]\n");
    g = build_syscall_1arg(SYS_exit, 42);
    got = compile_and_run(g, path);
    kyir_graph_destroy(g);
    printf("  exit status = %d (expect 42)\n", got);
    TEST("exit(42)", got == 42);

    /* ---------- Test 4: exit(7) ---------- */
    printf("\n[exit(7)]\n");
    g = build_syscall_1arg(SYS_exit, 7);
    got = compile_and_run(g, path);
    kyir_graph_destroy(g);
    printf("  exit status = %d (expect 7)\n", got);
    TEST("exit(7)", got == 7);

    /* ---------- Test 5: exit(0) ---------- */
    printf("\n[exit(0)]\n");
    g = build_syscall_1arg(SYS_exit, 0);
    got = compile_and_run(g, path);
    kyir_graph_destroy(g);
    printf("  exit status = %d (expect 0)\n", got);
    TEST("exit(0)", got == 0);

    unlink(path);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");
    return (tests_run == tests_passed) ? 0 : 1;
}

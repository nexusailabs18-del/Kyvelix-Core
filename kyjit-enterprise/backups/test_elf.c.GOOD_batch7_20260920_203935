/*
 * test_elf.c — verify ELF64 executable generation.
 *
 * Builds IR for "return 42;", compiles to x86-64, wraps in ELF,
 * writes to /tmp, executes via system(), checks exit status.
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

/* Build IR: return CONST; */
static KyGraph *build_return_const(int64_t value) {
    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);

    KyNode *c = kyir_const_i64(g, value);
    kyir_block_add_node(b0, c);

    KyNode *ret = kyir_node_create(g, IR_RETURN);
    kyir_node_add_value_input(ret, c);
    kyir_node_add_use(c, ret);
    kyir_block_add_node(b0, ret);

    return g;
}

/* Run "./path" and return its exit status (or -1 on error). */
static int run_and_get_exit(const char *path) {
    pid_t pid = fork();
    if (pid < 0) return -1;
    if (pid == 0) {
        /* Child */
        execl(path, path, (char *)NULL);
        _exit(127);   /* exec failed */
    }
    int status = 0;
    if (waitpid(pid, &status, 0) < 0) return -1;
    if (WIFEXITED(status)) return WEXITSTATUS(status);
    return -1;
}

/* One test: compile "return X;", write ELF, run, check exit code == X */
static int test_return_value(int64_t value, const char *path) {
    KyGraph *g = build_return_const(value);

    void *code = mmap(NULL, 4096, PROT_READ | PROT_WRITE | PROT_EXEC,
                      MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    if (code == MAP_FAILED) { kyir_graph_destroy(g); return -1; }

    size_t code_len = 0;
    int rc = kyx64_compile(g, (uint8_t *)code, 4096, &code_len);
    if (rc != 0) { munmap(code, 4096); kyir_graph_destroy(g); return -1; }

    rc = kyx64_write_elf_executable((uint8_t *)code, code_len, path, 0755);
    munmap(code, 4096);
    kyir_graph_destroy(g);

    if (rc != 0) return -1;
    return run_and_get_exit(path);
}

int main(void) {
    printf("KYJIT ELF Executable Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n");

    const char *path = "/tmp/kyjit_test_bin";

    printf("\n[return 42]\n");
    int got = test_return_value(42, path);
    printf("  exit status = %d (expect 42)\n", got);
    TEST("exit 42", got == 42);

    printf("\n[return 0]\n");
    got = test_return_value(0, path);
    printf("  exit status = %d (expect 0)\n", got);
    TEST("exit 0", got == 0);

    printf("\n[return 1]\n");
    got = test_return_value(1, path);
    printf("  exit status = %d (expect 1)\n", got);
    TEST("exit 1", got == 1);

    printf("\n[return 7]\n");
    got = test_return_value(7, path);
    printf("  exit status = %d (expect 7)\n", got);
    TEST("exit 7", got == 7);

    printf("\n[return 100]\n");
    got = test_return_value(100, path);
    printf("  exit status = %d (expect 100)\n", got);
    TEST("exit 100", got == 100);

    /* Cleanup */
    unlink(path);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");
    return (tests_run == tests_passed) ? 0 : 1;
}

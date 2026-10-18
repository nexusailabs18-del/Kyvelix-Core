/*
 * test_write.c — "hello world" via write(1, data, len).
 *
 * Builds IR:
 *   write(1, 0x401000, len);   // SYS_write to stdout
 *   return 0;
 * Compiles to ELF with data section containing the message.
 * Runs the child, captures stdout, verifies it matches.
 */

#include "kyjit_ir.h"
#include "kyjit_elf.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
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

#define SYS_write 1

/* Build IR: write(fd, data_vaddr, len); return 0; */
static KyGraph *build_write_prog(int64_t fd, uint64_t data_vaddr, int64_t len) {
    KyGraph *g = kyir_graph_create();
    KyBlock *b0 = kyir_block_create(g);

    KyNode *nr  = kyir_const_i64(g, SYS_write);
    kyir_block_add_node(b0, nr);
    KyNode *a0  = kyir_const_i64(g, fd);
    kyir_block_add_node(b0, a0);
    KyNode *a1  = kyir_const_i64(g, (int64_t)data_vaddr);
    kyir_block_add_node(b0, a1);
    KyNode *a2  = kyir_const_i64(g, len);
    kyir_block_add_node(b0, a2);

    KyNode *w = kyir_syscall(g, nr, a0, a1, a2, NULL, NULL, NULL);
    kyir_block_add_node(b0, w);

    KyNode *zero = kyir_const_i64(g, 0);
    kyir_block_add_node(b0, zero);
    KyNode *ret = kyir_node_create(g, IR_RETURN);
    kyir_node_add_value_input(ret, zero);
    kyir_node_add_use(zero, ret);
    kyir_block_add_node(b0, ret);

    return g;
}

/* Compile + write ELF with data; run child; capture stdout into out_buf. */
static int run_capture(KyGraph *g, const char *path,
                       const uint8_t *data, size_t data_len,
                       char *out_buf, size_t out_size, ssize_t *out_len) {
    void *code = mmap(NULL, 4096, PROT_READ|PROT_WRITE|PROT_EXEC,
                      MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    if (code == MAP_FAILED) return -1;

    size_t code_len = 0;
    if (kyx64_compile(g, (uint8_t *)code, 4096, &code_len) != 0) {
        munmap(code, 4096);
        return -1;
    }
    if (kyx64_write_elf_executable_ex((uint8_t *)code, code_len,
                                      data, data_len, path, 0755) != 0) {
        munmap(code, 4096);
        return -1;
    }
    munmap(code, 4096);

    int pipefd[2];
    if (pipe(pipefd) != 0) return -1;

    pid_t pid = fork();
    if (pid < 0) { close(pipefd[0]); close(pipefd[1]); return -1; }
    if (pid == 0) {
        dup2(pipefd[1], 1);   /* child stdout -> pipe */
        close(pipefd[0]);
        close(pipefd[1]);
        execl(path, path, (char *)NULL);
        _exit(127);
    }
    close(pipefd[1]);

    ssize_t n = read(pipefd[0], out_buf, out_size - 1);
    close(pipefd[0]);

    int status = 0;
    waitpid(pid, &status, 0);

    if (n < 0) n = 0;
    out_buf[n] = 0;
    *out_len = n;

    return WIFEXITED(status) ? WEXITSTATUS(status) : -1;
}

int main(void) {
    printf("KYJIT write() Test v%s\n", KYIR_VERSION_STRING);
    printf("========================================\n");

    const char *path = "/tmp/kyjit_hello";

    /* ---- Test 1: write "hello\n" ---- */
    printf("\n[write(1, \"hello\\n\", 6)]\n");
    {
        const char *msg = "hello\n";
        KyGraph *g = build_write_prog(1, KYJIT_ELF_DATA_VADDR, strlen(msg));
        char buf[128];
        ssize_t n = 0;
        int exit_code = run_capture(g, path,
                                    (const uint8_t *)msg, strlen(msg),
                                    buf, sizeof(buf), &n);
        kyir_graph_destroy(g);
        printf("  child exit=%d, captured %zd bytes: \"%s\"\n",
               exit_code, n, buf);
        TEST("child exited cleanly", exit_code == 0);
        TEST("captured length matches", n == (ssize_t)strlen(msg));
        TEST("captured content matches", strcmp(buf, msg) == 0);
    }

    /* ---- Test 2: write a longer message ---- */
    printf("\n[write(1, \"KYJIT compiler!\\n\", 16)]\n");
    {
        const char *msg = "KYJIT compiler!\n";
        KyGraph *g = build_write_prog(1, KYJIT_ELF_DATA_VADDR, strlen(msg));
        char buf[128];
        ssize_t n = 0;
        int exit_code = run_capture(g, path,
                                    (const uint8_t *)msg, strlen(msg),
                                    buf, sizeof(buf), &n);
        kyir_graph_destroy(g);
        printf("  child exit=%d, captured %zd bytes: \"%s\"\n",
               exit_code, n, buf);
        TEST("longer message: exit clean", exit_code == 0);
        TEST("longer message: length", n == (ssize_t)strlen(msg));
        TEST("longer message: content", strcmp(buf, msg) == 0);
    }

    /* ---- Test 3: write just a newline ---- */
    printf("\n[write(1, \"\\n\", 1)]\n");
    {
        const char *msg = "\n";
        KyGraph *g = build_write_prog(1, KYJIT_ELF_DATA_VADDR, 1);
        char buf[128];
        ssize_t n = 0;
        int exit_code = run_capture(g, path,
                                    (const uint8_t *)msg, 1,
                                    buf, sizeof(buf), &n);
        kyir_graph_destroy(g);
        printf("  child exit=%d, captured %zd bytes\n", exit_code, n);
        TEST("newline: exit clean", exit_code == 0);
        TEST("newline: one byte", n == 1);
        TEST("newline: byte is '\\n'", buf[0] == '\n');
    }

    unlink(path);

    printf("\n========================================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_run - tests_passed);
    printf("========================================\n");
    return (tests_run == tests_passed) ? 0 : 1;
}

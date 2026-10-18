#include <stdio.h>
#include <stdlib.h>
#include <assert.h>
#include <string.h>
#include <sys/mman.h>
#include "kyjit_ir.h"

static int test_chain_spills() {
    KyGraph *g = kyir_graph_create();
    KyIRBuilder *b = kyir_builder_create(g);
    KyBlock *entry = kyir_builder_new_block(b);
    kyir_builder_set_block(b, entry);

    KyNode *v = kyir_build_const(b, 1);
    for (int i = 0; i < 30; i++) {
        KyNode *c = kyir_build_const(b, 1);
        v = kyir_build_add(b, v, c);
    }
    kyir_build_return(b, v);

    kyir_cfg_build(g);
    KyDomTree *dt = kyir_domtree_build(g);
    KySchedule *s = kyir_schedule_build(g, dt);
    KyX64Backend *backend = kyx64_backend_create(g, s, dt);
    
    kyx64_linearize(backend);
    kyx64_select(backend);
    kyx64_allocate(backend);
    
    int spills = backend->num_spills;
    printf("Chain of 30 adds: %d spills\n", spills);
    assert(spills <= 10); // A smart allocator should achieve 0 or near 0 spills here
    
    kyx64_backend_destroy(backend);
    kyir_schedule_destroy(s);
    kyir_domtree_destroy(dt);
    
    // Compile and execute to verify correctness
    uint8_t code[4096];
    size_t code_len = 0;
    int rc = kyx64_compile(g, code, sizeof(code), &code_len);
    assert(rc == 0);
    
    void *exec_mem = mmap(NULL, 4096, PROT_READ | PROT_WRITE | PROT_EXEC, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    assert(exec_mem != MAP_FAILED);
    memcpy(exec_mem, code, code_len);
    
    int (*func)() = (int (*)())exec_mem;
    int result = func();
    printf("Execution result: %d\n", result);
    assert(result == 31);
    
    munmap(exec_mem, 4096);
    kyir_builder_destroy(b);
    kyir_graph_destroy(g);
    return 1;
}

static int test_simple_zero_spills() {
    KyGraph *g = kyir_graph_create();
    KyIRBuilder *b = kyir_builder_create(g);
    KyBlock *entry = kyir_builder_new_block(b);
    kyir_builder_set_block(b, entry);

    KyNode *c1 = kyir_build_const(b, 10);
    KyNode *c2 = kyir_build_const(b, 20);
    KyNode *sum = kyir_build_add(b, c1, c2);
    kyir_build_return(b, sum);

    kyir_cfg_build(g);
    KyDomTree *dt = kyir_domtree_build(g);
    KySchedule *s = kyir_schedule_build(g, dt);
    KyX64Backend *backend = kyx64_backend_create(g, s, dt);
    
    kyx64_linearize(backend);
    kyx64_select(backend);
    kyx64_allocate(backend);
    
    int spills = backend->num_spills;
    printf("Simple add: %d spills\n", spills);
    assert(spills == 0);
    
    kyx64_backend_destroy(backend);
    kyir_schedule_destroy(s);
    kyir_domtree_destroy(dt);
    kyir_builder_destroy(b);
    kyir_graph_destroy(g);
    return 1;
}

static int test_interference() {
    KyGraph *g = kyir_graph_create();
    KyIRBuilder *b = kyir_builder_create(g);
    KyBlock *entry = kyir_builder_new_block(b);
    kyir_builder_set_block(b, entry);

    KyNode *v1 = kyir_build_const(b, 1);
    KyNode *v2 = kyir_build_const(b, 2);
    KyNode *v3 = kyir_build_const(b, 3);
    
    KyNode *a1 = kyir_build_add(b, v1, v2);
    KyNode *a2 = kyir_build_add(b, a1, v3);
    kyir_build_return(b, a2);

    kyir_cfg_build(g);
    KyDomTree *dt = kyir_domtree_build(g);
    KySchedule *s = kyir_schedule_build(g, dt);
    KyX64Backend *backend = kyx64_backend_create(g, s, dt);
    
    kyx64_linearize(backend);
    kyx64_select(backend);
    kyx64_allocate(backend);
    
    assert(backend->num_spills == 0);
    
    kyx64_backend_destroy(backend);
    kyir_schedule_destroy(s);
    kyir_domtree_destroy(dt);
    kyir_builder_destroy(b);
    kyir_graph_destroy(g);
    return 1;
}

int main() {
    int passed = 0;
    int total = 3;
    
    if (test_chain_spills()) passed++;
    if (test_simple_zero_spills()) passed++;
    if (test_interference()) passed++;
    
    printf("Tests passed: %d/%d\n", passed, total);
    return passed == total ? 0 : 1;
}
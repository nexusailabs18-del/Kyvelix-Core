#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

/* KYJIT Sandbox — isolate untrusted code */

typedef struct kyjit_sandbox {
    size_t max_memory;       /* Max memory allowed */
    size_t current_memory;   /* Current memory used */
    double max_time_ms;      /* Max execution time */
    int max_operations;      /* Max operations */
    int current_operations;  /* Current op count */
    int is_active;           /* Sandbox active? */
} kyjit_sandbox_t;

kyjit_sandbox_t* kyjit_sandbox_create(size_t max_memory, double max_time_ms, int max_ops) {
    kyjit_sandbox_t *sb = calloc(1, sizeof(kyjit_sandbox_t));
    if (!sb) return NULL;
    
    sb->max_memory = max_memory;
    sb->current_memory = 0;
    sb->max_time_ms = max_time_ms;
    sb->max_operations = max_ops;
    sb->current_operations = 0;
    sb->is_active = 1;
    
    return sb;
}

void* kyjit_sandbox_alloc(kyjit_sandbox_t *sb, size_t size) {
    if (!sb->is_active) return NULL;
    
    if (sb->current_memory + size > sb->max_memory) {
        fprintf(stderr, "SANDBOX: Memory limit exceeded (%zu + %zu > %zu)\n",
                sb->current_memory, size, sb->max_memory);
        return NULL;
    }
    
    void *ptr = malloc(size);
    if (ptr) sb->current_memory += size;
    return ptr;
}

int kyjit_sandbox_tick(kyjit_sandbox_t *sb) {
    if (!sb->is_active) return -1;
    
    sb->current_operations++;
    if (sb->current_operations > sb->max_operations) {
        fprintf(stderr, "SANDBOX: Operation limit exceeded (%d > %d)\n",
                sb->current_operations, sb->max_operations);
        sb->is_active = 0;
        return -1;
    }
    return 0;
}

int kyjit_sandbox_is_active(kyjit_sandbox_t *sb) {
    return sb->is_active;
}

void kyjit_sandbox_free(kyjit_sandbox_t *sb, void *ptr, size_t size) {
    if (!sb) return;
    free(ptr);
    if (sb->current_memory >= size) {
        sb->current_memory -= size;
    }
}

void kyjit_sandbox_destroy(kyjit_sandbox_t *sb) {
    if (!sb) return;
    free(sb);
}

int kyjit_sandbox_available(void) { return 1; }

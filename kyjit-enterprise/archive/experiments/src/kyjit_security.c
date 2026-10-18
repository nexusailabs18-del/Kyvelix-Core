#include "kyjit_security.h"
#include <sys/mman.h>
#include <stdlib.h>

int kyjit_security_init(void) {
    return 0;
}

void* kyjit_alloc_executable(size_t size) {
    return mmap(NULL, size, PROT_READ|PROT_EXEC,
                MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
}

void* kyjit_alloc_writable(size_t size) {
    return mmap(NULL, size, PROT_READ|PROT_WRITE,
                MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
}

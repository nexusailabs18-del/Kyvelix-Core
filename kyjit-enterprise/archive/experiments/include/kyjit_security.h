#ifndef KYJIT_SECURITY_H
#define KYJIT_SECURITY_H
#include <stddef.h>
int kyjit_security_init(void);
void* kyjit_alloc_executable(size_t size);
void* kyjit_alloc_writable(size_t size);
#endif

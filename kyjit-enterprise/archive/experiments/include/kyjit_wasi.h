#ifndef KYJIT_WASI_H
#define KYJIT_WASI_H

#include <stddef.h>

typedef struct kyjit_wasi kyjit_wasi_t;

kyjit_wasi_t* kyjit_wasi_create(void);
int kyjit_wasi_set_preopen(kyjit_wasi_t *wasi, const char *dir);
int kyjit_wasi_write(kyjit_wasi_t *wasi, const char *data, size_t len);
int kyjit_wasi_exit(kyjit_wasi_t *wasi, int code);
void kyjit_wasi_destroy(kyjit_wasi_t *wasi);
int kyjit_wasi_available(void);

#endif

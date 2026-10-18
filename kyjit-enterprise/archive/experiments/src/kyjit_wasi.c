#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* KYJIT WASI — WASM System Interface support */

typedef struct kyjit_wasi {
    int stdout_fd;
    int stderr_fd;
    int exit_code;
    char *preopen_dir;
} kyjit_wasi_t;

kyjit_wasi_t* kyjit_wasi_create(void) {
    kyjit_wasi_t *wasi = calloc(1, sizeof(kyjit_wasi_t));
    if (!wasi) return NULL;
    
    wasi->stdout_fd = 1;
    wasi->stderr_fd = 2;
    wasi->exit_code = 0;
    wasi->preopen_dir = NULL;
    
    return wasi;
}

int kyjit_wasi_set_preopen(kyjit_wasi_t *wasi, const char *dir) {
    if (!wasi) return -1;
    if (wasi->preopen_dir) free(wasi->preopen_dir);
    wasi->preopen_dir = strdup(dir);
    return 0;
}

int kyjit_wasi_write(kyjit_wasi_t *wasi, const char *data, size_t len) {
    if (!wasi) return -1;
    return fwrite(data, 1, len, stdout);
}

int kyjit_wasi_exit(kyjit_wasi_t *wasi, int code) {
    if (!wasi) return -1;
    wasi->exit_code = code;
    return 0;
}

void kyjit_wasi_destroy(kyjit_wasi_t *wasi) {
    if (!wasi) return;
    if (wasi->preopen_dir) free(wasi->preopen_dir);
    free(wasi);
}

int kyjit_wasi_available(void) { return 1; }

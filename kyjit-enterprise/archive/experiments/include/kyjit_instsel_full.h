#ifndef KYJIT_INSTSEL_FULL_H
#define KYJIT_INSTSEL_FULL_H
#include <stdint.h>
#include <string.h>
#include <sys/mman.h>

enum { R_RAX=0, R_RCX=1, R_RDX=2, R_RBX=3, R_RSP=4, R_RBP=5, R_RSI=6, R_RDI=7,
       R_R8=8, R_R9=9, R_R10=10, R_R11=11 };

static inline uint8_t *ky_emit_mov_r64_imm64(uint8_t *code, uint8_t reg, uint64_t imm) {
    *code++ = 0x48;
    *code++ = 0xB8 + reg;
    memcpy(code, &imm, 8); code += 8;
    return code;
}

static inline uint8_t *ky_emit_add_r64_r64(uint8_t *code, uint8_t dst, uint8_t src) {
    *code++ = 0x48;
    *code++ = 0x01;
    *code++ = 0xC0 + (src << 3) + dst;
    return code;
}

static inline uint8_t *ky_emit_ret(uint8_t *code) {
    *code++ = 0xC3;
    return code;
}

static inline void *ky_jit_make_executable(uint8_t *code, size_t len) {
    void *mem = mmap(NULL, 4096, PROT_READ|PROT_WRITE|PROT_EXEC,
                     MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    if (mem == MAP_FAILED) return NULL;
    memcpy(mem, code, len);
    __builtin___clear_cache((char*)mem, (char*)mem + len);
    return mem;
}
#endif

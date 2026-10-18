#ifndef KYJIT_NATIVE_H
#define KYJIT_NATIVE_H
#include <stdint.h>
#include <string.h>
#include <sys/mman.h>

// x86-64 registers
enum { R_RAX=0, R_RCX=1, R_RDX=2, R_RBX=3, R_RSP=4, R_RBP=5, R_RSI=6, R_RDI=7 };

// Emit MOV RAX, [RDI + offset] — property access in 1 instruction
static inline uint8_t *ky_emit_load_property(uint8_t *code, uint32_t offset) {
    *code++ = 0x48;             // REX.W (64-bit)
    *code++ = 0x8B;             // MOV r64, r/m64
    if (offset < 128) {
        *code++ = 0x47;         // ModRM: [RDI + disp8]
        *code++ = (uint8_t)offset;
    } else {
        *code++ = 0x87;         // ModRM: [RDI + disp32]
        memcpy(code, &offset, 4); code += 4;
    }
    return code;
}

// Emit RET
static inline uint8_t *ky_emit_ret(uint8_t *code) {
    *code++ = 0xC3;
    return code;
}

// Make executable
static inline void *ky_native_make_exec(uint8_t *code, size_t len) {
    void *mem = mmap(NULL, 4096, PROT_READ|PROT_WRITE|PROT_EXEC,
                     MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    if (mem == MAP_FAILED) return NULL;
    memcpy(mem, code, len);
    return mem;
}
#endif

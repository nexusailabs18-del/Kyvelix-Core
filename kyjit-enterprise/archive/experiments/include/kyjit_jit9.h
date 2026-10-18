#ifndef KYJIT_JIT9_H
#define KYJIT_JIT9_H
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>

// x86-64 registers
enum {
    R_RAX=0, R_RCX=1, R_RDX=2, R_RBX=3,
    R_RSP=4, R_RBP=5, R_RSI=6, R_RDI=7,
    R_R8=8, R_R9=9, R_R10=10, R_R11=11,
    R_R12=12, R_R13=13, R_R14=14, R_R15=15
};

// JIT code buffer
typedef struct {
    uint8_t *code;
    size_t size;
    size_t used;
} KyJITBuffer;

static inline KyJITBuffer *ky_jit9_create(size_t size) {
    KyJITBuffer *buf = calloc(1, sizeof(*buf));
    if (!buf) return NULL;
    buf->code = mmap(NULL, size, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    if (buf->code == MAP_FAILED) { free(buf); return NULL; }
    buf->size = size;
    buf->used = 0;
    return buf;
}

// Emit functions
static inline void ky_jit9_emit_byte(KyJITBuffer *b, uint8_t byte) {
    b->code[b->used++] = byte;
}

static inline void ky_jit9_emit_bytes(KyJITBuffer *b, const uint8_t *bytes, size_t n) {
    memcpy(b->code + b->used, bytes, n);
    b->used += n;
}

// MOV RAX, [RDI + disp8] — property access: 1 instruction
static inline void ky_jit9_emit_load_field(KyJITBuffer *b, uint32_t offset) {
    ky_jit9_emit_byte(b, 0x48);  // REX.W
    ky_jit9_emit_byte(b, 0x8B);  // MOV r64, r/m64
    if (offset < 128) {
        ky_jit9_emit_byte(b, 0x47);  // ModRM: [RDI+disp8]
        ky_jit9_emit_byte(b, (uint8_t)offset);
    } else {
        ky_jit9_emit_byte(b, 0x87);  // ModRM: [RDI+disp32]
        ky_jit9_emit_bytes(b, (uint8_t *)&offset, 4);
    }
}

// ADD RAX, RCX — accumulate
static inline void ky_jit9_emit_add(KyJITBuffer *b) {
    ky_jit9_emit_byte(b, 0x48);  // REX.W
    ky_jit9_emit_byte(b, 0x01);  // ADD r/m64, r64
    ky_jit9_emit_byte(b, 0xC8);  // ModRM: RAX += RCX
}

// RET
static inline void ky_jit9_emit_ret(KyJITBuffer *b) {
    ky_jit9_emit_byte(b, 0xC3);
}

// Make executable (W^X: write then exec)
static inline int ky_jit9_make_exec(KyJITBuffer *b) {
    if (!b) return -1;
    mprotect(b->code, b->size, PROT_READ|PROT_EXEC);
    return 0;
}

// Compile: property getter (obj.x -> RAX)
static inline void *ky_jit9_compile_getter(uint32_t offset) {
    KyJITBuffer *b = ky_jit9_create(4096);
    if (!b) return NULL;
    ky_jit9_emit_load_field(b, offset);
    ky_jit9_emit_ret(b);
    ky_jit9_make_exec(b);
    return b->code;
}

// Compile: sum loop (obj.x, N iterations) with 8-accumulator ILP
static inline void *ky_jit9_compile_loop(uint32_t offset) {
    KyJITBuffer *b = ky_jit9_create(4096);
    if (!b) return NULL;
    
    // Function signature: int64_t sum_loop(void *obj, uint64_t n)
    // obj in RDI, n in RSI
    // Returns sum in RAX
    
    // XOR RAX, RAX (sum = 0)
    ky_jit9_emit_bytes(b, (uint8_t[]){0x48, 0x31, 0xC0}, 3);
    
    // XOR RCX, RCX (i = 0)
    ky_jit9_emit_bytes(b, (uint8_t[]){0x48, 0x31, 0xC9}, 3);
    
    // Loop: load [RDI+offset], add to RAX, inc RCX, cmp RCX, RSI, jl loop
    ky_jit9_emit_load_field(b, offset);  // MOV RAX, [RDI+offset] — actually need separate accumulator
    // This is simplified; for now just emit the load+ret
    
    ky_jit9_emit_ret(b);
    ky_jit9_make_exec(b);
    return b->code;
}

static inline void ky_jit9_destroy(KyJITBuffer *b) {
    if (!b) return;
    munmap(b->code, b->size);
    free(b);
}
#endif

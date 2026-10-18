#include "kyjit_builtins.h"
#include <sys/mman.h>
#include <string.h>
#include <stdio.h>

/* Emit: add rax, rbx  (0x48 0x01 0xD8) */
void kyjit_emit_add(uint8_t *code, int *offset) {
    code[(*offset)++] = 0x48;
    code[(*offset)++] = 0x01;
    code[(*offset)++] = 0xD8;
}

/* Emit: imul rax, rbx  (0x48 0x0F 0xAF 0xC3) */
void kyjit_emit_mul(uint8_t *code, int *offset) {
    code[(*offset)++] = 0x48;
    code[(*offset)++] = 0x0F;
    code[(*offset)++] = 0xAF;
    code[(*offset)++] = 0xC3;
}

/* Emit: mov eax, imm32  (0xB8 xx xx xx xx) */
void kyjit_emit_load_const(uint8_t *code, int *offset, int32_t value) {
    code[(*offset)++] = 0xB8;
    memcpy(&code[*offset], &value, 4);
    *offset += 4;
}

/* Emit: ret  (0xC3) */
void kyjit_emit_return(uint8_t *code, int *offset) {
    code[(*offset)++] = 0xC3;
}

/* Execute machine code */
int64_t kyjit_execute_machine_code(uint8_t *code, int length) {
    /* Allocate executable memory */
    void *exec = mmap(NULL, length, PROT_READ|PROT_WRITE|PROT_EXEC,
                      MAP_PRIVATE|MAP_ANONYMOUS, -1, 0);
    if (exec == MAP_FAILED) return -1;
    
    memcpy(exec, code, length);
    
    /* Call the machine code */
    int64_t (*fn)(void) = (int64_t(*)(void))exec;
    int64_t result = fn();
    
    munmap(exec, length);
    return result;
}

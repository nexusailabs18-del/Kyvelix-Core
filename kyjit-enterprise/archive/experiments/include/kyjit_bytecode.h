#ifndef KYJIT_BYTECODE_H
#define KYJIT_BYTECODE_H

#include <stdint.h>
#include <stddef.h>

/* Bytecode opcodes */
typedef enum {
    BC_LOAD_CONST = 1,
    BC_LOAD_VAR,
    BC_STORE_VAR,
    BC_ADD,
    BC_SUB,
    BC_MUL,
    BC_DIV,
    BC_RETURN,
    BC_CALL,
    BC_JUMP,
    BC_JUMP_IF_FALSE,
} KyBytecodeOp;

/* Single bytecode instruction */
typedef struct {
    uint8_t op;
    int32_t operand1;
    int32_t operand2;
} KyBytecode;

/* Bytecode buffer */
typedef struct {
    KyBytecode *code;
    int length;
    int capacity;
} KyBytecodeBuf;

/* Stream tokens directly to bytecode — NO AST */
KyBytecodeBuf* kyjit_compile_to_bytecode(const char *source);
void kyjit_bytecode_free(KyBytecodeBuf *buf);
int kyjit_bytecode_execute(KyBytecodeBuf *buf);

#endif

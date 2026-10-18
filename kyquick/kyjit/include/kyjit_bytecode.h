/*
 * include/kyjit_bytecode.h - KyJIT bytecode normalization interface
 *
 * This layer decodes QuickJS bytecode into a stable KyOpcode representation.
 * It is independent of QuickJS JSValue and does not leak QuickJS internals
 * beyond the opcode names used in the .c file.
 *
 * Target QuickJS version: 2026-06-04
 */

#ifndef KYJIT_BYTECODE_H
#define KYJIT_BYTECODE_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ------------------------------------------------------------------------- */
/* KyJIT opcode enum                                                          */

typedef enum KyOpcode {
    KY_OP_INVALID = 0,

    /* Stack / constants */
    KY_OP_PUSH_I32,
    KY_OP_PUSH_CONST,

    /* Local variables */
    KY_OP_GET_LOCAL,
    KY_OP_PUT_LOCAL,
    KY_OP_GET_ARG,

    KY_OP_GET_LOCAL0,
    KY_OP_GET_LOCAL1,
    KY_OP_GET_LOCAL2,
    KY_OP_GET_LOCAL3,

    KY_OP_PUT_LOCAL0,
    KY_OP_PUT_LOCAL1,
    KY_OP_PUT_LOCAL2,
    KY_OP_PUT_LOCAL3,

    /* Arithmetic */
    KY_OP_ADD,
    KY_OP_SUB,
    KY_OP_MUL,
    KY_OP_DIV,
    KY_OP_MOD,

    /* Bitwise */
    KY_OP_AND,
    KY_OP_OR,
    KY_OP_XOR,
    KY_OP_SHL,
    KY_OP_SHR,
    KY_OP_SAR,

    /* Increment / decrement */
    KY_OP_INC,
    KY_OP_DEC,

    /* Comparisons */
    KY_OP_LT,
    KY_OP_LTE,
    KY_OP_GT,
    KY_OP_GTE,
    KY_OP_EQ,
    KY_OP_NEQ,
    KY_OP_STRICT_EQ,
    KY_OP_STRICT_NEQ,

    /* Branches */
    KY_OP_IF_TRUE,
    KY_OP_IF_FALSE,
    KY_OP_GOTO,

    /* Array access */
    KY_OP_GET_ARRAY_EL,

    KY_OP_COUNT
} KyOpcode;

/* Operand format for metadata */
typedef enum KyOperandFormat {
    KY_FMT_NONE = 0,
    KY_FMT_INT32,       /* signed 32-bit immediate (branch or push_i32) */
    KY_FMT_UINT32,      /* unsigned 32-bit index (push_const) */
    KY_FMT_UINT16,      /* unsigned 16-bit index (get_loc, put_loc, get_arg) */
    KY_FMT_BRANCH       /* signed 32-bit relative offset (goto, if_*) */
} KyOperandFormat;

/* Decoded instruction */
typedef struct KyDecodedOp {
    KyOpcode opcode;

    uint8_t size;        /* total instruction size in bytes */
    uint8_t n_pop;       /* number of stack slots popped */
    uint8_t n_push;      /* number of stack slots pushed */

    uint32_t operand;    /* normalized operand (index, immediate, etc.) */
    int32_t relative_offset; /* for branch opcodes: offset from next instruction */
} KyDecodedOp;

/* Bytecode region */
typedef struct KyBytecodeRegion {
    const uint8_t *start;          /* first byte of loop body */
    const uint8_t *end_exclusive;  /* one past last byte of loop body */
    uint32_t local_count;
    uint32_t arg_count;
    uint32_t const_count;
} KyBytecodeRegion;

/* ------------------------------------------------------------------------- */
/* Public functions                                                           */

/*
 * Initialize the QuickJS-to-KyOpcode mapping.
 * Must be called once before any decode operation.
 * Returns 0 on success, -1 if required QuickJS opcodes are missing.
 */
int ky_bytecode_init(void);

/*
 * Decode one instruction at `pc`.
 *
 * Returns 0 on success, -1 on error:
 *   - pc out of region
 *   - insufficient bytes for opcode and operands
 *   - unsupported or invalid opcode
 *   - invalid operand (e.g., local index out of range)
 *
 * On success, *out contains the normalized instruction.
 *
 * Branch semantics:
 *   relative_offset is ALWAYS relative to the byte immediately after the
 *   branch instruction (i.e., instruction_start + instruction_size).
 *   The absolute target is computed as:
 *       target = pc + size + relative_offset
 */
int ky_bytecode_decode(const KyBytecodeRegion *region,
                       const uint8_t *pc,
                       KyDecodedOp *out);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_BYTECODE_H */
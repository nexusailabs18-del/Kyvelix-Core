/*
 * kyjit_bytecode.c - QuickJS bytecode → KyOpcode normalization
 *
 * This file contains QuickJS-specific opcode definitions. It is the only
 * KyJIT component that directly includes quickjs-opcode.h and knows the
 * numeric opcode values.
 *
 * Target QuickJS version: 2026-06-04
 */

#include "kyjit_bytecode.h"
#include "quickjs-opcode.h"

/* ------------------------------------------------------------------------- */
/* QuickJS opcode enum generation (only for mapping)                          */

typedef enum {
#define DEF(id, size, n_pop, n_push, f) QJ_OP_##id,
#include "quickjs-opcode.h"
#undef DEF
    QJ_OP_COUNT
} QJOpcode;

/* Metadata for all QuickJS opcodes (size, pops, pushes, format) */
static const struct {
    uint8_t size;
    uint8_t n_pop;
    uint8_t n_push;
    uint8_t format;   /* QuickJS OP_FMT_* */
} qj_op_meta[QJ_OP_COUNT] = {
#define DEF(id, size, n_pop, n_push, f) { size, n_pop, n_push, f },
#include "quickjs-opcode.h"
#undef DEF
};

/* Mapping from QuickJS opcode enum to KyOpcode */
static KyOpcode qj_to_ky[QJ_OP_COUNT];

/* Thread-safe one-time initialization flag */
#include <stdatomic.h>
static atomic_int ky_bytecode_initialized = 0;

/* Initialize the mapping table */
int ky_bytecode_init(void)
{
    if (atomic_load_explicit(&ky_bytecode_initialized, memory_order_acquire))
        return 0;

    /* Start with all entries invalid */
    for (int i = 0; i < QJ_OP_COUNT; i++)
        qj_to_ky[i] = KY_OP_INVALID;

    /* Map supported QuickJS opcodes to KyOpcode */
#define MAP_QJ_TO_KY(qj_name, ky_name) \
    qj_to_ky[QJ_OP_##qj_name] = KY_OP_##ky_name;

    MAP_QJ_TO_KY(push_i32,   PUSH_I32)
    MAP_QJ_TO_KY(push_const, PUSH_CONST)
    MAP_QJ_TO_KY(get_loc,    GET_LOCAL)
    MAP_QJ_TO_KY(put_loc,    PUT_LOCAL)
    MAP_QJ_TO_KY(get_arg,    GET_ARG)

    /* Short opcodes exist only when SHORT_OPCODES is enabled.
       We detect them via enum existence checks. */
#ifdef SHORT_OPCODES
    MAP_QJ_TO_KY(get_loc0,   GET_LOCAL0)
    MAP_QJ_TO_KY(get_loc1,   GET_LOCAL1)
    MAP_QJ_TO_KY(get_loc2,   GET_LOCAL2)
    MAP_QJ_TO_KY(get_loc3,   GET_LOCAL3)
    MAP_QJ_TO_KY(put_loc0,   PUT_LOCAL0)
    MAP_QJ_TO_KY(put_loc1,   PUT_LOCAL1)
    MAP_QJ_TO_KY(put_loc2,   PUT_LOCAL2)
    MAP_QJ_TO_KY(put_loc3,   PUT_LOCAL3)
#endif

    MAP_QJ_TO_KY(add,        ADD)
    MAP_QJ_TO_KY(sub,        SUB)
    MAP_QJ_TO_KY(mul,        MUL)
    MAP_QJ_TO_KY(div,        DIV)
    MAP_QJ_TO_KY(mod,        MOD)

    MAP_QJ_TO_KY(and,        AND)
    MAP_QJ_TO_KY(or,         OR)
    MAP_QJ_TO_KY(xor,        XOR)
    MAP_QJ_TO_KY(shl,        SHL)
    MAP_QJ_TO_KY(shr,        SHR)
    MAP_QJ_TO_KY(sar,        SAR)

    MAP_QJ_TO_KY(inc,        INC)
    MAP_QJ_TO_KY(dec,        DEC)

    MAP_QJ_TO_KY(lt,         LT)
    MAP_QJ_TO_KY(lte,        LTE)
    MAP_QJ_TO_KY(gt,         GT)
    MAP_QJ_TO_KY(gte,        GTE)
    MAP_QJ_TO_KY(eq,         EQ)
    MAP_QJ_TO_KY(neq,        NEQ)
    MAP_QJ_TO_KY(strict_eq,  STRICT_EQ)
    MAP_QJ_TO_KY(strict_neq, STRICT_NEQ)

    MAP_QJ_TO_KY(if_true,    IF_TRUE)
    MAP_QJ_TO_KY(if_false,   IF_FALSE)
    MAP_QJ_TO_KY(goto,       GOTO)

    MAP_QJ_TO_KY(get_array_el, GET_ARRAY_EL)

#undef MAP_QJ_TO_KY

    atomic_store_explicit(&ky_bytecode_initialized, 1, memory_order_release);
    return 0;
}

/* Convert QuickJS OP_FMT_* to KyOperandFormat */
static KyOperandFormat ky_format_from_quickjs(uint8_t qj_fmt)
{
    switch (qj_fmt) {
    case OP_FMT_NONE:   return KY_FMT_NONE;
    case OP_FMT_INT32:  return KY_FMT_INT32;
    case OP_FMT_CONST:  return KY_FMT_UINT32;
    case OP_FMT_LOCAL:  return KY_FMT_UINT16;
    case OP_FMT_ARG:    return KY_FMT_UINT16;
    case OP_FMT_BRANCH: return KY_FMT_BRANCH;
    default:            return KY_FMT_NONE;
    }
}

/* Decode one instruction */
int ky_bytecode_decode(const KyBytecodeRegion *region,
                       const uint8_t *pc,
                       KyDecodedOp *out)
{
    if (!region || !pc || !out)
        return -1;

    if (pc < region->start || pc >= region->end_exclusive)
        return -1;

    uint8_t raw_op = *pc;
    if (raw_op >= QJ_OP_COUNT)
        return -1;

    KyOpcode op = qj_to_ky[raw_op];
    if (op == KY_OP_INVALID)
        return -1;

    const uint8_t size = qj_op_meta[raw_op].size;
    const uint8_t n_pop = qj_op_meta[raw_op].n_pop;
    const uint8_t n_push = qj_op_meta[raw_op].n_push;
    const KyOperandFormat fmt = ky_format_from_quickjs(qj_op_meta[raw_op].format);

    /* Safe bounds check using pointer subtraction */
    if (pc > region->end_exclusive ||
        (size_t)(region->end_exclusive - pc) < size)
        return -1;

    out->opcode = op;
    out->size = size;
    out->n_pop = n_pop;
    out->n_push = n_push;
    out->operand = 0;
    out->relative_offset = 0;

    switch (fmt) {
    case KY_FMT_NONE:
        break;

    case KY_FMT_INT32:
        out->operand = (uint32_t)((int32_t)((uint32_t)pc[1] |
                                            ((uint32_t)pc[2] << 8) |
                                            ((uint32_t)pc[3] << 16) |
                                            ((uint32_t)pc[4] << 24)));
        if (op == KY_OP_PUSH_I32) {
            /* operand is the immediate */
        } else if (op == KY_OP_GOTO ||
                   op == KY_OP_IF_TRUE ||
                   op == KY_OP_IF_FALSE) {
            out->relative_offset = (int32_t)out->operand;
            out->operand = 0;
        }
        break;

    case KY_FMT_UINT32:
        out->operand = (uint32_t)pc[1] |
                       ((uint32_t)pc[2] << 8) |
                       ((uint32_t)pc[3] << 16) |
                       ((uint32_t)pc[4] << 24);
        break;

    case KY_FMT_UINT16:
        out->operand = (uint16_t)(pc[1] | (pc[2] << 8));
        if (op == KY_OP_GET_LOCAL || op == KY_OP_PUT_LOCAL) {
            if (out->operand >= region->local_count)
                return -1;
        } else if (op == KY_OP_GET_ARG) {
            if (out->operand >= region->arg_count)
                return -1;
        }
        break;

    case KY_FMT_BRANCH:
        return -1;

    default:
        return -1;
    }

    /* Normalize short local opcodes to operand values */
    switch (op) {
    case KY_OP_GET_LOCAL0: out->operand = 0; break;
    case KY_OP_GET_LOCAL1: out->operand = 1; break;
    case KY_OP_GET_LOCAL2: out->operand = 2; break;
    case KY_OP_GET_LOCAL3: out->operand = 3; break;
    case KY_OP_PUT_LOCAL0: out->operand = 0; break;
    case KY_OP_PUT_LOCAL1: out->operand = 1; break;
    case KY_OP_PUT_LOCAL2: out->operand = 2; break;
    case KY_OP_PUT_LOCAL3: out->operand = 3; break;
    default: break;
    }

    return 0;
}
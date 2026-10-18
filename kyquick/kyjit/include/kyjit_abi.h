/*
 * kyjit_abi.h - KyJIT stable ABI (v1)
 *
 * Stable ABI shared by:
 *   - JIT IR
 *   - optimizer
 *   - register allocator
 *   - native backend
 *   - runtime
 *   - QuickJS adapter
 *
 * This file MUST NOT include quickjs.h.
 *
 * ABI v1:
 *   - KyValue is exactly 16 bytes.
 *   - KyValue field offsets are fixed.
 *   - KyType is exactly uint32_t.
 *   - x86-64 is the initial native target.
 *   - Only entry guards are supported.
 *   - No mid-trace side exits yet.
 */

#ifndef KYJIT_ABI_H
#define KYJIT_ABI_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ------------------------------------------------------------------------- */
/* Portable compile-time assertions                                           */

#ifdef __cplusplus

#define KYJIT_STATIC_ASSERT(cond, msg) static_assert((cond), msg)
#define KYJIT_ALIGNOF(type) alignof(type)

#else

#define KYJIT_STATIC_ASSERT(cond, msg) _Static_assert((cond), msg)
#define KYJIT_ALIGNOF(type) _Alignof(type)

#endif

/* ------------------------------------------------------------------------- */
/* ABI version / architecture                                                 */

#define KYJIT_ABI_VERSION 1u
#define KYJIT_ARCH_X86_64 1u

/* ------------------------------------------------------------------------- */
/* KyType                                                                     */

/*
 * KyType is explicitly uint32_t.
 *
 * Do NOT use:
 *
 *     typedef enum KyType { ... } KyType;
 *
 * for the ABI itself, because the size of a C enum is implementation-defined.
 */

typedef uint32_t KyType;

enum {
    KY_T_UNDEFINED = 0,
    KY_T_NULL,
    KY_T_BOOL,
    KY_T_INT32,
    KY_T_INT64,
    KY_T_DOUBLE,
    KY_T_STRING,
    KY_T_OBJECT,
    KY_T_EXCEPTION,
    KY_T_COUNT
};

KYJIT_STATIC_ASSERT(
    sizeof(KyType) == sizeof(uint32_t),
    "KyType ABI changed"
);

/* ------------------------------------------------------------------------- */
/* KyValue                                                                    */

/*
 * Canonical 16-byte memory representation.
 *
 * Offset 0:  payload (8 bytes)
 * Offset 8:  type    (4 bytes)
 * Offset 12: flags   (4 bytes)
 */

typedef struct KyValue {
    uint64_t payload;
    uint32_t type;
    uint32_t flags;
} KyValue;

KYJIT_STATIC_ASSERT(
    sizeof(KyValue) == 16,
    "KyValue ABI size changed"
);

KYJIT_STATIC_ASSERT(
    KYJIT_ALIGNOF(KyValue) >= 8,
    "KyValue alignment too small"
);

KYJIT_STATIC_ASSERT(
    offsetof(KyValue, payload) == 0,
    "KyValue.payload offset changed"
);

KYJIT_STATIC_ASSERT(
    offsetof(KyValue, type) == 8,
    "KyValue.type offset changed"
);

KYJIT_STATIC_ASSERT(
    offsetof(KyValue, flags) == 12,
    "KyValue.flags offset changed"
);

/* ------------------------------------------------------------------------- */
/* KyValue constructors                                                       */

static inline KyValue ky_undefined(void)
{
    KyValue v;

    v.payload = 0;
    v.type = KY_T_UNDEFINED;
    v.flags = 0;

    return v;
}

static inline KyValue ky_null(void)
{
    KyValue v;

    v.payload = 0;
    v.type = KY_T_NULL;
    v.flags = 0;

    return v;
}

static inline KyValue ky_bool(int b)
{
    KyValue v;

    v.payload = b ? 1u : 0u;
    v.type = KY_T_BOOL;
    v.flags = 0;

    return v;
}

static inline KyValue ky_int32(int32_t i)
{
    KyValue v;

    /*
     * Store the exact 32-bit two's-complement bit pattern
     * in the low 32 bits of payload.
     */
    v.payload = (uint64_t)(uint32_t)i;
    v.type = KY_T_INT32;
    v.flags = 0;

    return v;
}

static inline int32_t ky_get_int32(KyValue v)
{
    return (int32_t)(uint32_t)v.payload;
}

static inline int ky_is_int32(KyValue v)
{
    return v.type == KY_T_INT32;
}

/* ------------------------------------------------------------------------- */
/* Double                                                                     */

static inline KyValue ky_double(double d)
{
    KyValue v;
    union {
        double d;
        uint64_t u;
    } bits;

    bits.d = d;

    v.payload = bits.u;
    v.type = KY_T_DOUBLE;
    v.flags = 0;

    return v;
}

static inline double ky_get_double(KyValue v)
{
    union {
        double d;
        uint64_t u;
    } bits;

    bits.u = v.payload;

    return bits.d;
}

/* ------------------------------------------------------------------------- */
/* KyRuntimeContext                                                           */

typedef struct KyRuntimeContext KyRuntimeContext;

struct KyRuntimeContext {

    /*
     * Opaque QuickJS context.
     *
     * The ABI header intentionally does not include quickjs.h.
     */
    void *quickjs_ctx;

    /*
     * Adapter-managed buffers.
     *
     * The JIT core must NOT assume these are JSValue arrays.
     */
    void *var_buf;
    void *arg_buf;
    void *const_buf;

    /*
     * Reserved native shadow stack.
     */
    uint64_t *shadow_stack;

    /* Current QuickJS bytecode PC. */
    uint32_t bytecode_pc;

    /* VM stack depth at trace entry. */
    uint32_t stack_depth;

    /* Deoptimization metadata ID. */
    uint32_t deopt_id;

    /* Reserved instruction budget. */
    uint64_t instruction_budget;
};

/* ------------------------------------------------------------------------- */
/* JIT execution results                                                      */

#define KYJIT_OK      0
#define KYJIT_ERROR  -1
#define KYJIT_DEOPT  -2

/* ------------------------------------------------------------------------- */
/* Global executable-code budget                                              */

/*
 * Initial experimental limit:
 * 5 MiB of generated executable code.
 */
#define KYJIT_CODE_LIMIT (5u * 1024u * 1024u)

/* ------------------------------------------------------------------------- */
/* ABI sanity checks                                                          */

/*
 * These constants are useful to the backend and cache-key implementation.
 */
#define KYJIT_KYVALUE_SIZE       16u
#define KYJIT_KYVALUE_PAYLOAD_OFF 0u
#define KYJIT_KYVALUE_TYPE_OFF    8u
#define KYJIT_KYVALUE_FLAGS_OFF   12u

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_ABI_H */
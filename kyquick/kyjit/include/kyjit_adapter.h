/*
 * kyjit_adapter.h
 *
 * QuickJS 2026-06-04 <-> KyJIT ABI boundary.
 *
 * This is the ONLY KyJIT component that knows about QuickJS JSValue.
 * The JIT IR, optimizer, register allocator, and machine backends must
 * operate exclusively on KyValue and KyRuntimeContext.
 */

#ifndef KYJIT_ADAPTER_H
#define KYJIT_ADAPTER_H

#include <stdint.h>
#include "kyjit_abi.h"
#include "quickjs.h"

#ifdef __cplusplus
extern "C" {
#endif

#define KYJIT_QUICKJS_VERSION "2026-06-04"

typedef struct KyJitAdapter KyJitAdapter;

/* Creation / destruction */

int ky_adapter_create(
    KyJitAdapter **out,
    JSContext *ctx,
    void *var_buf,
    uint32_t local_count,
    void *arg_buf,
    uint32_t arg_count,
    void *const_buf,
    uint32_t const_count);

void ky_adapter_destroy(KyJitAdapter *adapter);

/* Bounds */

uint32_t ky_adapter_local_count(const KyJitAdapter *adapter);
uint32_t ky_adapter_arg_count(const KyJitAdapter *adapter);
uint32_t ky_adapter_const_count(const KyJitAdapter *adapter);

int ky_adapter_validate_local(
    const KyJitAdapter *adapter,
    uint32_t index);

int ky_adapter_validate_arg(
    const KyJitAdapter *adapter,
    uint32_t index);

int ky_adapter_validate_const(
    const KyJitAdapter *adapter,
    uint32_t index);

/* QuickJS -> KyValue */

int ky_adapter_load_local(
    KyJitAdapter *adapter,
    uint32_t index,
    KyValue *out);

int ky_adapter_load_arg(
    KyJitAdapter *adapter,
    uint32_t index,
    KyValue *out);

int ky_adapter_load_const(
    KyJitAdapter *adapter,
    uint32_t index,
    KyValue *out);

/* KyValue -> QuickJS local */

int ky_adapter_store_local(
    KyJitAdapter *adapter,
    uint32_t index,
    KyValue value);

/* Type queries */

int ky_adapter_is_int32(
    const KyJitAdapter *adapter,
    KyValue value);

/* Explicit conversion */

int ky_adapter_jsvalue_to_kyvalue(
    KyJitAdapter *adapter,
    JSValue value,
    KyValue *out);

int ky_adapter_kyvalue_to_jsvalue(
    KyJitAdapter *adapter,
    KyValue value,
    JSValue *out);

/* Context access */

KyRuntimeContext *ky_adapter_runtime(
    KyJitAdapter *adapter);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_ADAPTER_H */
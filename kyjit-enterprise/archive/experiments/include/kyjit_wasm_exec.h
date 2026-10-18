#ifndef KYJIT_WASM_EXEC_H
#define KYJIT_WASM_EXEC_H

#include <stdint.h>
#include <stddef.h>

typedef struct KyjitWasmExec KyjitWasmExec;

/* Create WASM execution engine */
KyjitWasmExec *kyjit_wasm_exec_create(void);

/* Compile a WASM module from bytes */
int kyjit_wasm_exec_compile(KyjitWasmExec *eng,
                                        const uint8_t *wasm, size_t len);

/* Execute a WASM function by name.
 * args: array of int32 arguments
 * num_args: number of arguments
 * result: output result
 * Returns 0 on success, -1 on error.
 */
int kyjit_wasm_exec_call(KyjitWasmExec *mod,
                         const char *func_name,
                         const int32_t *args, size_t num_args,
                         int32_t *result);

/* Free */
void kyjit_wasm_exec_free(KyjitWasmExec *e);

#endif

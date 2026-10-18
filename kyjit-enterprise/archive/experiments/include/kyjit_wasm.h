#ifndef KYJIT_WASM_H
#define KYJIT_WASM_H

#include <stdint.h>
#include <stddef.h>

typedef struct KyjitWasmEngine KyjitWasmEngine;
typedef struct KyjitWasmModule KyjitWasmModule;

/* Create WASM engine */
KyjitWasmEngine *kyjit_wasm_engine_create(void);

/* Compile WASM binary */
KyjitWasmModule *kyjit_wasm_compile(KyjitWasmEngine *engine,
                                    const uint8_t *wasm_bytes, size_t len);

/* Execute WASM function
 * Returns 0 on success, -1 on error
 */
int kyjit_wasm_execute(KyjitWasmModule *module,
                       const char *func_name,
                       const int32_t *args, size_t num_args,
                       int32_t *result);

/* Free module */
void kyjit_wasm_module_free(KyjitWasmModule *module);

/* Free engine */
void kyjit_wasm_engine_free(KyjitWasmEngine *engine);

#endif

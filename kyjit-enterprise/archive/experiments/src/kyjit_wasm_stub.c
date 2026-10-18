#include "kyjit_wasm.h"
#include <stdlib.h>

/* Temporary stub — will wire to Wasmtime after build completes */
struct KyjitWasmEngine { int dummy; };
struct KyjitWasmModule { int dummy; };

KyjitWasmEngine *kyjit_wasm_engine_create(void) {
    return calloc(1, sizeof(KyjitWasmEngine));
}

KyjitWasmModule *kyjit_wasm_compile(KyjitWasmEngine *engine,
                                    const uint8_t *wasm_bytes, size_t len) {
    (void)engine; (void)wasm_bytes; (void)len;
    return calloc(1, sizeof(KyjitWasmModule));
}

int kyjit_wasm_execute(KyjitWasmModule *module,
                       const char *func_name,
                       const int32_t *args, size_t num_args,
                       int32_t *result) {
    (void)module; (void)func_name; (void)args; (void)num_args; (void)result;
    return -1;
}

void kyjit_wasm_module_free(KyjitWasmModule *module) {
    free(module);
}

void kyjit_wasm_engine_free(KyjitWasmEngine *engine) {
    free(engine);
}

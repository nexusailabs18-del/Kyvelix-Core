#include "kyjit_wasm.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

#include "wasmtime.h"

struct KyjitWasmEngine {
    wasm_engine_t *engine;
};

struct KyjitWasmModule {
    wasmtime_module_t *module;
    wasmtime_store_t *store;
    wasmtime_linker_t *linker;
};

KyjitWasmEngine *kyjit_wasm_engine_create(void) {
    KyjitWasmEngine *e = calloc(1, sizeof(*e));
    e->engine = wasm_engine_new();
    return e;
}

KyjitWasmModule *kyjit_wasm_compile(KyjitWasmEngine *engine,
                                    const uint8_t *wasm_bytes, size_t len) {
    KyjitWasmModule *m = calloc(1, sizeof(*m));

    wasmtime_error_t *error = wasmtime_module_new(
        engine->engine, wasm_bytes, len, &m->module);

    if (error) {
        wasm_name_t msg;
        wasm_name_new(&msg, 0, NULL);
        wasmtime_error_message(error, &msg);
        fprintf(stderr, "WASM compile error: %.*s\n",
                (int)msg.size, msg.data);
        wasm_byte_vec_delete(&msg);
        wasmtime_error_delete(error);
        free(m);
        return NULL;
    }

    m->store = wasmtime_store_new(engine->engine, NULL, NULL);
    m->linker = wasmtime_linker_new(engine->engine);

    return m;
}

int kyjit_wasm_execute(KyjitWasmModule *module,
                       const char *func_name,
                       const int32_t *args, size_t num_args,
                       int32_t *result) {
    (void)module; (void)func_name; (void)args; (void)num_args; (void)result;
    return -1;
}

void kyjit_wasm_module_free(KyjitWasmModule *module) {
    if (!module) return;
    if (module->linker) wasmtime_linker_delete(module->linker);
    if (module->store) wasmtime_store_delete(module->store);
    if (module->module) wasmtime_module_delete(module->module);
    free(module);
}

void kyjit_wasm_engine_free(KyjitWasmEngine *engine) {
    if (!engine) return;
    wasm_engine_delete(engine->engine);
    free(engine);
}

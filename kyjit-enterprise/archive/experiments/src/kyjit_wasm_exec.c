#include "kyjit_wasm_exec.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "wasmtime.h"

/* Full WASM execution: compile + instantiate + call */

typedef struct KyjitWasmExec {
    wasm_engine_t *engine;
    wasmtime_store_t *store;
    wasmtime_context_t *context;
    wasmtime_module_t *module;
    wasmtime_instance_t instance;
} KyjitWasmExec;

KyjitWasmExec* kyjit_wasm_exec_new(void) {
    KyjitWasmExec *w = calloc(1, sizeof(KyjitWasmExec));
    if (!w) return NULL;
    
    w->engine = wasm_engine_new();
    w->store = wasmtime_store_new(w->engine, NULL, NULL);
    w->context = wasmtime_store_context(w->store);
    
    return w;
}

int kyjit_wasm_exec_compile(KyjitWasmExec *w, const uint8_t *bytes, size_t len) {
    wasmtime_error_t *error = wasmtime_module_new(w->engine, (uint8_t*)bytes, len, &w->module);
    if (error) {
        wasmtime_error_delete(error);
        return -1;
    }
    
    wasm_trap_t *trap = NULL;
    error = wasmtime_instance_new(w->context, w->module, NULL, 0, &w->instance, &trap);
    if (error || trap) {
        return -1;
    }
    
    return 0;
}

int32_t kyjit_wasm_exec_call_i32_i32(KyjitWasmExec *w, const char *name, int32_t a, int32_t b) {
    wasmtime_extern_t func_extern;
    bool found = wasmtime_instance_export_get(w->context, &w->instance, name, strlen(name), &func_extern);
    if (!found || func_extern.kind != WASMTIME_EXTERN_FUNC) {
        return -1;
    }
    
    wasmtime_val_t args[2];
    wasmtime_val_t results[1];
    
    args[0].kind = WASMTIME_I32;
    args[0].of.i32 = a;
    args[1].kind = WASMTIME_I32;
    args[1].of.i32 = b;
    
    wasm_trap_t *trap = NULL;
    wasmtime_error_t *error = wasmtime_func_call(w->context, &func_extern.of.func, args, 2, results, 1, &trap);
    
    if (error || trap) {
        return -1;
    }
    
    return results[0].of.i32;
}

void kyjit_wasm_exec_free(KyjitWasmExec *w) {
    if (!w) return;
    if (w->module) wasmtime_module_delete(w->module);
    if (w->store) wasmtime_store_delete(w->store);
    if (w->engine) wasm_engine_delete(w->engine);
    free(w);
}

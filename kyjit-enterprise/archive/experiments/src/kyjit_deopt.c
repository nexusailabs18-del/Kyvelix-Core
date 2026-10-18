#include <stdint.h>
#include <string.h>
#include "kyjit_deopt.h"

/* SAFE V8-style Deoptimization
 * Instead of patching machine code (dangerous),
 * swap function pointers — same effect, zero crash risk.
 */

typedef struct {
    void *optimized_code;
    void *fallback_code;
    void *current_code;
    int deopt_count;
} KyjitDeoptEntry;

static KyjitDeoptEntry deopt_table[64];
static int deopt_count = 0;

int kyjit_deopt_register(void *optimized, void *fallback) {
    if (deopt_count >= 64) return -1;
    deopt_table[deopt_count].optimized_code = optimized;
    deopt_table[deopt_count].fallback_code = fallback;
    deopt_table[deopt_count].current_code = optimized;
    deopt_table[deopt_count].deopt_count = 0;
    return deopt_count++;
}

/* Swap to fallback (safe — just change pointer) */
void kyjit_deopt_patch_to_fallback(int entry_id) {
    KyjitDeoptEntry *e = &deopt_table[entry_id];
    e->deopt_count++;
    e->current_code = e->fallback_code;
}

int kyjit_deopt_should_bailout(int entry_id) {
    return deopt_table[entry_id].deopt_count > 10;
}

/* Return current code (fast or fallback) */
void* kyjit_deopt_execute(int entry_id, void *data) {
    (void)data;
    return deopt_table[entry_id].current_code;
}

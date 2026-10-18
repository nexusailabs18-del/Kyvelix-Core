#include "kyjit_osr.h"
#include <stdlib.h>
#include <string.h>

#define HOT_THRESHOLD 1000

struct KyjitOSR {
    int *hot_counts;
    int *compiled;
    int instruction_count;
};

KyjitOSR *kyjit_osr_create(void) {
    KyjitOSR *osr = calloc(1, sizeof(*osr));
    return osr;
}

void kyjit_osr_mark_hot(KyjitOSR *osr, int instruction_index) {
    if (!osr) return;

    /* Grow arrays if needed */
    if (instruction_index >= osr->instruction_count) {
        int new_count = instruction_index + 1;
        osr->hot_counts = realloc(osr->hot_counts, new_count * sizeof(int));
        osr->compiled = realloc(osr->compiled, new_count * sizeof(int));
        for (int i = osr->instruction_count; i < new_count; i++) {
            osr->hot_counts[i] = 0;
            osr->compiled[i] = 0;
        }
        osr->instruction_count = new_count;
    }

    osr->hot_counts[instruction_index]++;
}

int kyjit_osr_is_hot(KyjitOSR *osr, int instruction_index) {
    if (!osr || instruction_index >= osr->instruction_count) return 0;
    return osr->hot_counts[instruction_index] >= HOT_THRESHOLD &&
           !osr->compiled[instruction_index];
}

KyBytecodeBuf *kyjit_osr_compile_hot(KyjitOSR *osr, KyBytecodeBuf *original) {
    if (!osr || !original) return NULL;

    /* For now, just copy original and mark as compiled */
    KyBytecodeBuf *optimized = calloc(1, sizeof(*optimized));
    optimized->code = malloc(original->length * sizeof(KyBytecode));
    memcpy(optimized->code, original->code, original->length * sizeof(KyBytecode));
    optimized->length = original->length;
    optimized->capacity = original->length;

    /* Mark all instructions as compiled */
    for (int i = 0; i < original->length; i++) {
        if (i < osr->instruction_count) osr->compiled[i] = 1;
    }

    return optimized;
}

void kyjit_osr_reset(KyjitOSR *osr) {
    if (!osr) return;
    memset(osr->hot_counts, 0, osr->instruction_count * sizeof(int));
    memset(osr->compiled, 0, osr->instruction_count * sizeof(int));
}

void kyjit_osr_free(KyjitOSR *osr) {
    if (!osr) return;
    free(osr->hot_counts);
    free(osr->compiled);
    free(osr);
}

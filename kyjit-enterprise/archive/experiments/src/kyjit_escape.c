#include "kyjit_escape.h"
#include <stdlib.h>
#include <string.h>

#define MAX_OBJECTS 1024

struct KyjitEscapeAnalysis {
    uint8_t escaped[MAX_OBJECTS];
    uint8_t escape_type[MAX_OBJECTS];
    int     count;
};

KyjitEscapeAnalysis *kyjit_escape_create(void) {
    KyjitEscapeAnalysis *ea = calloc(1, sizeof(*ea));
    return ea;
}

void kyjit_escape_track_alloc(KyjitEscapeAnalysis *ea, void *obj, int obj_id) {
    (void)obj;
    if (!ea || obj_id >= MAX_OBJECTS) return;
    ea->escaped[obj_id] = 0;
    ea->escape_type[obj_id] = KYJIT_NO_ESCAPE;
    if (obj_id >= ea->count) ea->count = obj_id + 1;
}

void kyjit_escape_mark_arg(KyjitEscapeAnalysis *ea, int obj_id) {
    if (!ea || obj_id >= MAX_OBJECTS) return;
    ea->escaped[obj_id] = 1;
    ea->escape_type[obj_id] = KYJIT_ARG_ESCAPE;
}

void kyjit_escape_mark_return(KyjitEscapeAnalysis *ea, int obj_id) {
    if (!ea || obj_id >= MAX_OBJECTS) return;
    ea->escaped[obj_id] = 1;
    ea->escape_type[obj_id] = KYJIT_RETURN_ESCAPE;
}

void kyjit_escape_mark_global(KyjitEscapeAnalysis *ea, int obj_id) {
    if (!ea || obj_id >= MAX_OBJECTS) return;
    ea->escaped[obj_id] = 1;
    ea->escape_type[obj_id] = KYJIT_GLOBAL_ESCAPE;
}

KyjitEscapeResult kyjit_escape_analyze(KyjitEscapeAnalysis *ea, int obj_id) {
    if (!ea || obj_id >= MAX_OBJECTS || obj_id >= ea->count)
        return KYJIT_GLOBAL_ESCAPE;
    return (KyjitEscapeResult)ea->escape_type[obj_id];
}

void kyjit_escape_free(KyjitEscapeAnalysis *ea) {
    free(ea);
}

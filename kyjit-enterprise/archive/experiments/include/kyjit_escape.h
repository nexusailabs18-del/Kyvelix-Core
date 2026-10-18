#ifndef KYJIT_ESCAPE_H
#define KYJIT_ESCAPE_H

#include <stdint.h>

/* Escape analysis result */
typedef enum {
    KYJIT_NO_ESCAPE = 0,     /* Object stays in function → can scalar replace */
    KYJIT_ARG_ESCAPE = 1,    /* Passed as argument → may escape */
    KYJIT_RETURN_ESCAPE = 2, /* Returned → escapes */
    KYJIT_GLOBAL_ESCAPE = 3, /* Stored in global → escapes */
} KyjitEscapeResult;

typedef struct KyjitEscapeAnalysis KyjitEscapeAnalysis;

/* Create escape analysis */
KyjitEscapeAnalysis *kyjit_escape_create(void);

/* Track object allocation */
void kyjit_escape_track_alloc(KyjitEscapeAnalysis *ea, void *obj, int obj_id);

/* Track object usage */
void kyjit_escape_mark_arg(KyjitEscapeAnalysis *ea, int obj_id);
void kyjit_escape_mark_return(KyjitEscapeAnalysis *ea, int obj_id);
void kyjit_escape_mark_global(KyjitEscapeAnalysis *ea, int obj_id);

/* Analyze: does object escape? */
KyjitEscapeResult kyjit_escape_analyze(KyjitEscapeAnalysis *ea, int obj_id);

/* Free */
void kyjit_escape_free(KyjitEscapeAnalysis *ea);

#endif

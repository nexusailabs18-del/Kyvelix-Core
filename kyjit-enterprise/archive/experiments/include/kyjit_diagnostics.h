#ifndef KYJIT_DIAGNOSTICS_H
#define KYJIT_DIAGNOSTICS_H

#include <stdint.h>
#include <stddef.h>

/* Diagnostic severity */
typedef enum {
    KYJIT_DIAG_INFO = 0,
    KYJIT_DIAG_WARN = 1,
    KYJIT_DIAG_ERROR = 2,
    KYJIT_DIAG_FATAL = 3,
} KyjitDiagLevel;

/* Diagnostic category */
typedef enum {
    KYJIT_DIAG_JIT = 0,
    KYJIT_DIAG_GC,
    KYJIT_DIAG_PARSE,
    KYJIT_DIAG_RUNTIME,
    KYJIT_DIAG_MEMORY,
} KyjitDiagCategory;

typedef struct KyjitDiagnostics KyjitDiagnostics;

/* Create diagnostics */
KyjitDiagnostics *kyjit_diag_create(void);

/* Report a diagnostic */
void kyjit_diag_report(KyjitDiagnostics *d, KyjitDiagLevel level,
                       KyjitDiagCategory cat, const char *fmt, ...);

/* Get counts */
int kyjit_diag_error_count(KyjitDiagnostics *d);
int kyjit_diag_warning_count(KyjitDiagnostics *d);

/* Print summary */
void kyjit_diag_summary(KyjitDiagnostics *d);

/* Free */
void kyjit_diag_free(KyjitDiagnostics *d);

#endif

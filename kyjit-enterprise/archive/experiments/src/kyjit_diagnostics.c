#include "kyjit_diagnostics.h"
#include <stdio.h>
#include <stdlib.h>
#include <stdarg.h>
#include <string.h>

struct KyjitDiagnostics {
    int info_count;
    int warn_count;
    int error_count;
    int fatal_count;
};

static const char *level_name(KyjitDiagLevel level) {
    switch (level) {
        case KYJIT_DIAG_INFO:  return "INFO";
        case KYJIT_DIAG_WARN:  return "WARN";
        case KYJIT_DIAG_ERROR: return "ERROR";
        case KYJIT_DIAG_FATAL: return "FATAL";
        default: return "?";
    }
}

static const char *cat_name(KyjitDiagCategory cat) {
    switch (cat) {
        case KYJIT_DIAG_JIT:     return "JIT";
        case KYJIT_DIAG_GC:      return "GC";
        case KYJIT_DIAG_PARSE:   return "PARSE";
        case KYJIT_DIAG_RUNTIME: return "RUNTIME";
        case KYJIT_DIAG_MEMORY:  return "MEMORY";
        default: return "?";
    }
}

KyjitDiagnostics *kyjit_diag_create(void) {
    return calloc(1, sizeof(KyjitDiagnostics));
}

void kyjit_diag_report(KyjitDiagnostics *d, KyjitDiagLevel level,
                       KyjitDiagCategory cat, const char *fmt, ...) {
    if (!d) return;

    switch (level) {
        case KYJIT_DIAG_INFO:  d->info_count++; break;
        case KYJIT_DIAG_WARN:  d->warn_count++; break;
        case KYJIT_DIAG_ERROR: d->error_count++; break;
        case KYJIT_DIAG_FATAL: d->fatal_count++; break;
    }

    fprintf(stderr, "[%s][%s] ", level_name(level), cat_name(cat));

    va_list ap;
    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
    fputc('\n', stderr);
}

int kyjit_diag_error_count(KyjitDiagnostics *d) {
    return d ? d->error_count : 0;
}

int kyjit_diag_warning_count(KyjitDiagnostics *d) {
    return d ? d->warn_count : 0;
}

void kyjit_diag_summary(KyjitDiagnostics *d) {
    if (!d) return;
    printf("=== DIAGNOSTICS SUMMARY ===\n");
    printf("Info:    %d\n", d->info_count);
    printf("Warnings: %d\n", d->warn_count);
    printf("Errors:   %d\n", d->error_count);
    printf("Fatal:    %d\n", d->fatal_count);
}

void kyjit_diag_free(KyjitDiagnostics *d) {
    free(d);
}

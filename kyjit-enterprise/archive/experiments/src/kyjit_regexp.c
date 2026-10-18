#include "kyjit_regexp.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

typedef enum {
    RE_CHAR, RE_ANY, RE_STAR, RE_PLUS, RE_OPTIONAL,
    RE_OR, RE_CLASS, RE_START, RE_END, RE_MATCH
} ReOp;

typedef struct {
    ReOp  op;
    char  ch;
    char *class_bits;
} ReInst;

struct KyjitRegexp {
    ReInst *insts;
    int     count;
};

static int match_at(KyjitRegexp *re, int idx, const char *str, const char *start, const char *end);

/* Match one char without advancing pattern — used by quantifiers */
static int char_matches(KyjitRegexp *re, int idx, const char *str, const char *end) {
    if (str >= end) return 0;
    ReInst *inst = &re->insts[idx];
    switch (inst->op) {
        case RE_CHAR:  return *str == inst->ch;
        case RE_ANY:   return 1;
        case RE_CLASS: return inst->class_bits[(unsigned char)*str] != 0;
        default: return 0;
    }
}

/* Match starting at idx. Handles quantifiers by looking ahead one instruction. */
static int match_at(KyjitRegexp *re, int idx, const char *str, const char *start, const char *end) {
    if (idx >= re->count) return 1;

    ReInst *inst = &re->insts[idx];

    /* Look ahead: if the next instruction is a quantifier, handle it here */
    if (idx + 1 < re->count &&
        (re->insts[idx+1].op == RE_STAR ||
         re->insts[idx+1].op == RE_PLUS ||
         re->insts[idx+1].op == RE_OPTIONAL)) {

        ReOp quant = re->insts[idx+1].op;
        int min = (quant == RE_PLUS) ? 1 : 0;

        /* Greedy: consume as many as possible */
        const char *s = str;
        while (char_matches(re, idx, s, end)) {
            s++;
        }

        /* Backtrack from max down to min */
        for (const char *t = s; ; t--) {
            int consumed = (int)(t - str);
            if (consumed >= min) {
                if (match_at(re, idx+2, t, start, end)) return 1;
            }
            if (t == str) break;
        }
        return 0;
    }

    /* Normal instructions */
    switch (inst->op) {
        case RE_MATCH:
            return 1;

        case RE_END:
            return (str == end);

        case RE_START:
            return (str == start) ? match_at(re, idx+1, str, start, end) : 0;

        case RE_CHAR:
            if (str >= end) return 0;
            return (*str == inst->ch) ? match_at(re, idx+1, str+1, start, end) : 0;

        case RE_ANY:
            if (str >= end) return 0;
            return match_at(re, idx+1, str+1, start, end);

        case RE_CLASS:
            if (str >= end) return 0;
            return inst->class_bits[(unsigned char)*str] ? match_at(re, idx+1, str+1, start, end) : 0;

        case RE_OR:
            return match_at(re, idx+1, str, start, end) ||
                   match_at(re, idx+2, str, start, end);

        default:
            return 0;
    }
}

KyjitRegexp *kyjit_regexp_compile(const char *pattern) {
    KyjitRegexp *re = calloc(1, sizeof(*re));
    int cap = 64;
    re->insts = calloc(cap, sizeof(ReInst));

    const char *p = pattern;
    while (*p) {
        if (re->count >= cap) {
            cap *= 2;
            re->insts = realloc(re->insts, cap * sizeof(ReInst));
        }
        ReInst *inst = &re->insts[re->count];
        switch (*p) {
            case '.': inst->op = RE_ANY; break;
            case '*': inst->op = RE_STAR; break;
            case '+': inst->op = RE_PLUS; break;
            case '?': inst->op = RE_OPTIONAL; break;
            case '|': inst->op = RE_OR; break;
            case '^': inst->op = RE_START; break;
            case '$': inst->op = RE_END; break;
            case '[':
                inst->op = RE_CLASS;
                inst->class_bits = calloc(256, 1);
                p++;
                while (*p && *p != ']') {
                    inst->class_bits[(unsigned char)*p] = 1;
                    p++;
                }
                if (*p == ']') p++;
                re->count++;
                continue;
            case '\\':
                p++;
                inst->op = RE_CHAR;
                inst->ch = *p;
                break;
            default:
                inst->op = RE_CHAR;
                inst->ch = *p;
        }
        re->count++;
        p++;
    }
    re->insts[re->count].op = RE_MATCH;
    re->count++;
    return re;
}

int kyjit_regexp_match(KyjitRegexp *re, const char *str, size_t len) {
    if (!re || !str) return -1;
    const char *end = str + len;

    if (re->insts[0].op == RE_START) {
        return match_at(re, 0, str, str, end);
    }

    for (const char *s = str; s <= end; s++) {
        if (match_at(re, 0, s, str, end)) return 1;
    }
    return 0;
}

void kyjit_regexp_free(KyjitRegexp *re) {
    if (!re) return;
    for (int i = 0; i < re->count; i++) {
        if (re->insts[i].class_bits) free(re->insts[i].class_bits);
    }
    free(re->insts);
    free(re);
}

int kyjit_regexp_match_once(const char *pattern, const char *str) {
    KyjitRegexp *re = kyjit_regexp_compile(pattern);
    int result = kyjit_regexp_match(re, str, strlen(str));
    kyjit_regexp_free(re);
    return result;
}

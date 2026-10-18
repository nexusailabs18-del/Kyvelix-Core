#include "kyjit_bytecode.h"
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <stdio.h>

/* Parser state */
typedef struct {
    const char *p;
    KyBytecodeBuf *buf;
} Parser;

static void ensure_capacity(KyBytecodeBuf *buf, int needed) {
    if (buf->length + needed <= buf->capacity) return;
    int new_cap = buf->capacity ? buf->capacity * 2 : 64;
    buf->code = realloc(buf->code, new_cap * sizeof(KyBytecode));
    buf->capacity = new_cap;
}

static void emit(Parser *ps, KyBytecodeOp op, int32_t a, int32_t b) {
    KyBytecodeBuf *buf = ps->buf;
    ensure_capacity(buf, 1);
    buf->code[buf->length++] = (KyBytecode){op, a, b};
}

static void skip_ws(Parser *ps) {
    while (*ps->p && (*ps->p==' '||*ps->p=='\n'||*ps->p=='\t'||*ps->p==';'))
        ps->p++;
}

static int match_word(Parser *ps, const char *w) {
    int l = strlen(w);
    if (strncmp(ps->p, w, l)==0 && !isalnum(ps->p[l])) {
        ps->p += l;
        return 1;
    }
    return 0;
}

/* Forward decls */
static void parse_expression(Parser *ps);
static void parse_term(Parser *ps);
static void parse_factor(Parser *ps);

static void parse_factor(Parser *ps) {
    skip_ws(ps);
    if (isdigit(*ps->p)) {
        int val = 0;
        while (isdigit(*ps->p)) { val = val*10 + (*ps->p-'0'); ps->p++; }
        emit(ps, BC_LOAD_CONST, val, 0);
    } else if (isalpha(*ps->p)) {
        char name = *ps->p;
        ps->p++;
        emit(ps, BC_LOAD_VAR, name, 0);
    } else if (*ps->p == '(') {
        ps->p++;
        parse_expression(ps);
        skip_ws(ps);
        if (*ps->p == ')') ps->p++;
    }
}

static void parse_term(Parser *ps) {
    parse_factor(ps);
    while (1) {
        skip_ws(ps);
        if (*ps->p == '*') {
            ps->p++;
            parse_factor(ps);
            emit(ps, BC_MUL, 0, 0);
        } else if (*ps->p == '/') {
            ps->p++;
            parse_factor(ps);
            emit(ps, BC_DIV, 0, 0);
        } else break;
    }
}

static void parse_expression(Parser *ps) {
    parse_term(ps);
    while (1) {
        skip_ws(ps);
        if (*ps->p == '+') {
            ps->p++;
            parse_term(ps);
            emit(ps, BC_ADD, 0, 0);
        } else if (*ps->p == '-') {
            ps->p++;
            parse_term(ps);
            emit(ps, BC_SUB, 0, 0);
        } else break;
    }
}

KyBytecodeBuf* kyjit_compile_to_bytecode(const char *src) {
    KyBytecodeBuf *buf = calloc(1, sizeof(KyBytecodeBuf));
    Parser ps = { .p = src, .buf = buf };
    
    while (*ps.p) {
        skip_ws(&ps);
        if (!*ps.p) break;
        
        if (match_word(&ps, "var")) {
            skip_ws(&ps);
            char name = *ps.p++;
            skip_ws(&ps);
            if (*ps.p == '=') {
                ps.p++;
                parse_expression(&ps);
                emit(&ps, BC_STORE_VAR, name, 0);
            }
            continue;
        }
        
        if (match_word(&ps, "return")) {
            parse_expression(&ps);
            emit(&ps, BC_RETURN, 0, 0);
            continue;
        }
        
        /* Assignment without var */
        if (isalpha(*ps.p)) {
            char name = *ps.p++;
            Parser tmp = ps;
            skip_ws(&tmp);
            if (*tmp.p == '=') {
                ps = tmp;
                ps.p++;
                parse_expression(&ps);
                emit(&ps, BC_STORE_VAR, name, 0);
            } else {
                ps.p--; /* unget */
                parse_expression(&ps);
            }
            continue;
        }
        
        ps.p++; /* skip unknown */
    }
    
    return buf;
}

int kyjit_bytecode_execute(KyBytecodeBuf *buf) {
    int stack[256], sp=0;
    int vars[26]={0};
    
    for (int ip=0; ip<buf->length; ip++) {
        KyBytecode *bc = &buf->code[ip];
        switch(bc->op) {
            case BC_LOAD_CONST: stack[sp++]=bc->operand1; break;
            case BC_LOAD_VAR:   stack[sp++]=vars[bc->operand1-'a']; break;
            case BC_STORE_VAR:  vars[bc->operand1-'a']=stack[--sp]; break;
            case BC_ADD: {int b=stack[--sp],a=stack[--sp];stack[sp++]=a+b;break;}
            case BC_SUB: {int b=stack[--sp],a=stack[--sp];stack[sp++]=a-b;break;}
            case BC_MUL: {int b=stack[--sp],a=stack[--sp];stack[sp++]=a*b;break;}
            case BC_DIV: {int b=stack[--sp],a=stack[--sp];stack[sp++]=a/b;break;}
            case BC_RETURN: return (sp>0)?stack[sp-1]:0;
            default: break;
        }
    }
    return 0;
}

void kyjit_bytecode_free(KyBytecodeBuf *buf) {
    if(buf){free(buf->code);free(buf);}
}

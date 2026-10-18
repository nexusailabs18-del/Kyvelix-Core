#include "kyjit_interpreter.h"
#include <stdio.h>

#define STACK_SIZE 4096
#define VAR_COUNT  256

typedef struct {
    long long stack[STACK_SIZE];
    int sp;
    long long vars[VAR_COUNT];
} InterpState;

static const char *op_name(int op) {
    switch (op) {
        case BC_LOAD_CONST: return "LOAD_CONST";
        case BC_LOAD_VAR:   return "LOAD_VAR";
        case BC_STORE_VAR:  return "STORE_VAR";
        case BC_ADD:        return "ADD";
        case BC_SUB:        return "SUB";
        case BC_MUL:        return "MUL";
        case BC_DIV:        return "DIV";
        case BC_RETURN:     return "RETURN";
        case BC_CALL:       return "CALL";
        case BC_JUMP:       return "JUMP";
        case BC_JUMP_IF_FALSE: return "JUMP_IF_FALSE";
        default: return "UNKNOWN";
    }
}

void kyjit_interpreter_dump(KyBytecodeBuf *bc) {
    printf("=== Bytecode (%d instructions) ===\n", bc->length);
    for (int i = 0; i < bc->length; i++) {
        KyBytecode *b = &bc->code[i];
        printf("  [%04d] %-14s a=%d b=%d\n",
               i, op_name(b->op), b->operand1, b->operand2);
    }
}

int kyjit_interpreter_execute(KyBytecodeBuf *bc, int verbose) {
    InterpState st = {0};

    for (int ip = 0; ip < bc->length; ip++) {
        KyBytecode *b = &bc->code[ip];

        if (verbose >= 1) {
            printf("[%04d] %s a=%d b=%d\n",
                   ip, op_name(b->op), b->operand1, b->operand2);
        }

        switch (b->op) {
            case BC_LOAD_CONST:
                if (st.sp >= STACK_SIZE) {
                    fprintf(stderr, "Stack overflow\n");
                    return -1;
                }
                st.stack[st.sp++] = b->operand1;
                break;

            case BC_LOAD_VAR:
                if (st.sp >= STACK_SIZE) {
                    fprintf(stderr, "Stack overflow\n");
                    return -1;
                }
                if (b->operand1 < 0 || b->operand1 >= VAR_COUNT) {
                    fprintf(stderr, "Var index %d out of range\n", b->operand1);
                    return -1;
                }
                st.stack[st.sp++] = st.vars[b->operand1];
                break;

            case BC_STORE_VAR:
                if (st.sp <= 0) {
                    fprintf(stderr, "Stack underflow on STORE_VAR\n");
                    return -1;
                }
                if (b->operand1 < 0 || b->operand1 >= VAR_COUNT) {
                    fprintf(stderr, "Var index %d out of range\n", b->operand1);
                    return -1;
                }
                st.vars[b->operand1] = st.stack[--st.sp];
                break;

            case BC_ADD:
            case BC_SUB:
            case BC_MUL:
            case BC_DIV:
                if (st.sp < 2) {
                    fprintf(stderr, "Stack underflow on %s\n", op_name(b->op));
                    return -1;
                }
                {
                    long long rhs = st.stack[--st.sp];
                    long long lhs = st.stack[--st.sp];
                    long long result = 0;

                    switch (b->op) {
                        case BC_ADD: result = lhs + rhs; break;
                        case BC_SUB: result = lhs - rhs; break;
                        case BC_MUL: result = lhs * rhs; break;
                        case BC_DIV:
                            if (rhs == 0) {
                                fprintf(stderr, "Division by zero\n");
                                return -1;
                            }
                            result = lhs / rhs;
                            break;
                    }
                    st.stack[st.sp++] = result;
                }
                break;

            case BC_RETURN:
                if (st.sp <= 0) {
                    fprintf(stderr, "Stack underflow on RETURN\n");
                    return -1;
                }
                return (int)st.stack[st.sp - 1];

            case BC_CALL:
            case BC_JUMP:
            case BC_JUMP_IF_FALSE:
                fprintf(stderr, "Opcode %s not implemented\n", op_name(b->op));
                return -1;

            default:
                fprintf(stderr, "Unknown opcode %d at ip=%d\n", b->op, ip);
                return -1;
        }

        if (verbose >= 2) {
            printf("    stack=[");
            for (int i = 0; i < st.sp; i++) {
                printf("%lld%s", st.stack[i], i < st.sp-1 ? "," : "");
            }
            printf("]\n");
        }
    }

    return 0;
}

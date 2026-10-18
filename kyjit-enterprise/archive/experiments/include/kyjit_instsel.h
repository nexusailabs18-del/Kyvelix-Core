#ifndef KYJIT_INSTSEL_H
#define KYJIT_INSTSEL_H

#include <stdint.h>

#define likely(x)   __builtin_expect(!!(x), 1)
#define unlikely(x) __builtin_expect(!!(x), 0)
#define KY_ALIGN_64 __attribute__((aligned(64)))

// x86 instructions we can select
typedef enum {
    KY_X86_MOV = 0,
    KY_X86_ADD = 1,
    KY_X86_SUB = 2,
    KY_X86_MUL = 3,
    KY_X86_DIV = 4,
    KY_X86_LOAD = 5,
    KY_X86_STORE = 6,
    KY_X86_CMP = 7,
    KY_X86_JMP = 8,
    KY_X86_JE = 9,
    KY_X86_JNE = 10,
    KY_X86_CALL = 11,
    KY_X86_RET = 12,
    KY_X86_LEA = 13,
    KY_X86_PUSH = 14,
    KY_X86_POP = 15
} KyX86Opcode;

// Instruction
typedef struct KyInstruction {
    KyX86Opcode opcode;
    uint32_t operand_count;
    uint32_t operands[3];  // register indices or immediates
    uint32_t latency;       // cycles
} KyInstruction;

// Instruction scheduler
typedef struct KyScheduler {
    KyInstruction *instructions;
    uint32_t count;
    uint32_t capacity;
} KyScheduler;

static inline KyScheduler *ky_scheduler_create(uint32_t capacity) {
    KyScheduler *s = calloc(1, sizeof(KyScheduler));
    if (!s) return NULL;
    s->instructions = calloc(capacity, sizeof(KyInstruction));
    if (!s->instructions) { free(s); return NULL; }
    s->capacity = capacity;
    return s;
}

// Select instruction (simple mapping)
static inline KyInstruction ky_select_mov(uint32_t dst, uint32_t src) {
    KyInstruction inst = { .opcode = KY_X86_MOV, .operand_count = 2, .latency = 1 };
    inst.operands[0] = dst;
    inst.operands[1] = src;
    return inst;
}

static inline KyInstruction ky_select_add(uint32_t dst, uint32_t src1, uint32_t src2) {
    KyInstruction inst = { .opcode = KY_X86_ADD, .operand_count = 3, .latency = 1 };
    inst.operands[0] = dst;
    inst.operands[1] = src1;
    inst.operands[2] = src2;
    return inst;
}

// Schedule: sort by latency (simple greedy)
static inline void ky_schedule(KyScheduler *s) {
    if (!s) return;
    // Simple bubble sort by latency (shortest first)
    for (uint32_t i = 0; i < s->count; i++) {
        for (uint32_t j = i + 1; j < s->count; j++) {
            if (s->instructions[i].latency > s->instructions[j].latency) {
                KyInstruction tmp = s->instructions[i];
                s->instructions[i] = s->instructions[j];
                s->instructions[j] = tmp;
            }
        }
    }
}
#endif

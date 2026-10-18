#ifndef KYJIT_BUILTINS_H
#define KYJIT_BUILTINS_H

#include <stdint.h>

/* Inline builtins — generated as raw machine code
 * No function call, no stack frame, just instructions */

/* Emit ADD instruction directly */
void kyjit_emit_add(uint8_t *code, int *offset);

/* Emit MUL instruction directly */
void kyjit_emit_mul(uint8_t *code, int *offset);

/* Emit LOAD CONSTANT directly */
void kyjit_emit_load_const(uint8_t *code, int *offset, int32_t value);

/* Emit RETURN directly */
void kyjit_emit_return(uint8_t *code, int *offset);

/* Execute generated machine code */
int64_t kyjit_execute_machine_code(uint8_t *code, int length);

#endif

#ifndef KYJIT_INTERPRETER_H
#define KYJIT_INTERPRETER_H

#include "kyjit_bytecode.h"

/* Execute bytecode.
 * verbose=0: silent
 * verbose=1: print each instruction
 * verbose=2: print stack after each instruction
 *
 * Returns:
 *   >= 0  → result value
 *   -1    → error (stack overflow, unknown opcode, div by zero)
 */
int kyjit_interpreter_execute(KyBytecodeBuf *bytecode, int verbose);

/* Pretty-print bytecode for debugging */
void kyjit_interpreter_dump(KyBytecodeBuf *bytecode);

#endif

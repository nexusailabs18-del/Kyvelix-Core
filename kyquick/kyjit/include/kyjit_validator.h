#ifndef KYJIT_VALIDATOR_H
#define KYJIT_VALIDATOR_H

#include <stdint.h>
#include <stddef.h>
#include "kyjit_bytecode.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct KyValidationInfo {
    uint32_t instruction_count;
    uint32_t max_stack;
    uint32_t backedge_count;
    uint32_t branch_count;

    int has_backedge;
    int single_entry;
    int single_exit;
    int balanced_stack;
    int supported;
} KyValidationInfo;

/*
 * Validate a bytecode region for V1 native compilation.
 *
 * V1 accepts only a strict simple-loop shape:
 *   - all instructions decode successfully
 *   - branch targets are valid instruction boundaries or region end
 *   - stack never underflows
 *   - stack depth agrees at CFG joins
 *   - exactly one backedge
 *   - backedge targets the loop header
 *   - one logical exit
 *   - exit stack depth equals entry depth
 *
 * Returns:
 *   0  = valid / supported
 *  -1  = invalid / unsupported
 */
int ky_validate_loop(const KyBytecodeRegion *region,
                     int32_t initial_depth,
                     KyValidationInfo *info);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_VALIDATOR_H */
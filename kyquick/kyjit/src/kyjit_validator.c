/*
 * kyjit_validator.c - Implementation of the KyJIT loop validator
 *
 * Builds a control-flow graph using a worklist algorithm and verifies
 * stack depth consistency and loop shape.  This version is intentionally
 * strict: only simple single-entry loops with one backedge and a single
 * logical exit are accepted.
 */

#include "kyjit_validator.h"
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

/* ------------------------------------------------------------------------- */
/* CFG node types (internal)                                                  */

typedef struct ValidatorCFGNode {
    size_t offset;             /* byte offset from region start */
    int32_t stack_depth;       /* verified stack depth, -1 if unknown */
    int visited;               /* 1 if processed */
    struct ValidatorCFGNode **preds;   /* predecessor edges */
    int pred_count;
    int pred_cap;
    struct ValidatorCFGNode **succs;   /* successor edges */
    int succ_count;
    int succ_cap;
} ValidatorCFGNode;

/* ------------------------------------------------------------------------- */
/* Helper: add unique edge                                                    */

static int add_unique_edge(ValidatorCFGNode *src, ValidatorCFGNode *dst)
{
    for (int i = 0; i < src->succ_count; i++) {
        if (src->succs[i] == dst)
            return 0;
    }

    if (src->succ_count == src->succ_cap) {
        int ncap = src->succ_cap ? src->succ_cap * 2 : 4;
        ValidatorCFGNode **np = realloc(src->succs, (size_t)ncap * sizeof(*np));
        if (!np) return -1;
        src->succs = np;
        src->succ_cap = ncap;
    }
    src->succs[src->succ_count++] = dst;

    if (dst->pred_count == dst->pred_cap) {
        int ncap = dst->pred_cap ? dst->pred_cap * 2 : 4;
        ValidatorCFGNode **np = realloc(dst->preds, (size_t)ncap * sizeof(*np));
        if (!np) return -1;
        dst->preds = np;
        dst->pred_cap = ncap;
    }
    dst->preds[dst->pred_count++] = src;

    return 0;
}

/* ------------------------------------------------------------------------- */
/* Helper: record exit stack depth                                            */

static int record_exit_depth(int32_t depth, int32_t *exit_depth)
{
    if (*exit_depth < 0) {
        *exit_depth = depth;
        return 0;
    }
    return (*exit_depth == depth) ? 0 : -1;
}

/* ------------------------------------------------------------------------- */
/* Public validation function                                                  */

int ky_validate_loop(const KyBytecodeRegion *region,
                     int32_t initial_depth,
                     KyValidationInfo *info)
{
    size_t *offsets = NULL;
    KyDecodedOp *ops = NULL;
    ValidatorCFGNode *nodes = NULL;
    int32_t *queue = NULL;

    if (!region || region->start >= region->end_exclusive)
        return -1;

    if (initial_depth < 0)
        return -1;

    if (ky_bytecode_init() != 0)
        return -1;

    if (info) {
        memset(info, 0, sizeof(*info));
    }

    const uint8_t *pc = region->start;
    size_t region_size = (size_t)(region->end_exclusive - region->start);

    int inst_count = 0;
    int inst_cap = 0;

    while (pc < region->end_exclusive) {
        KyDecodedOp op;
        if (ky_bytecode_decode(region, pc, &op) != 0)
            goto fail;

        if (inst_count == inst_cap) {
            int ncap = inst_cap ? inst_cap * 2 : 16;
            size_t *no = realloc(offsets, (size_t)ncap * sizeof(size_t));
            if (!no) goto fail;
            offsets = no;
            KyDecodedOp *np = realloc(ops, (size_t)ncap * sizeof(KyDecodedOp));
            if (!np) goto fail;
            ops = np;
            inst_cap = ncap;
        }

        offsets[inst_count] = (size_t)(pc - region->start);
        ops[inst_count] = op;
        inst_count++;

        if (pc > region->end_exclusive ||
            (size_t)(region->end_exclusive - pc) < op.size)
            goto fail;
        pc += op.size;
    }

    if (pc != region->end_exclusive)
        goto fail;

    nodes = calloc((size_t)inst_count, sizeof(ValidatorCFGNode));
    if (!nodes)
        goto fail;

    for (int i = 0; i < inst_count; i++) {
        nodes[i].offset = offsets[i];
        nodes[i].stack_depth = -1;
        nodes[i].visited = 0;
    }

    int backedge_count = 0;
    int branch_count = 0;
    int has_exit = 0;

    for (int i = 0; i < inst_count; i++) {
        KyDecodedOp op = ops[i];
        size_t cur = offsets[i];
        size_t next_offset = cur + op.size;

        switch (op.opcode) {
        case KY_OP_GOTO:
            branch_count++;
            break;

        case KY_OP_IF_TRUE:
        case KY_OP_IF_FALSE:
            branch_count++;
            if (i + 1 < inst_count && offsets[i + 1] == next_offset) {
                if (add_unique_edge(&nodes[i], &nodes[i + 1]) != 0)
                    goto fail;
            } else if (next_offset > region_size) {
                goto fail;
            }
            break;

        default:
            if (i + 1 < inst_count && offsets[i + 1] == next_offset) {
                if (add_unique_edge(&nodes[i], &nodes[i + 1]) != 0)
                    goto fail;
            } else if (next_offset > region_size) {
                goto fail;
            }
            break;
        }

        if (op.opcode == KY_OP_GOTO ||
            op.opcode == KY_OP_IF_TRUE ||
            op.opcode == KY_OP_IF_FALSE)
        {
            if (op.relative_offset == 0 && op.opcode == KY_OP_GOTO)
                goto fail;

            int64_t target_i64 =
                (int64_t)cur + (int64_t)op.size + (int64_t)op.relative_offset;
            if (target_i64 < 0)
                goto fail;
            if ((uint64_t)target_i64 > (uint64_t)region_size)
                goto fail;
            size_t target = (size_t)target_i64;

            if (target < region_size) {
                int target_idx = -1;
                for (int j = 0; j < inst_count; j++) {
                    if (offsets[j] == target) {
                        target_idx = j;
                        break;
                    }
                }
                if (target_idx < 0)
                    goto fail;
                if (add_unique_edge(&nodes[i], &nodes[target_idx]) != 0)
                    goto fail;
            } else {
                has_exit = 1;
            }
        }
    }

    /* Fallthrough exit from last non-goto instruction */
    if (inst_count > 0 && ops[inst_count - 1].opcode != KY_OP_GOTO) {
        size_t last_next = offsets[inst_count - 1] + ops[inst_count - 1].size;
        if (last_next == region_size)
            has_exit = 1;
    }

    queue = malloc((size_t)inst_count * sizeof(int32_t));
    if (!queue)
        goto fail;
    int qhead = 0, qtail = 0;

    nodes[0].stack_depth = initial_depth;
    nodes[0].visited = 1;
    queue[qtail++] = 0;

    uint32_t max_stack = (uint32_t)initial_depth;
    int32_t exit_stack_depth = -1;

    while (qhead < qtail) {
        int cur_idx = queue[qhead++];
        ValidatorCFGNode *cur = &nodes[cur_idx];

        if (cur->stack_depth < 0)
            goto fail;

        KyDecodedOp op = ops[cur_idx];
        int32_t new_depth = cur->stack_depth - op.n_pop + op.n_push;
        if (new_depth < 0)
            goto fail;
        if ((uint32_t)new_depth > max_stack)
            max_stack = (uint32_t)new_depth;

        for (int s = 0; s < cur->succ_count; s++) {
            ValidatorCFGNode *succ = cur->succs[s];
            int succ_idx = -1;
            for (int j = 0; j < inst_count; j++) {
                if (&nodes[j] == succ) { succ_idx = j; break; }
            }
            if (succ_idx < 0)
                goto fail;

            if (!succ->visited) {
                succ->stack_depth = new_depth;
                succ->visited = 1;
                queue[qtail++] = succ_idx;
            } else if (succ->stack_depth != new_depth) {
                goto fail;
            }
        }

        /* Record exit depth for explicit branch exits */
        if (op.opcode == KY_OP_GOTO ||
            op.opcode == KY_OP_IF_TRUE ||
            op.opcode == KY_OP_IF_FALSE)
        {
            int64_t target_i64 =
                (int64_t)cur->offset + (int64_t)op.size + (int64_t)op.relative_offset;
            if (target_i64 == (int64_t)region_size) {
                if (record_exit_depth(new_depth, &exit_stack_depth) != 0)
                    goto fail;
            }
        }
    }

    /* Record exit depth for fallthrough exit from last instruction */
    if (inst_count > 0) {
        KyDecodedOp last = ops[inst_count - 1];
        if (last.opcode != KY_OP_GOTO) {
            size_t last_next = offsets[inst_count - 1] + last.size;
            if (last_next == region_size) {
                int32_t last_depth =
                    nodes[inst_count - 1].stack_depth
                    - last.n_pop
                    + last.n_push;
                if (last_depth < 0)
                    goto fail;
                if (record_exit_depth(last_depth, &exit_stack_depth) != 0)
                    goto fail;
            }
        }
    }

    /* All instructions reachable */
    for (int i = 0; i < inst_count; i++) {
        if (!nodes[i].visited)
            goto fail;
    }

    /* Backedge analysis */
    for (int i = 0; i < inst_count; i++) {
        for (int s = 0; s < nodes[i].succ_count; s++) {
            size_t src_offset = nodes[i].offset;
            size_t dst_offset = nodes[i].succs[s]->offset;
            if (dst_offset < src_offset) {
                backedge_count++;
                if (dst_offset != 0)
                    goto fail;
            } else if (dst_offset == src_offset) {
                goto fail;
            }
        }
    }

    if (nodes[0].offset != 0)
        goto fail;

    if (backedge_count != 1)
        goto fail;

    if (!has_exit)
        goto fail;

    if (exit_stack_depth != initial_depth)
        goto fail;

    if (info) {
        info->instruction_count = inst_count;
        info->max_stack = max_stack;
        info->backedge_count = backedge_count;
        info->branch_count = branch_count;
        info->has_backedge = 1;
        info->single_entry = 1;
        info->single_exit = 1;
        info->balanced_stack = 1;
        info->supported = 1;
    }

    free(queue);
    for (int i = 0; i < inst_count; i++) {
        free(nodes[i].preds);
        free(nodes[i].succs);
    }
    free(nodes);
    free(offsets);
    free(ops);
    return 0;

fail:
    if (queue)
        free(queue);
    if (nodes) {
        for (int i = 0; i < inst_count; i++) {
            free(nodes[i].preds);
            free(nodes[i].succs);
        }
        free(nodes);
    }
    free(offsets);
    free(ops);
    if (info) {
        memset(info, 0, sizeof(*info));
    }
    return -1;
}
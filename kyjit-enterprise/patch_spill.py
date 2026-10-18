import re

path = "src/kyjit_ir.c"
src = open(path).read()

# 1. Replace kyx64_allocate
old_alloc = re.search(r'int kyx64_allocate\(KyX64Backend \*b\) \{.*?\n\}\n', src, re.DOTALL)
if old_alloc:
    new_alloc = """int kyx64_allocate(KyX64Backend *b) {
    if (!b) return -1;
    
    int32_t min_offset = 0;
    for (uint32_t i = 0; i < b->graph->node_count; i++) {
        KyNode *n = b->graph->nodes[i];
        if (!n) continue;
        if (n->opcode == IR_LOAD_LOCAL || n->opcode == IR_STORE_LOCAL) {
            int32_t off = (int32_t)n->constant.i64;
            if (off < min_offset) min_offset = off;
        }
    }
    uint32_t locals_size = (uint32_t)(-min_offset);

    KyX64Reg all_regs[] = {
        X64_RAX, X64_RBX, X64_RCX, X64_RDX,
        X64_RSI, X64_RDI,
        X64_R8, X64_R9, 
        X64_R12, X64_R13, X64_R14, X64_R15
    };
    uint32_t num_all = sizeof(all_regs) / sizeof(all_regs[0]);
    
    KyX64Reg free_regs[16];
    uint32_t num_free = 0;
    for (uint32_t k = 0; k < num_all; k++) {
        KyX64Reg r = all_regs[k];
        if (r < 32 && (b->reserved_regs & (1u << r))) continue;
        free_regs[num_free++] = r;
    }
    uint32_t next_reg = 0;
    b->num_spills = 0;
    for (uint32_t i = 0; i < b->num_vregs; i++) {
        KyVReg *vr = b->vregs[i];
        if (!vr) continue;
        if (vr->kind == VREG_PHYSICAL) continue;
        if (vr->kind == VREG_VIRTUAL) {
            if (next_reg < num_free) {
                vr->phys = free_regs[next_reg++];
                b->vreg_phys[vr->id] = vr->phys;
            } else {
                vr->spilled = 1;
                vr->spill_slot = -(int32_t)(locals_size + 8 + (b->num_spills * 8));
                b->num_spills++;
            }
        }
    }
    
    uint32_t spill_size = b->num_spills * 8;
    uint32_t total = locals_size + spill_size;
    total = (total + 15) & ~15u;
    b->frame.frame_size = total;
    
    return 0;
}
"""
    src = src[:old_alloc.start()] + new_alloc + src[old_alloc.end():]
    print("[1] OK: kyx64_allocate replaced")
else:
    print("[1] FAIL: kyx64_allocate not found")

# 2. Inject resolve_vreg and EMIT_BINOP before kyx64_emit
helper = """
static KyX64Reg resolve_vreg(KyX64Backend *b, KyVReg *vr, uint8_t *code, size_t code_size, size_t *off, int scratch_idx) {
    (void)b;
    if (!vr) return X64_REG_NONE;
    if (!vr->spilled) return vr->phys;
    KyX64Reg scratch = (scratch_idx == 0) ? X64_R10 : X64_R11;
    encode_load(code, code_size, off, scratch, X64_RBP, vr->spill_slot);
    return scratch;
}

#define EMIT_BINOP(encode_func) \\
    do { \\
        if (!ins->dst || !ins->src2) break; \\
        KyX64Reg rs1 = resolve_vreg(b, ins->src1, code, code_size, &off, 0); \\
        KyX64Reg rs2 = resolve_vreg(b, ins->src2, code, code_size, &off, 1); \\
        if (rs1 == X64_REG_NONE || rs2 == X64_REG_NONE) break; \\
        KyX64Reg rd = ins->dst->spilled ? X64_R10 : ins->dst->phys; \\
        if (rd == X64_REG_NONE) break; \\
        if (rd != rs1) encode_mov_reg_reg(code, code_size, &off, rd, rs1); \\
        encode_func(code, code_size, &off, rd, rs2); \\
        if (ins->dst->spilled) encode_store(code, code_size, &off, X64_RBP, ins->dst->spill_slot, rd); \\
    } while(0)

int kyx64_emit(KyX64Backend *b, uint8_t *code, size_t code_size, size_t *code_len) {
"""
src = re.sub(r'int kyx64_emit\(KyX64Backend \*b, uint8_t \*code, size_t code_size, size_t \*code_len\) \{\n', helper, src, count=1)
print("[2] OK: resolve_vreg and EMIT_BINOP injected")

# 3. Remove old frame_size computation from kyx64_compile and kyx64_compile_to_elf
pattern_frame = re.compile(r'    /\* Compute frame size from locals \*/\n    \{.*?b->frame\.frame_size = frame_size;\n    \}\n', re.DOTALL)
src, n_subs = pattern_frame.subn('', src)
print(f"[3] OK: removed {n_subs} old frame_size computations")

# 4. Add R10/R11 reservation
old_scan = "    /* Scan for IR_DIV_I64"
new_scan = "    b->reserved_regs |= (1u << X64_R10);\n    b->reserved_regs |= (1u << X64_R11);\n\n    /* Scan for IR_DIV_I64"
src = src.replace(old_scan, new_scan)

old_scan2 = "    /* Reserve regs (same logic as kyx64_compile). */\n    for (uint32_t i = 0; i < g->node_count; i++) {"
new_scan2 = "    /* Reserve regs (same logic as kyx64_compile). */\n    b->reserved_regs |= (1u << X64_R10);\n    b->reserved_regs |= (1u << X64_R11);\n    for (uint32_t i = 0; i < g->node_count; i++) {"
src = src.replace(old_scan2, new_scan2)
print("[4] OK: R10/R11 reserved")

# 5. Replace switch cases in kyx64_emit
replacements = [
    ("""        case X64_MOV_REG_IMM: {
            if (ins->dst && ins->dst->phys != X64_REG_NONE) {
                /* If this instruction was tagged as a string-pointer
                 * placeholder (fixup_label == 0xFFFFFFFF sentinel),
                 * record the byte offset where the 8-byte imm lands.
                 * encode_mov_imm64 writes: 48 REX.B opcode, then 8
                 * bytes of imm, so the imm starts at off+2. */
                if (ins->fixup_label == 0xFFFFFFFF) {
                    record_string_patch(b, (uint32_t)(off + 2),
                                        ins->fixup_offset);
                }
                rc = encode_mov_imm64(code, code_size, &off,
                                      ins->dst->phys, ins->imm);
            }
            break;
        }""",
     """        case X64_MOV_REG_IMM: {
            if (!ins->dst) break;
            if (ins->dst->spilled) {
                encode_mov_imm64(code, code_size, &off, X64_R10, ins->imm);
                encode_store(code, code_size, &off, X64_RBP, ins->dst->spill_slot, X64_R10);
            } else if (ins->dst->phys != X64_REG_NONE) {
                if (ins->fixup_label == 0xFFFFFFFF) {
                    record_string_patch(b, (uint32_t)(off + 2), ins->fixup_offset);
                }
                rc = encode_mov_imm64(code, code_size, &off, ins->dst->phys, ins->imm);
            }
            break;
        }"""),
    ("""        case X64_MOV_REG_REG: {
            if (ins->imm >= 0 && ins->imm < X64_REG_COUNT) {
                /* Physical target */
                if (ins->src1 && ins->src1->phys != X64_REG_NONE) {
                    rc = encode_mov_reg_reg(code, code_size, &off,
                                            (KyX64Reg)ins->imm,
                                            ins->src1->phys);
                }
            } else if (ins->dst && ins->src1 &&
                       ins->dst->phys != X64_REG_NONE &&
                       ins->src1->phys != X64_REG_NONE) {
                rc = encode_mov_reg_reg(code, code_size, &off,
                                        ins->dst->phys, ins->src1->phys);
            }
            break;
        }""",
     """        case X64_MOV_REG_REG: {
            if (ins->imm >= 0 && ins->imm < X64_REG_COUNT) {
                KyX64Reg rd = (KyX64Reg)ins->imm;
                KyX64Reg rs1 = resolve_vreg(b, ins->src1, code, code_size, &off, 0);
                if (rs1 != X64_REG_NONE) rc = encode_mov_reg_reg(code, code_size, &off, rd, rs1);
            } else if (ins->dst && ins->src1) {
                KyX64Reg rs1 = resolve_vreg(b, ins->src1, code, code_size, &off, 0);
                if (rs1 == X64_REG_NONE) break;
                KyX64Reg rd = ins->dst->spilled ? X64_R10 : ins->dst->phys;
                if (rd == X64_REG_NONE) break;
                if (rd != rs1) encode_mov_reg_reg(code, code_size, &off, rd, rs1);
                if (ins->dst->spilled) encode_store(code, code_size, &off, X64_RBP, ins->dst->spill_slot, rd);
            }
            break;
        }"""),
    ("""        case X64_ADD_REG_REG: {
            if (ins->dst && ins->src2 &&
                ins->dst->phys != X64_REG_NONE &&
                ins->src2->phys != X64_REG_NONE) {
                /* mov dst, src1; add dst, src2 */
                if (ins->src1 && ins->src1->phys != X64_REG_NONE &&
                    ins->src1->phys != ins->dst->phys) {
                    encode_mov_reg_reg(code, code_size, &off,
                                       ins->dst->phys, ins->src1->phys);
                }
                rc = encode_add_reg_reg(code, code_size, &off,
                                        ins->dst->phys, ins->src2->phys);
            }
            break;
        }""", """        case X64_ADD_REG_REG: EMIT_BINOP(encode_add_reg_reg); break;"""),
    ("""        case X64_SUB_REG_REG: {
            if (ins->dst && ins->src2 &&
                ins->dst->phys != X64_REG_NONE &&
                ins->src2->phys != X64_REG_NONE) {
                if (ins->src1 && ins->src1->phys != X64_REG_NONE &&
                    ins->src1->phys != ins->dst->phys) {
                    encode_mov_reg_reg(code, code_size, &off,
                                       ins->dst->phys, ins->src1->phys);
                }
                rc = encode_sub_reg_reg(code, code_size, &off,
                                        ins->dst->phys, ins->src2->phys);
            }
            break;
        }""", """        case X64_SUB_REG_REG: EMIT_BINOP(encode_sub_reg_reg); break;"""),
    ("""        case X64_MUL_REG_REG: {
            if (ins->dst && ins->src2 &&
                ins->dst->phys != X64_REG_NONE &&
                ins->src2->phys != X64_REG_NONE) {
                if (ins->src1 && ins->src1->phys != X64_REG_NONE &&
                    ins->src1->phys != ins->dst->phys) {
                    encode_mov_reg_reg(code, code_size, &off,
                                       ins->dst->phys, ins->src1->phys);
                }
                rc = encode_imul_reg_reg(code, code_size, &off,
                                         ins->dst->phys, ins->src2->phys);
            }
            break;
        }""", """        case X64_MUL_REG_REG: EMIT_BINOP(encode_imul_reg_reg); break;"""),
    ("""        case X64_AND_REG_REG:
            if (ins->dst && ins->src2 &&
                ins->dst->phys  != X64_REG_NONE &&
                ins->src2->phys != X64_REG_NONE) {
                if (ins->src1 && ins->src1->phys != X64_REG_NONE &&
                    ins->src1->phys != ins->dst->phys) {
                    encode_mov_reg_reg(code, code_size, &off,
                                       ins->dst->phys, ins->src1->phys);
                }
                rc = encode_and_reg(code, code_size, &off,
                                    ins->dst->phys, ins->src2->phys);
            }
            break;""", """        case X64_AND_REG_REG: EMIT_BINOP(encode_and_reg); break;"""),
    ("""        case X64_OR_REG_REG:
            if (ins->dst && ins->src2 &&
                ins->dst->phys  != X64_REG_NONE &&
                ins->src2->phys != X64_REG_NONE) {
                if (ins->src1 && ins->src1->phys != X64_REG_NONE &&
                    ins->src1->phys != ins->dst->phys) {
                    encode_mov_reg_reg(code, code_size, &off,
                                       ins->dst->phys, ins->src1->phys);
                }
                rc = encode_or_reg(code, code_size, &off,
                                   ins->dst->phys, ins->src2->phys);
            }
            break;""", """        case X64_OR_REG_REG: EMIT_BINOP(encode_or_reg); break;"""),
    ("""        case X64_XOR_REG_REG:
            if (ins->dst && ins->src2 &&
                ins->dst->phys  != X64_REG_NONE &&
                ins->src2->phys != X64_REG_NONE) {
                if (ins->src1 && ins->src1->phys != X64_REG_NONE &&
                    ins->src1->phys != ins->dst->phys) {
                    encode_mov_reg_reg(code, code_size, &off,
                                       ins->dst->phys, ins->src1->phys);
                }
                rc = encode_xor_reg(code, code_size, &off,
                                    ins->dst->phys, ins->src2->phys);
            }
            break;""", """        case X64_XOR_REG_REG: EMIT_BINOP(encode_xor_reg); break;"""),
    ("""        case X64_CMP_REG_IMM:
            /* Matches extended emitter: operand is in src1, not dst. */
            if (ins->src1 && ins->src1->phys != X64_REG_NONE)
                rc = encode_cmp_imm(code, code_size, &off,
                                    ins->src1->phys, (int32_t)ins->imm);
            break;""",
     """        case X64_CMP_REG_IMM: {
            if (!ins->src1) break;
            KyX64Reg rs1 = resolve_vreg(b, ins->src1, code, code_size, &off, 0);
            if (rs1 != X64_REG_NONE) rc = encode_cmp_imm(code, code_size, &off, rs1, (int32_t)ins->imm);
            break;
        }"""),
    ("""        case X64_CMP_REG_REG:
            if (ins->dst && ins->src1 &&
                ins->dst->phys  != X64_REG_NONE &&
                ins->src1->phys != X64_REG_NONE)
                rc = encode_cmp_reg(code, code_size, &off,
                                    ins->dst->phys, ins->src1->phys);
            break;""",
     """        case X64_CMP_REG_REG: {
            if (!ins->dst || !ins->src1) break;
            KyX64Reg rs1 = resolve_vreg(b, ins->dst, code, code_size, &off, 0);
            KyX64Reg rs2 = resolve_vreg(b, ins->src1, code, code_size, &off, 1);
            if (rs1 == X64_REG_NONE || rs2 == X64_REG_NONE) break;
            rc = encode_cmp_reg(code, code_size, &off, rs1, rs2);
            break;
        }"""),
    ("""        case X64_SHL_REG_IMM:
            if (ins->dst && ins->dst->phys != X64_REG_NONE)
                rc = encode_shl_imm(code, code_size, &off,
                                    ins->dst->phys, (uint8_t)ins->imm);
            break;
        case X64_SHR_REG_IMM:
            if (ins->dst && ins->dst->phys != X64_REG_NONE)
                rc = encode_shr_imm(code, code_size, &off,
                                    ins->dst->phys, (uint8_t)ins->imm);
            break;
        case X64_SAR_REG_IMM:
            if (ins->dst && ins->dst->phys != X64_REG_NONE)
                rc = encode_sar_imm(code, code_size, &off,
                                    ins->dst->phys, (uint8_t)ins->imm);
            break;""",
     """        case X64_SHL_REG_IMM:
        case X64_SHR_REG_IMM:
        case X64_SAR_REG_IMM: {
            if (!ins->dst) break;
            KyX64Reg rd = ins->dst->spilled ? X64_R10 : ins->dst->phys;
            if (rd == X64_REG_NONE) break;
            if (ins->dst->spilled) encode_load(code, code_size, &off, rd, X64_RBP, ins->dst->spill_slot);
            if (ins->op == X64_SHL_REG_IMM) rc = encode_shl_imm(code, code_size, &off, rd, (uint8_t)ins->imm);
            else if (ins->op == X64_SHR_REG_IMM) rc = encode_shr_imm(code, code_size, &off, rd, (uint8_t)ins->imm);
            else rc = encode_sar_imm(code, code_size, &off, rd, (uint8_t)ins->imm);
            if (ins->dst->spilled) encode_store(code, code_size, &off, X64_RBP, ins->dst->spill_slot, rd);
            break;
        }"""),
    ("""        case X64_SHL_REG_CL:
            if (ins->dst && ins->dst->phys != X64_REG_NONE)
                rc = encode_shl_cl(code, code_size, &off, ins->dst->phys);
            break;
        case X64_SHR_REG_CL:
            if (ins->dst && ins->dst->phys != X64_REG_NONE)
                rc = encode_shr_cl(code, code_size, &off, ins->dst->phys);
            break;
        case X64_SAR_REG_CL:
            if (ins->dst && ins->dst->phys != X64_REG_NONE)
                rc = encode_sar_cl(code, code_size, &off, ins->dst->phys);
            break;""",
     """        case X64_SHL_REG_CL:
        case X64_SHR_REG_CL:
        case X64_SAR_REG_CL: {
            if (!ins->dst) break;
            KyX64Reg rd = ins->dst->spilled ? X64_R10 : ins->dst->phys;
            if (rd == X64_REG_NONE) break;
            if (ins->dst->spilled) encode_load(code, code_size, &off, rd, X64_RBP, ins->dst->spill_slot);
            if (ins->op == X64_SHL_REG_CL) rc = encode_shl_cl(code, code_size, &off, rd);
            else if (ins->op == X64_SHR_REG_CL) rc = encode_shr_cl(code, code_size, &off, rd);
            else rc = encode_sar_cl(code, code_size, &off, rd);
            if (ins->dst->spilled) encode_store(code, code_size, &off, X64_RBP, ins->dst->spill_slot, rd);
            break;
        }"""),
    ("""        case X64_NEG:
            if (ins->dst && ins->dst->phys != X64_REG_NONE)
                rc = encode_neg(code, code_size, &off, ins->dst->phys);
            break;""",
     """        case X64_NEG: {
            if (!ins->dst) break;
            KyX64Reg rd = ins->dst->spilled ? X64_R10 : ins->dst->phys;
            if (rd == X64_REG_NONE) break;
            if (ins->dst->spilled) encode_load(code, code_size, &off, rd, X64_RBP, ins->dst->spill_slot);
            rc = encode_neg(code, code_size, &off, rd);
            if (ins->dst->spilled) encode_store(code, code_size, &off, X64_RBP, ins->dst->spill_slot, rd);
            break;
        }"""),
    ("""        case X64_NOT:
            if (ins->dst && ins->dst->phys != X64_REG_NONE)
                rc = encode_not(code, code_size, &off, ins->dst->phys);
            break;""",
     """        case X64_NOT: {
            if (!ins->dst) break;
            KyX64Reg rd = ins->dst->spilled ? X64_R10 : ins->dst->phys;
            if (rd == X64_REG_NONE) break;
            if (ins->dst->spilled) encode_load(code, code_size, &off, rd, X64_RBP, ins->dst->spill_slot);
            rc = encode_not(code, code_size, &off, rd);
            if (ins->dst->spilled) encode_store(code, code_size, &off, X64_RBP, ins->dst->spill_slot, rd);
            break;
        }"""),
    ("""        case X64_IDIV_REG:
            if (ins->src1 && ins->src1->phys != X64_REG_NONE)
                rc = encode_idiv_reg(code, code_size, &off, ins->src1->phys);
            break;""",
     """        case X64_IDIV_REG: {
            if (!ins->src1) break;
            KyX64Reg rs1 = resolve_vreg(b, ins->src1, code, code_size, &off, 0);
            if (rs1 == X64_REG_NONE) break;
            rc = encode_idiv_reg(code, code_size, &off, rs1);
            break;
        }"""),
    ("""        case X64_SETE:
        case X64_SETNE:
        case X64_SETL:
        case X64_SETLE:
        case X64_SETG:
        case X64_SETGE:
            if (ins->dst && ins->dst->phys != X64_REG_NONE)
                rc = encode_setcc(code, code_size, &off, ins->op,
                                  ins->dst->phys);
            break;""",
     """        case X64_SETE: case X64_SETNE: case X64_SETL: case X64_SETLE: case X64_SETG: case X64_SETGE: {
            if (!ins->dst) break;
            KyX64Reg rd = ins->dst->spilled ? X64_R10 : ins->dst->phys;
            if (rd == X64_REG_NONE) break;
            rc = encode_setcc(code, code_size, &off, ins->op, rd);
            if (ins->dst->spilled) encode_store(code, code_size, &off, X64_RBP, ins->dst->spill_slot, rd);
            break;
        }"""),
    ("""        case X64_MOVZX_BYTE:
            if (ins->dst && ins->src1 &&
                ins->dst->phys  != X64_REG_NONE &&
                ins->src1->phys != X64_REG_NONE)
                rc = encode_movzx_byte(code, code_size, &off,
                                       ins->dst->phys, ins->src1->phys);
            break;
        case X64_MOVZX_WORD:
            if (ins->dst && ins->src1 &&
                ins->dst->phys  != X64_REG_NONE &&
                ins->src1->phys != X64_REG_NONE)
                rc = encode_movzx_word(code, code_size, &off,
                                       ins->dst->phys, ins->src1->phys);
            break;
        case X64_MOVZX_DWORD:
            if (ins->dst && ins->src1 &&
                ins->dst->phys  != X64_REG_NONE &&
                ins->src1->phys != X64_REG_NONE)
                rc = encode_movzx_dword(code, code_size, &off,
                                        ins->dst->phys, ins->src1->phys);
            break;
        case X64_MOVSX_BYTE:
            if (ins->dst && ins->src1 &&
                ins->dst->phys  != X64_REG_NONE &&
                ins->src1->phys != X64_REG_NONE)
                rc = encode_movsx_byte(code, code_size, &off,
                                       ins->dst->phys, ins->src1->phys);
            break;
        case X64_MOVSX_WORD:
            if (ins->dst && ins->src1 &&
                ins->dst->phys  != X64_REG_NONE &&
                ins->src1->phys != X64_REG_NONE)
                rc = encode_movsx_word(code, code_size, &off,
                                       ins->dst->phys, ins->src1->phys);
            break;""",
     """        case X64_MOVZX_BYTE:
        case X64_MOVZX_WORD:
        case X64_MOVZX_DWORD:
        case X64_MOVSX_BYTE:
        case X64_MOVSX_WORD: {
            if (!ins->dst || !ins->src1) break;
            KyX64Reg rs1 = resolve_vreg(b, ins->src1, code, code_size, &off, 0);
            if (rs1 == X64_REG_NONE) break;
            KyX64Reg rd = ins->dst->spilled ? X64_R10 : ins->dst->phys;
            if (rd == X64_REG_NONE) break;
            if (ins->op == X64_MOVZX_BYTE) rc = encode_movzx_byte(code, code_size, &off, rd, rs1);
            else if (ins->op == X64_MOVZX_WORD) rc = encode_movzx_word(code, code_size, &off, rd, rs1);
            else if (ins->op == X64_MOVZX_DWORD) rc = encode_movzx_dword(code, code_size, &off, rd, rs1);
            else if (ins->op == X64_MOVSX_BYTE) rc = encode_movsx_byte(code, code_size, &off, rd, rs1);
            else rc = encode_movsx_word(code, code_size, &off, rd, rs1);
            if (ins->dst->spilled) encode_store(code, code_size, &off, X64_RBP, ins->dst->spill_slot, rd);
            break;
        }"""),
    ("""        case X64_AND_REG_IMM:
            if (ins->dst && ins->dst->phys != X64_REG_NONE)
                rc = encode_and_imm(code, code_size, &off,
                                    ins->dst->phys, (int32_t)ins->imm);
            break;
        case X64_OR_REG_IMM:
            if (ins->dst && ins->dst->phys != X64_REG_NONE)
                rc = encode_or_imm(code, code_size, &off,
                                   ins->dst->phys, (int32_t)ins->imm);
            break;
        case X64_XOR_REG_IMM:
            if (ins->dst && ins->dst->phys != X64_REG_NONE)
                rc = encode_xor_imm(code, code_size, &off,
                                    ins->dst->phys, (int32_t)ins->imm);
            break;""",
     """        case X64_AND_REG_IMM:
        case X64_OR_REG_IMM:
        case X64_XOR_REG_IMM: {
            if (!ins->dst) break;
            KyX64Reg rs1 = resolve_vreg(b, ins->dst, code, code_size, &off, 0);
            if (rs1 == X64_REG_NONE) break;
            KyX64Reg rd = ins->dst->spilled ? X64_R10 : ins->dst->phys;
            if (rd == X64_REG_NONE) break;
            if (rd != rs1) encode_mov_reg_reg(code, code_size, &off, rd, rs1);
            if (ins->op == X64_AND_REG_IMM) rc = encode_and_imm(code, code_size, &off, rd, (int32_t)ins->imm);
            else if (ins->op == X64_OR_REG_IMM) rc = encode_or_imm(code, code_size, &off, rd, (int32_t)ins->imm);
            else rc = encode_xor_imm(code, code_size, &off, rd, (int32_t)ins->imm);
            if (ins->dst->spilled) encode_store(code, code_size, &off, X64_RBP, ins->dst->spill_slot, rd);
            break;
        }"""),
    ("""        case X64_MOV_REG_MEM:
            if (ins->dst && ins->src1 &&
                ins->dst->phys  != X64_REG_NONE &&
                ins->src1->phys != X64_REG_NONE)
                rc = encode_load(code, code_size, &off,
                                 ins->dst->phys, ins->src1->phys,
                                 ins->mem_offset);
            break;""",
     """        case X64_MOV_REG_MEM: {
            if (!ins->dst || !ins->src1) break;
            KyX64Reg base = resolve_vreg(b, ins->src1, code, code_size, &off, 0);
            if (base == X64_REG_NONE) break;
            KyX64Reg rd = ins->dst->spilled ? X64_R10 : ins->dst->phys;
            if (rd == X64_REG_NONE) break;
            rc = encode_load(code, code_size, &off, rd, base, ins->mem_offset);
            if (ins->dst->spilled) encode_store(code, code_size, &off, X64_RBP, ins->dst->spill_slot, rd);
            break;
        }"""),
    ("""        case X64_MOV_MEM_REG:
            if (ins->dst && ins->src1 &&
                ins->dst->phys  != X64_REG_NONE &&
                ins->src1->phys != X64_REG_NONE)
                rc = encode_store(code, code_size, &off,
                                  ins->dst->phys, ins->mem_offset,
                                  ins->src1->phys);
            break;""",
     """        case X64_MOV_MEM_REG: {
            if (!ins->dst || !ins->src1) break;
            KyX64Reg base = resolve_vreg(b, ins->dst, code, code_size, &off, 0);
            KyX64Reg rs1 = resolve_vreg(b, ins->src1, code, code_size, &off, 1);
            if (base == X64_REG_NONE || rs1 == X64_REG_NONE) break;
            rc = encode_store(code, code_size, &off, base, ins->mem_offset, rs1);
            break;
        }"""),
    ("""        case X64_LEA:
            if (ins->dst && ins->src1 &&
                ins->dst->phys  != X64_REG_NONE &&
                ins->src1->phys != X64_REG_NONE)
                rc = encode_lea(code, code_size, &off,
                                ins->dst->phys, ins->src1->phys,
                                ins->mem_offset);
            break;""",
     """        case X64_LEA: {
            if (!ins->dst || !ins->src1) break;
            KyX64Reg base = resolve_vreg(b, ins->src1, code, code_size, &off, 0);
            if (base == X64_REG_NONE) break;
            KyX64Reg rd = ins->dst->spilled ? X64_R10 : ins->dst->phys;
            if (rd == X64_REG_NONE) break;
            rc = encode_lea(code, code_size, &off, rd, base, ins->mem_offset);
            if (ins->dst->spilled) encode_store(code, code_size, &off, X64_RBP, ins->dst->spill_slot, rd);
            break;
        }""")
]

for old, new in replacements:
    if old in src:
        src = src.replace(old, new)
    else:
        print(f"WARN: could not find replacement anchor for: {old[:40]}...")

print("[5] OK: kyx64_emit cases replaced")

open(path, "w").write(src)
/*
 * kyjit_frontend.h — Source-to-IR frontend for the .ky language.
 *
 * Parses a .ky source string and produces a KyGraph ready for
 * kyx64_compile(). No loops, no functions, no locals — integer
 * expressions, forward branches, return, and write().
 *
 * Grammar (EBNF):
 *
 *   program    := stmt*
 *   stmt       := "if" "(" expr ")" stmt ("else" stmt)?
 *               | "{" stmt* "}"
 *               | "return" expr ";"
 *               | "write" "(" expr "," STRING "," expr ")" ";"
 *               | expr ";"
 *
 *   expr       := or_expr
 *   or_expr    := xor_expr ("|" xor_expr)*
 *   xor_expr   := and_expr ("^" and_expr)*
 *   and_expr   := shift_expr ("&" shift_expr)*
 *   shift_expr := add_expr (("<<" | ">>") add_expr)*
 *   add_expr   := mul_expr (("+" | "-") mul_expr)*
 *   mul_expr   := unary (("*" | "/" | "%") unary)*
 *   unary      := "-" unary | "~" unary | primary
 *   primary    := INT | "(" expr ")"
 *
 * Integer literals: decimal (42) or hex (0x2A).
 * Comments: // to end of line, /* ... * / (nested not supported).
 *
 * "write(fd, \"string\", len);" is special-cased to emit an
 * IR_SYSCALL for SYS_write (nr 1). fd and len are expressions;
 * "string" must be a literal (the frontend emits it into the
 * data section — not yet supported, so this form is limited).
 * For now, write() takes a numeric fd, an integer literal
 * "as a pointer to a user-supplied string", and a length.
 * See test_frontend.c for the supported usage.
 */

#ifndef KYJIT_FRONTEND_H
#define KYJIT_FRONTEND_H

#include "kyjit_ir.h"
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Parse a .ky source string and return a fresh KyGraph.
 *
 * On success: returns a non-NULL KyGraph. The caller owns it and
 * must eventually call kyir_graph_destroy().
 *
 * On failure: returns NULL. If errbuf is non-NULL, a human-readable
 * error message (including filename, line, column, and a short
 * description) is written to it, up to errlen-1 bytes plus a NUL.
 */
KyGraph *kyjit_parse_source(const char *src,
                            const char *filename,
                            char *errbuf,
                            size_t errlen);

#ifdef __cplusplus
}
#endif

#endif /* KYJIT_FRONTEND_H */

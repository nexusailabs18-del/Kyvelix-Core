#ifndef KYJIT_REGEXP_H
#define KYJIT_REGEXP_H

#include <stdint.h>
#include <stddef.h>

/* Simple regex pattern */
typedef struct KyjitRegexp KyjitRegexp;

/* Compile a regex pattern.
 * Supported: literal strings, . (any char), * (zero or more),
 *            + (one or more), ? (optional), | (alternation),
 *            [abc] (character class), ^ (start), $ (end)
 */
KyjitRegexp *kyjit_regexp_compile(const char *pattern);

/* Match string against compiled regexp.
 * Returns 1 if match, 0 if no match, -1 on error.
 */
int kyjit_regexp_match(KyjitRegexp *re, const char *str, size_t len);

/* Free compiled regexp */
void kyjit_regexp_free(KyjitRegexp *re);

/* Utility: quick match without keeping regexp */
int kyjit_regexp_match_once(const char *pattern, const char *str);

#endif

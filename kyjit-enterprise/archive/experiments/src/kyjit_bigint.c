#include "kyjit_bigint.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

#define BASE 1000000000ULL  /* 1e9 */
#define DIGITS 9

struct KyjitBigInt {
    uint32_t *limbs;   /* little-endian, base 1e9 */
    int       count;
    int       negative;
};

/* Create zero */
static KyjitBigInt *bigint_create(int count) {
    KyjitBigInt *n = calloc(1, sizeof(*n));
    n->limbs = calloc(count ? count : 1, sizeof(uint32_t));
    n->count = count ? count : 1;
    return n;
}

static void bigint_trim(KyjitBigInt *n) {
    while (n->count > 1 && n->limbs[n->count-1] == 0) n->count--;
}

KyjitBigInt *kyjit_bigint_from_int64(int64_t val) {
    KyjitBigInt *n = bigint_create(1);
    if (val < 0) {
        n->negative = 1;
        val = -val;
    }
    n->limbs[0] = (uint32_t)(val % BASE);
    if ((uint64_t)val >= BASE) {
        n->count = 2;
        n->limbs = realloc(n->limbs, 2 * sizeof(uint32_t));
        n->limbs[1] = (uint32_t)(val / BASE);
    }
    return n;
}

KyjitBigInt *kyjit_bigint_from_string(const char *str) {
    KyjitBigInt *result = kyjit_bigint_zero();
    const char *p = str;
    if (*p == '-') {
        result->negative = 1;
        p++;
    }

    while (*p) {
        /* result = result * 10 + digit */
        KyjitBigInt *ten = kyjit_bigint_from_int64(10);
        KyjitBigInt *digit = kyjit_bigint_from_int64(*p - '0');
        KyjitBigInt *tmp = kyjit_bigint_mul(result, ten);
        KyjitBigInt *tmp2 = kyjit_bigint_add(tmp, digit);

        kyjit_bigint_free(result);
        kyjit_bigint_free(ten);
        kyjit_bigint_free(digit);
        kyjit_bigint_free(tmp);
        result = tmp2;
        p++;
    }
    return result;
}

KyjitBigInt *kyjit_bigint_copy(KyjitBigInt *src) {
    KyjitBigInt *n = bigint_create(src->count);
    memcpy(n->limbs, src->limbs, src->count * sizeof(uint32_t));
    n->negative = src->negative;
    return n;
}

void kyjit_bigint_free(KyjitBigInt *n) {
    if (!n) return;
    free(n->limbs);
    free(n);
}

KyjitBigInt *kyjit_bigint_add(KyjitBigInt *a, KyjitBigInt *b) {
    int max_count = a->count > b->count ? a->count : b->count;
    KyjitBigInt *r = bigint_create(max_count + 1);

    uint64_t carry = 0;
    for (int i = 0; i < max_count; i++) {
        uint64_t av = (i < a->count) ? a->limbs[i] : 0;
        uint64_t bv = (i < b->count) ? b->limbs[i] : 0;
        uint64_t sum = av + bv + carry;
        r->limbs[i] = (uint32_t)(sum % BASE);
        carry = sum / BASE;
    }
    if (carry) {
        r->limbs[max_count] = (uint32_t)carry;
        r->count = max_count + 1;
    } else {
        r->count = max_count;
    }
    bigint_trim(r);
    return r;
}

KyjitBigInt *kyjit_bigint_sub(KyjitBigInt *a, KyjitBigInt *b) {
    /* Simple: assume a >= b */
    KyjitBigInt *r = bigint_create(a->count);
    int64_t borrow = 0;
    for (int i = 0; i < a->count; i++) {
        int64_t av = a->limbs[i];
        int64_t bv = (i < b->count) ? b->limbs[i] : 0;
        int64_t diff = av - bv - borrow;
        if (diff < 0) {
            diff += BASE;
            borrow = 1;
        } else {
            borrow = 0;
        }
        r->limbs[i] = (uint32_t)diff;
    }
    bigint_trim(r);
    return r;
}

KyjitBigInt *kyjit_bigint_mul(KyjitBigInt *a, KyjitBigInt *b) {
    KyjitBigInt *r = bigint_create(a->count + b->count);

    for (int i = 0; i < a->count; i++) {
        uint64_t carry = 0;
        for (int j = 0; j < b->count; j++) {
            uint64_t product = (uint64_t)a->limbs[i] * b->limbs[j] +
                               r->limbs[i+j] + carry;
            r->limbs[i+j] = (uint32_t)(product % BASE);
            carry = product / BASE;
        }
        r->limbs[i + b->count] = (uint32_t)carry;
    }
    r->count = a->count + b->count;
    bigint_trim(r);
    return r;
}

int kyjit_bigint_cmp(KyjitBigInt *a, KyjitBigInt *b) {
    if (a->count != b->count)
        return a->count > b->count ? 1 : -1;

    for (int i = a->count - 1; i >= 0; i--) {
        if (a->limbs[i] != b->limbs[i])
            return a->limbs[i] > b->limbs[i] ? 1 : -1;
    }
    return 0;
}

int64_t kyjit_bigint_to_int64(KyjitBigInt *n) {
    int64_t result = 0;
    for (int i = n->count - 1; i >= 0; i--) {
        result = result * BASE + n->limbs[i];
    }
    return n->negative ? -result : result;
}

char *kyjit_bigint_to_string(KyjitBigInt *n) {
    /* Simple for small numbers */
    char buf[128];
    if (n->negative) {
        snprintf(buf, sizeof(buf), "-%ld", (long)kyjit_bigint_to_int64(n));
    } else {
        snprintf(buf, sizeof(buf), "%ld", (long)kyjit_bigint_to_int64(n));
    }
    return strdup(buf);
}

KyjitBigInt *kyjit_bigint_zero(void) {
    return kyjit_bigint_from_int64(0);
}

KyjitBigInt *kyjit_bigint_one(void) {
    return kyjit_bigint_from_int64(1);
}

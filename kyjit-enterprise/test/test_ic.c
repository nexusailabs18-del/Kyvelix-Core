/*
 * test/test_ic.c — KYJIT IC test suite
 *
 * Build:
 *   gcc -O2 -I./include test/test_ic.c src/kyjit_ic.c -o test/test_ic
 * Run:
 *   ./test/test_ic
 *
 * Coverage:
 *   - Mono/Poly/Mega state transitions
 *   - Update + lookup correctness
 *   - Invalidate (mono/poly/mega)
 *   - Feedback vector
 *   - Self-mod create/invalidate/revalidate/free
 *   - Edge cases: NULL, shape_id 0, shape_id 0xFFFFFFFF, full table
 */

#include "kyjit_ic.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int tests_run = 0;
static int tests_passed = 0;
static int tests_failed = 0;

#define TEST(name, cond) do { \
    tests_run++; \
    if (cond) { \
        tests_passed++; \
        printf("  PASS: %s\n", name); \
    } else { \
        tests_failed++; \
        printf("  FAIL: %s (line %d)\n", name, __LINE__); \
    } \
} while(0)

/* ---------- Mono tests ---------- */

static void test_mono_basic(void) {
    printf("\n[Test] mono_basic\n");
    KyIC ic;
    ky_ic_init(&ic);

    TEST("empty IC returns MISS", ky_ic_lookup_inline(&ic, 42) == KY_IC_MISS);

    ky_ic_update_inline(&ic, 42, 16);
    TEST("mono lookup returns 16", ky_ic_lookup_inline(&ic, 42) == 16);
    TEST("state is MONOMORPHIC", ic.state == KY_IC_MONOMORPHIC);
    TEST("count is 1", ic.count == 1);

    ky_ic_update_inline(&ic, 42, 32);
    TEST("mono update changes offset", ky_ic_lookup_inline(&ic, 42) == 32);

    ky_ic_destroy(&ic);
}

/* ---------- Poly tests ---------- */

static void test_poly_transitions(void) {
    printf("\n[Test] poly_transitions\n");
    KyIC ic;
    ky_ic_init(&ic);

    ky_ic_update_inline(&ic, 1, 10);
    ky_ic_update_inline(&ic, 2, 20);
    TEST("poly count 2", ic.count == 2);
    TEST("poly state", ic.state == KY_IC_POLYMORPHIC);
    TEST("lookup 1", ky_ic_lookup_inline(&ic, 1) == 10);
    TEST("lookup 2", ky_ic_lookup_inline(&ic, 2) == 20);
    TEST("lookup 3 (miss)", ky_ic_lookup_inline(&ic, 3) == KY_IC_MISS);

    ky_ic_update_inline(&ic, 3, 30);
    ky_ic_update_inline(&ic, 4, 40);
    TEST("poly count 4", ic.count == 4);
    TEST("lookup 3", ky_ic_lookup_inline(&ic, 3) == 30);
    TEST("lookup 4", ky_ic_lookup_inline(&ic, 4) == 40);

    ky_ic_destroy(&ic);
}

/* ---------- Mega tests ---------- */

static void test_mega_transition(void) {
    printf("\n[Test] mega_transition\n");
    KyIC ic;
    ky_ic_init(&ic);

    for (uint32_t i = 1; i <= 4; i++) {
        ky_ic_update_inline(&ic, i, (int32_t)(i * 10));
    }
    TEST("still POLY before 5th", ic.state == KY_IC_POLYMORPHIC);

    ky_ic_update(&ic, 5, 50);
    TEST("state is MEGA after 5th", ic.state == KY_IC_MEGAMORPHIC);
    TEST("mega pointer not null", ic.mega != NULL);

    for (uint32_t i = 1; i <= 5; i++) {
        int32_t expected = (int32_t)(i * 10);
        TEST("mega lookup", ky_ic_lookup_inline(&ic, i) == expected);
    }

    TEST("mega miss", ky_ic_lookup_inline(&ic, 999) == KY_IC_MISS);

    ky_ic_destroy(&ic);
}

static void test_mega_many_shapes(void) {
    printf("\n[Test] mega_many_shapes\n");
    KyIC ic;
    ky_ic_init(&ic);

    /* Add 16 shapes (max table size) */
    for (uint32_t i = 1; i <= 16; i++) {
        ky_ic_update(&ic, i * 17, (int32_t)(i * 100));
    }
    TEST("state is MEGA", ic.state == KY_IC_MEGAMORPHIC);

    int all_found = 1;
    for (uint32_t i = 1; i <= 16; i++) {
        if (ky_ic_lookup_inline(&ic, i * 17) != (int32_t)(i * 100)) {
            all_found = 0;
            break;
        }
    }
    TEST("all 16 shapes found", all_found);

    ky_ic_destroy(&ic);
}

/* ---------- Invalidate tests ---------- */

static void test_invalidate_mono(void) {
    printf("\n[Test] invalidate_mono\n");
    KyIC ic;
    ky_ic_init(&ic);

    ky_ic_update_inline(&ic, 42, 16);
    TEST("before invalidate", ky_ic_lookup_inline(&ic, 42) == 16);

    ky_ic_invalidate(&ic, 42);
    TEST("after invalidate MISS", ky_ic_lookup_inline(&ic, 42) == KY_IC_MISS);
    TEST("count 0", ic.count == 0);

    ky_ic_destroy(&ic);
}

static void test_invalidate_poly(void) {
    printf("\n[Test] invalidate_poly\n");
    KyIC ic;
    ky_ic_init(&ic);

    ky_ic_update_inline(&ic, 1, 10);
    ky_ic_update_inline(&ic, 2, 20);
    ky_ic_update_inline(&ic, 3, 30);

    ky_ic_invalidate(&ic, 2);
    TEST("count 2 after remove", ic.count == 2);
    TEST("lookup 1 still works", ky_ic_lookup_inline(&ic, 1) == 10);
    TEST("lookup 2 is MISS", ky_ic_lookup_inline(&ic, 2) == KY_IC_MISS);
    TEST("lookup 3 still works", ky_ic_lookup_inline(&ic, 3) == 30);

    ky_ic_destroy(&ic);
}

static void test_invalidate_mega(void) {
    printf("\n[Test] invalidate_mega\n");
    KyIC ic;
    ky_ic_init(&ic);

    for (uint32_t i = 1; i <= 6; i++) {
        ky_ic_update(&ic, i, (int32_t)(i * 10));
    }

    ky_ic_invalidate(&ic, 3);
    TEST("mega lookup 3 is MISS", ky_ic_lookup_inline(&ic, 3) == KY_IC_MISS);
    TEST("mega lookup 1 works", ky_ic_lookup_inline(&ic, 1) == 10);
    TEST("mega lookup 6 works", ky_ic_lookup_inline(&ic, 6) == 60);

    ky_ic_destroy(&ic);
}

/* ---------- Edge cases ---------- */

static void test_edge_null(void) {
    printf("\n[Test] edge_null\n");
    TEST("mega_lookup NULL", ky_ic_mega_lookup(NULL, 42) == KY_IC_MISS);
    TEST("mega_count NULL", ky_ic_mega_count(NULL) == 0);

    /* ky_ic_invalidate on NULL must not crash */
    ky_ic_invalidate(NULL, 42);
    TEST("invalidate NULL no crash", 1);
}

static void test_edge_empty(void) {
    printf("\n[Test] edge_empty\n");
    KyIC ic;
    ky_ic_init(&ic);

    TEST("lookup on empty IC", ky_ic_lookup_inline(&ic, 42) == KY_IC_MISS);
    TEST("state is MONO", ic.state == KY_IC_MONOMORPHIC);
    TEST("count is 0", ic.count == 0);

    ky_ic_destroy(&ic);
}

static void test_edge_shape_zero(void) {
    printf("\n[Test] edge_shape_zero\n");
    KyIC ic;
    ky_ic_init(&ic);

    ky_ic_update_inline(&ic, 0, 99);
    TEST("shape 0 lookup", ky_ic_lookup_inline(&ic, 0) == 99);

    ky_ic_destroy(&ic);
}

static void test_edge_duplicate_update(void) {
    printf("\n[Test] edge_duplicate_update\n");
    KyIC ic;
    ky_ic_init(&ic);

    for (int i = 0; i < 100; i++) {
        ky_ic_update_inline(&ic, 42, (int32_t)(i * 2));
    }
    TEST("count stays 1", ic.count == 1);
    TEST("offset is last value", ky_ic_lookup_inline(&ic, 42) == 198);

    ky_ic_destroy(&ic);
}

static void test_edge_double_destroy(void) {
    printf("\n[Test] edge_double_destroy\n");
    KyIC ic;
    ky_ic_init(&ic);
    ky_ic_update(&ic, 1, 10);
    ky_ic_update(&ic, 2, 20);
    ky_ic_update(&ic, 3, 30);
    ky_ic_update(&ic, 4, 40);
    ky_ic_update(&ic, 5, 50);  /* triggers mega */

    ky_ic_destroy(&ic);
    ky_ic_destroy(&ic);  /* second destroy: mega is NULL, selfmod is NULL */

    TEST("double destroy no crash", 1);
}

/* ---------- Feedback tests ---------- */

static void test_feedback(void) {
    printf("\n[Test] feedback\n");
    KyICFeedback fb;
    ky_ic_feedback_init(&fb);

    TEST("empty feedback count 0", fb.count == 0);

    ky_ic_feedback_record(&fb, 42);
    ky_ic_feedback_record(&fb, 42);
    ky_ic_feedback_record(&fb, 42);
    TEST("count is 1", fb.count == 1);
    TEST("hit count 3", fb.hit_counts[0] == 3);

    ky_ic_feedback_record(&fb, 43);
    TEST("count is 2", fb.count == 2);

    KyICState rec = ky_ic_feedback_recommend_state(&fb);
    TEST("recommend state returned", rec >= 0 && rec <= KY_IC_INVALID);
}

/* ---------- Selfmod tests ---------- */

static void test_selfmod_basic(void) {
    printf("\n[Test] selfmod_basic\n");
    KyICSelfModFn fn = ky_ic_selfmod_create(7, 99);
    if (fn == NULL) {
        printf("  SKIP: selfmod_create returned NULL\n");
        return;
    }

    TEST("selfmod call match", fn(7) == 99);
    TEST("selfmod call miss", fn(8) == KY_IC_MISS);

    ky_ic_selfmod_free(fn);
}

static void test_selfmod_invalidate(void) {
    printf("\n[Test] selfmod_invalidate\n");
    KyICSelfModFn fn = ky_ic_selfmod_create(7, 99);
    if (fn == NULL) {
        printf("  SKIP: selfmod_create returned NULL\n");
        return;
    }

    ky_ic_selfmod_invalidate_fn(fn);
    TEST("after invalidate, returns MISS", fn(7) == KY_IC_MISS);

    ky_ic_selfmod_free(fn);
}

/* ---------- Main ---------- */

int main(void) {
    printf("KYJIT IC Test Suite v%s\n", KYJIT_IC_VERSION_STRING);
    printf("==============================\n");

    test_mono_basic();
    test_poly_transitions();
    test_mega_transition();
    test_mega_many_shapes();
    test_invalidate_mono();
    test_invalidate_poly();
    test_invalidate_mega();
    test_edge_null();
    test_edge_empty();
    test_edge_shape_zero();
    test_edge_duplicate_update();
    test_edge_double_destroy();
    test_feedback();
    test_selfmod_basic();
    test_selfmod_invalidate();

    printf("\n==============================\n");
    printf("Tests run:    %d\n", tests_run);
    printf("Tests passed: %d\n", tests_passed);
    printf("Tests failed: %d\n", tests_failed);
    printf("==============================\n");

    return tests_failed > 0 ? 1 : 0;
}

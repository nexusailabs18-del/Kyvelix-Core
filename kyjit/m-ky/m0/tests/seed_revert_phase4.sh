#!/usr/bin/env bash
# Phase 4 §4.4 — seed-and-revert against the *shipping* scalar analyses.
#
# The in-process sibling, `seed_and_revert_reports_violations` in
# `tests/phase4_fuzz.rs`, weakens the *oracle*. This script does the harder, more
# honest version: it injects a real bug into `src/fud`, runs the corresponding
# fuzz driver, and requires it to REPORT violations; then restores the file and
# requires the driver to go quiet. A fuzz driver that passes both with a bug in
# the analysis and with the bug removed is checking nothing — this is the only
# way to know it actually inspects the code it claims to.
#
# Four seeds, one per analysis, each the §4.4-named error:
#   tripcount  `(n + d - 1) / d` → `n / d`      — the ceil → floor off-by-one
#   constprop  meet of differing constants returns the first — Rule 1 ↔ Rule 2
#   induct     the IV base `a` is reported as `a + 1` — a trip-index/base off-by-one
#   scalardd   a loop-carried distance of 1 is reported as 2 — a wrong direction
#
# The tree is restored from backups on ANY exit (including Ctrl-C), and this
# script never uses `git checkout`, so uncommitted Phase 4 work is safe.
#
# It edits tracked source files in place, so TWO CONCURRENT RUNS WOULD CORRUPT
# EACH OTHER (one restores while the other still expects the bug). A lock makes
# that an error rather than a silent wrong answer.
set -u
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 1

LOCK=/tmp/seed_revert_phase4.lock
exec 200>"$LOCK"
if ! flock -n 200; then
    echo "Another seed_revert_phase4.sh is already running (lock: $LOCK); exiting."
    exit 1
fi

FILES=(src/fud/tripcount.rs src/fud/constprop.rs src/fud/induct.rs src/fud/scalardd.rs)
BAK=$(mktemp -d)
for f in "${FILES[@]}"; do
    mkdir -p "$BAK/$(dirname "$f")"
    cp "$f" "$BAK/$f"
done
restore() {
    for f in "${FILES[@]}"; do
        [ -f "$BAK/$f" ] && cp "$BAK/$f" "$f"
    done
    rm -rf "$BAK"
}
trap restore EXIT INT TERM

# patch FILE NEEDLE REPLACEMENT — replace exactly one unique occurrence.
patch() {
    local file="$1" needle="$2" repl="$3"
    python3 - "$file" "$needle" "$repl" <<'PY'
import sys
path, needle, repl = sys.argv[1], sys.argv[2], sys.argv[3]
src = open(path).read()
n = src.count(needle)
if n != 1:
    sys.exit(f"patch: expected exactly 1 occurrence of {needle!r} in {path}, found {n}")
open(path, "w").write(src.replace(needle, repl))
PY
}

# every seed must produce a rebuild; cargo recompiles per file edit.
fails=0
expect_fire() {
    local name="$1" driver="$2"
    if cargo test --test phase4_fuzz --release "$driver" 2>&1 | grep -qE 'test result: ok'; then
        echo "SEED $name: fuzz driver '$driver' PASSED with the bug present — it is VACUOUS"
        fails=$((fails + 1))
    else
        echo "SEED $name: fuzz driver '$driver' correctly reported violations"
    fi
}

echo "== tripcount: ceil → floor =="
patch src/fud/tripcount.rs "(n + d - 1) / d" "n / d"
expect_fire tripcount fuzz_trip_count_matches_enumeration
cp "$BAK/src/fud/tripcount.rs" src/fud/tripcount.rs

echo "== constprop: unsound meet of differing constants =="
patch src/fud/constprop.rs "                } else {
                    AbsVal::Bottom
                }" "                } else {
                    AbsVal::Const(*a)
                }"
expect_fire constprop fuzz_constprop_matches_analysis
cp "$BAK/src/fud/constprop.rs" src/fud/constprop.rs

echo "== induct: off-by-one base value =="
patch src/fud/induct.rs "                let cls = Class::LinearIv { a, b };" "                let cls = Class::LinearIv { a: a + 1, b };"
expect_fire induct fuzz_induct_matches_analysis
cp "$BAK/src/fud/induct.rs" src/fud/induct.rs

echo "== scalardd: wrong carried distance =="
patch src/fud/scalardd.rs "{ None } else { Some(1) }" "{ None } else { Some(2) }"
expect_fire scalardd fuzz_scalar_dd_matches_analysis
cp "$BAK/src/fud/scalardd.rs" src/fud/scalardd.rs

echo "== revert: every driver must go quiet =="
if cargo test --test phase4_fuzz --release 2>&1 | grep -qE 'test result: FAILED|panicked'; then
    echo "REVERT FAILED: a driver still reports violations after the tree was restored"
    fails=$((fails + 1))
else
    echo "REVERT OK: all Phase 4 drivers pass on the restored tree"
fi

# Every seed edits a tracked-source file in place, so "the tests pass again" is
# only meaningful if the tree is the tree we started from. A driver can go quiet
# because a patch happened to survive, and this sweep would then be reporting on
# a source tree nobody has reviewed. Compare against the pristine copies byte for
# byte. `git diff` would be the obvious check but is useless here: these four
# files are new in Phase 4 and still untracked, so it prints nothing either way.
for f in "${FILES[@]}"; do
    if ! cmp -s "$BAK/$f" "$f"; then
        echo "RESTORE FAILED: $f is not the file this script started with"
        fails=$((fails + 1))
    fi
done
[ "$fails" -eq 0 ] && echo "TREE OK: all four analysis files byte-identical to their pristine copies"

if [ "$fails" -ne 0 ]; then
    echo "RESULT: $fails check(s) failed — inspect the files above before trusting any Phase 4 result"
    exit 1
fi
echo "RESULT: all four seeds fired, and all four drivers are clean after revert"

#!/bin/sh
# Test command for the parity fixture. Each grep pins a few tokens of
# src/demo/core.clj. A mutant that changes a pinned token fails this script
# and counts as killed. Unpinned tokens survive.
set -e
f=src/demo/core.clj
grep -Fq -e '(if (= n 0) 0' "$f"
grep -Fq -e '(dec (count xs))' "$f"
grep -Fq -e '(if-not (and flag' "$f"
grep -Fq -e '(take (inc (second xs))' "$f"

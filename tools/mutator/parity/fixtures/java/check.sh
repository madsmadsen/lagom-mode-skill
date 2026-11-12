#!/bin/sh
# Test command for the parity fixture. Each grep pins a few tokens of
# src/main/java/demo/Counter.java. Unpinned tokens survive.
set -e
f=src/main/java/demo/Counter.java
grep -Fq -e 'return count >= limit' "$f"
grep -Fq -e 'return count + delta / 1;' "$f"
grep -Fq -e 'if (delta < 0)' "$f"
grep -Fq -e 'return b == false;' "$f"

#!/bin/sh
# Test command for the parity fixture. Each grep pins a few tokens of
# src/counter.ts. Unpinned tokens survive.
set -e
f=src/counter.ts
grep -Fq -e 'if (delta === undefined)' "$f"
grep -Fq -e 'name?.trim()' "$f"
grep -Fq -e 'return this.count >= limit' "$f"

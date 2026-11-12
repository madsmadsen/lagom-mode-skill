#!/bin/sh
# Test command for the parity fixture. Each grep pins a few tokens of demo.go.
# Unpinned tokens survive.
set -e
f=demo.go
grep -Fq -e 'if delta < 0 {' "$f"
grep -Fq -e 'return n >= limit' "$f"
grep -Fq -e 'return c.count / 2' "$f"

#!/bin/sh
# Test command for the parity fixture. Each grep pins a few tokens of
# src/demo/board.py. Unpinned tokens survive.
set -e
f=src/demo/board.py
grep -Fq -e 'return row >= 0' "$f"
grep -Fq -e 'if bonus < 0:' "$f"
grep -Fq -e 'def _full(self) -> bool:' "$f"

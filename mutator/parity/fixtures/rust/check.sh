#!/bin/sh
# Test command for the parity fixture. Each grep pins a few tokens of
# src/lib.rs. Unpinned tokens survive.
set -e
f=src/lib.rs
grep -Fq -e '-n + 0' "$f"
grep -Fq -e 'self.count == 0' "$f"
grep -Fq -e 'pub fn bump(&mut self, delta: i32) -> i32 {' "$f"

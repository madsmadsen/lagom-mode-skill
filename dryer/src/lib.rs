// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
//! dryer: a structural duplicate finder for Clojure, Java, Go, TypeScript,
//! Rust, and Python. A Rust port of the Python tool `dryer`.
//!
//! Pipeline: discover source files, parse them (tree-sitter or the EDN-aware
//! Clojure reader), normalize each selected form into a tree that drops local
//! names and literals, fingerprint the tree, and report pairs whose Jaccard
//! similarity meets the threshold.

pub mod args;
pub mod astnorm;
pub mod cli;
pub mod clojure;
pub mod discover;
pub mod extract;
pub mod model;
pub mod report;
pub mod scan;
pub mod select;
pub mod shape;
pub mod treesitter;

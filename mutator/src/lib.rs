//! mutator: mutation testing for Clojure, Java, Go, TypeScript, Rust, and Python.
//!
//! A Rust port of github.com/unclebob/mutator (BSD Zero Clause License).

pub mod bind;
pub mod cli;
pub mod clojure_reader;
pub mod clojure_sites;
pub mod cover_run;
pub mod coverage;
pub mod discover;
pub mod edn;
pub mod engine;
pub mod functions;
pub mod metrics;
pub mod model;
pub mod report;
pub mod runner;
pub mod sites;
pub mod ts;
pub mod workers;

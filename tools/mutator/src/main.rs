// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(mutator::cli::run(&args));
}

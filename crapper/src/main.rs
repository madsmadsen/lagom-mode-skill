// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: none

fn main() {
    std::process::exit(crapper::cli::run(
        &std::env::args().skip(1).collect::<Vec<_>>(),
    ))
}

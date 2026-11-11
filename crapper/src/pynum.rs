// Copyright (C) 2026 Mads Madsen <mads@onmadsen.com>
// SPDX-License-Identifier: MIT
// upstream: https://github.com/unclebob/crapper
// upstream-commit: 6fd92c4c9bcc56efb64a2f8743d5a2993bcfd42d
// upstream-path: none

//! Python's number rules for ASCII text: the original reads numbers with `int()` and `float()`.

/// Whether every underscore has an ASCII digit directly before and after it.
fn underscores_between_digits(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().all(|(index, &byte)| {
        byte != b'_'
            || (index > 0
                && index + 1 < bytes.len()
                && bytes[index - 1].is_ascii_digit()
                && bytes[index + 1].is_ascii_digit())
    })
}

/// Python's `float()` for ASCII text: surrounding whitespace is ignored, and
/// an underscore is allowed only directly between two digits.
pub fn python_float(raw: &str) -> Option<f64> {
    let text = raw.trim();
    if underscores_between_digits(text) {
        text.replace('_', "").parse().ok()
    } else {
        None
    }
}

/// Python's `int()` for ASCII text: surrounding whitespace is ignored, one optional
/// sign is allowed, and an underscore is allowed only directly between two digits.
///
/// Python integers have no size limit. The magnitude is clamped at `i64::MAX`,
/// so a negative number below `-i64::MAX` gives `-i64::MAX`.
pub fn python_int(raw: &str) -> Option<i64> {
    let text = raw.trim();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let well_formed = digits.bytes().any(|byte| byte.is_ascii_digit())
        && digits
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'_')
        && underscores_between_digits(digits);
    if !well_formed {
        return None;
    }
    let magnitude = digits
        .bytes()
        .filter(|&byte| byte != b'_')
        .fold(0_i64, |acc, byte| {
            acc.saturating_mul(10)
                .saturating_add(i64::from(byte - b'0'))
        });
    Some(if negative { -magnitude } else { magnitude })
}

#[cfg(test)]
mod tests {
    use super::*;

    // CR-2
    #[test]
    fn cr_2_python_float_accepts_what_python_accepts() {
        assert_eq!(python_float("1_0"), Some(10.0));
        assert_eq!(python_float("1_0.5_0"), Some(10.5));
        assert_eq!(python_float("1e1_0"), Some(1e10));
        assert_eq!(python_float(" 12 "), Some(12.0));
    }

    // CR-2
    #[test]
    fn cr_2_python_float_rejects_misplaced_underscores() {
        for raw in [
            "_1", "1_", "1__0", "1_.5", "1._5", "1e_5", "1_e5", "-_1", "abc", "",
        ] {
            assert_eq!(python_float(raw), None, "{raw:?} should be rejected");
        }
    }

    // CR-3
    #[test]
    fn cr_3_python_int_accepts_what_python_accepts() {
        assert_eq!(python_int("10"), Some(10));
        assert_eq!(python_int("+5"), Some(5));
        assert_eq!(python_int("-5"), Some(-5));
        assert_eq!(python_int(" 7 "), Some(7));
        assert_eq!(python_int("1_0"), Some(10));
        assert_eq!(python_int("0_4"), Some(4));
        assert_eq!(python_int("+0_6"), Some(6));
        assert_eq!(python_int("00012"), Some(12));
        assert_eq!(python_int("99999999999999999999999"), Some(i64::MAX));
        assert_eq!(python_int("-99999999999999999999999"), Some(-i64::MAX));
        assert_eq!(python_int("9223372036854775807"), Some(i64::MAX));
    }

    // CR-3
    #[test]
    fn cr_3_python_int_rejects_what_python_rejects() {
        for raw in [
            "", "+", "-", "_1", "1_", "1__0", "+-1", "1.5", "0x10", "1 0", "1_-1", "+_1", "٣",
        ] {
            assert_eq!(python_int(raw), None, "{raw:?} should be rejected");
        }
    }
}

// Derived from dryer by Robert C. Martin (https://github.com/unclebob/dryer), BSD Zero Clause License.
// Ports src/dryer/cli.py: HELP, Options, parse_args and their helpers.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::discover;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Scan,
    Help,
    Format,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub action: Action,
    pub message: String,
    pub exit_code: i32,
    pub project_root: PathBuf,
    pub source_roots: Vec<String>,
    pub positionals: Vec<String>,
    pub threshold: f64,
    pub min_lines: usize,
    pub min_nodes: usize,
    pub format: String,
    pub changed: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            action: Action::Scan,
            message: String::new(),
            exit_code: 0,
            project_root: PathBuf::from("."),
            source_roots: Vec::new(),
            positionals: Vec::new(),
            threshold: 0.82,
            min_lines: 4,
            min_nodes: 20,
            format: "text".to_string(),
            changed: false,
        }
    }
}

/// Sorted union of the skipped and test directory names, joined as an English list.
fn skipped_directory_text() -> String {
    let names: Vec<&str> = discover::SKIP_DIRS
        .iter()
        .chain(discover::TEST_DIRS.iter())
        .copied()
        .collect::<BTreeSet<&str>>()
        .into_iter()
        .collect();
    match names.split_last() {
        Some((last, rest)) => format!("{}, and {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// The exact text of the `--help` output.
pub fn help_text() -> String {
    let skipped = skipped_directory_text();
    format!(
        r#"Usage: dryer [options] [path-or-filter ...]

Find candidate duplicate code in Clojure, Java, Go, TypeScript, Rust, and
Python. Each form is normalized — call and method names, operators, and
tree shape stay; local names, field names, and literals do not — and compared
with the other forms of the same language. The score is Jaccard similarity of
those structural fingerprints, the same score as dry4clj.

Prints a report and writes .metrics/dry.edn.

Languages: Clojure (.clj .cljc .cljs .cljd .bb), Java (.java), Go (.go),
TypeScript (.ts .tsx .mts .cts), Rust (.rs), Python (.py).

Options:
  -h, --help                    Print this help and exit.
  --root <path>                 Project root. Metrics are written here.
                                Default: the current directory.
  -s, --source-root <path>      Walk this tree instead of the project root.
                                May be repeated.
  --changed                     Compare added and modified source files from
                                git status.
  --threshold N                 Minimum similarity score, default 0.82.
  --min-lines N                 Minimum source lines in a candidate form,
                                default 4.
  --min-nodes N                 Minimum normalized syntax nodes, default 20.
  --format F                    text or edn, default text.
  --edn                         Same as --format edn.
  --text                        Same as --format text.

Arguments:
  path              File or directory to compare. Test paths are included when
                    you name them explicitly.
  filter            When the argument is not a path, only source files whose
                    path contains this text are compared.

With no paths, source files under the project root are compared. Directories
named {skipped} are skipped, as are *_test.go, *.test.cts,
*.spec.cts, *.spec.ts, test_*.py, and *_test.py files.

Clojure compares every top-level form except ns. The other languages compare
functions and methods. A form is only compared with forms in the same language.

Exit codes:
  0  the report was written
  1  usage error
  2  unknown output format
"#
    )
}

/// Parses the arguments exactly as the Python `parse_args` does.
pub fn parse_args(args: &[String]) -> Options {
    if args
        .iter()
        .any(|arg| arg.as_str() == "-h" || arg.as_str() == "--help")
    {
        return Options {
            action: Action::Help,
            message: help_text(),
            exit_code: 0,
            ..Options::default()
        };
    }
    let mut options = Options::default();
    if let Err(message) = apply_all(args, &mut options) {
        return usage_error(&message);
    }
    if options.format != "text" && options.format != "edn" {
        return Options {
            action: Action::Format,
            message: format!("Unknown format: {}\n", options.format),
            exit_code: 2,
            ..Options::default()
        };
    }
    options
}

fn usage_error(message: &str) -> Options {
    Options {
        action: Action::Help,
        message: format!("{message}\n\n{}", help_text()),
        exit_code: 1,
        ..Options::default()
    }
}

fn apply_all(args: &[String], options: &mut Options) -> Result<(), String> {
    let mut index = 0;
    while index < args.len() {
        index += apply_one(args, index, options)?;
    }
    Ok(())
}

/// Applies the argument at `index` and returns how many arguments it consumed.
fn apply_one(args: &[String], index: usize, options: &mut Options) -> Result<usize, String> {
    let arg = args[index].as_str();
    if let Some(field) = value_field(arg) {
        apply_option(field, take(args, index, arg)?, arg, options)?;
        return Ok(2);
    }
    if apply_flag(arg, options) {
        return Ok(1);
    }
    if arg.starts_with('-') {
        return Err(format!("Unknown option: {arg}"));
    }
    options.positionals.push(arg.to_string());
    Ok(1)
}

/// The option that receives a value. Each field is set in exactly one place: `apply_option`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    SourceRoot,
    Root,
    Threshold,
    MinLines,
    MinNodes,
    Format,
}

/// Maps a value-taking flag to the field it sets.
fn value_field(arg: &str) -> Option<Field> {
    match arg {
        "-s" | "--source-root" => Some(Field::SourceRoot),
        "--root" => Some(Field::Root),
        "--threshold" => Some(Field::Threshold),
        "--min-lines" => Some(Field::MinLines),
        "--min-nodes" => Some(Field::MinNodes),
        "--format" => Some(Field::Format),
        _ => None,
    }
}

/// Parses `value` for `field` (numbers and counts are validated; text is stored as given) and stores it.
fn apply_option(
    field: Field,
    value: &str,
    option: &str,
    options: &mut Options,
) -> Result<(), String> {
    match field {
        Field::SourceRoot => options.source_roots.push(value.to_string()),
        Field::Root => options.project_root = PathBuf::from(value),
        Field::Format => options.format = value.to_string(),
        Field::Threshold => options.threshold = parse_number(value, option)?,
        Field::MinLines => options.min_lines = parse_count(value, option)?,
        Field::MinNodes => options.min_nodes = parse_count(value, option)?,
    }
    Ok(())
}

/// Flags take no value. Returns false when `arg` is not a flag.
fn apply_flag(arg: &str, options: &mut Options) -> bool {
    match arg {
        "--edn" => options.format = "edn".to_string(),
        "--text" => options.format = "text".to_string(),
        "--changed" => options.changed = true,
        _ => return false,
    }
    true
}

/// The value after the option at `index`. A missing, empty, or dash-led value is an error.
fn take<'a>(args: &'a [String], index: usize, option: &str) -> Result<&'a str, String> {
    match args.get(index + 1) {
        Some(value) if !value.is_empty() && !value.starts_with('-') => Ok(value.as_str()),
        _ => Err(format!("{option} requires a value")),
    }
}

fn parse_number(value: &str, option: &str) -> Result<f64, String> {
    python_float(value).ok_or_else(|| format!("{option} requires a number"))
}

/// Python `float()`: surrounding whitespace is ignored and single underscores may sit between digits.
fn python_float(value: &str) -> Option<f64> {
    strip_digit_underscores(value.trim())?.parse::<f64>().ok()
}

/// Python `int()` for option counts: negative values are errors, large values saturate.
fn parse_count(value: &str, option: &str) -> Result<usize, String> {
    let (negative, unsigned) = split_sign(value.trim());
    let digits = strip_digit_underscores(unsigned)
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| format!("{option} requires an integer"))?;
    if negative && digits.bytes().any(|byte| byte != b'0') {
        return Err(format!("{option} requires a non-negative integer"));
    }
    Ok(saturating_usize(&digits))
}

fn split_sign(text: &str) -> (bool, &str) {
    match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    }
}

/// Removes underscores that sit between two ASCII digits. Any other underscore yields None.
fn strip_digit_underscores(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let well_placed = bytes.iter().enumerate().all(|(index, &byte)| {
        byte != b'_'
            || (index > 0
                && bytes[index - 1].is_ascii_digit()
                && bytes
                    .get(index + 1)
                    .is_some_and(|next| next.is_ascii_digit()))
    });
    well_placed.then(|| text.replace('_', ""))
}

fn saturating_usize(digits: &str) -> usize {
    digits.bytes().fold(0usize, |total, byte| {
        total
            .saturating_mul(10)
            .saturating_add(usize::from(byte - b'0'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN_HELP: &str = r#"Usage: dryer [options] [path-or-filter ...]

Find candidate duplicate code in Clojure, Java, Go, TypeScript, Rust, and
Python. Each form is normalized — call and method names, operators, and
tree shape stay; local names, field names, and literals do not — and compared
with the other forms of the same language. The score is Jaccard similarity of
those structural fingerprints, the same score as dry4clj.

Prints a report and writes .metrics/dry.edn.

Languages: Clojure (.clj .cljc .cljs .cljd .bb), Java (.java), Go (.go),
TypeScript (.ts .tsx .mts .cts), Rust (.rs), Python (.py).

Options:
  -h, --help                    Print this help and exit.
  --root <path>                 Project root. Metrics are written here.
                                Default: the current directory.
  -s, --source-root <path>      Walk this tree instead of the project root.
                                May be repeated.
  --changed                     Compare added and modified source files from
                                git status.
  --threshold N                 Minimum similarity score, default 0.82.
  --min-lines N                 Minimum source lines in a candidate form,
                                default 4.
  --min-nodes N                 Minimum normalized syntax nodes, default 20.
  --format F                    text or edn, default text.
  --edn                         Same as --format edn.
  --text                        Same as --format text.

Arguments:
  path              File or directory to compare. Test paths are included when
                    you name them explicitly.
  filter            When the argument is not a path, only source files whose
                    path contains this text are compared.

With no paths, source files under the project root are compared. Directories
named .clj-kondo, .git, .hg, .idea, .metrics, .svn, .venv, __pycache__, __tests__, build, coverage, dist, node_modules, out, spec, specs, target, test, testdata, tests, vendor, and venv are skipped, as are *_test.go, *.test.cts,
*.spec.cts, *.spec.ts, test_*.py, and *_test.py files.

Clojure compares every top-level form except ns. The other languages compare
functions and methods. A form is only compared with forms in the same language.

Exit codes:
  0  the report was written
  1  usage error
  2  unknown output format
"#;

    fn parse(args: &[&str]) -> Options {
        let owned: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        parse_args(&owned)
    }

    // DR-2: no arguments yields the documented defaults.
    #[test]
    fn dr2_defaults_when_no_arguments() {
        let options = parse(&[]);
        assert_eq!(options.action, Action::Scan);
        assert_eq!(options.message, "");
        assert_eq!(options.exit_code, 0);
        assert_eq!(options.project_root, PathBuf::from("."));
        assert!(options.source_roots.is_empty());
        assert!(options.positionals.is_empty());
        assert_eq!(options.threshold, 0.82);
        assert_eq!(options.min_lines, 4);
        assert_eq!(options.min_nodes, 20);
        assert_eq!(options.format, "text");
        assert!(!options.changed);
    }

    // DR-2: --root, repeated -s/--source-root, and --changed are read.
    #[test]
    fn dr2_root_source_roots_and_changed_are_read() {
        let options = parse(&[
            "--root",
            "proj",
            "-s",
            "a",
            "--source-root",
            "b",
            "--changed",
        ]);
        assert_eq!(options.action, Action::Scan);
        assert_eq!(options.project_root, PathBuf::from("proj"));
        assert_eq!(options.source_roots, vec!["a", "b"]);
        assert!(options.changed);
    }

    // DR-12: threshold, min-lines, min-nodes, --edn, and positionals are read.
    #[test]
    fn dr12_threshold_min_lines_min_nodes_and_positionals() {
        let options = parse(&[
            "--threshold",
            "0.9",
            "--min-lines",
            "5",
            "--min-nodes",
            "30",
            "--edn",
            "spec",
        ]);
        assert_eq!(options.threshold, 0.9);
        assert_eq!(options.min_lines, 5);
        assert_eq!(options.min_nodes, 30);
        assert_eq!(options.format, "edn");
        assert_eq!(options.positionals, vec!["spec"]);
    }

    // DR-12: whichever of --edn, --text, or --format comes last wins.
    #[test]
    fn dr12_last_format_flag_wins() {
        assert_eq!(parse(&["--edn", "--text"]).format, "text");
        assert_eq!(parse(&["--text", "--edn"]).format, "edn");
        assert_eq!(parse(&["--format", "edn", "--text"]).format, "text");
        assert_eq!(parse(&["--text", "--format", "edn"]).format, "edn");
    }

    // DR-13: an unknown format exits 2 with the exact message.
    #[test]
    fn dr13_unknown_format_exits_two() {
        let options = parse(&["--format", "xml"]);
        assert_eq!(options.action, Action::Format);
        assert_eq!(options.exit_code, 2);
        assert_eq!(options.message, "Unknown format: xml\n");
    }

    // DR-13: an unknown option exits 1 with the message and help text.
    #[test]
    fn dr13_unknown_option_exits_one() {
        let options = parse(&["--nope"]);
        assert_eq!(options.action, Action::Help);
        assert_eq!(options.exit_code, 1);
        assert_eq!(
            options.message,
            format!("Unknown option: --nope\n\n{}", help_text())
        );
    }

    // DR-13: a lone dash is an unknown option; an empty string is a positional.
    #[test]
    fn dr13_lone_dash_is_unknown_and_empty_is_positional() {
        let dash = parse(&["-"]);
        assert_eq!(dash.exit_code, 1);
        assert!(dash.message.starts_with("Unknown option: -\n\n"));
        let empty = parse(&[""]);
        assert_eq!(empty.exit_code, 0);
        assert_eq!(empty.positionals, vec![""]);
    }

    // DR-13: an option at the end without a value exits 1.
    #[test]
    fn dr13_option_without_value_exits_one() {
        let options = parse(&["--threshold"]);
        assert_eq!(options.exit_code, 1);
        assert!(options
            .message
            .starts_with("--threshold requires a value\n\n"));
    }

    // DR-13: an empty value is rejected.
    #[test]
    fn dr13_empty_value_is_rejected() {
        let options = parse(&["file.py", "--min-lines", ""]);
        assert_eq!(options.exit_code, 1);
        assert!(options
            .message
            .starts_with("--min-lines requires a value\n\n"));
    }

    // DR-13: another flag is not taken as a value.
    #[test]
    fn dr13_flag_is_not_a_value() {
        let options = parse(&["--min-lines", "--min-nodes", "1"]);
        assert_eq!(options.exit_code, 1);
        assert!(options
            .message
            .starts_with("--min-lines requires a value\n\n"));
    }

    // DR-13: a negative threshold is taken as a missing value and exits 1.
    #[test]
    fn dr13_negative_threshold_is_rejected() {
        let options = parse(&["--threshold", "-1"]);
        assert_eq!(options.exit_code, 1);
        assert!(options
            .message
            .starts_with("--threshold requires a value\n\n"));
    }

    // DR-13: a negative min-lines is rejected with exit 1.
    #[test]
    fn dr13_negative_min_lines_is_rejected() {
        let options = parse(&["--min-lines", " -1 "]);
        assert_eq!(options.exit_code, 1);
        assert!(options
            .message
            .starts_with("--min-lines requires a non-negative integer\n\n"));
    }

    // DR-13: a fractional min-lines is not an integer and exits 1.
    #[test]
    fn dr13_fractional_min_lines_is_rejected() {
        let options = parse(&["--min-lines", "2.5"]);
        assert_eq!(options.exit_code, 1);
        assert!(options
            .message
            .starts_with("--min-lines requires an integer\n\n"));
    }

    // DR-13: a non-number threshold exits 1.
    #[test]
    fn dr13_threshold_must_be_a_number() {
        let options = parse(&["--threshold", "abc"]);
        assert_eq!(options.exit_code, 1);
        assert!(options
            .message
            .starts_with("--threshold requires a number\n\n"));
    }

    // DR-13: -h/--help wins over every other error, even as an option value.
    #[test]
    fn dr13_help_wins_over_errors() {
        for args in [
            vec!["--threshold", "--help"],
            vec!["--format", "xml", "-h"],
            vec!["--nope", "-h"],
            vec!["--min-lines", "x", "--help"],
        ] {
            let options = parse(&args);
            assert_eq!(options.action, Action::Help);
            assert_eq!(options.exit_code, 0);
            assert_eq!(options.message, help_text());
        }
    }

    // DR-2: the help text is the exact Python HELP output.
    #[test]
    fn dr2_help_text_matches_python_help() {
        assert_eq!(help_text(), GOLDEN_HELP);
    }

    // DR-2: zero is a valid count for both min options.
    #[test]
    fn dr2_zero_is_a_valid_count() {
        let options = parse(&["--min-lines", "0", "--min-nodes", "0"]);
        assert_eq!(options.action, Action::Scan);
        assert_eq!(options.min_lines, 0);
        assert_eq!(options.min_nodes, 0);
    }

    // DR-2: counts accept surrounding whitespace, a plus sign, and digit underscores.
    #[test]
    fn dr2_count_accepts_python_int_forms() {
        assert_eq!(parse_count("+1_000", "--min-lines"), Ok(1000));
        assert_eq!(parse_count(" 7 ", "--min-lines"), Ok(7));
        assert_eq!(parse_count("-0", "--min-lines"), Ok(0));
    }

    // DR-2: a negative count is rejected as non-negative.
    #[test]
    fn dr2_negative_count_is_rejected() {
        assert_eq!(
            parse_count("-1", "--min-lines"),
            Err("--min-lines requires a non-negative integer".to_string())
        );
    }

    // DR-2: misplaced underscores, blanks, and non-digits are not integers.
    #[test]
    fn dr2_count_rejects_non_integers() {
        for text in ["1__0", "_1", "1_", "5.0", "", "+", "0x1"] {
            assert_eq!(
                parse_count(text, "--min-nodes"),
                Err("--min-nodes requires an integer".to_string()),
                "{text}"
            );
        }
    }

    // DR-2: counts beyond usize saturate.
    #[test]
    fn dr2_huge_count_saturates() {
        assert_eq!(
            parse_count("999999999999999999999999999999", "--min-lines"),
            Ok(usize::MAX)
        );
    }

    // DR-2: threshold follows Python float() rules.
    #[test]
    fn dr2_number_follows_python_float_rules() {
        assert_eq!(python_float(" 0.9 "), Some(0.9));
        assert_eq!(python_float("1_0.5"), Some(10.5));
        assert_eq!(python_float("1e-3"), Some(0.001));
        assert_eq!(python_float("inf"), Some(f64::INFINITY));
        assert_eq!(python_float("-infinity"), Some(f64::NEG_INFINITY));
        assert!(python_float("nan").is_some_and(f64::is_nan));
        assert_eq!(python_float("1__0"), None);
        assert_eq!(python_float("_1"), None);
        assert_eq!(python_float(""), None);
    }

    // DR-12: positionals keep their order and the default format is text.
    #[test]
    fn dr12_positionals_keep_order() {
        let options = parse(&["src", "--text", "filter"]);
        assert_eq!(options.positionals, vec!["src", "filter"]);
        assert_eq!(options.format, "text");
    }
}
